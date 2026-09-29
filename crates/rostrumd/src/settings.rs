//! `~/.config/rostrum/rostrumd.json`: the daemon's own, non-secret settings.
//!
//! Written with every field filled in on first run, so the user sees what can
//! be changed. A field left out of an edited file takes its default; an
//! unknown field is an error, because a misspelt `http_prot` silently doing
//! nothing is worse than refusing to start.
//!
//! The file is parsed into [`SettingsFile`] (every field optional, as a person
//! may write it) and validated into [`Settings`], which cannot hold a zero
//! port, two servers on one port, no bind address, or a relative path.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    num::NonZeroU16,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HTTP_PORT: u16 = 8484;
pub const DEFAULT_HTTPS_PORT: u16 = 8485;
pub const DEFAULT_CODE_TTL_SECS: u64 = 300;
/// A code has to survive walking to the phone; one that lives an hour is a
/// standing credential.
pub const CODE_TTL_RANGE_SECS: std::ops::RangeInclusive<u64> = 30..=3600;
const DEFAULT_STATE_DIR: &str = "~/.local/share/rostrum/server";

/// The file as a person writes it: every field optional.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsFile {
    pub machine_name: Option<String>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub bind: Option<Vec<IpAddr>>,
    pub state_dir: Option<PathBuf>,
    pub apk_dir: Option<PathBuf>,
    pub code_ttl_secs: Option<u64>,
}

impl SettingsFile {
    /// What first run writes: every field, paths in `~` form.
    pub fn defaults(hostname: &str) -> Self {
        Self {
            machine_name: Some(hostname.to_string()),
            http_port: Some(DEFAULT_HTTP_PORT),
            https_port: Some(DEFAULT_HTTPS_PORT),
            bind: Some(vec![
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            ]),
            state_dir: Some(PathBuf::from(DEFAULT_STATE_DIR)),
            apk_dir: Some(PathBuf::from(DEFAULT_STATE_DIR).join("apk")),
            code_ttl_secs: Some(DEFAULT_CODE_TTL_SECS),
        }
    }

    /// Validate, filling absent fields with defaults. `home` expands `~`.
    pub fn resolve(self, hostname: &str, home: &Path) -> Result<Settings, SettingsError> {
        let machine_name = self
            .machine_name
            .unwrap_or_else(|| hostname.to_string())
            .trim()
            .to_string();
        if machine_name.is_empty() {
            return Err(SettingsError::EmptyMachineName);
        }

        let port = |field: &'static str, value: Option<u16>, default: u16| {
            NonZeroU16::new(value.unwrap_or(default)).ok_or(SettingsError::ZeroPort { field })
        };
        let http_port = port("http_port", self.http_port, DEFAULT_HTTP_PORT)?;
        let https_port = port("https_port", self.https_port, DEFAULT_HTTPS_PORT)?;
        if http_port == https_port {
            return Err(SettingsError::PortsCollide(http_port.get()));
        }

        let mut bind = Vec::new();
        for addr in self.bind.unwrap_or_else(|| {
            vec![
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            ]
        }) {
            if !bind.contains(&addr) {
                bind.push(addr);
            }
        }
        if bind.is_empty() {
            return Err(SettingsError::NoBindAddress);
        }

        let state_dir = absolute(
            "state_dir",
            &self
                .state_dir
                .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR)),
            home,
        )?;
        let apk_dir = match self.apk_dir {
            Some(dir) => absolute("apk_dir", &dir, home)?,
            None => state_dir.join("apk"),
        };

        let ttl = self.code_ttl_secs.unwrap_or(DEFAULT_CODE_TTL_SECS);
        if !CODE_TTL_RANGE_SECS.contains(&ttl) {
            return Err(SettingsError::CodeTtl(ttl));
        }

        Ok(Settings {
            machine_name,
            http_port,
            https_port,
            bind,
            state_dir,
            apk_dir,
            code_ttl: Duration::from_secs(ttl),
        })
    }
}

/// Validated settings. Construct through [`SettingsFile::resolve`] or
/// [`Settings::load_or_init`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    machine_name: String,
    http_port: NonZeroU16,
    https_port: NonZeroU16,
    bind: Vec<IpAddr>,
    state_dir: PathBuf,
    apk_dir: PathBuf,
    code_ttl: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} is not valid rostrumd settings: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write default settings to {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`machine_name` is empty")]
    EmptyMachineName,
    #[error("`{field}` is 0, which is not a port")]
    ZeroPort { field: &'static str },
    #[error("`http_port` and `https_port` are both {0}; they need different ports")]
    PortsCollide(u16),
    #[error("`bind` lists no addresses")]
    NoBindAddress,
    #[error("`{field}` is `{}`, which is relative; use an absolute path or one starting with `~`", path.display())]
    RelativePath { field: &'static str, path: PathBuf },
    #[error("`code_ttl_secs` is {0}; it must be between 30 and 3600")]
    CodeTtl(u64),
    #[error("could not determine the home directory")]
    NoHome,
}

impl Settings {
    /// `~/.config/rostrum/rostrumd.json`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("rostrum").join("rostrumd.json"))
    }

    /// Read `path`, or write the defaults there first if it does not exist.
    /// Returns whether the file was created.
    pub fn load_or_init(
        path: &Path,
        hostname: &str,
        home: &Path,
    ) -> Result<(Self, bool), SettingsError> {
        let (file, created) = if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|source| SettingsError::Read {
                path: path.to_path_buf(),
                source,
            })?;
            let file = serde_json::from_str::<SettingsFile>(&text).map_err(|source| {
                SettingsError::Parse {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            (file, false)
        } else {
            let file = SettingsFile::defaults(hostname);
            write_defaults(path, &file)?;
            (file, true)
        };
        Ok((file.resolve(hostname, home)?, created))
    }

    pub fn machine_name(&self) -> &str {
        &self.machine_name
    }

    pub fn http_port(&self) -> u16 {
        self.http_port.get()
    }

    pub fn https_port(&self) -> u16 {
        self.https_port.get()
    }

    /// Never empty, no duplicates.
    pub fn bind(&self) -> &[IpAddr] {
        &self.bind
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn apk_dir(&self) -> &Path {
        &self.apk_dir
    }

    pub fn code_ttl(&self) -> Duration {
        self.code_ttl
    }

    /// `<state_dir>/tls`: `cert.pem` and `key.pem`.
    pub fn tls_dir(&self) -> PathBuf {
        self.state_dir.join("tls")
    }

    /// `<state_dir>/devices.json`.
    pub fn devices_file(&self) -> PathBuf {
        self.state_dir.join("devices.json")
    }

    /// `<state_dir>/handoffs.json`.
    pub fn handoffs_file(&self) -> PathBuf {
        self.state_dir.join("handoffs.json")
    }
}

fn write_defaults(path: &Path, file: &SettingsFile) -> Result<(), SettingsError> {
    let io = |source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    // Serialising a struct of plain fields cannot fail.
    let text = serde_json::to_string_pretty(file).unwrap_or_default();
    std::fs::write(path, text + "\n").map_err(io)
}

/// Expand a leading `~` and insist on an absolute result: a systemd service's
/// working directory is not somewhere a relative path should resolve against.
fn absolute(field: &'static str, path: &Path, home: &Path) -> Result<PathBuf, SettingsError> {
    let expanded = match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path.to_path_buf(),
    };
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Err(SettingsError::RelativePath {
            field,
            path: path.to_path_buf(),
        })
    }
}

/// The kernel's hostname: the default machine name, and a name the page
/// server accepts in a `Host` header.
pub fn hostname() -> String {
    ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "rostrumd".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::ScratchDir;

    fn home() -> PathBuf {
        PathBuf::from("/home/tester")
    }

    #[test]
    fn first_run_writes_every_field_and_resolves_to_the_defaults() {
        let scratch = ScratchDir::new("settings-first");
        let path = scratch.join("rostrum/rostrumd.json");
        let (settings, created) =
            Settings::load_or_init(&path, "framework", &home()).expect("defaults");
        assert!(created);
        assert_eq!(settings.machine_name(), "framework");
        assert_eq!(settings.http_port(), 8484);
        assert_eq!(settings.https_port(), 8485);
        assert_eq!(
            settings.bind(),
            &[
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V6(Ipv6Addr::UNSPECIFIED)
            ]
        );
        assert_eq!(
            settings.state_dir(),
            Path::new("/home/tester/.local/share/rostrum/server")
        );
        assert_eq!(
            settings.apk_dir(),
            Path::new("/home/tester/.local/share/rostrum/server/apk")
        );
        assert_eq!(settings.code_ttl(), Duration::from_secs(300));

        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
        for field in [
            "machine_name",
            "http_port",
            "https_port",
            "bind",
            "state_dir",
            "apk_dir",
            "code_ttl_secs",
        ] {
            assert!(!written[field].is_null(), "{field} should be written");
        }
        assert_eq!(written["state_dir"], "~/.local/share/rostrum/server");

        let (again, created) = Settings::load_or_init(&path, "framework", &home()).expect("reload");
        assert!(!created);
        assert_eq!(again, settings);
    }

    #[test]
    fn absent_fields_take_defaults_and_apk_dir_follows_state_dir() {
        let file: SettingsFile =
            serde_json::from_str(r#"{"state_dir": "/srv/rostrum", "http_port": 9000}"#)
                .expect("parse");
        let settings = file.resolve("desk", &home()).expect("valid");
        assert_eq!(settings.machine_name(), "desk");
        assert_eq!(settings.http_port(), 9000);
        assert_eq!(settings.https_port(), 8485);
        assert_eq!(settings.apk_dir(), Path::new("/srv/rostrum/apk"));
        assert_eq!(
            settings.devices_file(),
            Path::new("/srv/rostrum/devices.json")
        );
        assert_eq!(settings.tls_dir(), Path::new("/srv/rostrum/tls"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<SettingsFile>(r#"{"http_prot": 1}"#).is_err());
    }

    #[test]
    fn invalid_settings_are_named() {
        let resolve = |json: &str| {
            serde_json::from_str::<SettingsFile>(json)
                .expect("parse")
                .resolve("desk", &home())
        };
        assert!(matches!(
            resolve(r#"{"http_port": 8485}"#),
            Err(SettingsError::PortsCollide(8485))
        ));
        assert!(matches!(
            resolve(r#"{"https_port": 0}"#),
            Err(SettingsError::ZeroPort {
                field: "https_port"
            })
        ));
        assert!(matches!(
            resolve(r#"{"bind": []}"#),
            Err(SettingsError::NoBindAddress)
        ));
        assert!(matches!(
            resolve(r#"{"state_dir": "relative/dir"}"#),
            Err(SettingsError::RelativePath {
                field: "state_dir",
                ..
            })
        ));
        assert!(matches!(
            resolve(r#"{"code_ttl_secs": 5}"#),
            Err(SettingsError::CodeTtl(5))
        ));
        assert!(matches!(
            resolve(r#"{"machine_name": "   "}"#),
            Err(SettingsError::EmptyMachineName)
        ));
    }

    #[test]
    fn duplicate_bind_addresses_collapse() {
        let settings = serde_json::from_str::<SettingsFile>(r#"{"bind": ["::", "::", "0.0.0.0"]}"#)
            .expect("parse")
            .resolve("desk", &home())
            .expect("valid");
        assert_eq!(settings.bind().len(), 2);
    }

    #[test]
    fn a_broken_file_is_an_error_not_a_silent_default() {
        let scratch = ScratchDir::new("settings-broken");
        let path = scratch.join("rostrumd.json");
        std::fs::write(&path, "{ not json").expect("write");
        assert!(matches!(
            Settings::load_or_init(&path, "desk", &home()),
            Err(SettingsError::Parse { .. })
        ));
    }

    #[test]
    fn the_hostname_is_never_empty() {
        assert!(!hostname().is_empty());
    }
}
