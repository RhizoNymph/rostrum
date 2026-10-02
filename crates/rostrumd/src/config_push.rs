//! A phone sending its settings to the desktop: `PUT /api/v1/config`.
//!
//! The push replaces the shareable settings (`rostrum_config::shared`) and
//! nothing else. One task, [`ConfigWriter`], owns every write the daemon
//! makes to `config.json`, so two pushes cannot interleave a read and a
//! write; each push:
//!
//! 1. reads the file as it is now (refusing one that is not JSON);
//! 2. compares the push's `base` with the revision of the shareable settings
//!    on disk, and stops with [`PushResult::Changed`] if they differ;
//! 3. applies the push to those settings ([`apply`]) — a field the push leaves
//!    `None` keeps its value;
//! 4. overlays only the shareable keys onto the document, so the desktop's own
//!    settings and keys this build does not know survive, and writes it
//!    atomically. The desktop app notices the new file and reloads.

use std::{collections::BTreeMap, path::PathBuf};

use rostrum_config::{Config, ConfigError, SharedSettings, document, overlay_shared};
use rostrum_core::{LoginKey, RepoId};
use rostrum_remote::{ConfigPush, ConfigRevision, DesktopConfig, RevisedConfig};
use tokio::sync::{mpsc, oneshot};

use crate::rostrum_config::revised_config;

/// Bounds the desktop's own fetches respect: one GraphQL page.
pub const PER_REPO: std::ops::RangeInclusive<u32> = 1..=100;
/// GitHub's own limit on a login.
const MAX_LOGIN: usize = 39;

/// Why a push was refused before anything was written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PushInvalid {
    #[error("{0} is listed twice")]
    DuplicateRepo(RepoId),
    #[error("{field} must be between 1 and 100, not {value}")]
    OutOfRange { field: &'static str, value: u32 },
    #[error("`{0}` is not a GitHub login")]
    Login(String),
    #[error("{0} has trunks listed twice")]
    DuplicateTrunks(RepoId),
}

/// A GitHub login: letters, digits and single hyphens, not at either end, at
/// most 39 characters. `LoginKey` has already lowercased and trimmed it.
fn is_login(login: &LoginKey) -> bool {
    let text = login.as_str();
    !text.is_empty()
        && text.len() <= MAX_LOGIN
        && !text.starts_with('-')
        && !text.ends_with('-')
        && !text.contains("--")
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Check a push's values, everything that can be checked without the file.
pub fn validate(push: &DesktopConfig) -> Result<(), PushInvalid> {
    let mut seen = std::collections::BTreeSet::new();
    for repo in &push.repos {
        if !seen.insert(repo) {
            return Err(PushInvalid::DuplicateRepo(repo.clone()));
        }
    }
    let range = |field, value: u32| {
        if PER_REPO.contains(&value) {
            Ok(())
        } else {
            Err(PushInvalid::OutOfRange { field, value })
        }
    };
    range("prs_per_repo", push.prs_per_repo)?;
    if let Some(issues) = push.issues_per_repo {
        range("issues_per_repo", issues)?;
    }
    if let Some(login) = push.authors.iter().find(|login| !is_login(login)) {
        return Err(PushInvalid::Login(login.to_string()));
    }
    if let Some(trunks) = &push.trunks {
        let mut repos = std::collections::BTreeSet::new();
        for entry in trunks {
            if !repos.insert(&entry.repo) {
                return Err(PushInvalid::DuplicateTrunks(entry.repo.clone()));
            }
        }
    }
    Ok(())
}

/// `current` with the push applied. A field the push leaves `None` keeps
/// `current`'s value. Assumes [`validate`] passed.
pub fn apply(current: &SharedSettings, push: &DesktopConfig) -> SharedSettings {
    SharedSettings {
        repos: push.repos.iter().map(ToString::to_string).collect(),
        prs_per_repo: push.prs_per_repo,
        issues_per_repo: push.issues_per_repo.unwrap_or(current.issues_per_repo),
        hide_drafts: push.hide_drafts,
        hide_empty_repos: push.hide_empty_repos,
        authors: push.authors.iter().cloned().collect(),
        include_involved: push.include_involved,
        autostash: push.autostash,
        repo_sort: push.repo_sort.unwrap_or(current.repo_sort),
        item_sort: push.item_sort.unwrap_or(current.item_sort),
        trunks: match &push.trunks {
            None => current.trunks.clone(),
            Some(trunks) => trunks
                .iter()
                .map(|entry| {
                    (
                        entry.repo.to_string(),
                        entry.trunks.iter().map(ToString::to_string).collect(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        },
    }
}

/// How a push the writer accepted ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushResult {
    /// Written (or nothing to write). `changed` lists the keys that moved.
    Applied {
        config: RevisedConfig,
        changed: Vec<&'static str>,
    },
    /// Not written: the shareable settings are no longer at the push's base.
    Changed(RevisedConfig),
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The file is JSON but not a config this build can read.
    #[error("{} is not a config rostrum can read; fix it on the desktop first", .0.display())]
    Unreadable(PathBuf),
    #[error("the config writer has stopped")]
    Stopped,
}

struct Request {
    push: ConfigPush,
    reply: oneshot::Sender<Result<PushResult, WriteError>>,
}

/// The one writer of `config.json` inside the daemon. Cheap to clone.
#[derive(Clone, Debug)]
pub struct ConfigWriter {
    tx: mpsc::Sender<Request>,
}

impl ConfigWriter {
    /// Start the writer for `path`. Must be called inside a Tokio runtime.
    pub fn spawn(path: PathBuf) -> Self {
        let (tx, mut rx) = mpsc::channel::<Request>(16);
        tokio::spawn(async move {
            while let Some(Request { push, reply }) = rx.recv().await {
                // One push at a time, read to write: nothing else in the
                // daemon writes this file.
                let _ = reply.send(write(&path, &push));
            }
        });
        Self { tx }
    }

    /// Apply a validated push.
    pub async fn push(&self, push: ConfigPush) -> Result<PushResult, WriteError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Request { push, reply })
            .await
            .map_err(|_| WriteError::Stopped)?;
        answer.await.map_err(|_| WriteError::Stopped)?
    }
}

fn write(path: &std::path::Path, push: &ConfigPush) -> Result<PushResult, WriteError> {
    let (mut document, current) = match document::read(path)? {
        None => (serde_json::Value::Null, Config::default()),
        Some(snapshot) => {
            let config = snapshot
                .config
                .ok_or_else(|| WriteError::Unreadable(path.to_path_buf()))?;
            (snapshot.document, config)
        }
    };
    let before = current.shared();
    if let Some(base) = &push.base
        && *base != ConfigRevision(before.revision())
    {
        return Ok(PushResult::Changed(revised_config(&current)));
    }
    let after = apply(&before, &push.config);
    let changed = before.changed_keys(&after);
    if changed.is_empty() {
        return Ok(PushResult::Applied {
            config: revised_config(&current),
            changed,
        });
    }
    overlay_shared(&mut document, &after);
    document::write_atomic(path, &document)?;
    let mut written = current;
    written.set_shared(after);
    Ok(PushResult::Applied {
        config: revised_config(&written),
        changed,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rostrum_core::{ItemSortKey, Sort, branches::TrunkName};
    use rostrum_remote::RepoTrunks;
    use serde_json::json;

    use super::*;
    use crate::{fsutil::ScratchDir, rostrum_config::desktop_config};

    fn push_of(config: DesktopConfig, base: Option<ConfigRevision>) -> ConfigPush {
        ConfigPush { config, base }
    }

    fn seed(path: &std::path::Path) -> serde_json::Value {
        let document = json!({
            "repos": ["old/one"],
            "refresh_secs": 45,
            "notifications": true,
            "feed_tab": "issues",
            "clones": {"old/one": "~/src/one"},
            "conflict_handler": {"command": "claude {context}"},
            "a_key_from_the_future": {"kept": [1, 2, 3]},
        });
        std::fs::write(path, serde_json::to_vec_pretty(&document).expect("json")).expect("seed");
        document
    }

    fn proposed() -> DesktopConfig {
        DesktopConfig {
            repos: vec![RepoId::new("new", "two"), RepoId::new("new", "three")],
            prs_per_repo: 40,
            hide_drafts: true,
            hide_empty_repos: false,
            authors: vec![LoginKey::new("Ada-Lin")],
            include_involved: true,
            autostash: true,
            issues_per_repo: Some(10),
            repo_sort: None,
            item_sort: Some(Sort::new(ItemSortKey::Title)),
            trunks: Some(vec![RepoTrunks {
                repo: RepoId::new("new", "two"),
                trunks: vec![TrunkName::parse("main").expect("valid")],
            }]),
        }
    }

    #[test]
    fn values_are_validated() {
        assert!(validate(&proposed()).is_ok());
        let mut dup = proposed();
        dup.repos.push(RepoId::new("new", "two"));
        assert_eq!(
            validate(&dup),
            Err(PushInvalid::DuplicateRepo(RepoId::new("new", "two")))
        );
        let mut zero = proposed();
        zero.prs_per_repo = 0;
        assert!(matches!(
            validate(&zero),
            Err(PushInvalid::OutOfRange {
                field: "prs_per_repo",
                ..
            })
        ));
        let mut many = proposed();
        many.issues_per_repo = Some(101);
        assert!(matches!(
            validate(&many),
            Err(PushInvalid::OutOfRange {
                field: "issues_per_repo",
                ..
            })
        ));
        for bad in [
            "",
            "-ada",
            "ada-",
            "a--b",
            "ada lin",
            "ünï",
            &"a".repeat(40),
        ] {
            let mut login = proposed();
            login.authors = vec![LoginKey::new(bad)];
            assert!(
                matches!(validate(&login), Err(PushInvalid::Login(_))),
                "{bad:?}"
            );
        }
        let mut trunks = proposed();
        trunks.trunks = Some(vec![
            RepoTrunks {
                repo: RepoId::new("new", "two"),
                trunks: vec![],
            },
            RepoTrunks {
                repo: RepoId::new("new", "two"),
                trunks: vec![],
            },
        ]);
        assert!(matches!(
            validate(&trunks),
            Err(PushInvalid::DuplicateTrunks(_))
        ));
    }

    #[test]
    fn unset_fields_keep_the_desktops_values() {
        let current = Config::default().shared();
        let mut push = proposed();
        push.issues_per_repo = None;
        push.repo_sort = None;
        push.item_sort = None;
        push.trunks = None;
        let applied = apply(&current, &push);
        assert_eq!(applied.issues_per_repo, current.issues_per_repo);
        assert_eq!(applied.repo_sort, current.repo_sort);
        assert_eq!(applied.item_sort, current.item_sort);
        assert_eq!(applied.trunks, current.trunks);
        assert_eq!(applied.prs_per_repo, 40);
        assert_eq!(
            applied.repos,
            vec!["new/two".to_string(), "new/three".to_string()]
        );
    }

    #[tokio::test]
    async fn only_the_shareable_keys_change_and_everything_else_survives() {
        let scratch = ScratchDir::new("push-only");
        let path = scratch.join("config.json");
        let before = seed(&path);
        let writer = ConfigWriter::spawn(path.clone());

        let result = writer
            .push(push_of(proposed(), None))
            .await
            .expect("writes");
        let PushResult::Applied { config, changed } = result else {
            panic!("applied");
        };
        assert!(changed.contains(&"repos") && changed.contains(&"item_sort"));
        assert!(!changed.contains(&"repo_sort"), "left unset");

        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        for key in [
            "refresh_secs",
            "notifications",
            "feed_tab",
            "clones",
            "conflict_handler",
            "a_key_from_the_future",
        ] {
            assert_eq!(after[key], before[key], "{key} must survive a push");
        }
        assert_eq!(after["repos"], json!(["new/two", "new/three"]));
        assert_eq!(after["authors"], json!(["ada-lin"]));
        assert_eq!(after["trunks"], json!({"new/two": ["main"]}));
        // What the push answered is what a GET now reads.
        let reread: Config = serde_json::from_value(after).expect("config");
        assert_eq!(revised_config(&reread), config);
    }

    #[tokio::test]
    async fn a_stale_base_writes_nothing_and_returns_the_current_settings() {
        let scratch = ScratchDir::new("push-stale");
        let path = scratch.join("config.json");
        seed(&path);
        let bytes_before = std::fs::read(&path).expect("read");
        let writer = ConfigWriter::spawn(path.clone());

        let stale = ConfigRevision("0000".into());
        let result = writer
            .push(push_of(proposed(), Some(stale)))
            .await
            .expect("answers");
        let PushResult::Changed(current) = result else {
            panic!("changed");
        };
        assert_eq!(current.config.repos, vec![RepoId::new("old", "one")]);
        assert_eq!(
            std::fs::read(&path).expect("read"),
            bytes_before,
            "untouched"
        );

        // With the current revision as base it applies.
        let fresh = writer
            .push(push_of(proposed(), Some(current.revision.clone())))
            .await
            .expect("writes");
        assert!(matches!(fresh, PushResult::Applied { .. }));
    }

    #[tokio::test]
    async fn concurrent_pushes_on_one_base_apply_exactly_once() {
        let scratch = ScratchDir::new("push-race");
        let path = scratch.join("config.json");
        seed(&path);
        let base = {
            let config: Config =
                serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
            ConfigRevision(config.shared().revision())
        };
        let writer = ConfigWriter::spawn(path.clone());
        let mut tasks = Vec::new();
        for n in 1..=12u32 {
            let writer = writer.clone();
            let base = base.clone();
            tasks.push(tokio::spawn(async move {
                let mut push = proposed();
                push.prs_per_repo = n;
                writer
                    .push(push_of(push, Some(base)))
                    .await
                    .expect("answers")
            }));
        }
        let mut applied = 0;
        let mut changed = 0;
        for task in tasks {
            match task.await.expect("joins") {
                PushResult::Applied { .. } => applied += 1,
                PushResult::Changed(_) => changed += 1,
            }
        }
        assert_eq!((applied, changed), (1, 11), "one wins; the rest see it");
        // The file is whole and readable.
        let config: Config =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert!(PER_REPO.contains(&config.prs_per_repo));
    }

    #[tokio::test]
    async fn concurrent_unconditional_pushes_leave_a_whole_file() {
        let scratch = ScratchDir::new("push-many");
        let path = scratch.join("config.json");
        seed(&path);
        let writer = Arc::new(ConfigWriter::spawn(path.clone()));
        let mut tasks = Vec::new();
        for n in 1..=20u32 {
            let writer = writer.clone();
            tasks.push(tokio::spawn(async move {
                let mut push = proposed();
                push.prs_per_repo = n;
                writer.push(push_of(push, None)).await.expect("answers")
            }));
        }
        for task in tasks {
            assert!(matches!(
                task.await.expect("joins"),
                PushResult::Applied { .. }
            ));
        }
        let after: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("whole JSON");
        assert_eq!(
            after["conflict_handler"],
            json!({"command": "claude {context}"})
        );
    }

    #[tokio::test]
    async fn a_push_that_changes_nothing_writes_nothing() {
        let scratch = ScratchDir::new("push-noop");
        let path = scratch.join("config.json");
        seed(&path);
        let bytes_before = std::fs::read(&path).expect("read");
        let current: Config = serde_json::from_slice(&bytes_before).expect("json");
        let writer = ConfigWriter::spawn(path.clone());
        let result = writer
            .push(push_of(desktop_config(&current), None))
            .await
            .expect("answers");
        assert!(matches!(result, PushResult::Applied { ref changed, .. } if changed.is_empty()));
        assert_eq!(std::fs::read(&path).expect("read"), bytes_before);
    }

    #[tokio::test]
    async fn a_file_that_is_not_json_is_refused_and_left_alone() {
        let scratch = ScratchDir::new("push-malformed");
        let path = scratch.join("config.json");
        std::fs::write(&path, "{ not json").expect("seed");
        let writer = ConfigWriter::spawn(path.clone());
        assert!(matches!(
            writer.push(push_of(proposed(), None)).await,
            Err(WriteError::Config(ConfigError::Malformed { .. }))
        ));
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "{ not json");
    }

    #[tokio::test]
    async fn a_missing_file_is_created_from_the_defaults_plus_the_push() {
        let scratch = ScratchDir::new("push-missing");
        let path = scratch.join("config.json");
        let writer = ConfigWriter::spawn(path.clone());
        writer
            .push(push_of(proposed(), None))
            .await
            .expect("writes");
        let config: Config =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert_eq!(config.prs_per_repo, 40);
        assert_eq!(config.refresh_secs, Config::default().refresh_secs);
    }
}
