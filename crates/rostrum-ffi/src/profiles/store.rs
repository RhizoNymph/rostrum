//! `profiles.json`: which profiles exist and which is active.
//!
//! No secret is ever written here. A profile's GitHub token and device token
//! are Kotlin's, kept in the Keystore under the profile id; what this file
//! holds is what the profile list shows, plus the desktop's certificate
//! fingerprint (public — it is in every pairing link) so a re-pairing lands in
//! the profile it belongs to.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use rostrum_remote::CertFingerprint;
use serde::{Deserialize, Serialize};

use crate::{
    error::RostrumError,
    profiles::{ProfileInfo, ProfileKind},
};

/// Bumped when the file's shape changes incompatibly.
const FILE_VERSION: u32 = 1;
/// Bytes of randomness in a profile id: 16 hex characters.
const ID_BYTES: usize = 8;

/// One profile, as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProfileRecord {
    pub id: String,
    pub label: String,
    pub kind: StoredKind,
    pub github_login: Option<String>,
    pub created_at_ms: i64,
    pub last_used_ms: i64,
}

/// What a profile is for. A desktop profile keeps the full fingerprint, which
/// is what identifies the desktop across re-pairings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StoredKind {
    Desktop {
        machine: String,
        fingerprint: CertFingerprint,
    },
    TokenOnly,
}

impl ProfileRecord {
    pub(crate) fn info(&self) -> ProfileInfo {
        ProfileInfo {
            id: self.id.clone(),
            label: self.label.clone(),
            kind: match &self.kind {
                StoredKind::Desktop {
                    machine,
                    fingerprint,
                } => ProfileKind::Desktop {
                    machine: machine.clone(),
                    fingerprint_short: fingerprint.short(),
                },
                StoredKind::TokenOnly => ProfileKind::TokenOnly,
            },
            github_login: self.github_login.clone(),
            created_at_ms: self.created_at_ms,
            last_used_ms: self.last_used_ms,
        }
    }

    pub(crate) fn fingerprint(&self) -> Option<CertFingerprint> {
        match &self.kind {
            StoredKind::Desktop { fingerprint, .. } => Some(*fingerprint),
            StoredKind::TokenOnly => None,
        }
    }
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RegistryFile {
    pub version: u32,
    pub active: Option<String>,
    pub profiles: Vec<ProfileRecord>,
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            version: FILE_VERSION,
            active: None,
            profiles: Vec::new(),
        }
    }
}

impl RegistryFile {
    /// Read the file; a missing one is an empty registry. An unreadable or
    /// malformed one is an error rather than a fresh start: profile data
    /// directories sit beside it, and pretending there are none would orphan
    /// them.
    pub(crate) fn load(path: &Path) -> Result<Self, RostrumError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(RostrumError::Storage {
                    reason: format!("could not read {}: {error}", path.display()),
                });
            }
        };
        let mut file: Self =
            serde_json::from_str(&text).map_err(|error| RostrumError::Storage {
                reason: format!("{} is not a profile registry: {error}", path.display()),
            })?;
        if file.version != FILE_VERSION {
            return Err(RostrumError::Storage {
                reason: format!(
                    "{} is version {}; this build reads version {FILE_VERSION}",
                    path.display(),
                    file.version
                ),
            });
        }
        // An active id naming no profile points at nothing; drop it.
        if let Some(active) = &file.active
            && file.get(active).is_none()
        {
            file.active = None;
        }
        Ok(file)
    }

    /// Write the file atomically: a temporary beside it, then a rename, so a
    /// crash leaves either the old registry or the new one, never half.
    pub(crate) fn save(&self, path: &Path) -> Result<(), RostrumError> {
        let storage = |error: std::io::Error| RostrumError::Storage {
            reason: format!("could not write {}: {error}", path.display()),
        };
        let text = serde_json::to_string_pretty(self).map_err(|error| {
            RostrumError::internal(format!("could not encode profiles: {error}"))
        })?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(storage)?;
        std::fs::rename(&temporary, path).map_err(storage)
    }

    pub(crate) fn get(&self, id: &str) -> Option<&ProfileRecord> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Result<&mut ProfileRecord, RostrumError> {
        self.profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or_else(|| RostrumError::ProfileNotFound { id: id.to_string() })
    }

    /// The profile paired with the desktop presenting `fingerprint`.
    pub(crate) fn by_fingerprint(&self, fingerprint: CertFingerprint) -> Option<&ProfileRecord> {
        self.profiles
            .iter()
            .find(|profile| profile.fingerprint() == Some(fingerprint))
    }

    /// Most recently used first; ties by newest created, then by id, so the
    /// order never shuffles between two reads.
    pub(crate) fn ordered(&self) -> Vec<ProfileInfo> {
        let mut profiles: Vec<&ProfileRecord> = self.profiles.iter().collect();
        profiles.sort_by(|a, b| {
            b.last_used_ms
                .cmp(&a.last_used_ms)
                .then(b.created_at_ms.cmp(&a.created_at_ms))
                .then(a.id.cmp(&b.id))
        });
        profiles.into_iter().map(ProfileRecord::info).collect()
    }
}

/// A fresh random profile id: 16 lowercase hex characters.
pub(crate) fn new_id() -> Result<String, RostrumError> {
    let mut bytes = [0u8; ID_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| RostrumError::internal(format!("no randomness available: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Whether `name` has the shape of a profile id, so only directories this
/// registry could have made are ever considered for cleanup.
pub(crate) fn is_profile_id(name: &str) -> bool {
    name.len() == ID_BYTES * 2
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Milliseconds since the Unix epoch, now.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, created: i64, used: i64) -> ProfileRecord {
        ProfileRecord {
            id: id.into(),
            label: format!("profile {id}"),
            kind: StoredKind::TokenOnly,
            github_login: None,
            created_at_ms: created,
            last_used_ms: used,
        }
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rostrum-profiles-{tag}-{}", now_ms()));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_missing_file_is_an_empty_registry() {
        let dir = scratch("missing");
        assert_eq!(
            RegistryFile::load(&dir.join("profiles.json")).expect("load"),
            RegistryFile::default()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_registry_round_trips_atomically() {
        let dir = scratch("round-trip");
        let path = dir.join("profiles.json");
        let file = RegistryFile {
            version: FILE_VERSION,
            active: Some("aaaaaaaaaaaaaaaa".into()),
            profiles: vec![
                record("aaaaaaaaaaaaaaaa", 1, 5),
                ProfileRecord {
                    kind: StoredKind::Desktop {
                        machine: "desk".into(),
                        fingerprint: CertFingerprint::of_der(b"cert"),
                    },
                    github_login: Some("octocat".into()),
                    ..record("bbbbbbbbbbbbbbbb", 2, 3)
                },
            ],
        };
        file.save(&path).expect("save");
        assert_eq!(RegistryFile::load(&path).expect("load"), file);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_malformed_file_is_an_error_not_a_fresh_start() {
        let dir = scratch("malformed");
        let path = dir.join("profiles.json");
        std::fs::write(&path, "{ nope").expect("write");
        assert!(matches!(
            RegistryFile::load(&path),
            Err(RostrumError::Storage { .. })
        ));
        std::fs::write(&path, r#"{"version":99,"active":null,"profiles":[]}"#).expect("write");
        assert!(matches!(
            RegistryFile::load(&path),
            Err(RostrumError::Storage { .. })
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_active_id_naming_no_profile_is_dropped_on_load() {
        let dir = scratch("dangling");
        let path = dir.join("profiles.json");
        RegistryFile {
            active: Some("0123456789abcdef".into()),
            ..RegistryFile::default()
        }
        .save(&path)
        .expect("save");
        assert_eq!(RegistryFile::load(&path).expect("load").active, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn profiles_are_ordered_most_recently_used_first() {
        let file = RegistryFile {
            profiles: vec![
                record("c", 1, 10),
                record("a", 5, 30),
                record("b", 2, 30),
                record("d", 9, 30),
                record("e", 9, 30),
            ],
            ..RegistryFile::default()
        };
        let ids: Vec<String> = file.ordered().into_iter().map(|info| info.id).collect();
        // Used at 30: newest created first (d and e tie, by id), then a, b.
        assert_eq!(ids, vec!["d", "e", "a", "b", "c"]);
    }

    #[test]
    fn a_desktop_profile_is_found_by_its_fingerprint_and_shows_the_short_form() {
        let fingerprint = CertFingerprint::of_der(b"cert");
        let file = RegistryFile {
            profiles: vec![
                record("a", 1, 1),
                ProfileRecord {
                    kind: StoredKind::Desktop {
                        machine: "desk".into(),
                        fingerprint,
                    },
                    ..record("b", 1, 1)
                },
            ],
            ..RegistryFile::default()
        };
        let found = file.by_fingerprint(fingerprint).expect("found");
        assert_eq!(found.id, "b");
        assert_eq!(
            found.info().kind,
            ProfileKind::Desktop {
                machine: "desk".into(),
                fingerprint_short: fingerprint.short()
            }
        );
        assert!(
            file.by_fingerprint(CertFingerprint::of_der(b"other"))
                .is_none()
        );
    }

    #[test]
    fn ids_are_sixteen_random_hex_characters() {
        let a = new_id().expect("id");
        let b = new_id().expect("id");
        assert!(is_profile_id(&a) && is_profile_id(&b), "{a} {b}");
        assert_ne!(a, b);
        for not_an_id in [
            "",
            "0123456789ABCDEF",
            "0123456789abcde",
            "0123456789abcdefg",
            "../../etc/passw",
        ] {
            assert!(!is_profile_id(not_an_id), "{not_an_id}");
        }
    }

    #[test]
    fn a_missing_profile_is_reported_by_id() {
        let mut file = RegistryFile::default();
        assert_eq!(
            file.get_mut("0123456789abcdef").map(|_| ()),
            Err(RostrumError::ProfileNotFound {
                id: "0123456789abcdef".into()
            })
        );
    }
}
