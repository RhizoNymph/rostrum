//! Detail pane for one pull request: conversation, files, and checks.
//!
//! A fresh `PrDetail` entity is built whenever the selection changes. Dropping
//! the previous one cancels its in-flight tasks, so a slow response for an
//! earlier pull request can never land in the current one.

mod checks;
mod conversation;
mod files;

use std::{collections::HashSet, rc::Rc, sync::Arc};

use futures::future::BoxFuture;
use gpui::{
    App, Context, Entity, Hsla, ListAlignment, ListState, Subscription, Task, Window, div,
    prelude::*, px, rems,
};
use gpui_tokio::Tokio;
use rostrum_core::{Conversation, Label, PrNumber, PullRequest, RepoId, ReviewDecision, Side};
use rostrum_db::Db;
use rostrum_diff::{DiffFile, FileStatus, Highlighter, PatchAvailability, parse_patch};
use rostrum_github::{
    DraftComment, GitHubClient, GitHubError, IssueState, MergeMethod, PullRequestFile, ReviewEvent,
    SubmitReview,
};
use rostrum_ui::{
    ActiveTheme, TextInput,
    components::{
        Button, ButtonStyle, Chip, DiffStat, Dot, Initial, Tab, h_flex, hex_color, tab_bar, v_flex,
    },
};

use crate::sync::Store;

gpui::actions!(detail, [CopySelection]);

/// Key bindings for the detail pane. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("ctrl-c", CopySelection, Some("Detail")),
        gpui::KeyBinding::new("cmd-c", CopySelection, Some("Detail")),
    ]);
}

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
///
/// `start_line`/`start_side` are set only for a multi-line selection; GitHub
/// wants them omitted entirely for a single-line comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftAnchor {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start_line: Option<u32>,
    pub start_side: Option<Side>,
}

impl DraftAnchor {
    pub fn single(path: impl Into<String>, line: u32, side: Side) -> Self {
        Self {
            path: path.into(),
            line,
            side,
            start_line: None,
            start_side: None,
        }
    }

    /// Extend an anchor to cover the range between it and `line`.
    ///
    /// GitHub requires `start_line <= line`, so the two are ordered here rather
    /// than trusting the click order.
    pub fn extended_to(&self, line: u32, side: Side) -> Self {
        let (start, end) = if line < self.anchor_start() {
            (line, self.line)
        } else {
            (self.anchor_start(), line)
        };
        Self {
            path: self.path.clone(),
            line: end,
            side,
            start_line: (start != end).then_some(start),
            start_side: (start != end).then_some(side),
        }
    }

    fn anchor_start(&self) -> u32 {
        self.start_line.unwrap_or(self.line)
    }

    /// Whether `line` on `side` falls inside this anchor.
    pub fn covers(&self, path: &str, line: u32, side: Side) -> bool {
        self.path == path && self.side == side && (self.anchor_start()..=self.line).contains(&line)
    }
}

pub struct PrDetail {
    pub(crate) store: Entity<Store>,
    pub(crate) repo: RepoId,
    pub(crate) number: PrNumber,
    tab: DetailTab,
    pub(crate) conversation: Loadable<Conversation>,
    pub(crate) files: Loadable<Vec<DiffFile>>,
    /// Every label defined on the repository — the picker's palette, not the
    /// labels on this pull request. Fetched on first open of the picker.
    pub(crate) repo_labels: Loadable<Vec<Label>>,
    /// Whether the label picker panel is showing.
    label_picker_open: bool,
    composer: Entity<TextInput>,
    /// Comments drafted against the diff but not yet submitted, i.e. GitHub's
    /// pending-review model held locally until the review is sent.
    pub(crate) pending: Vec<DraftComment>,
    /// Open inline composer and the anchor it will attach to.
    pub(crate) inline: Option<(DraftAnchor, Entity<TextInput>)>,
    /// Commit the pending drafts were written against. A force-push changes
    /// the head sha, which invalidates every anchor.
    pub(crate) pending_head_sha: Option<String>,
    /// Open reply composer, keyed by the comment id it replies to.
    pub(crate) reply: Option<(u64, Entity<TextInput>)>,
    /// Files collapsed in the diff view, by index.
    pub(crate) collapsed: HashSet<usize>,
    /// Selected run of diff lines, for copying.
    pub(crate) line_selection: Option<files::LineSelection>,
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
            repo_labels: Loadable::Idle,
            label_picker_open: false,
            composer,
            pending: Vec::new(),
            inline: None,
            pending_head_sha: None,
            reply: None,
            collapsed: HashSet::new(),
            line_selection: None,
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
        detail.load_cached(cx);
        detail.load_conversation(cx);
        detail
    }

    fn db(&self, cx: &Context<Self>) -> Option<Arc<Db>> {
        self.store.read(cx).db()
    }

    /// Paint from the cache while the network request is in flight, and restore
    /// any review drafted in an earlier session.
    fn load_cached(&mut self, cx: &mut Context<Self>) {
        let Some(db) = self.db(cx) else {
            return;
        };
        let repo = self.repo.clone();
        let number = self.number;

        self.tasks.push(cx.spawn(async move |this, cx| {
            let loaded = Tokio::spawn(&*cx, async move {
                let conversation = db.load_conversation(&repo, number).await?;
                let drafts = db.load_drafts(&repo, number).await?;
                Ok::<_, rostrum_db::DbError>((conversation, drafts))
            })
            .await;

            this.update(cx, |this, cx| {
                match loaded {
                    Ok(Ok((conversation, drafts))) => {
                        // Only fill gaps: a network response that already
                        // arrived is always newer than the cache.
                        if let Some(conversation) = conversation
                            && this.conversation.loaded().is_none()
                        {
                            this.conversation = Loadable::Loaded(conversation);
                        }
                        if let Some(set) = drafts
                            && this.pending.is_empty()
                        {
                            tracing::debug!(count = set.comments.len(), "restored drafts");
                            this.pending = set.comments;
                            this.pending_head_sha = Some(set.head_sha);
                        }
                        this.rebuild_diff_rows(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "could not read the cached pull request")
                    }
                    Err(error) => tracing::warn!(%error, "cache read did not complete"),
                }
            })
            .ok();
        }));
    }

    /// Write the pending review to disk. Drafts are the user's unsent work, so
    /// every change to them is persisted immediately.
    fn persist_drafts(&self, cx: &Context<Self>) {
        let Some(db) = self.db(cx) else {
            return;
        };
        let repo = self.repo.clone();
        let number = self.number;
        let head_sha = self.pending_head_sha.clone().unwrap_or_default();
        let drafts = self.pending.clone();

        Tokio::spawn(cx, async move {
            let result = if drafts.is_empty() {
                db.clear_drafts(&repo, number).await
            } else {
                db.save_drafts(&repo, number, &head_sha, &drafts).await
            };
            if let Err(error) = result {
                tracing::warn!(%repo, %error, "could not persist review drafts");
            }
        })
        .detach();
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

    /// Like [`Self::on_click`] but forwards the click event, for handlers that
    /// care about modifier keys.
    pub(crate) fn on_click_with(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &gpui::ClickEvent, &mut Context<Self>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        let entity = cx.entity();
        move |event, _window, cx| {
            let event = event.clone();
            entity.update(cx, |this, cx| f(this, &event, cx));
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
                        if let Some(db) = this.db(cx) {
                            let (repo, number) = (this.repo.clone(), this.number);
                            let snapshot = conversation.clone();
                            Tokio::spawn(&*cx, async move {
                                if let Err(error) =
                                    db.save_conversation(&repo, number, &snapshot).await
                                {
                                    tracing::warn!(%error, "could not cache conversation");
                                }
                            })
                            .detach();
                        }
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

    /// Cache key for a pull request's diff.
    fn files_cache_key(&self) -> String {
        format!("files:{}{}", self.repo, self.number)
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client(cx) else {
            return;
        };
        self.files = Loadable::Loading;
        cx.notify();

        let repo = self.repo.clone();
        let number = self.number;
        let db = self.db(cx);
        // A pull request's diff is a pure function of its head commit, so the
        // head sha is a stronger validator than an HTTP ETag: if it has not
        // moved, the diff cannot have changed, and no request is needed.
        let head_sha = self.pull(cx).map(|pull| pull.head_sha).unwrap_or_default();
        let key = self.files_cache_key();

        self.tasks.push(cx.spawn(async move |this, cx| {
            // Fetch and parse together, off the main thread: patch parsing is
            // pure CPU work and a large pull request has a lot of it.
            let result = Tokio::spawn(&*cx, async move {
                if let Some(db) = db.as_ref()
                    && !head_sha.is_empty()
                    && let Ok(Some(cached)) = db.load_etag(&key).await
                    && cached.etag == head_sha
                    && let Ok(files) = serde_json::from_str::<Vec<PullRequestFile>>(&cached.body)
                {
                    tracing::debug!(%repo, "diff served from cache");
                    return Ok::<Vec<DiffFile>, GitHubError>(
                        files.into_iter().map(to_diff_file).collect(),
                    );
                }

                let fetched = client.files(&repo, number).await?;

                if let Some(db) = db.as_ref()
                    && !head_sha.is_empty()
                    && let Ok(body) = serde_json::to_string(&fetched)
                    && let Err(error) = db.save_etag(&key, &head_sha, &body).await
                {
                    tracing::warn!(%error, "could not cache diff");
                }

                Ok(fetched.into_iter().map(to_diff_file).collect::<Vec<_>>())
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

    /// Fetch the repository's label palette.
    ///
    /// Only the picker needs this, and most pull requests are opened without
    /// ever touching it, so it is never fetched on open.
    fn load_repository_labels(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client(cx) else {
            return;
        };
        self.repo_labels = Loadable::Loading;
        cx.notify();

        let repo = self.repo.clone();
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result =
                Tokio::spawn(&*cx, async move { client.repository_labels(&repo).await }).await;
            this.update(cx, |this, cx| {
                this.repo_labels = match result {
                    Ok(Ok(labels)) => {
                        tracing::debug!(count = labels.len(), "repository labels loaded");
                        Loadable::Loaded(labels)
                    }
                    Ok(Err(err)) => Loadable::Failed(err.to_string()),
                    Err(err) => Loadable::Failed(err.to_string()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    /// Show or hide the label picker, loading the palette the first time it is
    /// opened.
    fn toggle_label_picker(&mut self, cx: &mut Context<Self>) {
        self.label_picker_open = !self.label_picker_open;
        if self.label_picker_open && self.repo_labels.is_idle() {
            self.load_repository_labels(cx);
        }
        cx.notify();
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
        self.pending_head_sha = None;
        self.persist_drafts(cx);
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

    /// Apply or remove one label, whichever `applied` says is the current state.
    ///
    /// Both directions go through [`Self::mutate`], so the in-flight guard, the
    /// error banner, and the authoritative reload all apply: the chips redraw
    /// from GitHub's answer rather than from a guess made here.
    fn toggle_label(&mut self, name: String, applied: bool, cx: &mut Context<Self>) {
        if applied {
            self.mutate("Removing label", cx, move |client, repo, number| {
                Box::pin(async move { client.remove_label(&repo, number, &name).await })
            });
        } else {
            self.mutate("Adding label", cx, move |client, repo, number| {
                Box::pin(async move { client.add_labels(&repo, number, &[name]).await })
            });
        }
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

    /// Open a composer on a line, or extend the open one into a range when
    /// `extend` is set (shift-click) and the line is on the same file and side.
    pub(crate) fn open_inline_composer(
        &mut self,
        anchor: DraftAnchor,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        if extend
            && let Some((open, input)) = self.inline.take()
            && open.path == anchor.path
            && open.side == anchor.side
        {
            self.inline = Some((open.extended_to(anchor.line, anchor.side), input));
            self.rebuild_diff_rows(cx);
            return;
        }

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
            // Tag the batch with the commit it was written against, so a
            // force-push before submission can be detected.
            if self.pending.is_empty() {
                self.pending_head_sha = self.pull(cx).map(|pull| pull.head_sha);
            }
            self.pending.push(DraftComment {
                path: anchor.path,
                line: anchor.line,
                side: anchor.side,
                start_line: anchor.start_line,
                start_side: anchor.start_side,
                body,
            });
        }
        self.persist_drafts(cx);
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
        if self.pending.is_empty() {
            self.pending_head_sha = None;
        }
        self.persist_drafts(cx);
        self.rebuild_diff_rows(cx);
    }

    pub(crate) fn discard_all_drafts(&mut self, cx: &mut Context<Self>) {
        self.pending.clear();
        self.pending_head_sha = None;
        self.inline = None;
        self.persist_drafts(cx);
        self.rebuild_diff_rows(cx);
    }

    /// Whether the pull request has moved since the pending drafts were
    /// written, which invalidates their line anchors.
    pub(crate) fn drafts_are_stale(&self, pull: &PullRequest) -> bool {
        drafts_are_stale(self.pending_head_sha.as_deref(), &pull.head_sha)
    }

    pub(crate) fn open_reply(&mut self, target: u64, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Reply…", cx).lines(2, 8));
        self.reply = Some((target, input));
        cx.notify();
    }

    /// Select a diff line, or extend the current selection when shift is held.
    pub(crate) fn select_line(&mut self, row: usize, extend: bool, cx: &mut Context<Self>) {
        self.line_selection = Some(match (extend, self.line_selection) {
            (true, Some(selection)) => selection.extended_to(row),
            _ => files::LineSelection::new(row),
        });
        cx.notify();
    }

    /// Copy the selected diff lines. No selection is not an error — the user
    /// pressed copy with nothing selected.
    fn on_copy(&mut self, _: &CopySelection, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.line_selection else {
            return;
        };
        let Some(files) = self.files.loaded() else {
            return;
        };
        let text = files::selected_text(&self.diff_rows, files, selection);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
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
        // A label mutation is a whole-pane operation: while one is in flight the
        // chips still show the pre-mutation truth, so every affordance that
        // would change them is inert until the reload lands.
        let busy = self.busy.is_some();

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
                    .children(
                        pull.labels
                            .iter()
                            .enumerate()
                            .map(|(ix, label)| self.render_label_chip(ix, label, busy, &theme, cx)),
                    )
                    .child(
                        Button::new(
                            "toggle-label-picker",
                            if self.label_picker_open {
                                "Close labels"
                            } else {
                                "Labels…"
                            },
                        )
                        .tooltip("Add or remove labels")
                        .on_click(Self::on_click(cx, |this, cx| this.toggle_label_picker(cx))),
                    ),
            )
            .when(self.label_picker_open, |el| {
                el.child(self.render_label_picker(pull, busy, &theme, cx))
            })
    }

    /// One applied label, with the affordance that takes it off again.
    fn render_label_chip(
        &self,
        ix: usize,
        label: &Label,
        busy: bool,
        theme: &rostrum_ui::Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let color = hex_color(&label.color).unwrap_or(theme.text_muted);
        let name = label.name.clone();
        let danger = theme.danger;

        h_flex()
            .gap_0p5()
            .child(Chip::new(label.name.clone()).color(color))
            .child(
                div()
                    .id(("remove-label", ix))
                    .px_1()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child("×")
                    .when(!busy, |el| {
                        el.cursor_pointer()
                            .hover(move |el| el.text_color(danger))
                            .on_click(Self::on_click(cx, move |this, cx| {
                                this.toggle_label(name.clone(), true, cx)
                            }))
                    }),
            )
    }

    /// The label picker: an inline panel under the header listing every label
    /// the repository defines, each toggled on or off.
    fn render_label_picker(
        &self,
        pull: &PullRequest,
        busy: bool,
        theme: &rostrum_ui::Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let panel = v_flex()
            .gap_1()
            .p_2()
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.border);

        match &self.repo_labels {
            Loadable::Idle | Loadable::Loading => panel.child(
                div()
                    .text_size(rems(0.75))
                    .text_color(theme.text_subtle)
                    .child("Loading labels…"),
            ),
            Loadable::Failed(message) => panel.child(
                div()
                    .text_size(rems(0.75))
                    .text_color(theme.danger)
                    .child(message.clone()),
            ),
            Loadable::Loaded(labels) if labels.is_empty() => panel.child(
                div()
                    .text_size(rems(0.75))
                    .text_color(theme.text_subtle)
                    .child("This repository defines no labels"),
            ),
            Loadable::Loaded(labels) => {
                let applied: HashSet<&str> = pull
                    .labels
                    .iter()
                    .map(|label| label.name.as_str())
                    .collect();

                panel.child(
                    div()
                        .id("label-picker")
                        .max_h(px(200.))
                        .overflow_y_scroll()
                        .child(v_flex().gap_0p5().children(labels.iter().enumerate().map(
                            |(ix, label)| {
                                let is_applied = applied.contains(label.name.as_str());
                                let color = hex_color(&label.color).unwrap_or(theme.text_muted);
                                let name = label.name.clone();
                                let hover_bg = theme.surface;

                                h_flex()
                                    .id(("repo-label", ix))
                                    .gap_2()
                                    .px_1()
                                    .py_0p5()
                                    .child(
                                        div()
                                            .w(px(12.))
                                            .flex_none()
                                            .text_size(rems(0.7))
                                            .text_color(theme.text)
                                            .child(if is_applied { "✓" } else { "" }),
                                    )
                                    .child(Chip::new(label.name.clone()).color(color))
                                    // The mutation is refused while another is
                                    // in flight, so the row must not look live.
                                    .when(busy, |el| el.opacity(0.45))
                                    .when(!busy, |el| {
                                        el.cursor_pointer()
                                            .hover(move |el| el.bg(hover_bg))
                                            .on_click(Self::on_click(cx, move |this, cx| {
                                                this.toggle_label(name.clone(), is_applied, cx)
                                            }))
                                    })
                            },
                        ))),
                )
            }
        }
    }

    fn render_actions(&self, pull: &PullRequest, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let merge = pull.merge_status();
        let busy = self.busy.is_some();
        let pending = self.pending.len();
        let stale = self.drafts_are_stale(pull);

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
                el.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Chip::new(format!("{pending} pending inline comment(s)"))
                                .color(theme.warning),
                        )
                        .child(
                            Button::new("discard-drafts", "Discard")
                                .on_click(Self::on_click(cx, |this, cx| {
                                    this.discard_all_drafts(cx)
                                })),
                        ),
                )
            })
            .when(stale, |el| {
                // A force-push moved the diff out from under these comments.
                // Submitting now would anchor them to lines that have shifted.
                el.child(
                    div()
                        .p_2()
                        .rounded_tl(px(5.))
                        .rounded_tr(px(5.))
                        .rounded_bl(px(5.))
                        .rounded_br(px(5.))
                        .bg(Hsla {
                            a: 0.12,
                            ..theme.danger
                        })
                        .text_size(rems(0.75))
                        .text_color(theme.danger)
                        .child(
                            "This pull request was updated after these comments were drafted. \
                             Their line anchors may no longer be correct — discard them and \
                             re-read the diff before submitting.",
                        ),
                )
            })
            .child(self.composer.clone())
            // Stated in full rather than left to the merge button's tooltip: a
            // disabled button with no visible reason is the state people file
            // bugs about.
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Dot::new(theme.merge_color(merge)))
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_muted)
                            .child(merge.explanation()),
                    ),
            )
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
                            .disabled(busy || stale)
                            .tooltip(if stale {
                                "Discard the stale drafts first"
                            } else {
                                "Submit an approving review"
                            })
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.submit_review(ReviewEvent::Approve, cx)
                            })),
                    )
                    .child(
                        Button::new("request-changes", "Request changes")
                            .disabled(busy || stale)
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.submit_review(ReviewEvent::RequestChanges, cx)
                            })),
                    )
                    .child(
                        Button::new("merge", "Merge")
                            .style(ButtonStyle::Primary)
                            .disabled(busy || merge.blocks_merge())
                            .tooltip(merge.explanation())
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
            .on_action(cx.listener(Self::on_copy))
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

/// A force-push moves the head commit, invalidating every drafted line anchor.
///
/// An unknown sha on either side is treated as "not stale": blocking review
/// submission because we could not read a field would be worse than the risk it
/// guards against.
fn drafts_are_stale(drafted_against: Option<&str>, current: &str) -> bool {
    match drafted_against {
        Some(drafted) => !drafted.is_empty() && !current.is_empty() && drafted != current,
        None => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_written_against_the_current_head_are_fresh() {
        assert!(!drafts_are_stale(Some("abc"), "abc"));
    }

    #[test]
    fn drafts_written_against_an_older_head_are_stale() {
        assert!(drafts_are_stale(Some("abc"), "def"));
    }

    #[test]
    fn no_drafts_is_never_stale() {
        assert!(!drafts_are_stale(None, "abc"));
    }

    /// An unreadable sha must not block the user from submitting.
    #[test]
    fn unknown_shas_do_not_block_submission() {
        assert!(!drafts_are_stale(Some(""), "abc"));
        assert!(!drafts_are_stale(Some("abc"), ""));
    }
}
