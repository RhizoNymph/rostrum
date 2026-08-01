//! Detail pane for one pull request: conversation, files, and checks.
//!
//! A fresh `PrDetail` entity is built whenever the selection changes. Dropping
//! the previous one cancels its in-flight tasks, so a slow response for an
//! earlier pull request can never land in the current one.

mod checks;
mod conversation;
mod files;

use std::{collections::HashSet, rc::Rc};

use futures::future::BoxFuture;
use gpui::{
    App, Context, Entity, ListAlignment, ListState, Subscription, Task, Window, div, prelude::*,
    px, rems,
};
use gpui_tokio::Tokio;
use rostrum_core::{Conversation, Mergeable, PrNumber, PullRequest, RepoId, ReviewDecision, Side};
use rostrum_diff::{DiffFile, FileStatus, Highlighter, PatchAvailability, parse_patch};
use rostrum_github::{
    DraftComment, GitHubClient, GitHubError, IssueState, MergeMethod, PullRequestFile, ReviewEvent,
    SubmitReview,
};
use rostrum_ui::{
    ActiveTheme, TextInput,
    components::{
        Button, ButtonStyle, Chip, DiffStat, Initial, Tab, h_flex, hex_color, tab_bar, v_flex,
    },
};

use crate::sync::Store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    Conversation,
    Files,
    Checks,
}

impl DetailTab {
    const ALL: [Self; 3] = [Self::Conversation, Self::Files, Self::Checks];

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    fn from_index(ix: usize) -> Self {
        Self::ALL.get(ix).copied().unwrap_or(Self::Conversation)
    }
}

/// Async resource with an explicit failure state, so the UI can tell "still
/// loading" from "loaded and empty" from "failed".
pub enum Loadable<T> {
    Idle,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            Self::Loaded(value) => Some(value),
            _ => None,
        }
    }

    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// An outward-facing action, held until the user confirms it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Confirm {
    Merge(MergeMethod),
    Close,
}

impl Confirm {
    fn prompt(&self, pull: &PullRequest) -> String {
        match self {
            Self::Merge(method) => format!(
                "{} {} into {}?",
                match method {
                    MergeMethod::Merge => "Merge",
                    MergeMethod::Squash => "Squash and merge",
                    MergeMethod::Rebase => "Rebase and merge",
                },
                pull.number,
                pull.base_ref
            ),
            Self::Close => format!("Close {} without merging?", pull.number),
        }
    }
}

/// Where an inline comment will be attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftAnchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
}

pub struct PrDetail {
    pub(crate) store: Entity<Store>,
    pub(crate) repo: RepoId,
    pub(crate) number: PrNumber,
    tab: DetailTab,
    pub(crate) conversation: Loadable<Conversation>,
    pub(crate) files: Loadable<Vec<DiffFile>>,
    composer: Entity<TextInput>,
    /// Comments drafted against the diff but not yet submitted, i.e. GitHub's
    /// pending-review model held locally until the review is sent.
    pub(crate) pending: Vec<DraftComment>,
    /// Open inline composer and the anchor it will attach to.
    pub(crate) inline: Option<(DraftAnchor, Entity<TextInput>)>,
    /// Open reply composer, keyed by the comment id it replies to.
    pub(crate) reply: Option<(u64, Entity<TextInput>)>,
    /// Files collapsed in the diff view, by index.
    pub(crate) collapsed: HashSet<usize>,
    /// Flattened diff rows and the list that renders them.
    pub(crate) diff_rows: Rc<Vec<files::DiffRow>>,
    pub(crate) diff_list: ListState,
    confirm: Option<Confirm>,
    /// Label of an in-flight mutation; also blocks duplicate submission.
    busy: Option<&'static str>,
    error: Option<String>,
    pub(crate) highlighter: Rc<Highlighter>,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PrDetail {
    pub fn new(
        store: Entity<Store>,
        repo: RepoId,
        number: PrNumber,
        highlighter: Rc<Highlighter>,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| TextInput::new("Leave a comment…", cx).lines(3, 10));
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];

        let mut detail = Self {
            store,
            repo,
            number,
            tab: DetailTab::Conversation,
            conversation: Loadable::Idle,
            files: Loadable::Idle,
            composer,
            pending: Vec::new(),
            inline: None,
            reply: None,
            collapsed: HashSet::new(),
            diff_rows: Rc::new(Vec::new()),
            diff_list: ListState::new(0, ListAlignment::Top, px(600.)),
            confirm: None,
            busy: None,
            error: None,
            highlighter,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        tracing::debug!(repo = %detail.repo, pr = %detail.number, "opened pull request");
        detail.load_conversation(cx);
        detail
    }

    /// Adapt an entity mutation into the click-handler shape buttons expect.
    pub(crate) fn on_click(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        let entity = cx.entity();
        move |_event, _window, cx| {
            entity.update(cx, |this, cx| f(this, cx));
        }
    }

    fn client(&self, cx: &Context<Self>) -> Option<GitHubClient> {
        self.store.read(cx).client()
    }

    fn pull(&self, cx: &Context<Self>) -> Option<PullRequest> {
        let store = self.store.read(cx);
        let repo = store.state.repo(&self.repo)?;
        repo.prs.iter().find(|pr| pr.number == self.number).cloned()
    }

    // --- loading -----------------------------------------------------------

    fn load_conversation(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client(cx) else {
            return;
        };
        self.conversation = Loadable::Loading;
        cx.notify();

        let repo = self.repo.clone();
        let number = self.number;
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(
                &*cx,
                async move { client.conversation(&repo, number).await },
            )
            .await;
            this.update(cx, |this, cx| {
                this.conversation = match result {
                    Ok(Ok(conversation)) => {
                        tracing::debug!(
                            items = conversation.items.len(),
                            threads = conversation.threads.len(),
                            checks = conversation.checks.len(),
                            "conversation loaded"
                        );
                        Loadable::Loaded(conversation)
                    }
                    Ok(Err(err)) => Loadable::Failed(err.to_string()),
                    Err(err) => Loadable::Failed(err.to_string()),
                };
                // Threads are interleaved into the diff, so new conversation
                // data changes the diff row stream too.
                this.rebuild_diff_rows(cx);
            })
            .ok();
        }));
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client(cx) else {
            return;
        };
        self.files = Loadable::Loading;
        cx.notify();

        let repo = self.repo.clone();
        let number = self.number;
        self.tasks.push(cx.spawn(async move |this, cx| {
            // Fetch and parse together, off the main thread: patch parsing is
            // pure CPU work and a large pull request has a lot of it.
            let result = Tokio::spawn(&*cx, async move {
                client
                    .files(&repo, number)
                    .await
                    .map(|files| files.into_iter().map(to_diff_file).collect::<Vec<_>>())
            })
            .await;

            this.update(cx, |this, cx| {
                this.files = match result {
                    Ok(Ok(files)) => {
                        tracing::debug!(
                            files = files.len(),
                            hunks = files.iter().map(|f| f.hunks.len()).sum::<usize>(),
                            "diff loaded"
                        );
                        Loadable::Loaded(files)
                    }
                    Ok(Err(err)) => Loadable::Failed(err.to_string()),
                    Err(err) => Loadable::Failed(err.to_string()),
                };
                this.rebuild_diff_rows(cx);
            })
            .ok();
        }));
    }

    fn select_tab(&mut self, tab: DetailTab, cx: &mut Context<Self>) {
        self.tab = tab;
        // Tabs load lazily, and only once.
        if tab == DetailTab::Files && self.files.is_idle() {
            self.load_files(cx);
        }
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_conversation(cx);
        if !self.files.is_idle() {
            self.load_files(cx);
        }
    }

    // --- mutations ---------------------------------------------------------

    /// Run a mutation, then reload authoritatively rather than patching local
    /// state field by field.
    fn mutate<F>(&mut self, label: &'static str, cx: &mut Context<Self>, call: F)
    where
        F: FnOnce(GitHubClient, RepoId, PrNumber) -> BoxFuture<'static, Result<(), GitHubError>>
            + Send
            + 'static,
    {
        if self.busy.is_some() {
            return;
        }
        let Some(client) = self.client(cx) else {
            self.error = Some("Not authenticated".into());
            cx.notify();
            return;
        };

        self.busy = Some(label);
        self.error = None;
        cx.notify();

        let repo = self.repo.clone();
        let number = self.number;
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, async move { call(client, repo, number).await }).await;

            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(())) => {
                        this.error = None;
                        this.load_conversation(cx);
                        let repo = this.repo.clone();
                        this.store
                            .update(cx, |store, cx| store.refresh_repo(repo, cx));
                    }
                    Ok(Err(err)) => this.error = Some(err.to_string()),
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn post_comment(&mut self, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).text().trim().to_string();
        if body.is_empty() {
            return;
        }
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.mutate("Commenting", cx, move |client, repo, number| {
            Box::pin(async move { client.add_comment(&repo, number, &body).await })
        });
    }

    fn submit_review(&mut self, event: ReviewEvent, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).text().trim().to_string();
        let comments = std::mem::take(&mut self.pending);
        let review = SubmitReview::new(event, body).with_comments(comments);

        if review.is_empty() {
            self.error = Some("Write a comment or add inline feedback first".into());
            cx.notify();
            return;
        }

        self.composer.update(cx, |input, cx| input.clear(cx));
        self.mutate("Submitting review", cx, move |client, repo, number| {
            Box::pin(async move { client.submit_review(&repo, number, review).await })
        });
    }

    pub(crate) fn reply_to_thread(
        &mut self,
        in_reply_to: u64,
        body: String,
        cx: &mut Context<Self>,
    ) {
        if body.trim().is_empty() {
            return;
        }
        self.mutate("Replying", cx, move |client, repo, number| {
            Box::pin(async move {
                client
                    .reply_to_thread(&repo, number, in_reply_to, &body)
                    .await
            })
        });
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.confirm.take() else {
            return;
        };
        match action {
            Confirm::Merge(method) => {
                self.mutate("Merging", cx, move |client, repo, number| {
                    Box::pin(async move { client.merge(&repo, number, method).await })
                });
            }
            Confirm::Close => self.mutate("Closing", cx, move |client, repo, number| {
                Box::pin(async move { client.set_state(&repo, number, IssueState::Closed).await })
            }),
        }
    }

    // --- inline drafts -----------------------------------------------------

    pub(crate) fn open_inline_composer(&mut self, anchor: DraftAnchor, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Comment on this line…", cx).lines(2, 8));
        self.inline = Some((anchor, input));
        self.rebuild_diff_rows(cx);
    }

    pub(crate) fn commit_inline_draft(&mut self, cx: &mut Context<Self>) {
        let Some((anchor, input)) = self.inline.take() else {
            return;
        };
        let body = input.read(cx).text().trim().to_string();
        if !body.is_empty() {
            self.pending.push(DraftComment::single(
                anchor.path,
                anchor.line,
                anchor.side,
                body,
            ));
        }
        self.rebuild_diff_rows(cx);
    }

    pub(crate) fn discard_inline_draft(&mut self, cx: &mut Context<Self>) {
        self.inline = None;
        self.rebuild_diff_rows(cx);
    }

    pub(crate) fn discard_draft(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.pending.len() {
            self.pending.remove(ix);
        }
        self.rebuild_diff_rows(cx);
    }

    pub(crate) fn open_reply(&mut self, target: u64, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Reply…", cx).lines(2, 8));
        self.reply = Some((target, input));
        cx.notify();
    }

    pub(crate) fn toggle_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&ix) {
            self.collapsed.insert(ix);
        }
        self.rebuild_diff_rows(cx);
    }

    /// Rebuild the flattened diff stream.
    ///
    /// The list's item count must track the row vector exactly, so every
    /// mutation of the rows goes through here and its matching `splice`.
    pub(crate) fn rebuild_diff_rows(&mut self, cx: &mut Context<Self>) {
        let rows = files::build_rows(self);
        if rows != *self.diff_rows {
            self.diff_list.splice(0..self.diff_rows.len(), rows.len());
            self.diff_rows = Rc::new(rows);
        }
        cx.notify();
    }

    // --- rendering ---------------------------------------------------------

    fn render_header(&self, pull: &PullRequest, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let author = pull.author.as_ref().map(|a| a.login.clone());

        v_flex()
            .gap_2()
            .flex_none()
            .p_4()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(rems(1.1))
                    .text_color(theme.text)
                    .child(pull.title.clone()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .text_size(rems(0.76))
                    .text_color(theme.text_muted)
                    .child(format!("{} {}", self.repo, pull.number))
                    .when_some(author, |el, login| {
                        el.child(Initial::new(login.clone())).child(login)
                    })
                    .child(DiffStat::new(pull.additions, pull.deletions))
                    .child(format!("{} → {}", pull.head_ref, pull.base_ref)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .when(pull.is_draft, |el| {
                        el.child(Chip::new("draft").color(theme.draft))
                    })
                    .when_some(review_chip(pull.review_decision), |el, (text, color)| {
                        el.child(Chip::new(text).color(color(&theme)))
                    })
                    .children(pull.labels.iter().map(|label| {
                        let chip = Chip::new(label.name.clone());
                        match hex_color(&label.color) {
                            Some(color) => chip.color(color),
                            None => chip,
                        }
                    })),
            )
    }

    fn render_actions(&self, pull: &PullRequest, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let conflicting = pull.mergeable == Mergeable::Conflicting;
        let unknown = pull.mergeable == Mergeable::Unknown;
        let busy = self.busy.is_some();
        let pending = self.pending.len();

        v_flex()
            .gap_2()
            .flex_none()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .when_some(self.error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
            .when_some(self.busy, |el, label| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.text_subtle)
                        .child(format!("{label}…")),
                )
            })
            .when_some(self.confirm.clone(), |el, action| {
                el.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .bg(theme.surface_raised)
                        .child(
                            div()
                                .flex_1()
                                .text_size(rems(0.78))
                                .text_color(theme.text)
                                .child(action.prompt(pull)),
                        )
                        .child(
                            Button::new("confirm-yes", "Confirm")
                                .style(ButtonStyle::Danger)
                                .on_click(Self::on_click(cx, |this, cx| this.confirmed(cx))),
                        )
                        .child(Button::new("confirm-no", "Cancel").on_click(Self::on_click(
                            cx,
                            |this, cx| {
                                this.confirm = None;
                                cx.notify();
                            },
                        ))),
                )
            })
            .when(pending > 0, |el| {
                el.child(h_flex().gap_2().child(
                    Chip::new(format!("{pending} pending inline comment(s)")).color(theme.warning),
                ))
            })
            .child(self.composer.clone())
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("comment", "Comment")
                            .disabled(busy)
                            .on_click(Self::on_click(cx, |this, cx| this.post_comment(cx))),
                    )
                    .child(
                        Button::new("approve", "Approve")
                            .style(ButtonStyle::Primary)
                            .disabled(busy)
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.submit_review(ReviewEvent::Approve, cx)
                            })),
                    )
                    .child(
                        Button::new("request-changes", "Request changes")
                            .disabled(busy)
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.submit_review(ReviewEvent::RequestChanges, cx)
                            })),
                    )
                    .child(
                        Button::new("merge", "Merge")
                            .style(ButtonStyle::Primary)
                            .disabled(busy || conflicting || unknown)
                            .tooltip(if conflicting {
                                "This branch has conflicts that must be resolved"
                            } else if unknown {
                                "GitHub is still computing the merge state"
                            } else {
                                "Merge this pull request"
                            })
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.confirm = Some(Confirm::Merge(MergeMethod::Merge));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close", "Close")
                            .style(ButtonStyle::Danger)
                            .disabled(busy)
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.confirm = Some(Confirm::Close);
                                cx.notify();
                            })),
                    ),
            )
    }
}

impl Render for PrDetail {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(pull) = self.pull(cx) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_subtle)
                .child("This pull request is no longer open")
                .into_any_element();
        };

        let unresolved = self
            .conversation
            .loaded()
            .map_or(0, Conversation::unresolved_thread_count);
        let file_count = self.files.loaded().map_or(0, Vec::len);
        let check_count = self.conversation.loaded().map_or(0, |c| c.checks.len());

        let with_badge = |tab: Tab, count: usize| if count > 0 { tab.badge(count) } else { tab };
        let tabs = vec![
            with_badge(Tab::new("Conversation"), unresolved),
            with_badge(Tab::new("Files"), file_count),
            with_badge(Tab::new("Checks"), check_count),
        ];

        let entity = cx.entity();
        let body = match self.tab {
            DetailTab::Conversation => conversation::render(self, cx),
            DetailTab::Files => files::render(self, cx),
            DetailTab::Checks => checks::render(self, cx),
        };

        v_flex()
            .size_full()
            .child(self.render_header(&pull, cx))
            .child(div().flex_none().child(tab_bar(
                tabs,
                self.tab.index(),
                cx,
                move |ix, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.select_tab(DetailTab::from_index(ix), cx);
                    });
                },
            )))
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .child(self.render_actions(&pull, cx))
            .into_any_element()
    }
}

fn to_diff_file(file: PullRequestFile) -> DiffFile {
    let (hunks, availability) = match file.patch.as_deref() {
        Some(patch) => match parse_patch(patch) {
            Ok(hunks) => (hunks, PatchAvailability::Present),
            Err(error) => {
                tracing::warn!(path = %file.filename, %error, "could not parse patch");
                (Vec::new(), PatchAvailability::Truncated)
            }
        },
        None => (Vec::new(), PatchAvailability::Omitted),
    };

    DiffFile {
        path: file.filename,
        previous_path: file.previous_filename,
        status: FileStatus::from_api(&file.status),
        additions: file.additions,
        deletions: file.deletions,
        hunks,
        availability,
    }
}

type ThemeColor = fn(&rostrum_ui::Theme) -> gpui::Hsla;

fn review_chip(decision: Option<ReviewDecision>) -> Option<(&'static str, ThemeColor)> {
    match decision? {
        ReviewDecision::Approved => Some(("approved", |t| t.success)),
        ReviewDecision::ChangesRequested => Some(("changes requested", |t| t.danger)),
        ReviewDecision::ReviewRequired => Some(("review required", |t| t.text_muted)),
    }
}
