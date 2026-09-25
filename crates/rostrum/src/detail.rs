//! Detail pane for one pull request: conversation, files, and checks.
//!
//! A fresh `PrDetail` entity is built whenever the selection changes. Dropping
//! the previous one cancels its in-flight tasks, so a slow response for an
//! earlier pull request can never land in the current one.

mod checks;
mod conversation;
mod files;
mod overview;

use std::{collections::HashSet, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use futures::future::BoxFuture;
use gpui::{
    App, Context, Entity, Hsla, ListAlignment, ListState, Subscription, Task, Window, div,
    prelude::*, px, rems,
};
use gpui_tokio::Tokio;
use rostrum_core::{
    Conversation, Divergence, Label, PrNumber, PullRequest, RepoId, ReviewDecision, Side,
    TimelineItem,
};
use rostrum_db::Db;
use rostrum_diff::{DiffFile, FileStatus, Highlighter, PatchAvailability, parse_patch};
use rostrum_git::{Autostash, BranchName, GitError, InProgress, Operation, RemoteRef, Repo, Rev};
use rostrum_github::{
    BranchUpdateMethod, DraftComment, DraftState, GitHubClient, GitHubError, IssueState,
    MergeMethod, PullRequestFile, ReviewEvent, SubmitReview,
};
use rostrum_ui::{
    ActiveTheme, TextInput,
    components::{
        Button, ButtonStyle, Checkbox, Chip, DiffStat, Dot, Initial, Tab, h_flex, hex_color,
        tab_bar, v_flex,
    },
};

use rostrum_handoff::{PrMeta, session_exists, session_name};

use crate::{
    localops::{LocalJob, LocalOp, LocalResult, run_local_job},
    sync::Store,
};

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

/// How the Files tab presents the diff: the line-by-line diff itself, or the
/// visual overview of where the changes fall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FilesView {
    #[default]
    Diff,
    Overview,
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

/// What the local clone says about this pull request's branch.
///
/// Absent for the great majority of pull requests: the feed is built for
/// reading other people's work, and only a repository the user has configured a
/// clone for can answer any of this. `Loadable::Idle` is therefore the ordinary
/// resting state, not a sign that anything went wrong.
pub(crate) enum LocalState {
    /// The clone exists but no worktree has this branch checked out. Common
    /// in a one-worktree-per-branch layout for pull requests the user is not
    /// working on, so it is a quiet line rather than a failure.
    NotCheckedOut,
    CheckedOut(LocalBranch),
}

pub(crate) struct LocalBranch {
    /// The worktree this branch is checked out in — not necessarily the
    /// configured clone path, which may be any worktree of the repository.
    worktree: PathBuf,
    branch: BranchName,
    remote: RemoteRef,
    /// The local branch measured against its remote counterpart: `ahead` is
    /// work not pushed yet, `behind` is work not pulled yet.
    divergence: Divergence,
    /// Whether the refs these counts came from were refreshed just now. A fetch
    /// that failed leaves real numbers computed from stale refs, which is worth
    /// saying out loud rather than presenting as current.
    fetched: bool,
    /// Why a local action cannot run, if anything is in the way.
    blocker: Option<String>,
    /// A rebase or merge git has started and not finished in this worktree.
    in_progress: Option<InProgress>,
    /// When something is in progress and a handler is configured: whether the
    /// tmux session that was (or would have been) handed the conflict exists.
    handoff: Option<HandoffState>,
}

/// Whether a handed-off conflict still has someone working on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HandoffState {
    Running {
        session: String,
    },
    /// The worktree is mid-operation but no session by the expected name
    /// exists — the harness finished without continuing, or was killed.
    Gone {
        session: String,
    },
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
    /// The local clone's view of this branch, when a clone is configured.
    pub(crate) local: Loadable<LocalState>,
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
    /// Which presentation the Files tab is showing.
    pub(crate) files_view: FilesView,
    /// Selected run of diff lines, for copying.
    pub(crate) line_selection: Option<files::LineSelection>,
    /// Flattened diff rows and the list that renders them.
    pub(crate) diff_rows: Rc<Vec<files::DiffRow>>,
    pub(crate) diff_list: ListState,
    confirm: Option<Confirm>,
    /// Label of an in-flight mutation; also blocks duplicate submission.
    busy: Option<&'static str>,
    error: Option<String>,
    /// Something worth saying that is not a failure — a conflict handed off to
    /// a tmux session. Kept apart from `error` so it is not painted red.
    notice: Option<String>,
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
            local: Loadable::Idle,
            label_picker_open: false,
            composer,
            pending: Vec::new(),
            inline: None,
            pending_head_sha: None,
            reply: None,
            collapsed: HashSet::new(),
            files_view: FilesView::default(),
            line_selection: None,
            diff_rows: Rc::new(Vec::new()),
            diff_list: ListState::new(0, ListAlignment::Top, px(600.)),
            confirm: None,
            busy: None,
            error: None,
            notice: None,
            highlighter,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        tracing::debug!(repo = %detail.repo, pr = %detail.number, "opened pull request");
        detail.load_cached(cx);
        detail.load_conversation(cx);
        detail.load_local(cx);
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

    /// Read the local clone's view of this branch.
    ///
    /// Does nothing at all when no clone is configured for the repository,
    /// which is the common case — the panel simply does not appear. Loaded once
    /// per `PrDetail`, so the fetch it performs is bounded by the selection
    /// changing rather than by a timer.
    ///
    /// The configured path is any worktree of the clone; the branch is looked
    /// up across all of them, because a one-worktree-per-branch layout keeps
    /// `main` at the configured path and every pull request somewhere else.
    ///
    /// The fetch is allowed to fail. Its only job is to make the remote-tracking
    /// ref current; when it cannot — no network, a locked credential — the
    /// counts are still computed, still truthful about what is on disk, and
    /// flagged as unfetched so the panel can say so.
    fn load_local(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.store.read(cx).local_path(&self.repo) else {
            return;
        };
        let Some(head_ref) = self.pull(cx).map(|pull| pull.head_ref.clone()) else {
            return;
        };
        let store = self.store.read(cx);
        let autostash = if store.autostash() {
            Autostash::Enabled
        } else {
            Autostash::Disabled
        };
        let handler_configured = store.conflict_handler().is_some();
        let session = session_name(&self.repo, self.number);

        self.local = Loadable::Loading;
        cx.notify();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, async move {
                let branch = BranchName::new(head_ref)?;
                let clone = Repo::open(&path).await?;
                let Some(repo) = clone.worktree_for(&branch).await? else {
                    return Ok::<_, GitError>(LocalState::NotCheckedOut);
                };
                let remote = RemoteRef::origin(branch.clone());

                let fetched = match repo.fetch(&remote).await {
                    Ok(outcome) => {
                        tracing::debug!(?outcome, "fetched the pull request branch");
                        true
                    }
                    Err(error) => {
                        // Offline is an ordinary state for a desktop app, and
                        // the whole point of the local panel is that it still
                        // answers. Say so in the panel, not in an error banner.
                        tracing::debug!(%error, "could not fetch; using the refs already on disk");
                        false
                    }
                };

                let divergence = repo
                    .divergence(&Rev::Local(branch.clone()), &Rev::Remote(remote.clone()))
                    .await?;

                let status = repo.status().await?;
                let in_progress = status.in_progress;

                let blocker = repo
                    .preflight(Operation::PullRebase, Some(&branch), autostash)
                    .await?
                    .reason();

                // Only worth asking tmux when there is something a session
                // could be working on. An error here degrades to "unknown"
                // rather than failing a panel that is otherwise fine.
                let handoff = match (in_progress, handler_configured) {
                    (Some(_), true) => {
                        match session_exists(&session, Duration::from_secs(5)).await {
                            Ok(true) => Some(HandoffState::Running { session }),
                            Ok(false) => Some(HandoffState::Gone { session }),
                            Err(error) => {
                                tracing::warn!(%error, "could not ask tmux about the handoff session");
                                None
                            }
                        }
                    }
                    _ => None,
                };

                Ok(LocalState::CheckedOut(LocalBranch {
                    worktree: repo.root().to_path_buf(),
                    branch,
                    remote,
                    divergence,
                    fetched,
                    blocker,
                    in_progress,
                    handoff,
                }))
            })
            .await;

            this.update(cx, |this, cx| {
                this.local = match result {
                    Ok(Ok(local)) => Loadable::Loaded(local),
                    Ok(Err(err)) => Loadable::Failed(err.to_string()),
                    Err(err) => Loadable::Failed(err.to_string()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    /// Run one local git operation on this pull request's worktree.
    ///
    /// The whole sequence — find the worktree, run, hand off a conflict or
    /// abort it — is [`run_local_job`], shared with the feed's sync-all, so the
    /// two cannot drift. This method only owns the in-flight guard and the
    /// banner. A conflict is a *successful* call that did not finish the job,
    /// and is reported in git's own words; a handoff is reported as a notice,
    /// not an error, because nothing went wrong.
    ///
    /// The clone is re-read whichever way it went: a refused, conflicted, or
    /// handed-off operation changes what the buttons should offer.
    fn run_local_op(&mut self, op: LocalOp, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(pull) = self.pull(cx) else {
            return;
        };
        let store = self.store.read(cx);
        let Some(clone) = store.local_path(&self.repo) else {
            return;
        };
        let (branch, base) = match (
            BranchName::new(pull.head_ref.clone()),
            BranchName::new(pull.base_ref.clone()),
        ) {
            (Ok(branch), Ok(base)) => (branch, base),
            (Err(err), _) | (_, Err(err)) => {
                self.error = Some(err.to_string());
                cx.notify();
                return;
            }
        };
        let job = LocalJob {
            clone,
            branch,
            base,
            op,
            autostash: if store.autostash() {
                Autostash::Enabled
            } else {
                Autostash::Disabled
            },
            handler: store.conflict_handler(),
            pr: PrMeta {
                repo: self.repo.clone(),
                number: self.number,
                title: pull.title.clone(),
                url: pull.url.clone(),
                // The feed row carries no body; the conversation does, when
                // loaded, as its first timeline item. Hand over what is at hand.
                body: self
                    .conversation
                    .loaded()
                    .and_then(|conversation| {
                        conversation.items.iter().find_map(|item| match item {
                            TimelineItem::Body { body, .. } => Some(body.clone()),
                            _ => None,
                        })
                    })
                    .unwrap_or_default(),
                head_ref: pull.head_ref.clone(),
                base_ref: pull.base_ref.clone(),
            },
        };

        self.busy = Some(op.progress_label());
        self.error = None;
        self.notice = None;
        cx.notify();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, run_local_job(job)).await;

            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(LocalResult::UpToDate | LocalResult::Completed) => {}
                    Ok(handed @ LocalResult::HandedOff { .. }) => {
                        this.notice = Some(handed.detail());
                    }
                    Ok(other) => this.error = Some(other.detail()),
                    Err(err) => this.error = Some(err.to_string()),
                }
                this.load_local(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Abort whatever rebase or merge is stopped in this branch's worktree.
    ///
    /// Reachable only when the panel shows one in progress, and it takes the
    /// abort target from the worktree's own state rather than from memory, so
    /// it cannot run `merge --abort` on a rebase.
    fn abort_local(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(LocalState::CheckedOut(local)) = self.local.loaded() else {
            return;
        };
        let worktree = local.worktree.clone();

        self.busy = Some("Aborting");
        self.error = None;
        self.notice = None;
        cx.notify();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, async move {
                let repo = Repo::open(&worktree).await?;
                let status = repo.status().await?;
                let Some(target) = status.in_progress.and_then(InProgress::abort_target) else {
                    return Err(GitError::NothingToDescribe {
                        in_progress: status.in_progress,
                    });
                };
                repo.abort(target).await
            })
            .await;

            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => this.error = Some(err.to_string()),
                    Err(err) => this.error = Some(err.to_string()),
                }
                this.load_local(cx);
                cx.notify();
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
        self.load_local(cx);
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

    /// Catch this branch up with its base, on GitHub's side.
    ///
    /// Not held behind a confirmation. Both methods only ever *add* the base's
    /// commits to a branch that is behind it, and the `expectedHeadOid` sent
    /// with the mutation means a branch that moved since this was rendered is
    /// refused by GitHub rather than rewritten from a stale view.
    fn update_from_base(&mut self, method: BranchUpdateMethod, cx: &mut Context<Self>) {
        let Some((id, head_sha)) = self
            .pull(cx)
            .map(|pull| (pull.node_id.clone(), pull.head_sha.clone()))
        else {
            return;
        };
        self.mutate(method.progress_label(), cx, move |client, repo, number| {
            Box::pin(async move {
                client
                    .update_branch(&repo, number, &id, &head_sha, method)
                    .await
            })
        });
    }

    /// Move this pull request to the other side of the draft line.
    ///
    /// Unlike merge and close, this is not held behind a confirmation: both
    /// directions are one click away from being undone, and neither ends the
    /// pull request.
    ///
    /// `target` is an end state computed when the button was rendered, not a
    /// toggle evaluated here. A poll landing between render and click can only
    /// make the request redundant — which GitHub refuses, and the refusal shows
    /// in the error banner — never make it do the opposite of what the button
    /// said.
    fn set_draft(&mut self, target: DraftState, cx: &mut Context<Self>) {
        let Some(id) = self.pull(cx).map(|pull| pull.node_id) else {
            return;
        };
        self.mutate(target.progress_label(), cx, move |client, repo, number| {
            Box::pin(async move { client.set_draft(&repo, number, &id, target).await })
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

    pub(crate) fn set_files_view(&mut self, view: FilesView, cx: &mut Context<Self>) {
        self.files_view = view;
        cx.notify();
    }

    /// Leave the overview and land the diff on `file`'s header.
    ///
    /// The file is expanded first so the header row exists in the flattened
    /// stream, and the rows are rebuilt before the row index is looked up so
    /// the scroll target and the list's item count agree.
    pub(crate) fn jump_to_file(&mut self, file: usize, cx: &mut Context<Self>) {
        self.collapsed.remove(&file);
        self.files_view = FilesView::Diff;
        self.rebuild_diff_rows(cx);
        if let Some(row) = files::file_header_row(&self.diff_rows, file) {
            self.diff_list.scroll_to(gpui::ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
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
                    .child(format!("{} → {}", pull.head_ref, pull.base_ref))
                    // Only when there is something to catch up with: every open
                    // pull request is ahead of its base, so a chip saying so on
                    // all of them would be noise.
                    .when_some(
                        pull.base_divergence
                            .filter(|divergence| divergence.is_behind()),
                        |el, divergence| {
                            el.child(
                                Chip::new(format!("↓{} {}", divergence.behind, pull.base_ref))
                                    .color(theme.warning)
                                    .tooltip(
                                        "base-divergence",
                                        "Commits on the base branch this one does not have",
                                    ),
                            )
                        },
                    ),
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

    /// The local clone's row: where the checkout stands relative to the branch
    /// on GitHub and its base, and the ways to move it.
    ///
    /// Rendered only when a clone is configured and loaded. Everything here acts
    /// on the clone alone — nothing is pushed — so after a local merge or rebase
    /// the "ahead" count is what tells the user there is something to push.
    fn render_local(
        &self,
        state: &LocalState,
        busy: bool,
        theme: &rostrum_ui::Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let local = match state {
            LocalState::NotCheckedOut => {
                return h_flex()
                    .gap_2()
                    .items_center()
                    .child(Dot::new(theme.text_subtle))
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_subtle)
                            .child("Not checked out in any worktree of the clone"),
                    )
                    .into_any_element();
            }
            LocalState::CheckedOut(local) => local,
        };

        let divergence = local.divergence;
        let summary = match (divergence.behind, divergence.ahead) {
            (0, 0) => format!("Local {} matches {}", local.branch, local.remote),
            (0, ahead) => format!(
                "Local {} is {ahead} ahead of {}",
                local.branch, local.remote
            ),
            (behind, 0) => format!("Local {} is {behind} behind {}", local.branch, local.remote),
            (behind, ahead) => format!(
                "Local {} is {ahead} ahead, {behind} behind {}",
                local.branch, local.remote
            ),
        };

        // An operation git has not finished takes over the row: nothing else
        // can run until it is continued elsewhere or aborted here.
        if let Some(in_progress) = local.in_progress {
            let (text, color) = match &local.handoff {
                Some(HandoffState::Running { session }) => (
                    format!(
                        "{}, handed off to tmux session `{session}` — tmux attach -t ={session}",
                        capitalise(in_progress.describe())
                    ),
                    theme.accent,
                ),
                Some(HandoffState::Gone { session }) => (
                    format!(
                        "{} and no handoff session `{session}` is running — finish it in a terminal, or abort it here",
                        capitalise(in_progress.describe())
                    ),
                    theme.warning,
                ),
                None => (
                    format!(
                        "{} in this worktree — finish it in a terminal, or abort it here",
                        capitalise(in_progress.describe())
                    ),
                    theme.warning,
                ),
            };
            return h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(Dot::new(color))
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.text_muted)
                        .child(text),
                )
                .child(
                    Button::new("local-abort", "Abort")
                        .style(ButtonStyle::Danger)
                        .disabled(busy)
                        .tooltip("Run the matching --abort and return the worktree to how it was")
                        .on_click(Self::on_click(cx, |this, cx| this.abort_local(cx))),
                )
                .into_any_element();
        }

        let blocked = local.blocker.is_some();
        let disabled = busy || blocked;
        let tooltip = |when_clear: String| -> String {
            match &local.blocker {
                Some(reason) => reason.clone(),
                None => when_clear,
            }
        };
        let nothing_to_pull = !divergence.is_behind();
        let pull_tip = |verb: &str| {
            if nothing_to_pull {
                format!("Nothing to {verb}: the clone matches {}", local.remote)
            } else {
                format!("{} {} into your local branch", verb, local.remote)
            }
        };

        let autostash = self.store.read(cx).autostash();

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(Dot::new(if blocked {
                        theme.warning
                    } else {
                        theme.text_subtle
                    }))
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_muted)
                            .child(summary),
                    )
                    .when(!local.fetched, |el| {
                        el.child(
                            Chip::new("not fetched")
                                .color(theme.warning)
                                .tooltip(
                                    "local-stale",
                                    "Could not reach the remote; these counts come from the refs already on disk",
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        Button::new("local-pull", "Pull (rebase)")
                            .disabled(disabled || nothing_to_pull)
                            .tooltip(tooltip(pull_tip("Fetch and rebase")))
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.run_local_op(LocalOp::PullRebase, cx)
                            })),
                    )
                    .child(
                        Button::new("local-merge-remote", "Merge remote")
                            .disabled(disabled || nothing_to_pull)
                            .tooltip(tooltip(pull_tip("Merge")))
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.run_local_op(LocalOp::MergeRemote, cx)
                            })),
                    )
                    .child(
                        Button::new("local-merge-base", "Merge base")
                            .disabled(disabled)
                            .tooltip(tooltip(
                                "Merge the base branch into this worktree; nothing is pushed".into(),
                            ))
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.run_local_op(LocalOp::MergeBase, cx)
                            })),
                    )
                    .child(
                        Button::new("local-rebase-base", "Rebase onto base")
                            .disabled(disabled)
                            .tooltip(tooltip(
                                "Rebase this worktree onto the base branch; nothing is pushed".into(),
                            ))
                            .on_click(Self::on_click(cx, |this, cx| {
                                this.run_local_op(LocalOp::RebaseBase, cx)
                            })),
                    ),
            )
            .child(
                Checkbox::new("local-autostash", "Stash local changes", autostash).on_toggle(
                    Self::on_click(cx, move |this, cx| {
                        let next = !this.store.read(cx).autostash();
                        this.store
                            .update(cx, |store, cx| store.set_autostash(next, cx));
                        // The preflight verdict depends on this: a dirty
                        // worktree blocks a pull with it off and not with it on,
                        // so the buttons have to be re-evaluated.
                        this.load_local(cx);
                    }),
                ),
            )
            .into_any_element()
    }

    /// The "your branch is behind its base" row, with the two ways to fix it.
    ///
    /// Rendered only when there is something to catch up with, so a current
    /// branch costs no vertical space. Both buttons act on GitHub's copy; the
    /// local clone, when there is one, gets its own row beneath this.
    fn render_branch_sync(
        &self,
        pull: &PullRequest,
        divergence: Divergence,
        busy: bool,
        theme: &rostrum_ui::Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let summary = match divergence.ahead {
            0 => format!("{} commit(s) behind {}", divergence.behind, pull.base_ref),
            ahead => format!(
                "{} behind {}, {ahead} ahead",
                divergence.behind, pull.base_ref
            ),
        };

        // When nothing of this branch's own would be rewritten the two methods
        // produce the same commits, and saying so is friendlier than leaving the
        // reader to work out which to press.
        let equivalent = divergence.fast_forwards();

        h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(Dot::new(theme.warning))
            .child(
                div()
                    .text_size(rems(0.75))
                    .text_color(theme.text_muted)
                    .child(summary),
            )
            .child(
                Button::new("update-merge", "Update: Merge")
                    .disabled(busy)
                    .tooltip(if equivalent {
                        "Merge the base in. This branch has no commits of its own to rewrite, so rebasing would give the same result"
                    } else {
                        "Merge the base branch into this one, adding a merge commit"
                    })
                    .on_click(Self::on_click(cx, |this, cx| {
                        this.update_from_base(BranchUpdateMethod::Merge, cx)
                    })),
            )
            .child(
                Button::new("update-rebase", "Update: Rebase")
                    .disabled(busy)
                    .tooltip(if equivalent {
                        "Rebase onto the base. This branch has no commits of its own to rewrite, so merging would give the same result"
                    } else {
                        "Replay this branch's commits on top of the base, keeping history linear"
                    })
                    .on_click(Self::on_click(cx, |this, cx| {
                        this.update_from_base(BranchUpdateMethod::Rebase, cx)
                    })),
            )
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
            .when_some(self.notice.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.accent)
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
            .when_some(
                pull.base_divergence
                    .filter(|divergence| divergence.is_behind()),
                |el, divergence| {
                    el.child(self.render_branch_sync(pull, divergence, busy, &theme, cx))
                },
            )
            .when_some(self.local.loaded(), |el, local| {
                el.child(self.render_local(local, busy, &theme, cx))
            })
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
                    .child({
                        let target = DraftState::toggled_from(pull.is_draft);
                        Button::new(
                            "draft",
                            if pull.is_draft {
                                "Ready for review"
                            } else {
                                "Convert to draft"
                            },
                        )
                        .disabled(busy)
                        .tooltip(if pull.is_draft {
                            "Take this out of draft and request the reviews it is waiting on"
                        } else {
                            "Put this back into draft so it cannot be merged"
                        })
                        .on_click(Self::on_click(cx, move |this, cx| {
                            this.set_draft(target, cx)
                        }))
                    })
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

/// First letter upper-cased, for a phrase git gave us mid-sentence.
fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
