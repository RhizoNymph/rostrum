//! The state both servers share: identity, handles to the two actors, and the
//! backends tests replace.

use std::{ops::Deref, path::PathBuf, sync::Arc, time::Duration};

use rostrum_remote::{CertFingerprint, MachineInfo};
use tokio::sync::watch;

use crate::{
    config_push::ConfigWriter,
    error::StartupError,
    github::HandoverSource,
    jobs::{HandoffBook, JobRunner, Jobs},
    net::{NetworkView, request_host::TrustedNames},
    registry::{DeviceBook, Registry},
    rostrum_config::{self, RostrumConfig},
    stacks::{RepoSnapshots, StackOps},
    tmux::SessionLister,
};

/// Everything [`Daemon::start`] needs. The binary fills it from
/// [`crate::Settings`]; tests fill it from scratch directories and fakes.
pub struct DaemonParts {
    /// Shown on the page, in `Hello` and in `MachineInfo`.
    pub machine_name: String,
    /// The kernel hostname, trusted in a `Host` header.
    pub hostname: String,
    pub http_port: u16,
    pub https_port: u16,
    pub fingerprint: CertFingerprint,
    pub apk_dir: PathBuf,
    pub rostrum_config: RostrumConfig,
    pub code_ttl: Duration,
    pub devices_file: PathBuf,
    pub handoffs_file: PathBuf,
    pub runner: JobRunner,
    pub github: Arc<dyn HandoverSource>,
    pub tmux: Arc<dyn SessionLister>,
    pub network: watch::Receiver<NetworkView>,
    /// `rostrum-stack` over `gh` (a recording double in tests).
    pub stack_ops: Arc<dyn StackOps>,
    /// Open pull requests and stacks from GitHub, for validating.
    pub snapshots: Arc<dyn RepoSnapshots>,
    /// Where arranging puts its scratch worktrees.
    pub stack_scratch_dir: PathBuf,
}

pub struct Inner {
    pub machine_name: String,
    pub hostname: String,
    pub http_port: u16,
    pub https_port: u16,
    pub fingerprint: CertFingerprint,
    pub apk_dir: PathBuf,
    pub rostrum_config: RostrumConfig,
    /// The daemon's only writer of rostrum's `config.json` (phone pushes).
    pub config_writer: ConfigWriter,
    pub registry: Registry,
    pub jobs: Jobs,
    pub github: Arc<dyn HandoverSource>,
    pub tmux: Arc<dyn SessionLister>,
    pub network: watch::Receiver<NetworkView>,
    pub stack_ops: Arc<dyn StackOps>,
    pub snapshots: Arc<dyn RepoSnapshots>,
    pub stack_scratch_dir: PathBuf,
}

/// Shared by every request on both servers. Cheap to clone.
#[derive(Clone)]
pub struct Daemon(Arc<Inner>);

impl Deref for Daemon {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl Daemon {
    /// Load the devices and handoff record and start both actors. Must be
    /// called inside a Tokio runtime.
    pub fn start(parts: DaemonParts) -> Result<Self, StartupError> {
        let devices = DeviceBook::load(parts.devices_file)?;
        let handoffs = HandoffBook::load(parts.handoffs_file)?;
        tracing::info!(
            devices = devices.records().len(),
            handoffs = handoffs.records().len(),
            "loaded paired devices"
        );
        Ok(Self(Arc::new(Inner {
            machine_name: parts.machine_name,
            hostname: parts.hostname,
            http_port: parts.http_port,
            https_port: parts.https_port,
            fingerprint: parts.fingerprint,
            apk_dir: parts.apk_dir,
            config_writer: ConfigWriter::spawn(parts.rostrum_config.path().to_path_buf()),
            rostrum_config: parts.rostrum_config,
            registry: Registry::spawn(devices, parts.code_ttl),
            jobs: Jobs::spawn(parts.runner, handoffs),
            github: parts.github,
            tmux: parts.tmux,
            network: parts.network,
            stack_ops: parts.stack_ops,
            snapshots: parts.snapshots,
            stack_scratch_dir: parts.stack_scratch_dir,
        })))
    }

    /// The addresses as last probed.
    pub fn network_view(&self) -> NetworkView {
        self.network.borrow().clone()
    }

    /// Names a privileged page request may be addressed to.
    pub fn trusted_names(&self) -> TrustedNames {
        TrustedNames::new(&self.hostname, self.network.borrow().tailnet_name())
    }

    /// The machine as the phone sees it, from rostrum's config as it is now.
    pub fn machine_info(&self) -> MachineInfo {
        rostrum_config::machine_info(&self.rostrum_config.load(), &self.machine_name)
    }
}
