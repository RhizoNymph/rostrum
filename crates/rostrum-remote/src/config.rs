//! Sharing settings between a phone and the desktop, both ways.
//!
//! `GET /api/v1/config` answers a [`RevisedConfig`]: the shareable settings
//! ([`DesktopConfig`]) plus an opaque [`ConfigRevision`] of them. An older
//! phone reads the same JSON as a plain `DesktopConfig` and ignores the
//! revision.
//!
//! `PUT /api/v1/config` sends a [`ConfigPush`]: settings to replace the
//! desktop's with, and the revision they were previewed against. If the
//! desktop's settings have changed since, nothing is written and the answer
//! is 409 [`crate::ApiErrorCode::ConfigChanged`] with a [`ConfigConflict`]
//! body carrying the current settings, so the phone can show the user the new
//! difference before trying again. Without a `base` the push applies
//! unconditionally.
//!
//! The preview is computed on the phone with [`diff`]: there is no preview
//! route, because the phone already has both sides.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::api::{ApiErrorCode, DesktopConfig, RepoTrunks};

/// An opaque fingerprint of the desktop's shareable settings. Equal
/// revisions mean equal settings; nothing else about it is meaningful.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigRevision(pub String);

impl fmt::Display for ConfigRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `GET /api/v1/config`, and the answer to a successful `PUT`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisedConfig {
    #[serde(flatten)]
    pub config: DesktopConfig,
    pub revision: ConfigRevision,
}

/// `PUT /api/v1/config`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigPush {
    /// The settings to replace the desktop's with. `None` in an optional
    /// field leaves the desktop's value alone.
    #[serde(flatten)]
    pub config: DesktopConfig,
    /// The revision the user previewed against. Present ⇒ refused with 409
    /// if the desktop's settings have changed since.
    #[serde(default)]
    pub base: Option<ConfigRevision>,
}

/// The body of a 409 `config_changed`: an `ApiError` (`code`, `message`)
/// plus the desktop's settings as they are now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigConflict {
    pub code: ApiErrorCode,
    pub message: String,
    pub current: RevisedConfig,
}

/// How a push ended, when the desktop answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigPushOutcome {
    /// Written; the desktop's settings are now these.
    Applied(RevisedConfig),
    /// Not written: the desktop's settings changed since `base`. These are
    /// what they are now.
    Changed(RevisedConfig),
}

/// One shareable setting, by its `config.json` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigField {
    Repos,
    PrsPerRepo,
    IssuesPerRepo,
    HideDrafts,
    HideEmptyRepos,
    Authors,
    IncludeInvolved,
    Autostash,
    RepoSort,
    ItemSort,
    Trunks,
}

impl ConfigField {
    /// The key in `config.json`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Repos => "repos",
            Self::PrsPerRepo => "prs_per_repo",
            Self::IssuesPerRepo => "issues_per_repo",
            Self::HideDrafts => "hide_drafts",
            Self::HideEmptyRepos => "hide_empty_repos",
            Self::Authors => "authors",
            Self::IncludeInvolved => "include_involved",
            Self::Autostash => "autostash",
            Self::RepoSort => "repo_sort",
            Self::ItemSort => "item_sort",
            Self::Trunks => "trunks",
        }
    }

    /// A label for a preview.
    pub fn label(self) -> &'static str {
        match self {
            Self::Repos => "Repositories",
            Self::PrsPerRepo => "Pull requests per repository",
            Self::IssuesPerRepo => "Issues per repository",
            Self::HideDrafts => "Hide drafts",
            Self::HideEmptyRepos => "Hide empty repositories",
            Self::Authors => "Authors",
            Self::IncludeInvolved => "Include involved",
            Self::Autostash => "Autostash",
            Self::RepoSort => "Repository order",
            Self::ItemSort => "Item order",
            Self::Trunks => "Trunks",
        }
    }
}

/// One setting a push would change, with both values written out for a
/// preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigChange {
    pub field: ConfigField,
    pub before: String,
    pub after: String,
}

/// What pushing `proposed` would change in `current`, in field order. An
/// optional field the push leaves as `None` changes nothing.
pub fn diff(current: &DesktopConfig, proposed: &DesktopConfig) -> Vec<ConfigChange> {
    let mut changes = Vec::new();
    let mut check = |field: ConfigField, before: String, after: Option<String>| {
        if let Some(after) = after
            && after != before
        {
            changes.push(ConfigChange {
                field,
                before,
                after,
            });
        }
    };
    let list = |items: Vec<String>| {
        if items.is_empty() {
            "(none)".to_string()
        } else {
            items.join(", ")
        }
    };
    let repos =
        |config: &DesktopConfig| list(config.repos.iter().map(ToString::to_string).collect());
    let authors =
        |config: &DesktopConfig| list(config.authors.iter().map(ToString::to_string).collect());
    let trunks = |trunks: &Option<Vec<RepoTrunks>>| {
        trunks.as_ref().map(|all| {
            list(
                all.iter()
                    .map(|entry| {
                        let names: Vec<String> =
                            entry.trunks.iter().map(ToString::to_string).collect();
                        format!("{}: {}", entry.repo, names.join(" "))
                    })
                    .collect(),
            )
        })
    };
    let unset = || "(unset)".to_string();

    check(ConfigField::Repos, repos(current), Some(repos(proposed)));
    check(
        ConfigField::PrsPerRepo,
        current.prs_per_repo.to_string(),
        Some(proposed.prs_per_repo.to_string()),
    );
    check(
        ConfigField::IssuesPerRepo,
        current
            .issues_per_repo
            .map_or_else(unset, |n| n.to_string()),
        proposed.issues_per_repo.map(|n| n.to_string()),
    );
    check(
        ConfigField::HideDrafts,
        current.hide_drafts.to_string(),
        Some(proposed.hide_drafts.to_string()),
    );
    check(
        ConfigField::HideEmptyRepos,
        current.hide_empty_repos.to_string(),
        Some(proposed.hide_empty_repos.to_string()),
    );
    check(
        ConfigField::Authors,
        authors(current),
        Some(authors(proposed)),
    );
    check(
        ConfigField::IncludeInvolved,
        current.include_involved.to_string(),
        Some(proposed.include_involved.to_string()),
    );
    check(
        ConfigField::Autostash,
        current.autostash.to_string(),
        Some(proposed.autostash.to_string()),
    );
    check(
        ConfigField::RepoSort,
        current.repo_sort.map_or_else(unset, |s| s.summary()),
        proposed.repo_sort.map(|s| s.summary()),
    );
    check(
        ConfigField::ItemSort,
        current.item_sort.map_or_else(unset, |s| s.summary()),
        proposed.item_sort.map(|s| s.summary()),
    );
    check(
        ConfigField::Trunks,
        trunks(&current.trunks).unwrap_or_else(unset),
        trunks(&proposed.trunks),
    );
    changes
}

#[cfg(test)]
mod tests {
    use rostrum_core::{ItemSortKey, LoginKey, RepoId, RepoSortKey, Sort, branches::TrunkName};
    use serde_json::json;

    use super::*;

    fn desktop() -> DesktopConfig {
        DesktopConfig {
            repos: vec![RepoId::new("o", "a"), RepoId::new("o", "b")],
            prs_per_repo: 25,
            hide_drafts: false,
            hide_empty_repos: true,
            authors: vec![LoginKey::new("ada")],
            include_involved: false,
            autostash: false,
            issues_per_repo: Some(25),
            repo_sort: Some(Sort::new(RepoSortKey::Name)),
            item_sort: Some(Sort::new(ItemSortKey::Updated)),
            trunks: Some(vec![RepoTrunks {
                repo: RepoId::new("o", "a"),
                trunks: vec![TrunkName::parse("main").expect("valid")],
            }]),
        }
    }

    #[test]
    fn a_revised_config_is_a_desktop_config_plus_a_revision() {
        let revised = RevisedConfig {
            config: desktop(),
            revision: ConfigRevision("abc123".into()),
        };
        let value = serde_json::to_value(&revised).expect("serialises");
        assert_eq!(value["revision"], "abc123");
        assert_eq!(value["prs_per_repo"], 25);
        assert_eq!(value["trunks"][0]["trunks"], json!(["main"]));
        // An older phone reads the same JSON as a plain DesktopConfig.
        let old: DesktopConfig = serde_json::from_value(value.clone()).expect("older reader");
        assert_eq!(old, desktop());
        assert_eq!(
            serde_json::from_value::<RevisedConfig>(value).expect("round trip"),
            revised
        );
    }

    #[test]
    fn an_older_desktops_config_reads_with_the_new_fields_unset() {
        let value = json!({
            "repos": [{"owner": "o", "name": "a"}],
            "prs_per_repo": 25,
            "hide_drafts": false,
            "hide_empty_repos": true,
            "authors": [],
            "include_involved": false,
            "autostash": false,
        });
        let config: DesktopConfig = serde_json::from_value(value).expect("parses");
        assert_eq!(config.issues_per_repo, None);
        assert_eq!(config.trunks, None);
        // And unset fields are not written back out.
        let back = serde_json::to_value(&config).expect("serialises");
        assert!(back.get("repo_sort").is_none());
    }

    #[test]
    fn a_push_carries_its_base_beside_the_settings() {
        let push = ConfigPush {
            config: desktop(),
            base: Some(ConfigRevision("r1".into())),
        };
        let value = serde_json::to_value(&push).expect("serialises");
        assert_eq!(value["base"], "r1");
        assert_eq!(value["hide_empty_repos"], true);
        assert_eq!(
            serde_json::from_value::<ConfigPush>(value).expect("parses"),
            push
        );
        let unconditional: ConfigPush =
            serde_json::from_value(serde_json::to_value(desktop()).expect("json"))
                .expect("a bare DesktopConfig is a push without a base");
        assert_eq!(unconditional.base, None);
    }

    #[test]
    fn bad_values_do_not_parse() {
        let mut value = serde_json::to_value(desktop()).expect("json");
        value["trunks"][0]["trunks"] = json!(["-bad"]);
        assert!(serde_json::from_value::<ConfigPush>(value).is_err());
        let mut value = serde_json::to_value(desktop()).expect("json");
        value["repos"] = json!(["not-structured"]);
        assert!(serde_json::from_value::<ConfigPush>(value).is_err());
    }

    #[test]
    fn a_conflict_is_an_api_error_with_the_current_settings() {
        let conflict = ConfigConflict {
            code: ApiErrorCode::ConfigChanged,
            message: "changed".into(),
            current: RevisedConfig {
                config: desktop(),
                revision: ConfigRevision("r2".into()),
            },
        };
        let value = serde_json::to_value(&conflict).expect("serialises");
        assert_eq!(value["code"], "config_changed");
        // A generic client still reads it as an ApiError.
        let generic: crate::ApiError = serde_json::from_value(value.clone()).expect("api error");
        assert_eq!(generic.code, ApiErrorCode::ConfigChanged);
        assert_eq!(ApiErrorCode::ConfigChanged.http_status(), 409);
        assert_eq!(value["current"]["revision"], "r2");
    }

    #[test]
    fn the_diff_lists_exactly_what_would_change() {
        let current = desktop();
        assert!(diff(&current, &current).is_empty());

        let mut proposed = current.clone();
        proposed.repos.push(RepoId::new("o", "c"));
        proposed.hide_drafts = true;
        proposed.item_sort = Some(Sort::new(ItemSortKey::Title));
        let changes = diff(&current, &proposed);
        let fields: Vec<ConfigField> = changes.iter().map(|c| c.field).collect();
        assert_eq!(
            fields,
            vec![
                ConfigField::Repos,
                ConfigField::HideDrafts,
                ConfigField::ItemSort
            ]
        );
        assert_eq!(changes[0].before, "o/a, o/b");
        assert_eq!(changes[0].after, "o/a, o/b, o/c");
        assert_eq!(changes[1].after, "true");
    }

    #[test]
    fn a_field_the_push_leaves_unset_changes_nothing() {
        let current = desktop();
        let mut proposed = current.clone();
        proposed.issues_per_repo = None;
        proposed.repo_sort = None;
        proposed.item_sort = None;
        proposed.trunks = None;
        assert!(diff(&current, &proposed).is_empty());
    }

    #[test]
    fn every_field_has_its_config_key() {
        assert_eq!(ConfigField::PrsPerRepo.key(), "prs_per_repo");
        assert_eq!(
            serde_json::to_value(ConfigField::ItemSort).expect("json"),
            "item_sort"
        );
        assert_eq!(ConfigField::Trunks.label(), "Trunks");
    }
}
