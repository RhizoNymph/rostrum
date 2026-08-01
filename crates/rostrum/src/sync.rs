//! The store: canonical state plus the machinery that keeps it fresh.
//!
//! GPUI's executor is not Tokio and `reqwest` needs a Tokio reactor, so network
//! futures are handed to `gpui_tokio::Tokio`, which spawns them on a Tokio
//! handle and re-wraps the join handle as a `gpui::Task` (cancelled on drop).
//! Results are applied back on the main thread through `entity.update`.

use std::collections::HashMap;

use chrono::Utc;
use gpui::{Context, Task};
use gpui_tokio::Tokio;
use rostrum_core::{AppState, LoadState, RepoId};
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
}

impl Store {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (config, mut warnings) = Config::load();
        let (repo_ids, repo_warnings) = config.repo_ids();
        warnings.extend(repo_warnings);

        let mut store = Self {
            config,
            state: AppState::with_repos(repo_ids),
            auth: AuthStatus::Resolving,
            warnings,
            client: None,
            pending: HashMap::new(),
            poll: None,
        };
        store.authenticate(cx);
        store
    }

    pub fn is_refreshing(&self) -> bool {
        !self.pending.is_empty()
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
