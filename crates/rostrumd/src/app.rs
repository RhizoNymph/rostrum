//! Startup and shutdown, in order.
//!
//! 1. Settings (written with defaults on first run) and the state directory.
//! 2. The TLS identity: loaded, or generated once.
//! 3. A first network probe, then the watcher that keeps it current.
//! 4. The [`Daemon`] and its two actors.
//! 5. Listeners for both servers, then the servers.
//! 6. On SIGTERM/SIGINT: stop accepting, let requests finish, and wait for
//!    running git work to release its clones before exiting — a rebase is
//!    never killed half-way by a restart.

use std::{path::PathBuf, sync::Arc, time::Duration};

use rostrum_stack::GhCli;
use tokio::signal::unix::{SignalKind, signal};

use crate::{
    Daemon, DaemonParts, Settings, StartupError, TlsIdentity,
    fsutil::ensure_private_dir,
    github::GhHandover,
    jobs,
    net::{self, NetworkView, listen},
    rostrum_config::RostrumConfig,
    server, settings,
    stacks::{self, GhStackOps, GitHubSnapshots},
    tmux::TmuxCli,
};

/// How long in-flight requests get once shutdown starts.
const REQUEST_GRACE: Duration = Duration::from_secs(10);
/// How long running git work gets. rostrum-git bounds a write at five
/// minutes; the unit's `TimeoutStopSec` is longer than this.
const WORK_GRACE: Duration = Duration::from_secs(330);

/// Command-line arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// `--config <path>`: settings file other than the default.
    pub config: Option<PathBuf>,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    #[default]
    Run,
    Help,
    Version,
}

pub const USAGE: &str = "usage: rostrumd [--config <path>]

Serves the rostrum pairing page (HTTP) and the paired phone's API (HTTPS).
Settings: ~/.config/rostrum/rostrumd.json (written with defaults on first run).
Logging: RUST_LOG overrides the default filter.";

impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, StartupError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => parsed.action = Action::Help,
                "-V" | "--version" => parsed.action = Action::Version,
                "--config" => {
                    let path = args.next().ok_or_else(|| {
                        StartupError::Usage(format!("--config needs a path\n\n{USAGE}"))
                    })?;
                    parsed.config = Some(PathBuf::from(path));
                }
                other => {
                    if let Some(path) = other.strip_prefix("--config=") {
                        parsed.config = Some(PathBuf::from(path));
                    } else {
                        return Err(StartupError::Usage(format!(
                            "unknown argument `{other}`\n\n{USAGE}"
                        )));
                    }
                }
            }
        }
        Ok(parsed)
    }
}

pub async fn run(args: Args) -> Result<(), StartupError> {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("a rustls crypto provider was already installed");
    }

    let home = dirs::home_dir().ok_or(StartupError::NoHome)?;
    let hostname = settings::hostname();
    let settings_path = args
        .config
        .or_else(Settings::default_path)
        .ok_or(StartupError::NoConfigDir)?;
    let (settings, created) = Settings::load_or_init(&settings_path, &hostname, &home)?;
    if created {
        tracing::info!(path = %settings_path.display(), "wrote default settings");
    }
    ensure_private_dir(settings.state_dir()).map_err(|source| StartupError::Directory {
        path: settings.state_dir().to_path_buf(),
        source,
    })?;
    std::fs::create_dir_all(settings.apk_dir()).map_err(|source| StartupError::Directory {
        path: settings.apk_dir().to_path_buf(),
        source,
    })?;

    let (identity, provenance) =
        TlsIdentity::load_or_create(&settings.tls_dir(), settings.machine_name())?;
    let fingerprint = identity.fingerprint();
    tracing::info!(
        ?provenance,
        fingerprint = %fingerprint.to_hex(),
        short = %fingerprint.short(),
        "TLS identity ready"
    );

    let initial = net::view::probe().await;
    log_addresses(&initial, &settings);
    let network = net::view::spawn_watcher(initial, net::view::REFRESH);

    let rostrum_config = RostrumConfig::default_location().ok_or(StartupError::NoConfigDir)?;
    let daemon = Daemon::start(DaemonParts {
        machine_name: settings.machine_name().to_string(),
        hostname,
        http_port: settings.http_port(),
        https_port: settings.https_port(),
        fingerprint,
        apk_dir: settings.apk_dir().to_path_buf(),
        rostrum_config,
        code_ttl: settings.code_ttl(),
        devices_file: settings.devices_file(),
        handoffs_file: settings.handoffs_file(),
        runner: jobs::live_runner(),
        github: Arc::new(GhHandover::new(settings.machine_name())),
        tmux: Arc::new(TmuxCli),
        network,
        stack_ops: Arc::new(GhStackOps(Arc::new(GhCli))),
        snapshots: Arc::new(GitHubSnapshots),
        stack_scratch_dir: stacks::default_scratch_dir(),
    })?;

    let http = listen::bind_all(settings.bind(), settings.http_port()).map_err(|source| {
        StartupError::Listen {
            server: "page",
            source,
        }
    })?;
    let https = listen::bind_all(settings.bind(), settings.https_port()).map_err(|source| {
        StartupError::Listen {
            server: "API",
            source,
        }
    })?;
    let bound = |listeners: &[std::net::TcpListener]| {
        listeners
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .map(|addr| addr.to_string())
            .collect::<Vec<_>>()
    };
    tracing::info!(page = ?bound(&http), api = ?bound(&https), "listening");
    let mut servers = server::start(&daemon, Arc::new(identity.server_config()?), http, https)?;

    let mut terminate = signal(SignalKind::terminate()).map_err(StartupError::Signal)?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(StartupError::Signal)?;
    let failed = tokio::select! {
        _ = terminate.recv() => None,
        _ = interrupt.recv() => None,
        stopped = servers.any_stopped() => Some(stopped),
    };
    match &failed {
        None => tracing::info!("shutting down"),
        Some((server, reason)) => {
            tracing::error!(
                server,
                reason,
                "a server stopped unexpectedly; shutting down"
            );
        }
    }
    servers.shutdown(REQUEST_GRACE).await;
    if tokio::time::timeout(WORK_GRACE, daemon.jobs.shutdown())
        .await
        .is_err()
    {
        tracing::warn!("gave up waiting for running git work to finish");
    }
    tracing::info!("stopped");
    match failed {
        None => Ok(()),
        Some((server, reason)) => Err(StartupError::Serve {
            server,
            source: std::io::Error::other(reason),
        }),
    }
}

fn log_addresses(view: &NetworkView, settings: &Settings) {
    let page_urls: Vec<String> = view
        .lan
        .iter()
        .map(|ip| format!("http://{ip}:{}/", settings.http_port()))
        .chain(view.tailnet_page_urls(settings.http_port()))
        .collect();
    tracing::info!(
        machine = settings.machine_name(),
        pages = ?page_urls,
        advertised = ?view.advertised_hosts().iter().map(ToString::to_string).collect::<Vec<_>>(),
        "addresses"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, StartupError> {
        Args::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn arguments_parse() {
        assert_eq!(parse(&[]).expect("empty"), Args::default());
        assert_eq!(
            parse(&["--config", "/etc/r.json"]).expect("config").config,
            Some(PathBuf::from("/etc/r.json"))
        );
        assert_eq!(
            parse(&["--config=/etc/r.json"]).expect("config").config,
            Some(PathBuf::from("/etc/r.json"))
        );
        assert_eq!(parse(&["--version"]).expect("v").action, Action::Version);
        assert_eq!(parse(&["-h"]).expect("h").action, Action::Help);
        assert!(matches!(parse(&["--config"]), Err(StartupError::Usage(_))));
        assert!(matches!(
            parse(&["--port", "1"]),
            Err(StartupError::Usage(_))
        ));
    }
}
