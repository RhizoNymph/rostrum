//! The settings a paired phone and the desktop share.
//!
//! What to watch and how to read it: the repositories, how many pull requests
//! and issues to fetch, the feed filter, the two sorts, the trunks per
//! repository, and the stash default. The rest of the file stays with the
//! machine it is on:
//!
//! - `clones` and `conflict_handler` describe this computer's disk, and a
//!   handler command can carry secrets;
//! - `refresh_secs`, `notifications` and `notify_review_requests` are each
//!   device's own habits;
//! - `feed_tab` is where the user is right now, like a search half typed.
//!
//! [`SharedSettings`] is that subset in [`Config`]'s own types. Its
//! [`revision`](SharedSettings::revision) is a hash of exactly those values, so
//! "has anything a phone could have seen changed?" has one answer, and
//! [`overlay_shared`] writes them into a config document without touching any
//! other key — including ones this build does not know.

use std::collections::{BTreeMap, BTreeSet};

use rostrum_core::{ItemSortKey, LoginKey, RepoSortKey, Sort};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Config;

/// The JSON keys of `config.json` that [`SharedSettings`] owns. Every other
/// key in the file belongs to the machine.
pub const SHARED_KEYS: [&str; 11] = [
    "repos",
    "prs_per_repo",
    "issues_per_repo",
    "hide_drafts",
    "hide_empty_repos",
    "authors",
    "include_involved",
    "autostash",
    "repo_sort",
    "item_sort",
    "trunks",
];

/// The shareable part of a [`Config`]. Field names are the file's keys, so
/// serialising this is serialising those keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedSettings {
    pub repos: Vec<String>,
    pub prs_per_repo: u32,
    pub issues_per_repo: u32,
    pub hide_drafts: bool,
    pub hide_empty_repos: bool,
    pub authors: BTreeSet<LoginKey>,
    pub include_involved: bool,
    pub autostash: bool,
    pub repo_sort: Sort<RepoSortKey>,
    pub item_sort: Sort<ItemSortKey>,
    pub trunks: BTreeMap<String, Vec<String>>,
}

impl SharedSettings {
    /// A short, stable fingerprint of these values: the first 16 bytes of the
    /// SHA-256 of their JSON, as hex. Stable across runs and machines (field
    /// order is fixed; sets and maps are ordered), so a revision handed to a
    /// phone still means the same thing after a restart.
    pub fn revision(&self) -> String {
        // Serialising plain values cannot fail.
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        Sha256::digest(&bytes)[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// The keys whose values differ between `self` and `other`, in
    /// [`SHARED_KEYS`] order, for logging what a change touched.
    pub fn changed_keys(&self, other: &Self) -> Vec<&'static str> {
        let (Ok(serde_json::Value::Object(a)), Ok(serde_json::Value::Object(b))) =
            (serde_json::to_value(self), serde_json::to_value(other))
        else {
            return Vec::new();
        };
        SHARED_KEYS
            .into_iter()
            .filter(|key| a.get(*key) != b.get(*key))
            .collect()
    }
}

impl Config {
    /// The shareable settings this config holds.
    pub fn shared(&self) -> SharedSettings {
        SharedSettings {
            repos: self.repos.clone(),
            prs_per_repo: self.prs_per_repo,
            issues_per_repo: self.issues_per_repo,
            hide_drafts: self.hide_drafts,
            hide_empty_repos: self.hide_empty_repos,
            authors: self.authors.clone(),
            include_involved: self.include_involved,
            autostash: self.autostash,
            repo_sort: self.repo_sort,
            item_sort: self.item_sort,
            trunks: self.trunks.clone(),
        }
    }

    /// Replace exactly the shareable settings, leaving every machine-specific
    /// one as it is. A clone or trunk entry for a repository that is no longer
    /// watched is *not* dropped here: the clone is the machine's, and the
    /// desktop drops it when it removes a repository itself.
    pub fn set_shared(&mut self, shared: SharedSettings) {
        let SharedSettings {
            repos,
            prs_per_repo,
            issues_per_repo,
            hide_drafts,
            hide_empty_repos,
            authors,
            include_involved,
            autostash,
            repo_sort,
            item_sort,
            trunks,
        } = shared;
        self.repos = repos;
        self.prs_per_repo = prs_per_repo;
        self.issues_per_repo = issues_per_repo;
        self.hide_drafts = hide_drafts;
        self.hide_empty_repos = hide_empty_repos;
        self.authors = authors;
        self.include_involved = include_involved;
        self.autostash = autostash;
        self.repo_sort = repo_sort;
        self.item_sort = item_sort;
        self.trunks = trunks;
    }
}

/// Write `shared` into a config document, replacing only [`SHARED_KEYS`].
/// Every other key — the machine's own settings and keys this build does not
/// know — keeps its value. A document that is not an object becomes one.
pub fn overlay_shared(document: &mut serde_json::Value, shared: &SharedSettings) {
    overlay(document, serde_json::to_value(shared).unwrap_or_default());
}

/// Write every key `config` serialises into a config document, keeping the
/// keys it does not know about. The desktop's save goes through this, so a
/// key added by a newer build or by hand survives an older build's save.
pub fn overlay_config(document: &mut serde_json::Value, config: &Config) {
    overlay(document, serde_json::to_value(config).unwrap_or_default());
}

fn overlay(document: &mut serde_json::Value, values: serde_json::Value) {
    let serde_json::Value::Object(values) = values else {
        return;
    };
    if !document.is_object() {
        *document = serde_json::Value::Object(serde_json::Map::new());
    }
    if let serde_json::Value::Object(target) = document {
        for (key, value) in values {
            target.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::ItemSortKey;
    use serde_json::json;

    use super::*;

    fn shared() -> SharedSettings {
        Config::default().shared()
    }

    #[test]
    fn the_shared_keys_are_exactly_the_shared_fields() {
        let value = serde_json::to_value(shared()).expect("serialises");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = SHARED_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);

        // And every shared key is a real key of the config file.
        let config = serde_json::to_value(Config::default()).expect("serialises");
        for key in SHARED_KEYS {
            assert!(config.get(key).is_some(), "{key} is not a config key");
        }
    }

    #[test]
    fn machine_settings_are_not_shared() {
        for key in [
            "clones",
            "conflict_handler",
            "refresh_secs",
            "notifications",
            "notify_review_requests",
            "feed_tab",
        ] {
            assert!(
                !SHARED_KEYS.contains(&key),
                "{key} must stay on the machine"
            );
        }
    }

    #[test]
    fn the_revision_is_stable_and_tracks_every_shared_value() {
        let base = shared();
        assert_eq!(base.revision(), shared().revision());
        assert_eq!(base.revision().len(), 32);

        let mut changed = base.clone();
        changed.item_sort = Sort::new(ItemSortKey::Title);
        assert_ne!(changed.revision(), base.revision());
        assert_eq!(changed.changed_keys(&base), vec!["item_sort"]);

        let mut trunks = base.clone();
        trunks
            .trunks
            .insert("o/r".into(), vec!["main".into(), "staging".into()]);
        assert_ne!(trunks.revision(), base.revision());
        assert_eq!(trunks.changed_keys(&base), vec!["trunks"]);
    }

    #[test]
    fn machine_settings_do_not_move_the_revision() {
        let mut config = Config::default();
        let before = config.shared().revision();
        config.refresh_secs = 999;
        config.notifications = true;
        config
            .clones
            .insert("o/r".into(), std::path::PathBuf::from("/src/r"));
        assert_eq!(config.shared().revision(), before);
    }

    #[test]
    fn setting_shared_values_leaves_the_machine_ones() {
        let mut config = Config {
            refresh_secs: 30,
            notifications: true,
            ..Config::default()
        };
        config
            .clones
            .insert("o/r".into(), std::path::PathBuf::from("/src/r"));
        let mut incoming = config.shared();
        incoming.repos = vec!["a/b".into()];
        incoming.prs_per_repo = 7;
        config.set_shared(incoming.clone());
        assert_eq!(config.shared(), incoming);
        assert_eq!(config.refresh_secs, 30);
        assert!(config.notifications);
        assert_eq!(config.clones.len(), 1, "the machine's clone is kept");
    }

    #[test]
    fn overlaying_replaces_only_the_shared_keys() {
        let mut document = json!({
            "repos": ["old/one"],
            "prs_per_repo": 25,
            "clones": {"old/one": "~/src/one"},
            "conflict_handler": {"command": "claude {context}"},
            "refresh_secs": 45,
            "a_key_from_the_future": {"kept": [1, 2, 3]},
        });
        let mut incoming = shared();
        incoming.repos = vec!["new/two".into()];
        incoming.prs_per_repo = 40;
        overlay_shared(&mut document, &incoming);

        assert_eq!(document["repos"], json!(["new/two"]));
        assert_eq!(document["prs_per_repo"], 40);
        assert_eq!(document["clones"], json!({"old/one": "~/src/one"}));
        assert_eq!(
            document["conflict_handler"],
            json!({"command": "claude {context}"})
        );
        assert_eq!(document["refresh_secs"], 45);
        assert_eq!(
            document["a_key_from_the_future"],
            json!({"kept": [1, 2, 3]})
        );
        // The overlaid document still reads back as the incoming settings.
        let config: Config = serde_json::from_value(document).expect("parses");
        assert_eq!(config.shared(), incoming);
    }

    #[test]
    fn overlaying_a_whole_config_keeps_unknown_keys() {
        let mut document = json!({"a_key_from_the_future": true, "refresh_secs": 1});
        let config = Config {
            refresh_secs: 90,
            ..Config::default()
        };
        overlay_config(&mut document, &config);
        assert_eq!(document["a_key_from_the_future"], true);
        assert_eq!(document["refresh_secs"], 90);

        let mut not_an_object = json!([1, 2]);
        overlay_config(&mut not_an_object, &config);
        assert_eq!(not_an_object["refresh_secs"], 90);
    }
}
