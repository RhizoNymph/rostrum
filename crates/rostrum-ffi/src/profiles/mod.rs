//! Profiles: one per paired desktop (or per bare GitHub token), each with its
//! own repositories, feed filters, cache, drafts, remote and GitHub account.
//!
//! A profile is a directory holding a complete `RostrumCore` data dir; the
//! registry is the list of them and which is active. The feed shows the
//! active profile; Kotlin's background job walks every profile.
//!
//! ```text
//! <root>/profiles.json          the registry (atomic writes, no secrets)
//! <root>/profiles/<id>/         one core's data dir: config.json, cache.db
//! ```
//!
//! Secrets stay Kotlin's: the GitHub token and device token of each profile
//! are kept in the Keystore under the profile id and handed to that profile's
//! core with `set_github_token` / `set_remote`, exactly as for a single core.

mod store;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rostrum_remote::{Endpoint, PairingCode};

use crate::{
    engine::RostrumCore,
    error::RostrumError,
    remote::{PairingResult, link_offer, manual_offer},
};
use store::{ProfileRecord, RegistryFile, StoredKind, is_profile_id, new_id, now_ms};

/// What a profile is for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ProfileKind {
    /// Paired with a desktop, identified by its certificate.
    Desktop {
        machine: String,
        /// `4F2A · 91C0 · 7E3B`.
        fingerprint_short: String,
    },
    /// A GitHub token alone, with no desktop.
    TokenOnly,
}

/// One profile in the list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProfileInfo {
    /// Stable random id, 16 hex characters. Key Keystore secrets by it.
    pub id: String,
    /// The machine name by default; renameable.
    pub label: String,
    pub kind: ProfileKind,
    /// The last GitHub login Kotlin recorded for it (not a secret).
    pub github_login: Option<String>,
    pub created_at_ms: i64,
    pub last_used_ms: i64,
}

/// A pairing, and the profile it landed in.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProfilePairing {
    pub profile: ProfileInfo,
    /// `true` for a new profile; `false` when the desktop was already known
    /// by its fingerprint and was re-paired into its existing profile.
    pub created: bool,
    /// Persist its secrets under `profile.id`.
    pub pairing: PairingResult,
}

/// The phone's profiles. Open one per process with [`ProfileRegistry::open`];
/// get each profile's core from [`ProfileRegistry::core`], never by opening
/// its directory directly.
#[derive(uniffi::Object)]
pub struct ProfileRegistry {
    root: PathBuf,
    /// The registry as on disk. Every edit is written before memory changes,
    /// so the two cannot disagree. A plain mutex: the sync methods cannot
    /// await, and no lock is held across an await.
    file: Mutex<RegistryFile>,
    /// Open cores by profile id. Held across opening and removal, so one
    /// directory never has two cores and a removed profile is not reopened.
    cores: tokio::sync::Mutex<HashMap<String, Arc<RostrumCore>>>,
    /// Pairings one at a time, so the same desktop paired twice at once
    /// cannot make two profiles.
    pairing: tokio::sync::Mutex<()>,
}

impl ProfileRegistry {
    fn registry_path(&self) -> PathBuf {
        self.root.join("profiles.json")
    }

    fn profiles_dir(root: &Path) -> PathBuf {
        root.join("profiles")
    }

    fn profile_dir(&self, id: &str) -> PathBuf {
        Self::profiles_dir(&self.root).join(id)
    }

    fn read<R>(&self, read: impl FnOnce(&RegistryFile) -> R) -> Result<R, RostrumError> {
        let file = self
            .file
            .lock()
            .map_err(|_| RostrumError::internal("the profile registry lock was poisoned"))?;
        Ok(read(&file))
    }

    /// Change the registry: on a copy, written to disk, then adopted.
    fn edit<R>(
        &self,
        edit: impl FnOnce(&mut RegistryFile) -> Result<R, RostrumError>,
    ) -> Result<R, RostrumError> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| RostrumError::internal("the profile registry lock was poisoned"))?;
        let mut next = file.clone();
        let result = edit(&mut next)?;
        next.save(&self.registry_path())?;
        *file = next;
        Ok(result)
    }

    fn ensure_exists(&self, id: &str) -> Result<(), RostrumError> {
        if self.read(|file| file.get(id).is_some())? {
            Ok(())
        } else {
            Err(RostrumError::ProfileNotFound { id: id.to_string() })
        }
    }

    /// Delete profile directories no profile names: left by a crash in the
    /// middle of creating one. Only directories shaped like an id are touched.
    fn remove_orphans(root: &Path, file: &RegistryFile) {
        let Ok(entries) = std::fs::read_dir(Self::profiles_dir(root)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_profile_id(&name) && file.get(&name).is_none() {
                tracing::warn!(profile = %name, "removing an unregistered profile directory");
                if let Err(error) = std::fs::remove_dir_all(entry.path()) {
                    tracing::warn!(profile = %name, %error, "could not remove it");
                }
            }
        }
    }

    /// Pair into the profile of the desktop presenting this endpoint's
    /// certificate, or into a new one. A new profile is registered only once
    /// the pairing succeeded; on failure its directory is removed.
    async fn pair(
        &self,
        endpoint: Endpoint,
        code: PairingCode,
        device_name: String,
    ) -> Result<ProfilePairing, RostrumError> {
        let _one_at_a_time = self.pairing.lock().await;
        let fingerprint = endpoint.fingerprint();
        let existing = self.read(|file| file.by_fingerprint(fingerprint).map(|p| p.id.clone()))?;

        if let Some(id) = existing {
            let core = self.core(id.clone()).await?;
            let pairing = core.pair(endpoint, code, device_name).await?;
            let machine = pairing.machine.name.clone();
            let profile = self.edit(|file| {
                let record = file.get_mut(&id)?;
                record.kind = StoredKind::Desktop {
                    machine,
                    fingerprint,
                };
                Ok(record.info())
            })?;
            tracing::info!(profile = %profile.id, "re-paired into an existing profile");
            return Ok(ProfilePairing {
                profile,
                created: false,
                pairing,
            });
        }

        let id = new_id()?;
        let dir = self.profile_dir(&id);
        let cleanup = |dir: &Path| {
            if let Err(error) = std::fs::remove_dir_all(dir)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(dir = %dir.display(), %error, "could not remove a failed profile");
            }
        };
        let core = match RostrumCore::open(dir.to_string_lossy().into_owned()).await {
            Ok(core) => core,
            Err(error) => {
                cleanup(&dir);
                return Err(error);
            }
        };
        let pairing = match core.pair(endpoint, code, device_name).await {
            Ok(pairing) => pairing,
            Err(error) => {
                core.close_storage().await;
                drop(core);
                cleanup(&dir);
                return Err(error);
            }
        };
        let now = now_ms();
        let record = ProfileRecord {
            id: id.clone(),
            label: pairing.machine.name.clone(),
            kind: StoredKind::Desktop {
                machine: pairing.machine.name.clone(),
                fingerprint,
            },
            github_login: None,
            created_at_ms: now,
            last_used_ms: now,
        };
        let profile = record.info();
        if let Err(error) = self.edit(|file| {
            file.profiles.push(record);
            Ok(())
        }) {
            core.close_storage().await;
            drop(core);
            cleanup(&dir);
            return Err(error);
        }
        self.cores.lock().await.insert(id, core);
        tracing::info!(profile = %profile.id, "paired a new desktop profile");
        Ok(ProfilePairing {
            profile,
            created: true,
            pairing,
        })
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl ProfileRegistry {
    /// Open the registry under `root_dir` (app-private), creating it on first
    /// use. Profile directories left behind by an interrupted profile
    /// creation are removed.
    #[uniffi::constructor]
    pub fn open(root_dir: String) -> Result<Arc<Self>, RostrumError> {
        if root_dir.trim().is_empty() {
            return Err(RostrumError::invalid("the profile root directory is empty"));
        }
        let root = PathBuf::from(root_dir);
        std::fs::create_dir_all(Self::profiles_dir(&root)).map_err(|error| {
            RostrumError::Storage {
                reason: format!("could not create {}: {error}", root.display()),
            }
        })?;
        let file = RegistryFile::load(&root.join("profiles.json"))?;
        Self::remove_orphans(&root, &file);
        Ok(Arc::new(Self {
            root,
            file: Mutex::new(file),
            cores: tokio::sync::Mutex::new(HashMap::new()),
            pairing: tokio::sync::Mutex::new(()),
        }))
    }

    /// Every profile, most recently used first.
    pub fn profiles(&self) -> Vec<ProfileInfo> {
        self.read(RegistryFile::ordered).unwrap_or_default()
    }

    /// The active profile's id, if one is set.
    pub fn active_profile(&self) -> Option<String> {
        self.read(|file| file.active.clone()).ok().flatten()
    }

    /// Make a profile active and mark it used now.
    pub fn set_active_profile(&self, id: String) -> Result<ProfileInfo, RostrumError> {
        self.edit(|file| {
            let record = file.get_mut(&id)?;
            record.last_used_ms = now_ms();
            let info = record.info();
            file.active = Some(id.clone());
            Ok(info)
        })
    }

    /// The profile's core, opened on first use and shared after: the same id
    /// always returns the same object while the profile exists.
    pub async fn core(&self, id: String) -> Result<Arc<RostrumCore>, RostrumError> {
        self.ensure_exists(&id)?;
        let mut cores = self.cores.lock().await;
        // Checked again under the lock: a removal may have finished while
        // this call waited for it, and must not be undone by reopening.
        self.ensure_exists(&id)?;
        if let Some(core) = cores.get(&id) {
            return Ok(core.clone());
        }
        let core = RostrumCore::open(self.profile_dir(&id).to_string_lossy().into_owned()).await?;
        cores.insert(id, core.clone());
        Ok(core)
    }

    /// A profile for a GitHub token alone. Hand the token to its core.
    pub async fn create_token_profile(&self, label: String) -> Result<ProfileInfo, RostrumError> {
        let label = label.trim().to_string();
        if label.is_empty() {
            return Err(RostrumError::invalid("a profile needs a name"));
        }
        let now = now_ms();
        let record = ProfileRecord {
            id: new_id()?,
            label,
            kind: StoredKind::TokenOnly,
            github_login: None,
            created_at_ms: now,
            last_used_ms: now,
        };
        let info = record.info();
        self.edit(|file| {
            file.profiles.push(record);
            Ok(())
        })?;
        Ok(info)
    }

    /// Pair from a rostrum://pair link. If a profile already exists for the same
    /// certificate fingerprint it re-pairs INTO that profile (created = false);
    /// otherwise it creates a new Desktop profile. Does not change the active profile.
    pub async fn pair_desktop_with_link(
        &self,
        uri: String,
        device_name: String,
    ) -> Result<ProfilePairing, RostrumError> {
        let (endpoint, code) = link_offer(&uri)?;
        self.pair(endpoint, code, device_name).await
    }

    /// Pair by address and typed code, pinned to the fingerprint the core's
    /// `probe_desktop` reported; otherwise as `pair_desktop_with_link`.
    pub async fn pair_desktop_manual(
        &self,
        host: String,
        port: u16,
        fingerprint: String,
        code: String,
        device_name: String,
    ) -> Result<ProfilePairing, RostrumError> {
        let (endpoint, code) = manual_offer(&host, port, &fingerprint, &code)?;
        self.pair(endpoint, code, device_name).await
    }

    pub fn rename_profile(&self, id: String, label: String) -> Result<ProfileInfo, RostrumError> {
        let label = label.trim().to_string();
        if label.is_empty() {
            return Err(RostrumError::invalid("a profile needs a name"));
        }
        self.edit(|file| {
            let record = file.get_mut(&id)?;
            record.label = label;
            Ok(record.info())
        })
    }

    /// Record the GitHub login the profile's token belongs to (for the list);
    /// blank or `None` clears it.
    pub fn set_profile_login(
        &self,
        id: String,
        login: Option<String>,
    ) -> Result<ProfileInfo, RostrumError> {
        let login = login
            .map(|login| login.trim().to_string())
            .filter(|login| !login.is_empty());
        self.edit(|file| {
            let record = file.get_mut(&id)?;
            record.github_login = login;
            Ok(record.info())
        })
    }

    /// Best-effort unpair from its desktop (ignore unreachable), close its core,
    /// delete its data dir, and clear active if it was active. Kotlin deletes its secrets.
    ///
    /// The unpair is attempted when the profile's core is open with a remote
    /// set; every failure there is logged and ignored, since removing the
    /// profile must not depend on a desktop that may be gone for good.
    pub async fn remove_profile(&self, id: String) -> Result<(), RostrumError> {
        self.ensure_exists(&id)?;
        let open = self.cores.lock().await.get(&id).cloned();
        if let Some(core) = open {
            match core.unpair().await {
                Ok(()) | Err(RostrumError::NotPaired) => {}
                Err(error) => {
                    tracing::warn!(profile = %id, %error, "could not unpair; removing anyway");
                }
            }
        }

        let mut cores = self.cores.lock().await;
        if let Some(core) = cores.remove(&id) {
            core.close_storage().await;
        }
        let dir = self.profile_dir(&id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(RostrumError::Storage {
                    reason: format!("could not delete {}: {error}", dir.display()),
                });
            }
        }
        self.edit(|file| {
            file.profiles.retain(|profile| profile.id != id);
            if file.active.as_deref() == Some(id.as_str()) {
                file.active = None;
            }
            Ok(())
        })?;
        drop(cores);
        tracing::info!(profile = %id, "profile removed");
        Ok(())
    }
}
