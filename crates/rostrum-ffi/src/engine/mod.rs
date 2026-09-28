//! `RostrumCore`: the one object Kotlin holds, and the machinery behind it.
//!
//! - [`actor`]: the task that owns all mutable state.
//! - [`writer`]: the task that performs SQLite writes in state order.
//! - [`notifier`]: the task that delivers feed snapshots to the observer.
//! - [`state`]: the state itself.

pub(crate) mod actor;
pub(crate) mod notifier;
pub(crate) mod recent;
pub(crate) mod state;
pub(crate) mod writer;

use std::{path::PathBuf, sync::Arc};

use rostrum_config::Config;
use rostrum_db::Db;
use rostrum_github::GitHubError;

use crate::{
    engine::{
        actor::Actor,
        notifier::Notifier,
        state::{CoreState, Startup},
        writer::Writer,
    },
    error::RostrumError,
    session::GitHubApi,
};

/// The Android app's core. Create one per process with [`RostrumCore::open`]
/// and keep it for the life of the process; every other call hangs off it.
///
/// Secrets are never written to disk by the core. Kotlin keeps the GitHub
/// token and the desktop's device token in the Android Keystore and hands them
/// in after `open` with `set_github_token` and `set_remote`.
#[derive(uniffi::Object)]
pub struct RostrumCore {
    pub(crate) actor: Actor,
    pub(crate) db: Db,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Open the core over `data_dir`, an app-private directory: the SQLite
    /// cache lives at `<data_dir>/cache.db` and the settings at
    /// `<data_dir>/config.json` (the desktop's schema). Both are created on
    /// first use.
    #[uniffi::constructor]
    pub async fn open(data_dir: String) -> Result<Arc<Self>, RostrumError> {
        Self::open_with(data_dir, GitHubApi::GitHubCom).await
    }

    /// Problems found while loading the settings file — a malformed file is
    /// replaced by defaults rather than refusing to start, and this says so.
    pub async fn warnings(&self) -> Vec<String> {
        self.actor
            .call(|state| state.warnings.clone())
            .await
            .unwrap_or_default()
    }
}

impl RostrumCore {
    /// [`RostrumCore::open`] against another GitHub API root. Not exported to
    /// Kotlin: it exists so the integration tests can point the core at a
    /// local stand-in instead of api.github.com.
    #[doc(hidden)]
    pub async fn open_with_github_api(
        data_dir: String,
        graphql_url: String,
        rest_base: String,
    ) -> Result<Arc<Self>, RostrumError> {
        Self::open_with(
            data_dir,
            GitHubApi::Custom {
                graphql_url,
                rest_base,
            },
        )
        .await
    }

    async fn open_with(data_dir: String, github_api: GitHubApi) -> Result<Arc<Self>, RostrumError> {
        if data_dir.trim().is_empty() {
            return Err(RostrumError::invalid("the data directory is empty"));
        }
        let dir = PathBuf::from(data_dir);
        std::fs::create_dir_all(&dir).map_err(|error| RostrumError::Storage {
            reason: format!("could not create {}: {error}", dir.display()),
        })?;

        let config_path = dir.join("config.json");
        let (config, warnings) = Config::load_from(&config_path);
        let db = Db::open(&dir.join("cache.db")).await?;

        let writer = Writer::spawn(db.clone());
        let notifier = Notifier::spawn();
        let startup = Startup {
            github_api,
            config_path,
            config,
            warnings: warnings.into_iter().map(|warning| warning.0).collect(),
            writer,
            notifier,
        };
        let actor = Actor::spawn(|me| CoreState::new(startup, me));
        tracing::info!(dir = %dir.display(), "core opened");
        Ok(Arc::new(Self { actor, db }))
    }

    /// Close the SQLite pool, flushing it, before the profile directory under
    /// it is deleted. The core is unusable for storage afterwards; the
    /// registry drops it at the same time.
    pub(crate) async fn close_storage(&self) {
        self.db.close().await;
    }

    /// Map a GitHub failure, noting a rejected token in the session first so
    /// `github_status` reports it.
    pub(crate) async fn github_failed(&self, error: GitHubError) -> RostrumError {
        if matches!(error, GitHubError::Unauthorized) {
            let reason = error.to_string();
            let _ = self
                .actor
                .call(move |state| state.session.reject(reason))
                .await;
        }
        error.into()
    }

    /// [`RostrumCore::github_failed`] for a whole result.
    pub(crate) async fn github<T>(
        &self,
        result: Result<T, GitHubError>,
    ) -> Result<T, RostrumError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(self.github_failed(error).await),
        }
    }
}
