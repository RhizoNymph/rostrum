//! User configuration: which repositories to watch, and how often.
//!
//! Non-secret and human-editable. Tokens never appear here.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use rostrum_core::{RepoId, model::ParseRepoIdError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Repositories in `owner/name` form.
    pub repos: Vec<String>,
    /// Seconds between feed refreshes.
    pub refresh_secs: u64,
    /// Maximum open PRs fetched per repository.
    pub prs_per_repo: u32,
    /// Post a desktop notification when a refresh turns up a pull request that
    /// was not there before. Off by default: the feed already shows arrivals,
    /// and interrupting the desktop should be a deliberate opt-in.
    pub notifications: bool,
    /// Hide repositories that loaded successfully with nothing to show. On by
    /// default; a feed of a dozen repositories is mostly empty headers.
    pub hide_empty_repos: bool,
    /// Where a repository is cloned locally, keyed by `owner/name`.
    ///
    /// Deliberately a separate map rather than a field on each `repos` entry.
    /// Most watched repositories are ones the user only reads — a clone is the
    /// exception, not a property every entry has — and keeping it out of
    /// `repos` leaves that list a plain array of strings a human can edit
    /// without learning a second shape. It also means a config written by an
    /// older build still parses exactly as it did.
    ///
    /// A leading `~` is expanded against the home directory; see
    /// [`Config::local_path`].
    #[serde(default)]
    pub clones: BTreeMap<String, PathBuf>,
    /// Whether local pull/merge pass `--autostash`, letting git set aside
    /// uncommitted changes and restore them afterwards.
    ///
    /// Off by default. Stashing is the more convenient behaviour but it moves
    /// work the user did not hand over, so it is opted into rather than out of.
    /// Persisted because it is a working habit, not a per-pull-request choice.
    #[serde(default)]
    pub autostash: bool,
    /// What to do when a local rebase or merge stops on a conflict.
    ///
    /// Absent, the conflict is aborted and the clone left as it was found —
    /// rostrum has no conflict editor, so that is the honest answer to a
    /// button press. Present, the worktree is left mid-operation and the
    /// command is run in a tmux session with a context bundle, so something
    /// that *can* resolve conflicts gets to.
    #[serde(default)]
    pub conflict_handler: Option<ConflictHandler>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repos: vec![
                "zed-industries/zed".to_string(),
                "rust-lang/rust".to_string(),
            ],
            refresh_secs: 60,
            prs_per_repo: 25,
            notifications: false,
            hide_empty_repos: true,
            clones: BTreeMap::new(),
            autostash: false,
            conflict_handler: None,
        }
    }
}

/// A command to hand a stopped rebase or merge to.
///
/// `command` is a shell template. `{context}` is replaced by the path of a
/// markdown bundle describing the conflict, and `{worktree}` by the worktree
/// the operation stopped in; both are shell-quoted on substitution. The
/// template is typed into an interactive shell in a detached tmux session
/// whose working directory is the worktree, so anything the user could run
/// from a terminal there works here — `claude`, `aider`, a script.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictHandler {
    pub command: String,
}

/// Anything the user should know about but that should not stop startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning(pub String);

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("rostrum").join("config.json"))
    }

    /// Load config, falling back to defaults. Never fails: a broken or missing
    /// config yields defaults plus a warning, because refusing to start over a
    /// malformed file would be worse than showing the default repo list.
    pub fn load() -> (Self, Vec<Warning>) {
        let mut warnings = Vec::new();

        let Some(path) = Self::path() else {
            warnings.push(Warning(
                "could not determine a config directory; using defaults".into(),
            ));
            return (Self::default(), warnings);
        };

        if !path.exists() {
            let config = Self::default();
            if let Err(err) = config.save() {
                warnings.push(Warning(format!(
                    "could not write default config to {}: {err}",
                    path.display()
                )));
            }
            return (config, warnings);
        }

        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(config) => (config, warnings),
                Err(err) => {
                    warnings.push(Warning(format!(
                        "{} is not valid JSON ({err}); using defaults",
                        path.display()
                    )));
                    (Self::default(), warnings)
                }
            },
            Err(err) => {
                warnings.push(Warning(format!(
                    "could not read {} ({err}); using defaults",
                    path.display()
                )));
                (Self::default(), warnings)
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Parse the configured repositories, reporting malformed entries rather
    /// than silently dropping them.
    pub fn repo_ids(&self) -> (Vec<RepoId>, Vec<Warning>) {
        let mut ids = Vec::new();
        let mut warnings = Vec::new();

        for entry in &self.repos {
            match entry.parse::<RepoId>() {
                Ok(id) if ids.contains(&id) => {
                    warnings.push(Warning(format!("duplicate repository `{entry}` ignored")));
                }
                Ok(id) => ids.push(id),
                Err(err) => warnings.push(Warning(format!("skipping `{entry}`: {err}"))),
            }
        }

        (ids, warnings)
    }

    /// Add a repository, keeping `repos` sorted and free of duplicates.
    ///
    /// Returns the parsed id, or an error suitable for showing to the user.
    pub fn add_repo(&mut self, input: &str) -> Result<RepoId, String> {
        let id: RepoId = input
            .parse()
            .map_err(|err: ParseRepoIdError| err.to_string())?;
        let name = id.to_string();
        if self.repos.iter().any(|existing| existing == &name) {
            return Err(format!("{name} is already in the list"));
        }
        self.repos.push(name);
        self.repos.sort();
        Ok(id)
    }

    /// Remove a repository. Returns whether anything was removed.
    ///
    /// Drops the clone path with it: leaving an orphaned entry behind would
    /// silently reattach to a repository that happened to be re-added later.
    pub fn remove_repo(&mut self, id: &RepoId) -> bool {
        let name = id.to_string();
        let before = self.repos.len();
        self.repos.retain(|existing| existing != &name);
        self.clones.remove(&name);
        self.repos.len() != before
    }

    /// The local clone configured for a repository, with a leading `~`
    /// expanded.
    ///
    /// Performs no I/O and does not check that the path exists. A clone the
    /// user has moved or deleted should surface where every other failure in
    /// this app surfaces — as a loaded-and-failed panel naming the reason —
    /// rather than by silently hiding the local actions, and certainly not by
    /// stat-ing the filesystem on every frame of a render pass.
    pub fn local_path(&self, id: &RepoId) -> Option<PathBuf> {
        self.clones
            .get(&id.to_string())
            .map(|path| expand_tilde(path))
    }

    pub fn refresh_interval(&self) -> std::time::Duration {
        // A pathological config should not turn into a request storm.
        std::time::Duration::from_secs(self.refresh_secs.clamp(10, 3600))
    }
}

/// Expand a leading `~` against the home directory.
///
/// Config is hand-edited, and `~/Code/thing` is what a person writes. Nothing
/// else in the path is touched: `$VAR` and `~other` are left alone rather than
/// half-supported, because a path that silently resolves to the wrong clone is
/// worse than one that plainly does not exist.
fn expand_tilde(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_path_buf();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- conflict handler ---------------------------------------------------

    #[test]
    fn a_config_without_a_handler_keeps_the_abort_default() {
        let config: Config =
            serde_json::from_str(r#"{ "repos": ["a/b"] }"#).expect("older config should parse");
        assert!(config.conflict_handler.is_none());
    }

    #[test]
    fn a_handler_command_round_trips() {
        let text = r#"{ "conflict_handler": { "command": "claude 'fix {context}'" } }"#;
        let config: Config = serde_json::from_str(text).expect("should parse");
        let back = serde_json::to_string(&config).expect("should serialise");
        assert!(back.contains("fix {context}"));
        let handler = config.conflict_handler.expect("handler present");
        assert_eq!(handler.command, "claude 'fix {context}'");
    }

    // --- local clones -------------------------------------------------------

    /// A config written before clones existed must keep parsing, and must not
    /// acquire one. This is the whole reason `clones` is a separate defaulted
    /// map rather than a new shape for `repos`.
    #[test]
    fn a_config_without_clones_still_parses() {
        let text = r#"{ "repos": ["a/b"], "refresh_secs": 30 }"#;
        let config: Config = serde_json::from_str(text).expect("older config should parse");
        assert_eq!(config.repos, ["a/b"]);
        assert_eq!(config.refresh_secs, 30);
        assert!(config.clones.is_empty());
        assert_eq!(config.local_path(&"a/b".parse().expect("valid id")), None);
    }

    #[test]
    fn a_configured_clone_is_found_by_repo_id() {
        let mut config = Config::default();
        config
            .clones
            .insert("a/b".into(), PathBuf::from("/srv/checkouts/b"));

        let id: RepoId = "a/b".parse().expect("valid id");
        assert_eq!(
            config.local_path(&id),
            Some(PathBuf::from("/srv/checkouts/b"))
        );

        let other: RepoId = "a/c".parse().expect("valid id");
        assert_eq!(config.local_path(&other), None);
    }

    /// `~/Code/thing` is what a person types into a hand-edited config.
    #[test]
    fn a_leading_tilde_expands_to_the_home_directory() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(expand_tilde(Path::new("~/Code/x")), home.join("Code/x"));
        assert_eq!(expand_tilde(Path::new("~")), home);
    }

    /// Only a leading `~` is special. Half-supporting shell syntax would let a
    /// path resolve to the wrong clone instead of plainly not existing.
    #[test]
    fn other_paths_are_left_exactly_as_written() {
        for raw in ["/abs/path", "relative/path", "$HOME/x", "~other/x"] {
            assert_eq!(expand_tilde(Path::new(raw)), PathBuf::from(raw), "{raw}");
        }
    }

    #[test]
    fn removing_a_repository_drops_its_clone() {
        let mut config = Config {
            repos: vec!["a/b".into()],
            ..Default::default()
        };
        config.clones.insert("a/b".into(), PathBuf::from("/tmp/b"));

        let id: RepoId = "a/b".parse().expect("valid id");
        assert!(config.remove_repo(&id));
        assert!(config.clones.is_empty());
    }

    #[test]
    fn parses_valid_repos_and_reports_bad_ones() {
        let config = Config {
            repos: vec![
                "a/b".into(),
                "not-a-repo".into(),
                "https://github.com/c/d".into(),
            ],
            ..Default::default()
        };
        let (ids, warnings) = config.repo_ids();
        assert_eq!(
            ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["a/b", "c/d"]
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("not-a-repo"));
    }

    #[test]
    fn reports_duplicates_once() {
        let config = Config {
            repos: vec!["a/b".into(), "a/b".into()],
            ..Default::default()
        };
        let (ids, warnings) = config.repo_ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn adding_a_repo_normalises_and_sorts() {
        let mut config = Config {
            repos: vec!["z/z".into()],
            ..Default::default()
        };
        let id = config
            .add_repo("https://github.com/a/b")
            .expect("a pasted URL should be accepted");
        assert_eq!(id.to_string(), "a/b");
        assert_eq!(config.repos, ["a/b", "z/z"]);
    }

    #[test]
    fn adding_a_duplicate_is_reported_not_silently_dropped() {
        let mut config = Config {
            repos: vec!["a/b".into()],
            ..Default::default()
        };
        let error = config
            .add_repo("a/b")
            .expect_err("duplicate should be rejected");
        assert!(error.contains("already"), "{error}");
        assert_eq!(config.repos.len(), 1);
    }

    #[test]
    fn adding_a_malformed_repo_reports_the_parse_error() {
        let mut config = Config::default();
        let before = config.repos.len();
        assert!(config.add_repo("not-a-repo").is_err());
        assert_eq!(config.repos.len(), before);
    }

    #[test]
    fn removing_reports_whether_it_matched() {
        let mut config = Config {
            repos: vec!["a/b".into(), "c/d".into()],
            ..Default::default()
        };
        let id: RepoId = "a/b".parse().expect("valid");
        assert!(config.remove_repo(&id));
        assert_eq!(config.repos, ["c/d"]);
        assert!(!config.remove_repo(&id));
    }

    #[test]
    fn empty_repos_are_hidden_by_default() {
        assert!(Config::default().hide_empty_repos);
    }

    #[test]
    fn refresh_interval_is_clamped_to_a_sane_range() {
        let fast = Config {
            refresh_secs: 0,
            ..Default::default()
        };
        assert_eq!(fast.refresh_interval().as_secs(), 10);

        let slow = Config {
            refresh_secs: u64::MAX,
            ..Default::default()
        };
        assert_eq!(slow.refresh_interval().as_secs(), 3600);
    }

    #[test]
    fn deserializes_partial_config_using_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"repos":["x/y"]}"#).expect("partial config should load");
        assert_eq!(config.repos, ["x/y"]);
        assert_eq!(config.refresh_secs, Config::default().refresh_secs);
    }

    /// Desktop notifications interrupt the user, so an untouched config must
    /// never end up with them on.
    #[test]
    fn notifications_are_off_unless_opted_in() {
        assert!(!Config::default().notifications);

        let absent: Config =
            serde_json::from_str(r#"{"repos":["x/y"]}"#).expect("partial config should load");
        assert!(!absent.notifications);

        let opted_in: Config =
            serde_json::from_str(r#"{"notifications":true}"#).expect("partial config should load");
        assert!(opted_in.notifications);
    }
}
