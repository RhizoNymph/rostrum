//! Keeping the running app and `config.json` in step when something else
//! writes the file too — `rostrumd` applying settings a phone sent, or a
//! person with an editor.
//!
//! Two halves, both on [`rostrum_config::document`]:
//!
//! - **Saving is a compare-and-swap.** The store remembers the config as it
//!   last read or wrote it (`config_base`) and that file's hash. A save
//!   writes the in-memory config if the file still has that hash; otherwise
//!   it re-reads the file and re-applies only the fields the app changed on
//!   top of it ([`rostrum_config::merge3`]), so a write from outside is never
//!   clobbered. Keys the app does not know survive either way.
//! - **Watching.** Every [`WATCH_INTERVAL`] the file's modification time and
//!   size are checked; when they move and the content hash differs from the
//!   last one seen, the file is read and adopted: the filter, the tab and the
//!   repository list follow it without a restart, and repositories that
//!   appeared, or new fetch limits, trigger a refresh.

use std::{path::PathBuf, time::Duration};

use gpui::Context;
use rostrum_config::{Config, ContentHash, document, merge3, save_merged};
use rostrum_core::{FeedFilter, RepoId, RepoState};

use super::Store;

/// How often the file's metadata is checked. A stat is nearly free; the file
/// is read only when it moved.
pub(crate) const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// What the store last knew of the file on disk.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileState {
    /// The config as last read from or written to the file. `None` before
    /// startup's read, or when the file could not be read.
    pub(crate) base: Option<Config>,
    pub(crate) hash: Option<ContentHash>,
    /// Modification time and length at the last check, to skip reading a
    /// file that has not moved.
    pub(crate) stamp: Option<(std::time::SystemTime, u64)>,
}

impl FileState {
    /// Startup: `config` is what [`Config::load`] produced from `path`.
    pub(crate) fn at_startup(path: Option<&PathBuf>, config: &Config) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        Self {
            base: Some(config.clone()),
            hash: document::current_hash(path),
            stamp: stamp(path),
        }
    }
}

fn stamp(path: &PathBuf) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl Store {
    /// Save the in-memory config without overwriting anyone else's write.
    ///
    /// Config writes are small and infrequent; a failure is worth reporting
    /// but not worth interrupting the user over.
    pub(crate) fn persist_config(&mut self, cx: &mut Context<Self>) {
        let Some(path) = Config::path() else {
            tracing::warn!("no config directory; the change is not saved");
            return;
        };
        let base = self.file.base.clone().unwrap_or_else(|| self.config.clone());
        match save_merged(&path, &base, self.file.hash, &self.config) {
            Ok(saved) => {
                self.file.hash = Some(saved.hash);
                self.file.stamp = stamp(&path);
                self.file.base = Some(saved.config.clone());
                if saved.merged {
                    tracing::info!("config.json changed elsewhere; merged this change on top");
                    if saved.config != self.config {
                        let before = std::mem::replace(&mut self.config, saved.config);
                        self.sync_state_from_config(&before, cx);
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "could not save the config file"),
        }
    }

    /// Adopt `config.json` if something else changed it since the store last
    /// read or wrote it.
    pub(crate) fn check_config_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = Config::path() else {
            return;
        };
        let now = stamp(&path);
        if now.is_some() && now == self.file.stamp {
            return;
        }
        self.file.stamp = now;
        let snapshot = match document::read(&path) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => return,
            Err(error) => {
                tracing::debug!(%error, "config.json is not readable right now; keeping the current settings");
                return;
            }
        };
        if Some(snapshot.hash) == self.file.hash {
            return;
        }
        let Some(disk) = snapshot.config else {
            tracing::debug!("config.json changed into something that is not a config; ignoring it");
            return;
        };
        let base = self.file.base.clone().unwrap_or_else(|| self.config.clone());
        // Anything the app changed and has not saved is re-applied on top.
        let merged = merge3(&base, &disk, &self.config);
        let pending = merged != disk;
        self.file.base = Some(disk);
        self.file.hash = Some(snapshot.hash);
        tracing::info!("config.json changed on disk; reloading its settings");
        if merged != self.config {
            let before = std::mem::replace(&mut self.config, merged);
            self.sync_state_from_config(&before, cx);
        }
        if pending {
            self.persist_config(cx);
        }
    }

    /// Bring the running state in line with `self.config` after it changed
    /// from `before` without going through the UI.
    fn sync_state_from_config(&mut self, before: &Config, cx: &mut Context<Self>) {
        // The search box is a half-finished action, not a setting.
        let query = std::mem::take(&mut self.state.filter.query);
        self.state.filter = FeedFilter {
            query,
            ..self.config.feed_filter()
        };
        self.state.tab = self.config.feed_tab;

        let (ids, warnings) = self.config.repo_ids();
        for warning in warnings {
            tracing::warn!(warning = %warning.0, "problem in the reloaded config");
        }
        let gone: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .map(|repo| repo.id.clone())
            .filter(|id| !ids.contains(id))
            .collect();
        for id in &gone {
            self.forget_repo_state(id);
        }
        let mut added = Vec::new();
        for id in &ids {
            if self.state.repo_mut(id).is_none() {
                self.state.repos.push(RepoState::new(id.clone()));
                added.push(id.clone());
            }
        }
        // The feed follows the file's order, as it does at startup.
        self.state
            .repos
            .sort_by_key(|repo| ids.iter().position(|id| id == &repo.id));

        let limits_changed = before.prs_per_repo != self.config.prs_per_repo
            || before.issues_per_repo != self.config.issues_per_repo;
        if limits_changed {
            self.refresh_all(cx);
        } else {
            for id in added {
                self.refresh_repo(id.clone(), cx);
                self.refresh_issues(id, cx);
            }
        }
        if !gone.is_empty() {
            tracing::info!(removed = gone.len(), "repositories removed by a config change");
        }
        cx.notify();
    }

    /// Poll the file for writes from outside the app.
    pub(crate) fn start_config_watch(&mut self, cx: &mut Context<Self>) {
        self.config_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_INTERVAL).await;
                if this
                    .update(cx, |this, cx| this.check_config_file(cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}
