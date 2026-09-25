//! The store: canonical state plus the machinery that keeps it fresh.
//!
//! GPUI's executor is not Tokio and `reqwest` needs a Tokio reactor, so network
//! futures are handed to `gpui_tokio::Tokio`, which spawns them on a Tokio
//! handle and re-wraps the join handle as a `gpui::Task` (cancelled on drop).
//! Results are applied back on the main thread through `entity.update`.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{
    AppState, Divergence, LoadState, MergeStatus, PrNumber, PullRequest, RepoId, RepoState,
    carry_forward_divergence,
};
use rostrum_db::Db;
use rostrum_git::{Autostash, BranchName};
use rostrum_github::{GitHubClient, GitHubError, client::RepoPullRequests, resolve_token};
use rostrum_handoff::PrMeta;

use crate::{
    config::{Config, ConflictHandler, Warning},
    localops::{LocalJob, LocalOp, LocalResult, run_local_job},
};

/// How long to wait before re-asking for a merge state GitHub is computing,
/// doubling per attempt.
const MERGE_PROBE_DELAY: Duration = Duration::from_secs(2);
/// Probes per repository per poll cycle. Three attempts span 2s, 4s and 8s,
/// which covers the computation comfortably; beyond that the state is not
/// pending but withheld, and waiting harder will not reveal it.
const MAX_MERGE_PROBES: u8 = 3;

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

pub struct Store {
    pub config: Config,
    pub state: AppState,
    pub auth: AuthStatus,
    pub warnings: Vec<Warning>,
    client: Option<GitHubClient>,
    /// In-flight refresh per repository. Presence here is the overlap guard: a
    /// slow request can never stack up behind a fast timer.
    pending: HashMap<RepoId, Task<()>>,
    /// Held so the poll loop is not dropped (dropping a `Task` cancels it).
    poll: Option<Task<()>>,
    /// Follow-up refreshes chasing a merge state GitHub has not finished
    /// computing, one per repository. Replacing an entry cancels the previous
    /// timer, so a repo can never accumulate probes.
    merge_probes: HashMap<RepoId, Task<()>>,
    /// Probes already spent this poll cycle, per repository. Bounds the chase
    /// so a merge state that stays `UNKNOWN` — which is what a token without
    /// push access sees — does not become a permanent request loop.
    merge_probe_attempts: HashMap<RepoId, u8>,
    /// In-flight divergence batch per repository, issued after each refresh
    /// lands. Replacing an entry cancels the previous request, so a probe
    /// whose refresh has already been superseded never writes stale counts
    /// over newer ones.
    divergence_probes: HashMap<RepoId, Task<()>>,
    /// The latest "sync all", running or finished. Kept after completion so
    /// the per-row verdicts persist until the next sync replaces them.
    sync: Option<SyncProgress>,
    /// The sync's driver. Dropping it cancels the sync between jobs — never
    /// mid-git, because each job runs to completion on the Tokio side.
    sync_task: Option<Task<()>>,
    /// Local cache. `None` until it opens, and `None` forever if it fails —
    /// the app works without it, just without a warm start.
    db: Option<Arc<Db>>,
    _hydrate: Option<Task<()>>,
}

impl Store {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (config, mut warnings) = Config::load();
        let (repo_ids, repo_warnings) = config.repo_ids();
        warnings.extend(repo_warnings);

        let mut state = AppState::with_repos(repo_ids);
        state.filter.hide_empty_repos = config.hide_empty_repos;

        let mut store = Self {
            config,
            state,
            auth: AuthStatus::Resolving,
            warnings,
            client: None,
            pending: HashMap::new(),
            poll: None,
            merge_probes: HashMap::new(),
            merge_probe_attempts: HashMap::new(),
            divergence_probes: HashMap::new(),
            sync: None,
            sync_task: None,
            db: None,
            _hydrate: None,
        };
        store.open_database(cx);
        store.authenticate(cx);
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
                    if !prs.is_empty() {
                        cached.push((repo, prs));
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

    fn hydrate(
        &mut self,
        db: Arc<Db>,
        cached: Vec<(RepoId, Vec<PullRequest>)>,
        cx: &mut Context<Self>,
    ) {
        self.db = Some(db);

        for (id, prs) in cached {
            // Never clobber data that already arrived from the network: the
            // cache is only ever used to fill a gap.
            if let Some(repo) = self.state.repo_mut(&id)
                && repo.prs.is_empty()
            {
                tracing::debug!(repo = %id, count = prs.len(), "restored from cache");
                repo.prs = prs;
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
        self.persist_config();
        self.refresh_repo(id, cx);
        cx.notify();
        Ok(())
    }

    /// Remove a repository and everything the UI holds about it.
    pub fn remove_repo(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        if !self.config.remove_repo(id) {
            return;
        }
        self.state.repos.retain(|repo| &repo.id != id);
        // A selection pointing into the removed repo would resolve to nothing
        // and leave the detail pane stranded.
        if self
            .state
            .selection
            .as_ref()
            .is_some_and(|selection| &selection.repo == id)
        {
            self.state.selection = None;
        }
        // Dropping the in-flight tasks cancels their requests.
        self.pending.remove(id);
        self.divergence_probes.remove(id);
        // A verdict for a repository that is no longer listed has no row to
        // sit on, and would resurface if the repo were re-added.
        if let Some(sync) = &mut self.sync {
            sync.results.retain(|(repo, _), _| repo != id);
        }
        self.persist_config();
        cx.notify();
    }

    pub fn set_hide_empty_repos(&mut self, hide: bool, cx: &mut Context<Self>) {
        self.state.filter.hide_empty_repos = hide;
        self.config.hide_empty_repos = hide;
        self.persist_config();
        cx.notify();
    }

    pub fn set_autostash(&mut self, autostash: bool, cx: &mut Context<Self>) {
        self.config.autostash = autostash;
        self.persist_config();
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

    /// Config writes are small and infrequent; a failure is worth reporting but
    /// not worth interrupting the user over.
    fn persist_config(&self) {
        if let Err(error) = self.config.save() {
            tracing::warn!(%error, "could not save the config file");
        }
    }

    /// The cache handle, for views that persist their own state.
    pub fn db(&self) -> Option<Arc<Db>> {
        self.db.clone()
    }

    pub fn is_refreshing(&self) -> bool {
        !self.pending.is_empty()
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

    pub fn is_syncing(&self) -> bool {
        self.sync_task.is_some() && self.sync.as_ref().is_some_and(|sync| !sync.is_finished())
    }

    /// The latest sync's verdict for one pull request, if it took part.
    pub fn sync_result(&self, repo: &RepoId, number: PrNumber) -> Option<&LocalResult> {
        self.sync
            .as_ref()
            .and_then(|sync| sync.results.get(&(repo.clone(), number)))
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

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        // A full refresh is the start of a new cycle, so every repository gets
        // its probe budget back. Probes themselves call `refresh_repo`, which
        // leaves the budget alone.
        self.merge_probe_attempts.clear();

        let ids: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .map(|repo| repo.id.clone())
            .collect();
        for id in ids {
            self.refresh_repo(id, cx);
        }
    }

    pub fn refresh_repo(&mut self, id: RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        // Overlap guard.
        if self.pending.contains_key(&id) {
            return;
        }
        let Some(repo) = self.state.repo_mut(&id) else {
            return;
        };

        // Only show a spinner on first load; a background refresh of a repo
        // that already has data should not blank the card out.
        if repo.prs.is_empty() {
            repo.load = LoadState::Loading;
        }
        cx.notify();

        let limit = self.config.prs_per_repo;
        let fetch_id = id.clone();
        let apply_id = id.clone();

        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(&*cx, async move {
                client.open_pull_requests(&fetch_id, limit).await
            })
            .await;

            // No await points after this: `apply_refresh` removes this task's
            // own handle from `pending`, which drops it.
            this.update(cx, |this, cx| this.apply_refresh(&apply_id, outcome, cx))
                .ok();
        });

        self.pending.insert(id, task);
    }

    fn apply_refresh(
        &mut self,
        id: &RepoId,
        outcome: Result<Result<RepoPullRequests, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let now = Utc::now();

        match outcome {
            Ok(Ok(mut fetched)) => {
                if let Some(repo) = self.state.repo_mut(id) {
                    // The feed query never carries divergence; keep what the
                    // last batch found until the next one answers.
                    carry_forward_divergence(&repo.prs, &mut fetched.pull_requests);
                    repo.prs = fetched.pull_requests;
                    repo.load = LoadState::Loaded { at: now };
                }
                if let Some(db) = self.db.clone() {
                    let repo = id.clone();
                    let prs = self
                        .state
                        .repo(&repo)
                        .map(|repo| repo.prs.clone())
                        .unwrap_or_default();
                    // Fire and forget: a cache write failing must not disturb
                    // the refresh that produced it. sqlx needs the Tokio
                    // reactor, so this goes through the bridge rather than
                    // GPUI's executor.
                    Tokio::spawn(&*cx, async move {
                        if let Err(error) = db.save_pull_requests(&repo, &prs).await {
                            tracing::warn!(%repo, %error, "could not cache pull requests");
                        }
                    })
                    .detach();
                }

                self.probe_merge_state(id, cx);
                self.fetch_divergences(id, cx);

                if let Some(limit) = fetched.rate_limit {
                    tracing::debug!(
                        repo = %id,
                        cost = limit.cost,
                        remaining = limit.remaining,
                        "refreshed"
                    );
                }
            }
            Ok(Err(err)) => {
                tracing::warn!(repo = %id, error = %err, "refresh failed");
                if let Some(repo) = self.state.repo_mut(id) {
                    repo.load = LoadState::Failed {
                        message: err.to_string(),
                        at: now,
                    };
                }
            }
            Err(err) => {
                tracing::warn!(repo = %id, error = %err, "refresh task did not complete");
                if let Some(repo) = self.state.repo_mut(id) {
                    repo.load = LoadState::Failed {
                        message: format!("refresh did not complete: {err}"),
                        at: now,
                    };
                }
            }
        }

        self.pending.remove(id);
        cx.notify();
    }

    /// Re-query a repository shortly after a refresh that found merge states
    /// GitHub had not finished computing.
    ///
    /// GitHub computes `mergeable` and `mergeStateStatus` lazily: the query
    /// that asks for them returns `UNKNOWN` *and* starts the computation, so a
    /// single poll can never see the answer. Without this the feed would show
    /// "computing" until the next full poll — for a poll interval measured in
    /// minutes, that is every pull request, every launch.
    fn probe_merge_state(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        let computing = self.state.repo(id).is_some_and(|repo| {
            repo.prs
                .iter()
                .any(|pr| pr.merge_status() == MergeStatus::Computing)
        });

        if !computing {
            self.merge_probes.remove(id);
            self.merge_probe_attempts.remove(id);
            return;
        }

        let attempt = self.merge_probe_attempts.entry(id.clone()).or_insert(0);
        if *attempt >= MAX_MERGE_PROBES {
            // Out of budget until the next poll cycle. This is the steady state
            // for a repository whose merge state the token may not read, so it
            // must be quiet rather than an error.
            tracing::debug!(repo = %id, "merge state still unknown; waiting for the next poll");
            self.merge_probes.remove(id);
            return;
        }
        *attempt += 1;

        // Back off, because the wait is GitHub finishing a background job and
        // the second attempt is evidence the first was too early.
        let delay = MERGE_PROBE_DELAY * 2u32.pow(u32::from(*attempt - 1));
        tracing::debug!(
            repo = %id,
            attempt = *attempt,
            delay_ms = delay.as_millis(),
            "merge state computing; scheduling a probe"
        );
        let probe_id = id.clone();
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| this.refresh_repo(probe_id, cx))
                .ok();
        });
        self.merge_probes.insert(id.clone(), task);
    }

    /// Ask GitHub how far each of a repository's pull requests has drifted
    /// from its base, in one batched request, and write the answers back.
    ///
    /// Separate from the feed query because `Ref.compare` is not reachable
    /// from a `PullRequest` node; it hangs off the base ref, so it has to be
    /// a second document built from the refs the feed just returned.
    fn fetch_divergences(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(repo) = self.state.repo(id) else {
            return;
        };
        let numbers: Vec<PrNumber> = repo.prs.iter().map(|pr| pr.number).collect();
        let pairs: Vec<(String, String)> = repo
            .prs
            .iter()
            .map(|pr| (pr.base_ref.clone(), pr.head_ref.clone()))
            .collect();
        if pairs.is_empty() {
            self.divergence_probes.remove(id);
            return;
        }

        let fetch_id = id.clone();
        let apply_id = id.clone();
        let task = cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(
                &*cx,
                async move { client.divergences(&fetch_id, &pairs).await },
            )
            .await;

            // No await points after this: `apply_divergences` removes this
            // task's own handle from `divergence_probes`, which drops it.
            this.update(cx, |this, cx| {
                this.apply_divergences(&apply_id, &numbers, outcome, cx)
            })
            .ok();
        });
        self.divergence_probes.insert(id.clone(), task);
    }

    /// Write a batch of divergence answers onto the pull requests they were
    /// asked about.
    ///
    /// Matched by number rather than by position: a refresh may have landed
    /// while the batch was in flight, reordering or replacing the list, and a
    /// count written to the wrong row is worse than none.
    ///
    /// A failed batch is logged at debug and changes nothing. The counts on
    /// screen were right a minute ago and are still the best available
    /// answer; blanking them would turn a transient error into a visible
    /// flicker on every poll that hits one.
    fn apply_divergences(
        &mut self,
        id: &RepoId,
        numbers: &[PrNumber],
        outcome: Result<Result<Vec<Option<Divergence>>, GitHubError>, gpui_tokio::JoinError>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(Ok(divergences)) => {
                if let Some(repo) = self.state.repo_mut(id) {
                    let answered = numbers
                        .iter()
                        .zip(divergences)
                        .filter_map(|(number, divergence)| divergence.map(|d| (*number, d)));
                    for (number, divergence) in answered {
                        if let Some(pr) = repo.prs.iter_mut().find(|pr| pr.number == number) {
                            pr.base_divergence = Some(divergence);
                        }
                    }
                }
                tracing::debug!(repo = %id, count = numbers.len(), "divergence updated");
            }
            Ok(Err(error)) => {
                tracing::debug!(repo = %id, %error, "divergence batch failed; keeping previous counts");
            }
            Err(error) => {
                tracing::debug!(repo = %id, %error, "divergence batch did not complete");
            }
        }

        self.divergence_probes.remove(id);
        cx.notify();
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        let interval = self.config.refresh_interval();
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                // Errors here mean the store was dropped, so the loop ends.
                if this.update(cx, |this, cx| this.refresh_all(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// Toggle a repository's collapsed state.
    pub fn toggle_collapsed(&mut self, id: &RepoId, cx: &mut Context<Self>) {
        if let Some(repo) = self.state.repo_mut(id) {
            repo.collapsed = !repo.collapsed;
            cx.notify();
        }
    }
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
