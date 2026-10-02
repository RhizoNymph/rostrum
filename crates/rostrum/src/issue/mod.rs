//! The detail pane for one issue, and the form that opens a new one.
//!
//! Shaped like the pull request pane's Conversation tab — header, timeline,
//! composer — without Files and Checks, which an issue does not have. A fresh
//! [`IssuePane`] is built whenever the selection changes, so dropping the old
//! one cancels its in-flight requests.

pub(crate) mod create;
mod render;

use std::{rc::Rc, sync::Arc};

use gpui::{App, AppContext, Context, Entity, Subscription, Task};
use gpui_tokio::Tokio;
use rostrum_core::{Issue, IssueDetail, IssueNumber, Label, RepoId, User};
use rostrum_db::Db;
use rostrum_github::{
    AddLabels, Assignees, CommentBody, GitHubClient, IssueMutation, IssueStateChange,
};
use rostrum_ui::{InputEvent, TextInput};

use crate::{
    loadable::Loadable,
    pickers::{OpenPicker, Toggle},
    sync::Store,
};

pub use create::{NewIssueEvent, NewIssueForm};

pub struct IssuePane {
    store: Entity<Store>,
    pub(crate) repo: RepoId,
    pub(crate) number: IssueNumber,
    /// The issue and its timeline, from the detail query (or the cache while
    /// that is in flight). Independent of the feed, so the pane outlives the
    /// issue closing and leaving the open list.
    detail: Loadable<IssueDetail>,
    /// The repository's label palette, fetched the first time the picker opens.
    repo_labels: Loadable<Vec<Label>>,
    /// Who issues here can be assigned to, fetched the first time the
    /// assignee picker opens.
    assignable: Loadable<Vec<User>>,
    /// At most one picker is open at a time.
    picker: Option<OpenPicker>,
    composer: Entity<TextInput>,
    /// Label of the in-flight mutation; also the guard against a second one.
    busy: Option<&'static str>,
    error: Option<String>,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl IssuePane {
    pub fn new(
        store: Entity<Store>,
        repo: RepoId,
        number: IssueNumber,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| TextInput::new("Leave a comment…", cx).lines(3, 10));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe(&composer, |this, _, event, cx| {
                if matches!(event, InputEvent::Submit) {
                    this.post_comment(cx);
                }
            }),
        ];
        let mut pane = Self {
            store,
            repo,
            number,
            detail: Loadable::Idle,
            repo_labels: Loadable::Idle,
            assignable: Loadable::Idle,
            picker: None,
            composer,
            busy: None,
            error: None,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        tracing::debug!(repo = %pane.repo, issue = %pane.number, "opened issue");
        pane.load_cached(cx);
        pane.load_detail(cx);
        pane
    }

    fn client(&self, cx: &App) -> Option<GitHubClient> {
        self.store.read(cx).client()
    }

    fn db(&self, cx: &App) -> Option<Arc<Db>> {
        self.store.read(cx).db()
    }

    /// The issue to draw the header from: the detail query's copy once it
    /// has answered, otherwise the feed's, so the header paints at once.
    pub(crate) fn issue(&self, cx: &App) -> Option<Issue> {
        if let Some(detail) = self.detail.loaded() {
            return Some(detail.issue.clone());
        }
        let store = self.store.read(cx);
        store
            .state
            .repo(&self.repo)?
            .issues
            .iter()
            .find(|issue| issue.number == self.number)
            .cloned()
    }

    // --- loading -----------------------------------------------------------

    /// Paint from the cache while the network request is in flight. Only ever
    /// fills a gap: a response that already arrived is newer.
    fn load_cached(&mut self, cx: &mut Context<Self>) {
        let Some(db) = self.db(cx) else {
            return;
        };
        let (repo, number) = (self.repo.clone(), self.number);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let cached = Tokio::spawn(
                &*cx,
                async move { db.load_issue_detail(&repo, number).await },
            )
            .await;
            this.update(cx, |this, cx| match cached {
                Ok(Ok(Some(detail))) if this.detail.loaded().is_none() => {
                    this.detail = Loadable::Loaded(detail);
                    cx.notify();
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%error, "could not read the cached issue"),
                Err(error) => tracing::warn!(%error, "issue cache read did not complete"),
            })
            .ok();
        }));
    }

    fn load_detail(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client(cx) else {
            return;
        };
        // Keep showing what is there while reloading; only a first load
        // shows the spinner.
        if self.detail.loaded().is_none() {
            self.detail = Loadable::Loading;
        }
        cx.notify();

        let (repo, number) = (self.repo.clone(), self.number);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(
                &*cx,
                async move { client.issue_detail(&repo, number).await },
            )
            .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(detail)) => {
                        tracing::debug!(
                            items = detail.conversation.items.len(),
                            state = ?detail.issue.state,
                            "issue loaded"
                        );
                        this.cache_detail(&detail, cx);
                        this.detail = Loadable::Loaded(detail);
                    }
                    // A failed reload keeps the copy on screen and says why
                    // in the banner; a failed first load replaces the spinner.
                    Ok(Err(error)) => this.load_failed(error.to_string()),
                    Err(error) => this.load_failed(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn load_failed(&mut self, message: String) {
        tracing::warn!(repo = %self.repo, issue = %self.number, %message, "issue load failed");
        if self.detail.loaded().is_some() {
            self.error = Some(message);
        } else {
            self.detail = Loadable::Failed(message);
        }
    }

    fn cache_detail(&self, detail: &IssueDetail, cx: &mut Context<Self>) {
        let Some(db) = self.db(cx) else {
            return;
        };
        let repo = self.repo.clone();
        let snapshot = detail.clone();
        Tokio::spawn(cx, async move {
            if let Err(error) = db.save_issue_detail(&repo, &snapshot).await {
                tracing::warn!(%repo, %error, "could not cache issue");
            }
        })
        .detach();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_detail(cx);
    }

    /// Open `which` picker, or close it if it is already open, fetching what
    /// it lists the first time.
    fn toggle_picker(&mut self, which: OpenPicker, cx: &mut Context<Self>) {
        self.picker = if self.picker == Some(which) {
            None
        } else {
            Some(which)
        };
        match self.picker {
            Some(OpenPicker::Labels) if self.repo_labels.is_idle() => {
                self.repo_labels = Loadable::Loading;
                let repo = self.repo.clone();
                self.spawn_list_load(
                    cx,
                    move |client| async move { client.repository_labels(&repo).await },
                    |this, labels| this.repo_labels = labels,
                );
            }
            Some(OpenPicker::Assignees) if self.assignable.is_idle() => {
                self.assignable = Loadable::Loading;
                let repo = self.repo.clone();
                self.spawn_list_load(
                    cx,
                    move |client| async move { client.assignable_users(&repo).await },
                    |this, users| this.assignable = users,
                );
            }
            _ => {}
        }
        cx.notify();
    }

    /// Fetch one picker's list and store it with `apply`.
    fn spawn_list_load<T, F, Fut>(
        &mut self,
        cx: &mut Context<Self>,
        fetch: F,
        apply: impl FnOnce(&mut Self, Loadable<Vec<T>>) + 'static,
    ) where
        T: Send + 'static,
        F: FnOnce(GitHubClient) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<T>, rostrum_github::GitHubError>>
            + Send
            + 'static,
    {
        let Some(client) = self.client(cx) else {
            apply(self, Loadable::Failed("Not authenticated".into()));
            return;
        };
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, fetch(client)).await;
            this.update(cx, |this, cx| {
                apply(
                    this,
                    match result {
                        Ok(Ok(items)) => Loadable::Loaded(items),
                        Ok(Err(error)) => Loadable::Failed(error.to_string()),
                        Err(error) => Loadable::Failed(error.to_string()),
                    },
                );
                cx.notify();
            })
            .ok();
        }));
    }

    // --- mutations ---------------------------------------------------------

    /// Send one change, then reload authoritatively: the pane redraws from
    /// GitHub's answer, and the feed refreshes this repository's issues so a
    /// close or reopen moves the row.
    ///
    /// The same contract as the pull request pane's `mutate`: one in flight
    /// at a time, failures in the banner, nothing patched locally.
    fn mutate(&mut self, mutation: IssueMutation, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(client) = self.client(cx) else {
            self.error = Some("Not authenticated".into());
            cx.notify();
            return;
        };

        let label = mutation.progress_label();
        self.busy = Some(label);
        self.error = None;
        cx.notify();
        tracing::info!(repo = %self.repo, issue = %self.number, action = label, "issue mutation");

        let (repo, number) = (self.repo.clone(), self.number);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, async move {
                client.mutate_issue(&repo, number, &mutation).await
            })
            .await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(())) => {
                        this.load_detail(cx);
                        let repo = this.repo.clone();
                        this.store
                            .update(cx, |store, cx| store.refresh_issues(repo, cx));
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string()),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn post_comment(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Ok(body) = CommentBody::new(self.composer.read(cx).text().to_string()) else {
            return;
        };
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.mutate(IssueMutation::Comment(body), cx);
    }

    /// Close or reopen. No confirmation: either is undone by the other, and
    /// neither destroys anything.
    fn set_state(&mut self, change: IssueStateChange, cx: &mut Context<Self>) {
        self.mutate(IssueMutation::SetState(change), cx);
    }

    fn toggle_label(&mut self, name: String, applied: bool, cx: &mut Context<Self>) {
        self.mutate(
            if applied {
                IssueMutation::RemoveLabel(name)
            } else {
                IssueMutation::AddLabels(AddLabels::new([name]))
            },
            cx,
        );
    }

    fn toggle_assignee(&mut self, login: String, assigned: bool, cx: &mut Context<Self>) {
        let people = Assignees::new([login]);
        self.mutate(
            if assigned {
                IssueMutation::RemoveAssignees(people)
            } else {
                IssueMutation::AddAssignees(people)
            },
            cx,
        );
    }

    fn label_toggle(cx: &Context<Self>) -> Toggle {
        let entity = cx.entity();
        Rc::new(move |name, applied, cx: &mut App| {
            entity.update(cx, |this, cx| this.toggle_label(name, applied, cx));
        })
    }

    fn assignee_toggle(cx: &Context<Self>) -> Toggle {
        let entity = cx.entity();
        Rc::new(move |login, assigned, cx: &mut App| {
            entity.update(cx, |this, cx| this.toggle_assignee(login, assigned, cx));
        })
    }
}
