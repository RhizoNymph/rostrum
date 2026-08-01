//! User configuration: which repositories to watch, and how often.
//!
//! Non-secret and human-editable. Tokens never appear here.

use std::path::PathBuf;

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
        }
    }
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
    pub fn remove_repo(&mut self, id: &RepoId) -> bool {
        let name = id.to_string();
        let before = self.repos.len();
        self.repos.retain(|existing| existing != &name);
        self.repos.len() != before
    }

    pub fn refresh_interval(&self) -> std::time::Duration {
        // A pathological config should not turn into a request storm.
        std::time::Duration::from_secs(self.refresh_secs.clamp(10, 3600))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
