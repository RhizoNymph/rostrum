//! The store: canonical state plus the machinery that keeps it fresh.
//!
//! GPUI's executor is not Tokio and `reqwest` needs a Tokio reactor, so network
//! futures are handed to `gpui_tokio::Tokio`, which spawns them on a Tokio
//! handle and re-wraps the join handle as a `gpui::Task` (cancelled on drop).
//! Results are applied back on the main thread through `entity.update`.

mod ci;
mod closed;
mod config_file;
mod feed;
mod inflight;
mod issues;
mod linked;
mod poll;

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{
    AppState, AuthorEntry, ClosedPull, FeedFilter, FeedTab, Issue, ItemSortKey, LoginKey, PrNumber,
    PullRequest, RepoId, RepoMeta, RepoSortKey, RepoState, Stack, User,
    budget::{Feature, Pace},
    closed_roster, issue_roster, roster,
};
use rostrum_db::Db;
use rostrum_git::{Autostash, BranchName};
use rostrum_github::{GitHubClient, GitHubError, resolve_token};
use rostrum_handoff::PrMeta;

use rostrum_config::{Config, ConflictHandler, Warning};
use rostrum_local::{LocalJob, LocalOp, LocalResult, run_local_job};

mod push;
mod stacks;
mod verdict;

pub use push::PushAll;
pub use stacks::StackOpResult;
pub use verdict::{SyncVerdict, VerdictTone};

#[derive(Clone, Debug)]
pub enum AuthStatus {
    Resolving,
    Ready { source: String },
    Failed { message: String },
}

/// Which local operation a "sync all" runs on every checked-out pull request.
///
/// A subset of [`LocalOp`]: merging the remote branch into a local checkout
/// is a per-branch judgement call the detail pane offers, not something to do
/// to every worktree at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    Pull,
    MergeBase,
    RebaseBase,
}

impl SyncKind {
    /// Button text, and the prefix of the progress line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pull => "Pull all",
            Self::MergeBase => "Merge base into all",
            Self::RebaseBase => "Rebase all onto base",
        }
    }

    fn op(self) -> LocalOp {
        match self {
            Self::Pull => LocalOp::PullRebase,
            Self::MergeBase => LocalOp::MergeBase,
            Self::RebaseBase => LocalOp::RebaseBase,
        }
    }
}

/// One sync's bookkeeping, kept after it finishes so the verdicts stay on the
/// rows until the next sync replaces them.
#[derive(Clone, Debug)]
pub struct SyncProgress {
    pub kind: SyncKind,
    /// Jobs that have reported, including the ones that failed before they
    /// could be spawned; `done == total` means the sync is over.
    pub done: usize,
    pub total: usize,
    pub results: BTreeMap<(RepoId, PrNumber), LocalResult>,
}

impl SyncProgress {
    pub fn is_finished(&self) -> bool {
        self.done >= self.total
    }

    /// Tally of the outcomes worth reporting, for the line under the buttons.
    pub fn summary(&self) -> SyncSummary {
        summarise(self.results.values())
    }
}

/// How many pull requests landed in each reportable bucket.
///
/// `UpToDate` and `NotCheckedOut` are deliberately not counted: a sync over a
/// clone where most branches are not checked out would otherwise report
/// mostly noise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub updated: usize,
    pub handed_off: usize,
    pub conflicts: usize,
    pub refused: usize,
    pub failed: usize,
}

impl SyncSummary {
    /// Comma-separated, zero buckets omitted; a fixed phrase when every
    /// bucket is zero so the line never reads as an empty label.
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            (self.updated, "updated"),
            (self.handed_off, "handed off"),
            (self.conflicts, "conflicts"),
            (self.refused, "refused"),
            (self.failed, "failed"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, noun)| format!("{count} {noun}"))
        .collect();
        if parts.is_empty() {
            "nothing to update".to_string()
        } else {
            parts.join(", ")
        }
    }
}

fn summarise<'a>(results: impl Iterator<Item = &'a LocalResult>) -> SyncSummary {
    let mut summary = SyncSummary::default();
    for result in results {
        match result {
            LocalResult::Completed => summary.updated += 1,
            LocalResult::HandedOff { .. } => summary.handed_off += 1,
            LocalResult::Conflicted(_) => summary.conflicts += 1,
            LocalResult::Refused(_) => summary.refused += 1,
            LocalResult::Failed(_) => summary.failed += 1,
            LocalResult::UpToDate | LocalResult::NotCheckedOut => {}
        }
    }
    summary
}

/// One repository's worth of the local cache, read at startup.
struct Cached {
    repo: RepoId,
    prs: Vec<PullRequest>,
    issues: Vec<Issue>,
    closed: Vec<ClosedPull>,
    meta: Option<RepoMeta>,
    stacks: Vec<Stack>,
}

pub struct Store {
    pub config: Config,
    pub state: AppState,
    pub auth: AuthStatus,
    pub warnings: Vec<Warning>,
    client: Option<GitHubClient>,
    /// Pull request batches in flight. A repository in flight is left out of
    /// the next batch: the overlap guard, so a slow request can never stack
    /// up behind a fast timer.
    feed: inflight::InFlight,
    /// What follows the feed's batches: divergence, checks, merge probes;
    /// see [`feed`].
    cycle: feed::FeedCycle,
    /// Issue batches in flight, guarded the same way and kept apart from
    /// `feed` so a merge probe's pull request refresh never waits on, or
    /// cancels, an issue fetch.
    issues_flight: inflight::InFlight,
    /// In-flight closed pull request fetch per repository, guarded the same
    /// way; see [`closed`].
    pending_closed: HashMap<RepoId, Task<()>>,
    /// Every watched repository's CI checks, for the CI view.
    pub ci: rostrum_core::ci::CiChecks,
    /// Checks batches in flight, guarded like the others.
    ci_flight: inflight::InFlight,
    /// The CI view's faster poll, alive only while the view is showing.
    ci_watch: Option<Task<()>>,
    /// Held so the poll loop is not dropped (dropping a `Task` cancels it).
    poll: Option<Task<()>>,
    /// The periodic log of what each feature spent; see [`poll`].
    spend_log: Option<Task<()>>,
    /// The last interval the poll loop chose, and why.
    pace: Option<Pace>,
    /// The latest "sync all", running or finished. Kept after completion so
    /// the per-row verdicts persist until the next sync replaces them.
    sync: Option<SyncProgress>,
    /// The sync's driver. Dropping it cancels the sync between jobs — never
    /// mid-git, because each job runs to completion on the Tokio side.
    sync_task: Option<Task<()>>,
    /// The latest Push all, planning, confirming, running or finished; see
    /// [`push`]. Shares the toolbar with `sync`: one batch at a time, and
    /// the rows show whichever ran last.
    push_all: push::PushAllSync,
    /// Who the token belongs to, learned from the first refresh that answers.
    /// `None` before then, and the author filter simply has nobody pinned to
    /// the front of its roster until it fills in.
    viewer: Option<User>,
    /// Local cache. `None` until it opens, and `None` forever if it fails —
    /// the app works without it, just without a warm start.
    db: Option<Arc<Db>>,
    /// GitHub's stacks per repository and the running stack operation; see
    /// [`stacks`].
    stacks: stacks::StackSync,
    _hydrate: Option<Task<()>>,
    /// What the store last knew of `config.json` on disk; see
    /// [`config_file`].
    file: config_file::FileState,
    /// Held so the config-file watch is not dropped.
    config_watch: Option<Task<()>>,
    /// Pull requests opened from a `rostrum://pr` link that the feed does
    /// not hold, looked up by themselves; see [`linked`].
    pub linked: crate::link::linked::LinkedPulls,
}

impl Store {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (config, mut warnings) = Config::load();
        let (repo_ids, repo_warnings) = config.repo_ids();
        warnings.extend(repo_warnings);

        let mut state = AppState::with_repos(repo_ids);
        state.filter = config.feed_filter();
        state.tab = config.feed_tab;
        let file = config_file::FileState::at_startup(Config::path().as_ref(), &config);

        let mut store = Self {
            config,
            state,
            auth: AuthStatus::Resolving,
            warnings,
            client: None,
            feed: Default::default(),
            cycle: Default::default(),
            issues_flight: Default::default(),
            pending_closed: HashMap::new(),
            ci: Default::default(),
            ci_flight: Default::default(),
            ci_watch: None,
            poll: None,
            spend_log: None,
            pace: None,
            sync: None,
            sync_task: None,
            push_all: push::PushAllSync::default(),
            viewer: None,
            db: None,
            stacks: stacks::StackSync::default(),
            _hydrate: None,
            file,
            config_watch: None,
            linked: Default::default(),
        };
        store.open_database(cx);
        store.authenticate(cx);
        store.start_config_watch(cx);
        store
    }

    /// Path of the local cache, alongside the platform's other app data.
    fn database_path() -> Option<PathBuf> {
        dirs::data_dir().map(|dir| dir.join("rostrum").join("cache.db"))
    }

    /// Open the cache and paint whatever it already knows, so the feed has
    /// content before the first network round trip completes.
    fn open_database(&mut self, cx: &mut Context<Self>) {
        let Some(path) = Self::database_path() else {
            tracing::warn!("no data directory; running without a local cache");
            return;
        };
        let repos: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .map(|repo| repo.id.clone())
            .collect();

        self._hydrate = Some(cx.spawn(async move |this, cx| {
            let opened = Tokio::spawn(&*cx, async move {
                let db = Db::open(&path).await?;
                let mut cached = Vec::new();
                for repo in repos {
                    let prs = db.load_pull_requests(&repo).await?;
                    let issues = db.load_issues(&repo).await?;
                    let closed = db.load_closed_pulls(&repo).await?;
                    let meta = db.load_repo_meta(&repo).await?;
                    let stacks = db.load_stacks(&repo).await?;
                    if !prs.is_empty()
                        || !issues.is_empty()
                        || !closed.is_empty()
                        || meta.is_some()
                        || !stacks.is_empty()
                    {
                        cached.push(Cached {
                            repo,
                            prs,
                            issues,
                            closed,
                            meta,
                            stacks,
                        });
                    }
                }
                Ok::<_, rostrum_db::DbError>((db, cached))
            })
            .await;

            match opened {
                Ok(Ok((db, cached))) => {
                    this.update(cx, |this, cx| this.hydrate(Arc::new(db), cached, cx))
                        .ok();
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "could not open the local cache; continuing without it")
                }
                Err(error) => tracing::warn!(%error, "cache open did not complete"),
            }
        }));
    }

    fn hydrate(&mut self, db: Arc<Db>, cached: Vec<Cached>, cx: &mut Context<Self>) {
        self.db = Some(db);

        for Cached {
            repo: id,
            prs,
            issues,
            closed,
            meta,
            stacks,
        } in cached
        {
            self.hydrate_stacks(&id, stacks);
            let Some(repo) = self.state.repo_mut(&id) else {
                continue;
            };
            // Never clobber data that already arrived from the network: the
            // cache is only ever used to fill a gap — per list, since the two
            // arrive separately.
            if repo.prs.is_empty() && !prs.is_empty() {
                tracing::debug!(repo = %id, count = prs.len(), "restored pull requests from cache");
                repo.prs = prs;
            }
            if repo.issues.is_empty() && !issues.is_empty() {
                tracing::debug!(repo = %id, count = issues.len(), "restored issues from cache");
                repo.issues = issues;
            }
            if repo.closed.is_empty() && !closed.is_empty() {
                tracing::debug!(repo = %id, count = closed.len(), "restored closed pull requests from cache");
                repo.closed = closed;
            }
            if repo.meta.is_none() && meta.is_some() {
                tracing::debug!(repo = %id, "restored repository metadata from cache");
                repo.meta = meta;
            }
        }
        cx.notify();
    }

    /// Add a repository from user input, persist it, and start fetching it.
    ///
    /// Returns a message suitable for showing next to the input on failure.
    pub fn add_repo(&mut self, input: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let id = self.config.add_repo(input)?;
        self.state.repos.push(RepoState::new(id.clone()));
        // Keep the feed in the same order as the config, which `add_repo`
        // sorts, so the list does not jump around between launches.
        self.state.repos.sort_by(|a, b| a.id.cmp(&b.id));
        self.persist_config(cx);
        self.refresh_repo(id.clone(), cx);
        self.refresh_issues(id, cx);
        self.refresh_closed_if_shown(cx);
        cx.notify();
        Ok(())
    }

    /// Remove a repository and everything the UI holds about it.
    pub fn remove_repo(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        if !self.config.remove_repo(id) {
            return;
        }
        self.forget_repo_state(id);
        self.persist_config(cx);
        cx.notify();
    }

    /// Drop everything the running app holds about a repository that is no
    /// longer watched; the config is the caller's business.
    pub(crate) fn forget_repo_state(&mut self, id: &RepoId) {
        self.state.repos.retain(|repo| &repo.id != id);
        // A selection pointing into the removed repo would resolve to nothing
        // and leave the detail pane stranded.
        if self
            .state
            .selection
            .as_ref()
            .is_some_and(|selection| selection.repo() == id)
        {
            self.state.selection = None;
        }
        // A batch carrying it runs on for the others; its answer finds no
        // repository to land on. Its own closed fetch is cancelled.
        self.feed.forget(id);
        self.issues_flight.forget(id);
        self.ci_flight.forget(id);
        self.cycle.forget(id);
        self.pending_closed.remove(id);
        self.ci.forget(id);
        self.stacks.forget(id);
        // A verdict for a repository that is no longer listed has no row to
        // sit on, and would resurface if the repo were re-added.
        if let Some(sync) = &mut self.sync {
            sync.results.retain(|(repo, _), _| repo != id);
        }
    }

    pub fn set_hide_empty_repos(&mut self, hide: bool, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.hide_empty_repos = hide, cx);
    }

    pub fn set_hide_drafts(&mut self, hide: bool, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.hide_drafts = hide, cx);
    }

    pub fn toggle_author(&mut self, login: LoginKey, cx: &mut Context<Self>) {
        self.edit_filter(
            |filter| {
                filter.toggle_author(login);
            },
            cx,
        );
    }

    /// Show every author again, leaving the rest of the filter alone.
    ///
    /// One edit rather than a toggle per selected login: `edit_filter` writes
    /// the config, and clearing six authors is one preference change, not six.
    pub fn clear_authors(&mut self, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.authors.clear(), cx);
    }

    pub fn set_include_involved(&mut self, include: bool, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.include_involved = include, cx);
    }

    /// Reset every filter, the author selection included.
    ///
    /// A "clear" that left the selection behind would be the worst of both: a
    /// feed still narrowed, by the control the user just told to stop narrowing
    /// it. The sort is kept: it hides nothing, so it is not what "clear" is
    /// for.
    pub fn clear_filter(&mut self, cx: &mut Context<Self>) {
        self.edit_filter(|filter| *filter = filter.cleared(), cx);
    }

    /// Order repositories by `key`. A different key arrives in its default
    /// direction; see [`rostrum_core::Sort::choose`].
    pub fn choose_repo_sort(&mut self, key: RepoSortKey, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.sort.repos.choose(key), cx);
        tracing::debug!(sort = %self.state.filter.sort.repos.summary(), "repository sort changed");
    }

    pub fn reverse_repo_sort(&mut self, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.sort.repos.reverse(), cx);
        tracing::debug!(sort = %self.state.filter.sort.repos.summary(), "repository sort changed");
    }

    /// Order the items within each repository by `key`.
    pub fn choose_item_sort(&mut self, key: ItemSortKey, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.sort.items.choose(key), cx);
        tracing::debug!(sort = %self.state.filter.sort.items.summary(), "item sort changed");
    }

    pub fn reverse_item_sort(&mut self, cx: &mut Context<Self>) {
        self.edit_filter(|filter| filter.sort.items.reverse(), cx);
        tracing::debug!(sort = %self.state.filter.sort.items.summary(), "item sort changed");
    }

    /// Edit the filter and write the parts of it that outlive the session.
    ///
    /// Every persisted filter setting goes through here, so "changed in the UI"
    /// and "saved to disk" cannot come apart the way they did for `hide_drafts`.
    /// The search query is edited directly instead — see
    /// [`rostrum_config::Config::feed_filter`] for why it is not a preference.
    fn edit_filter(&mut self, edit: impl FnOnce(&mut FeedFilter), cx: &mut Context<Self>) {
        edit(&mut self.state.filter);
        self.config.absorb_filter(&self.state.filter);
        self.persist_config(cx);
        cx.notify();
    }

    /// The people the author filter can be pointed at, viewer first: the
    /// authors of whichever list the active tab shows.
    pub fn authors(&self) -> Vec<AuthorEntry> {
        let build = match self.state.tab {
            FeedTab::PullRequests => roster,
            FeedTab::Issues => issue_roster,
            FeedTab::Closed => closed_roster,
        };
        build(
            &self.state.repos,
            self.viewer.as_ref(),
            &self.state.filter.authors,
        )
    }

    pub fn set_autostash(&mut self, autostash: bool, cx: &mut Context<Self>) {
        self.config.autostash = autostash;
        self.persist_config(cx);
        cx.notify();
    }

    pub fn autostash(&self) -> bool {
        self.config.autostash
    }

    /// The configured conflict handler, if any. `None` means a local conflict
    /// is aborted; `Some` means it is left in place and handed to this command.
    pub fn conflict_handler(&self) -> Option<ConflictHandler> {
        self.config.conflict_handler.clone()
    }

    /// The cache handle, for views that persist their own state.
    pub fn db(&self) -> Option<Arc<Db>> {
        self.db.clone()
    }

    pub fn is_refreshing(&self) -> bool {
        !self.feed.is_empty() || !self.issues_flight.is_empty() || !self.pending_closed.is_empty()
    }

    /// The client, tallying what it spends under `feature`.
    fn client_for(&self, feature: Feature) -> Option<GitHubClient> {
        self.client
            .as_ref()
            .map(|client| client.for_feature(feature))
    }

    /// The authenticated client, once auth has resolved.
    ///
    /// `GitHubClient` is cheap to clone (an `Arc`-backed reqwest client plus a
    /// token), so views take a copy rather than borrowing the store across an
    /// await point.
    pub fn client(&self) -> Option<GitHubClient> {
        self.client.clone()
    }

    /// The local clone configured for a repository, if there is one.
    ///
    /// Most watched repositories have none — the feed is built for reading
    /// other people's work — so every caller must treat `None` as the ordinary
    /// case and hide the local affordances rather than reporting a problem.
    pub fn local_path(&self, id: &RepoId) -> Option<PathBuf> {
        self.config.local_path(id)
    }

    /// Whether any watched repository has a clone to sync. Gates the sync
    /// toolbar: three disabled buttons would be a puzzle, not a feature.
    pub fn has_any_clone(&self) -> bool {
        self.state
            .repos
            .iter()
            .any(|repo| self.local_path(&repo.id).is_some())
    }

    pub fn sync(&self) -> Option<&SyncProgress> {
        self.sync.as_ref()
    }

    /// Whether a batch — a local sync or a Push all — is under way. The
    /// toolbar runs one at a time.
    pub fn is_syncing(&self) -> bool {
        (self.sync_task.is_some() && self.sync.as_ref().is_some_and(|sync| !sync.is_finished()))
            || self.push_all.is_active()
    }

    /// The latest batch's verdict for one pull request, if it took part.
    pub fn sync_result(&self, repo: &RepoId, number: PrNumber) -> Option<SyncVerdict<'_>> {
        let key = (repo.clone(), number);
        if let Some(result) = self.push_all.result(&key) {
            return Some(SyncVerdict::Push(result));
        }
        self.sync
            .as_ref()
            .and_then(|sync| sync.results.get(&key))
            .map(SyncVerdict::Local)
    }

    /// Run one local operation over every pull request of every repository
    /// that has a clone, one at a time.
    ///
    /// Sequential on purpose: the jobs share a clone's object store and
    /// index locks, and a user watching the progress line wants a count that
    /// climbs, not a burst of interleaved git output. The job list is built
    /// synchronously so it reflects the feed as the button was clicked;
    /// refreshes landing mid-sync do not add or remove work.
    pub fn sync_all(&mut self, kind: SyncKind, cx: &mut Context<Self>) {
        if self.is_syncing() {
            return;
        }
        // The rows show one batch's verdicts at a time.
        self.push_all.clear();

        let autostash = if self.autostash() {
            Autostash::Enabled
        } else {
            Autostash::Disabled
        };
        let handler = self.conflict_handler();

        let mut results = BTreeMap::new();
        let mut jobs = Vec::new();
        for repo in &self.state.repos {
            let Some(clone) = self.local_path(&repo.id) else {
                continue;
            };
            for pr in &repo.prs {
                let key = (repo.id.clone(), pr.number);
                let (branch, base) = match (
                    BranchName::new(pr.head_ref.as_str()),
                    BranchName::new(pr.base_ref.as_str()),
                ) {
                    (Ok(branch), Ok(base)) => (branch, base),
                    (Err(err), _) | (_, Err(err)) => {
                        // A ref git would reject never reaches the worktree
                        // lookup; it is a verdict, not a job.
                        results.insert(key, LocalResult::Failed(err.to_string()));
                        continue;
                    }
                };
                jobs.push((
                    key,
                    LocalJob {
                        clone: clone.clone(),
                        branch,
                        base,
                        op: kind.op(),
                        autostash,
                        handler: handler.clone(),
                        pr: PrMeta {
                            repo: repo.id.clone(),
                            number: pr.number,
                            title: pr.title.clone(),
                            url: pr.url.clone(),
                            // The feed query does not fetch bodies (only the
                            // conversation view does), so a handoff bundle
                            // started from here carries the title and URL
                            // alone. Acceptable: the handler can read the
                            // body from the URL.
                            body: String::new(),
                            head_ref: pr.head_ref.clone(),
                            base_ref: pr.base_ref.clone(),
                        },
                    },
                ));
            }
        }

        // Pre-failed entries count as done from the start, so the progress
        // line's denominator is every pull request looked at and its
        // numerator climbs from where enumeration left off.
        let prefailed = results.len();
        self.sync = Some(SyncProgress {
            kind,
            done: prefailed,
            total: prefailed + jobs.len(),
            results,
        });
        cx.notify();

        self.sync_task = Some(cx.spawn(async move |this, cx| {
            for (key, job) in jobs {
                let outcome = Tokio::spawn(&*cx, run_local_job(job)).await;
                let result = match outcome {
                    Ok(result) => result,
                    Err(err) => LocalResult::Failed(format!("sync job did not complete: {err}")),
                };
                // An error here means the store is gone; nothing left to
                // report to.
                if this
                    .update(cx, |this, cx| this.record_sync_result(key, result, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn record_sync_result(
        &mut self,
        key: (RepoId, PrNumber),
        result: LocalResult,
        cx: &mut Context<Self>,
    ) {
        let Some(sync) = &mut self.sync else {
            return;
        };
        // A repository removed mid-sync has no row for the verdict; the job
        // still counts toward completion so the progress line reaches its
        // total.
        if self.state.repos.iter().any(|repo| repo.id == key.0) {
            tracing::debug!(repo = %key.0, number = key.1.0, ?result, "sync job finished");
            sync.results.insert(key, result);
        }
        sync.done += 1;
        cx.notify();
    }

    /// Resolve a token, then begin refreshing.
    fn authenticate(&mut self, cx: &mut Context<Self>) {
        self.auth = AuthStatus::Resolving;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let resolved = Tokio::spawn(&*cx, async move { resolve_token().await }).await;

            this.update(cx, |this, cx| {
                match resolved {
                    Ok(Ok((token, source))) => match GitHubClient::new(token) {
                        Ok(client) => {
                            this.client = Some(client);
                            this.auth = AuthStatus::Ready {
                                source: source.to_string(),
                            };
                            this.refresh_all(cx);
                            this.start_polling(cx);
                        }
                        Err(err) => {
                            this.auth = AuthStatus::Failed {
                                message: err.to_string(),
                            }
                        }
                    },
                    Ok(Err(err)) => {
                        this.auth = AuthStatus::Failed {
                            message: err.to_string(),
                        }
                    }
                    Err(err) => {
                        this.auth = AuthStatus::Failed {
                            message: format!("token lookup did not complete: {err}"),
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Toggle a repository's collapsed state.
    pub fn toggle_collapsed(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        if let Some(repo) = self.state.repo_mut(id) {
            repo.collapsed = !repo.collapsed;
            cx.notify();
        }
    }
}

/// Whether GitHub refused for rate limiting: the budget's fault, not the
/// repository's, and already a pause in the client's tracker.
fn is_rate_limit(error: &GitHubError) -> bool {
    matches!(
        error,
        GitHubError::RateLimited { .. } | GitHubError::SecondaryRateLimit { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(items: &[LocalResult]) -> Vec<LocalResult> {
        items.to_vec()
    }

    #[test]
    fn every_reportable_outcome_lands_in_its_own_bucket() {
        let all = results(&[
            LocalResult::Completed,
            LocalResult::Completed,
            LocalResult::HandedOff {
                session: "s".into(),
            },
            LocalResult::Conflicted("c".into()),
            LocalResult::Refused("r".into()),
            LocalResult::Failed("f".into()),
            LocalResult::UpToDate,
            LocalResult::NotCheckedOut,
        ]);
        assert_eq!(
            summarise(all.iter()),
            SyncSummary {
                updated: 2,
                handed_off: 1,
                conflicts: 1,
                refused: 1,
                failed: 1,
            }
        );
    }

    #[test]
    fn the_summary_line_omits_empty_buckets() {
        let summary = SyncSummary {
            updated: 3,
            conflicts: 1,
            ..SyncSummary::default()
        };
        assert_eq!(summary.describe(), "3 updated, 1 conflicts");
    }

    #[test]
    fn a_sync_with_nothing_remarkable_says_so_rather_than_going_blank() {
        let quiet = results(&[LocalResult::UpToDate, LocalResult::NotCheckedOut]);
        assert_eq!(summarise(quiet.iter()).describe(), "nothing to update");
    }

    #[test]
    fn a_sync_is_finished_once_every_job_has_reported() {
        let mut progress = SyncProgress {
            kind: SyncKind::Pull,
            done: 1,
            total: 2,
            results: BTreeMap::new(),
        };
        assert!(!progress.is_finished());
        progress.done = 2;
        assert!(progress.is_finished());
    }

    #[test]
    fn each_sync_kind_maps_to_the_matching_local_op() {
        assert_eq!(SyncKind::Pull.op(), LocalOp::PullRebase);
        assert_eq!(SyncKind::MergeBase.op(), LocalOp::MergeBase);
        assert_eq!(SyncKind::RebaseBase.op(), LocalOp::RebaseBase);
    }
}
