//! rostrum's own `config.json`: clones, the conflict handler, autostash.
//!
//! Re-read on every request that needs it, so a clone added in the desktop
//! app works from the phone without restarting the daemon. The daemon only
//! reads this file. When it does not exist the defaults are used without
//! writing them — creating the desktop app's config is the desktop app's job.

use std::path::{Path, PathBuf};

use rostrum_config::Config;
use rostrum_core::RepoId;
use rostrum_remote::{API_VERSION, CloneInfo, MachineInfo};

#[derive(Clone, Debug)]
pub struct RostrumConfig {
    path: PathBuf,
}

impl RostrumConfig {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// `~/.config/rostrum/config.json`.
    pub fn default_location() -> Option<Self> {
        Config::path().map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Config {
        if !self.path.exists() {
            return Config::default();
        }
        let (config, warnings) = Config::load_from(&self.path);
        for warning in warnings {
            tracing::warn!(path = %self.path.display(), warning = %warning.0, "problem with rostrum's config");
        }
        config
    }
}

/// Every configured clone whose key is a valid `owner/name`, tilde-expanded.
pub fn clones(config: &Config) -> Vec<CloneInfo> {
    config
        .clones
        .keys()
        .filter_map(|key| match key.parse::<RepoId>() {
            Ok(repo) => Some(repo),
            Err(error) => {
                tracing::debug!(key, %error, "skipping a clone whose repository is not owner/name");
                None
            }
        })
        .filter_map(|repo| {
            let path = config.local_path(&repo)?;
            Some(CloneInfo {
                repo,
                path: path.display().to_string(),
            })
        })
        .collect()
}

pub fn machine_info(config: &Config, name: &str) -> MachineInfo {
    MachineInfo {
        name: name.to_string(),
        version: crate::VERSION.to_string(),
        api_version: API_VERSION,
        clones: clones(config),
        handler_configured: config.conflict_handler.is_some(),
        autostash: config.autostash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::ScratchDir;

    #[test]
    fn a_missing_config_is_the_default_and_is_not_created() {
        let scratch = ScratchDir::new("rostrum-config-missing");
        let path = scratch.join("config.json");
        let config = RostrumConfig::at(path.clone()).load();
        assert!(config.clones.is_empty());
        assert!(!path.exists(), "the daemon must not write rostrum's config");
    }

    #[test]
    fn edits_are_seen_on_the_next_load() {
        let scratch = ScratchDir::new("rostrum-config-edit");
        let path = scratch.join("config.json");
        let source = RostrumConfig::at(path.clone());
        std::fs::write(&path, r#"{"clones": {}}"#).expect("write");
        assert!(source.load().clones.is_empty());
        std::fs::write(
            &path,
            r#"{"clones": {"o/r": "/src/r"}, "autostash": true, "conflict_handler": {"command": "x {context}"}}"#,
        )
        .expect("write");
        let config = source.load();
        let info = machine_info(&config, "desk");
        assert_eq!(info.name, "desk");
        assert_eq!(info.api_version, API_VERSION);
        assert!(info.autostash);
        assert!(info.handler_configured);
        assert_eq!(
            info.clones,
            vec![CloneInfo {
                repo: RepoId::new("o", "r"),
                path: "/src/r".into()
            }]
        );
    }

    #[test]
    fn a_clone_under_a_malformed_key_is_skipped() {
        let config: Config =
            serde_json::from_str(r#"{"clones": {"not-a-repo": "/x", "a/b": "/y"}}"#)
                .expect("parse");
        let found = clones(&config);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].repo, RepoId::new("a", "b"));
    }
}
