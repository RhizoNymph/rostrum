//! The store: canonical state plus the machinery that keeps it fresh.
//!
//! GPUI's executor is not Tokio and `reqwest` needs a Tokio reactor, so network
//! futures are handed to `gpui_tokio::Tokio`, which spawns them on a Tokio
//! handle and re-wraps the join handle as a `gpui::Task` (cancelled on drop).
//! Results are applied back on the main thread through `entity.update`.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::Utc;
use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{AppState, LoadState, PullRequest, RepoId, RepoState};
use rostrum_db::Db;
use rostrum_github::{GitHubClient, GitHubError, client::RepoPullRequests, resolve_token};

use crate::config::{Config, Warning};

#[derive(Clone, Debug)]
pub enum AuthStatus {
    Resolving,
    Ready { source: String },
    Failed { message: String },
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
        // Dropping the in-flight task cancels its request.
        self.pending.remove(id);
        self.persist_config();
        cx.notify();
    }

    pub fn set_hide_empty_repos(&mut self, hide: bool, cx: &mut Context<Self>) {
        self.state.filter.hide_empty_repos = hide;
        self.config.hide_empty_repos = hide;
        self.persist_config();
        cx.notify();
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
            Ok(Ok(fetched)) => {
                if let Some(repo) = self.state.repo_mut(id) {
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
