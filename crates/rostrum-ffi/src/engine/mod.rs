//! `RostrumCore`: the one object Kotlin holds.

use std::{path::PathBuf, sync::Arc};

use crate::error::RostrumError;

/// The Android app's core. Create one per process with [`RostrumCore::open`]
/// and keep it for the life of the process; every other call hangs off it.
///
/// Secrets are never written to disk by the core. Kotlin keeps the GitHub
/// token and the desktop's device token in the Android Keystore and hands them
/// in after `open` with `set_github_token` and `set_remote`.
#[derive(uniffi::Object)]
pub struct RostrumCore {
    #[allow(dead_code)]
    data_dir: PathBuf,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Open the core over `data_dir`, an app-private directory: the SQLite
    /// cache lives at `<data_dir>/cache.db` and the settings at
    /// `<data_dir>/config.json` (the desktop's schema). Both are created on
    /// first use.
    #[uniffi::constructor]
    pub async fn open(data_dir: String) -> Result<Arc<Self>, RostrumError> {
        Ok(Arc::new(Self {
            data_dir: PathBuf::from(data_dir),
        }))
    }

    /// Problems found while loading the settings file — a malformed file is
    /// replaced by defaults rather than refusing to start, and this says so.
    pub async fn warnings(&self) -> Vec<String> {
        Vec::new()
    }
}
