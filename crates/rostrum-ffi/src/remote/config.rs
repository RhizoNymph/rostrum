//! Copying the paired desktop's config: which repositories to watch, how many
//! pull requests and issues to fetch, the feed's filter preferences and
//! sorts, the trunks, and the stash default. The refresh interval, the
//! notification switches and the search query stay the phone's own.
//!
//! The preview also carries the desktop's revision and both differences —
//! what copying would change here, and what pushing this phone's settings
//! would change there (`remote::push`) — computed with `rostrum_remote::diff`
//! on the two sides' shareable settings.

use std::collections::BTreeSet;

use rostrum_config::Config;
use rostrum_core::{FeedFilter, LoginKey, RepoId, branches::TrunkName};
use rostrum_remote::{
    ConfigChange as WireChange, ConfigField as WireField, DesktopConfig, RevisedConfig,
    api::RepoTrunks, diff,
};

use crate::{
    engine::RostrumCore,
    error::RostrumError,
    settings::{ISSUES_PER_REPO, PRS_PER_REPO, Settings},
};

/// One shareable setting, by its key in `config.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
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

impl From<WireField> for ConfigField {
    fn from(field: WireField) -> Self {
        match field {
            WireField::Repos => Self::Repos,
            WireField::PrsPerRepo => Self::PrsPerRepo,
            WireField::IssuesPerRepo => Self::IssuesPerRepo,
            WireField::HideDrafts => Self::HideDrafts,
            WireField::HideEmptyRepos => Self::HideEmptyRepos,
            WireField::Authors => Self::Authors,
            WireField::IncludeInvolved => Self::IncludeInvolved,
            WireField::Autostash => Self::Autostash,
            WireField::RepoSort => Self::RepoSort,
            WireField::ItemSort => Self::ItemSort,
            WireField::Trunks => Self::Trunks,
        }
    }
}

/// One setting that would change, both values written out.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConfigChange {
    pub field: ConfigField,
    /// `Pull requests per repository`.
    pub label: String,
    pub before: String,
    pub after: String,
}

impl From<WireChange> for ConfigChange {
    fn from(change: WireChange) -> Self {
        Self {
            field: change.field.into(),
            label: change.field.label().to_string(),
            before: change.before,
            after: change.after,
        }
    }
}

/// What copying the desktop's config would do, shown before doing it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DesktopConfigPreview {
    /// The desktop's machine name.
    pub machine: String,
    /// The desktop's repositories as `owner/name`, in its order.
    pub repos: Vec<String>,
    /// In the desktop's list, not on this phone.
    pub added: Vec<String>,
    /// On this phone, not in the desktop's list: copying drops them.
    pub removed: Vec<String>,
    pub prs_per_repo: u32,
    pub hide_drafts: bool,
    pub hide_empty_repos: bool,
    pub authors: Vec<String>,
    pub include_involved: bool,
    pub autostash: bool,
    /// `false` when copying would change nothing.
    pub changes_anything: bool,
    /// The desktop's revision of these settings: pass it to
    /// `push_config_to_desktop` so a push refuses to overwrite a change made
    /// since. Empty only in a preview built without one.
    #[uniffi(default = "")]
    pub revision: String,
    /// `None` from a desktop that does not share it.
    #[uniffi(default)]
    pub issues_per_repo: Option<u32>,
    /// What copying the desktop's settings would change on this phone.
    #[uniffi(default)]
    pub copy_changes: Vec<ConfigChange>,
    /// What pushing this phone's settings would change on the desktop.
    #[uniffi(default)]
    pub push_changes: Vec<ConfigChange>,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Read the paired desktop's shareable config and diff it against this
    /// phone's settings.
    pub async fn desktop_config(&self) -> Result<DesktopConfigPreview, RostrumError> {
        let client = self.remote().await?;
        let (machine, desktop) = tokio::join!(client.machine(), client.config_with_revision());
        let (machine, desktop) = (machine?, desktop?);
        self.actor
            .call(move |state| revised_preview(&machine.name, &desktop, &state.config))
            .await
    }

    /// Replace this phone's repositories, prs_per_repo, feed preferences
    /// (hide drafts, hide empty repos, authors, include involved) and
    /// autostash with the desktop's. It re-fetches the config instead of
    /// applying a possibly stale preview, persists it, drops feed state for
    /// removed repos, notifies the feed observer, and returns the new
    /// Settings. The caller then runs refresh_feed.
    pub async fn copy_desktop_config(&self) -> Result<Settings, RostrumError> {
        let client = self.remote().await?;
        let desktop = client.config().await?;
        self.change_config(
            move |_, config| {
                apply(config, &desktop);
                Ok(())
            },
            |state, ()| {
                let (ids, _) = state.config.repo_ids();
                for id in state.feed.set_repos(ids) {
                    state.forget_repo(&id);
                }
                // The search box is a half-finished action, not a setting.
                state.feed.filter = FeedFilter {
                    query: std::mem::take(&mut state.feed.filter.query),
                    ..state.config.feed_filter()
                };
                state.publish();
                tracing::info!(
                    repos = state.config.repos.len(),
                    "copied the desktop's config"
                );
                Ok(state.settings())
            },
        )
        .await
    }
}

/// The desktop's repositories, in its order, without repeats.
fn desktop_repos(desktop: &DesktopConfig) -> Vec<RepoId> {
    let mut seen = BTreeSet::new();
    desktop
        .repos
        .iter()
        .filter(|repo| seen.insert((*repo).clone()))
        .cloned()
        .collect()
}

/// The desktop's author filter, in its order, without blanks or repeats.
fn desktop_authors(desktop: &DesktopConfig) -> Vec<LoginKey> {
    let mut seen = BTreeSet::new();
    desktop
        .authors
        .iter()
        .filter(|login| !login.is_empty() && seen.insert((*login).clone()))
        .cloned()
        .collect()
}

/// Replace `config`'s copyable settings with the desktop's. The refresh
/// interval and notification switches are the phone's own and stay; a clone
/// path follows its repository out, as `Config::remove_repo` does.
pub(crate) fn apply(config: &mut Config, desktop: &DesktopConfig) {
    config.repos = desktop_repos(desktop)
        .iter()
        .map(ToString::to_string)
        .collect();
    let watched: BTreeSet<String> = config.repos.iter().cloned().collect();
    config.clones.retain(|repo, _| watched.contains(repo));
    config.prs_per_repo = desktop
        .prs_per_repo
        .clamp(*PRS_PER_REPO.start(), *PRS_PER_REPO.end());
    config.hide_drafts = desktop.hide_drafts;
    config.hide_empty_repos = desktop.hide_empty_repos;
    config.authors = desktop_authors(desktop).into_iter().collect();
    config.include_involved = desktop.include_involved;
    config.autostash = desktop.autostash;
    // A desktop too old to share these leaves the phone's alone.
    if let Some(issues) = desktop.issues_per_repo {
        config.issues_per_repo = issues.clamp(*ISSUES_PER_REPO.start(), *ISSUES_PER_REPO.end());
    }
    if let Some(sort) = desktop.repo_sort {
        config.repo_sort = sort;
    }
    if let Some(sort) = desktop.item_sort {
        config.item_sort = sort;
    }
    if let Some(trunks) = &desktop.trunks {
        config.trunks = trunks
            .iter()
            .map(|entry| {
                (
                    entry.repo.to_string(),
                    entry.trunks.iter().map(ToString::to_string).collect(),
                )
            })
            .collect();
    }
}

/// This phone's shareable settings in the protocol's shape, every optional
/// field present: what a push sends, and the phone's side of both diffs.
/// Entries that do not parse are left out, as the desktop leaves them out.
pub(crate) fn shareable(config: &Config) -> DesktopConfig {
    let (repos, _) = config.repo_ids();
    DesktopConfig {
        repos,
        prs_per_repo: config
            .prs_per_repo
            .clamp(*PRS_PER_REPO.start(), *PRS_PER_REPO.end()),
        hide_drafts: config.hide_drafts,
        hide_empty_repos: config.hide_empty_repos,
        authors: config
            .authors
            .iter()
            .filter(|login| !login.is_empty())
            .cloned()
            .collect(),
        include_involved: config.include_involved,
        autostash: config.autostash,
        issues_per_repo: Some(
            config
                .issues_per_repo
                .clamp(*ISSUES_PER_REPO.start(), *ISSUES_PER_REPO.end()),
        ),
        repo_sort: Some(config.repo_sort),
        item_sort: Some(config.item_sort),
        trunks: Some(
            config
                .trunks
                .iter()
                .filter_map(|(repo, names)| {
                    Some(RepoTrunks {
                        repo: repo.parse().ok()?,
                        trunks: names
                            .iter()
                            .filter_map(|name| TrunkName::parse(name).ok())
                            .collect(),
                    })
                })
                .collect(),
        ),
    }
}

/// [`preview`] of a revised config, with its revision and both differences.
pub(crate) fn revised_preview(
    machine: &str,
    desktop: &RevisedConfig,
    phone: &Config,
) -> DesktopConfigPreview {
    let mine = shareable(phone);
    DesktopConfigPreview {
        revision: desktop.revision.0.clone(),
        copy_changes: diff(&mine, &desktop.config)
            .into_iter()
            .map(Into::into)
            .collect(),
        push_changes: diff(&desktop.config, &mine)
            .into_iter()
            .map(Into::into)
            .collect(),
        ..preview(machine, &desktop.config, phone)
    }
}

/// Diff the desktop's config against the phone's.
///
/// `changes_anything` is decided by applying the copy to a clone of the
/// phone's config and comparing the two, so it cannot disagree with what
/// copying does. A phone entry that does not parse is no repository the
/// desktop has, so it is listed as removed.
pub(crate) fn preview(
    machine: &str,
    desktop: &DesktopConfig,
    phone: &Config,
) -> DesktopConfigPreview {
    let desktop_ids = desktop_repos(desktop);
    let phone_ids: BTreeSet<RepoId> = phone
        .repos
        .iter()
        .filter_map(|entry| entry.parse().ok())
        .collect();
    let added: Vec<String> = desktop_ids
        .iter()
        .filter(|repo| !phone_ids.contains(*repo))
        .map(ToString::to_string)
        .collect();
    let mut seen = BTreeSet::new();
    let removed: Vec<String> = phone
        .repos
        .iter()
        .filter(|entry| match entry.parse::<RepoId>() {
            Ok(id) => !desktop_ids.contains(&id),
            // Not a repository at all, so not one the desktop has.
            Err(_) => true,
        })
        .filter(|entry| seen.insert((*entry).clone()))
        .cloned()
        .collect();

    let mut copied = phone.clone();
    apply(&mut copied, desktop);
    let changes_anything = serde_json::to_value(&copied).ok() != serde_json::to_value(phone).ok();

    DesktopConfigPreview {
        machine: machine.to_string(),
        repos: desktop_ids.iter().map(ToString::to_string).collect(),
        added,
        removed,
        prs_per_repo: copied.prs_per_repo,
        hide_drafts: copied.hide_drafts,
        hide_empty_repos: copied.hide_empty_repos,
        authors: desktop_authors(desktop)
            .iter()
            .map(|login| login.as_str().to_string())
            .collect(),
        include_involved: copied.include_involved,
        autostash: copied.autostash,
        changes_anything,
        revision: String::new(),
        issues_per_repo: desktop.issues_per_repo,
        copy_changes: Vec::new(),
        push_changes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf};

    use rostrum_core::{LoginKey, RepoId};

    use super::*;

    fn repo(name: &str) -> RepoId {
        name.parse().expect("repo")
    }

    fn desktop() -> DesktopConfig {
        DesktopConfig {
            repos: vec![repo("zed-industries/zed"), repo("octo/repo")],
            prs_per_repo: 30,
            hide_drafts: true,
            hide_empty_repos: false,
            authors: vec![LoginKey::new("Alice"), LoginKey::new("bob")],
            include_involved: true,
            autostash: true,
            // An older desktop: the later fields absent.
            issues_per_repo: None,
            repo_sort: None,
            item_sort: None,
            trunks: None,
        }
    }

    /// The phone as a fresh install left it, watching `repos`.
    fn phone(repos: &[&str]) -> Config {
        Config {
            repos: repos.iter().map(|repo| repo.to_string()).collect(),
            ..Default::default()
        }
    }

    /// The phone after copying `desktop()`, reached by hand rather than
    /// through `apply`.
    fn copied() -> Config {
        Config {
            repos: vec!["zed-industries/zed".into(), "octo/repo".into()],
            prs_per_repo: 30,
            hide_drafts: true,
            hide_empty_repos: false,
            authors: BTreeSet::from([LoginKey::new("alice"), LoginKey::new("bob")]),
            include_involved: true,
            autostash: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_preview_lists_what_would_be_added_and_dropped() {
        let preview = preview("desk", &desktop(), &phone(&["octo/repo", "rust-lang/rust"]));
        assert_eq!(preview.machine, "desk");
        assert_eq!(preview.repos, vec!["zed-industries/zed", "octo/repo"]);
        assert_eq!(preview.added, vec!["zed-industries/zed"]);
        assert_eq!(preview.removed, vec!["rust-lang/rust"]);
        assert_eq!(preview.prs_per_repo, 30);
        assert!(preview.hide_drafts && !preview.hide_empty_repos);
        assert_eq!(preview.authors, vec!["alice", "bob"]);
        assert!(preview.include_involved && preview.autostash);
        assert!(preview.changes_anything);
    }

    #[test]
    fn a_phone_already_matching_the_desktop_changes_nothing() {
        let preview = preview("desk", &desktop(), &copied());
        assert!(preview.added.is_empty() && preview.removed.is_empty());
        assert!(!preview.changes_anything);
    }

    #[test]
    fn the_phones_own_settings_do_not_count_as_changes() {
        let mut phone = copied();
        phone.refresh_secs = 600;
        phone.notifications = true;
        phone.notify_review_requests = true;
        assert!(!preview("desk", &desktop(), &phone).changes_anything);
    }

    #[test]
    fn any_copied_setting_that_differs_is_a_change() {
        let edits: [fn(&mut Config); 7] = [
            |config| config.prs_per_repo = 25,
            |config| config.hide_drafts = false,
            |config| config.hide_empty_repos = true,
            |config| {
                config.authors.remove(&LoginKey::new("bob"));
            },
            |config| config.include_involved = false,
            |config| config.autostash = false,
            // The same repositories in another order reorder the feed.
            |config| config.repos.reverse(),
        ];
        for (ix, edit) in edits.into_iter().enumerate() {
            let mut phone = copied();
            edit(&mut phone);
            let preview = preview("desk", &desktop(), &phone);
            assert!(preview.changes_anything, "edit {ix}");
            assert!(
                preview.added.is_empty() && preview.removed.is_empty(),
                "edit {ix}"
            );
        }
    }

    /// An entry the phone cannot parse is not a repository the desktop has;
    /// copying drops it like any other.
    #[test]
    fn a_malformed_phone_entry_is_listed_as_removed() {
        let preview = preview("desk", &desktop(), &phone(&["octo/repo", "not a repo"]));
        assert_eq!(preview.removed, vec!["not a repo"]);
    }

    #[test]
    fn applying_replaces_the_copyable_settings_and_keeps_the_phones_own() {
        let mut config = Config {
            repos: vec!["rust-lang/rust".into()],
            refresh_secs: 600,
            notifications: true,
            notify_review_requests: true,
            clones: [
                ("rust-lang/rust".to_string(), PathBuf::from("/r")),
                ("octo/repo".to_string(), PathBuf::from("/o")),
            ]
            .into(),
            ..Default::default()
        };
        apply(&mut config, &desktop());
        assert_eq!(config.repos, vec!["zed-industries/zed", "octo/repo"]);
        assert_eq!(config.prs_per_repo, 30);
        assert!(config.hide_drafts && !config.hide_empty_repos);
        assert_eq!(
            config.authors,
            BTreeSet::from([LoginKey::new("alice"), LoginKey::new("bob")])
        );
        assert!(config.include_involved && config.autostash);
        // The phone's own habits survive.
        assert_eq!(config.refresh_secs, 600);
        assert!(config.notifications && config.notify_review_requests);
        // A clone path follows its repository out, as `remove_repo` does.
        assert_eq!(config.clones.keys().collect::<Vec<_>>(), vec!["octo/repo"]);
        assert!(!preview("desk", &desktop(), &config).changes_anything);
    }

    #[test]
    fn applying_normalises_what_the_desktop_sent() {
        let mut desktop = desktop();
        desktop.prs_per_repo = 0;
        desktop.repos.push(repo("octo/repo"));
        desktop.authors.push(LoginKey::new("  "));
        let mut config = Config::default();
        apply(&mut config, &desktop);
        assert_eq!(config.prs_per_repo, 1);
        assert_eq!(config.repos, vec!["zed-industries/zed", "octo/repo"]);
        assert_eq!(config.authors.len(), 2);
        let preview = preview("desk", &desktop, &Config::default());
        assert_eq!(preview.prs_per_repo, 1);
        assert_eq!(preview.repos, vec!["zed-industries/zed", "octo/repo"]);
        assert_eq!(preview.authors, vec!["alice", "bob"]);
    }

    #[test]
    fn a_newer_desktops_sorts_trunks_and_issue_count_are_copied() {
        let mut desktop = desktop();
        desktop.issues_per_repo = Some(500);
        desktop.item_sort = Some(rostrum_core::Sort::new(rostrum_core::ItemSortKey::Title));
        desktop.trunks = Some(vec![RepoTrunks {
            repo: repo("octo/repo"),
            trunks: vec![TrunkName::parse("develop").expect("trunk")],
        }]);
        let mut config = Config::default();
        apply(&mut config, &desktop);
        assert_eq!(config.issues_per_repo, 100, "clamped");
        assert_eq!(config.item_sort.key(), rostrum_core::ItemSortKey::Title);
        // Absent from the desktop: the phone's own stays.
        assert_eq!(config.repo_sort, Config::default().repo_sort);
        assert_eq!(
            config.trunks.get("octo/repo"),
            Some(&vec!["develop".to_string()])
        );
    }

    #[test]
    fn the_phones_shareable_settings_round_trip_through_a_copy() {
        let mut phone = copied();
        phone.issues_per_repo = 40;
        phone
            .trunks
            .insert("octo/repo".into(), vec!["release".into()]);
        phone
            .trunks
            .insert("not a repo".into(), vec!["main".into()]);
        let shared = shareable(&phone);
        assert_eq!(shared.issues_per_repo, Some(40));
        assert_eq!(shared.trunks.as_ref().map(Vec::len), Some(1));
        // Copying the phone's own settings back changes nothing shareable.
        let mut copy = Config::default();
        apply(&mut copy, &shared);
        assert!(diff(&shareable(&copy), &shared).is_empty());
        let revised = RevisedConfig {
            config: shared,
            revision: rostrum_remote::ConfigRevision("r9".into()),
        };
        let preview = revised_preview("desk", &revised, &phone);
        assert_eq!(preview.revision, "r9");
        assert!(preview.copy_changes.is_empty() && preview.push_changes.is_empty());
    }
}
