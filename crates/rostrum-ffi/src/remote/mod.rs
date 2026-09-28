//! The paired desktop (`rostrumd`): pairing, and the local worktree
//! operations it runs on the phone's behalf.
//!
//! Every call goes through `rostrum-remote`'s pinned client: the desktop's
//! self-signed certificate is trusted by fingerprint alone, and a request is
//! sent to at most one of its addresses.

mod types;

pub use types::{
    CloneInfo, DesktopGitHubToken, DesktopProbe, HandoffSession, HandoffState, InProgress,
    InProgressKind, JobOutcome, JobResult, LocalBranch, LocalOp, LocalStatus, MachineInfo,
    PairingPreview, PairingResult, RemoteStatus, SyncAllOp, SyncEntry, SyncEntryState, SyncRun,
    SyncSummary,
};

use crate::{engine::RostrumCore, error::RostrumError};

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Read a `rostrum://pair?…` link (from a QR code) without contacting
    /// anything.
    pub fn parse_pairing_link(&self, uri: String) -> Result<PairingPreview, RostrumError> {
        let _ = uri;
        Err(RostrumError::unimplemented("parse_pairing_link"))
    }

    /// Pair using a link: checks the desktop's protocol version, exchanges
    /// the code for a device token, and makes this desktop the session's
    /// remote. If no GitHub token is set and the desktop hands one over, it
    /// is applied for this session too. Persist the result's secrets.
    pub async fn pair_with_link(
        &self,
        uri: String,
        device_name: String,
    ) -> Result<PairingResult, RostrumError> {
        let _ = (uri, device_name);
        Err(RostrumError::unimplemented("pair_with_link"))
    }

    /// Ask a desktop typed in by address who it is and which certificate it
    /// presents, trusting nothing yet.
    pub async fn probe_desktop(&self, host: String, port: u16) -> Result<DesktopProbe, RostrumError> {
        let _ = (host, port);
        Err(RostrumError::unimplemented("probe_desktop"))
    }

    /// Pair by address and typed code, pinned to the `fingerprint` from
    /// `probe_desktop` (after the user compared it). Otherwise as
    /// `pair_with_link`.
    pub async fn pair_manual(
        &self,
        host: String,
        port: u16,
        fingerprint: String,
        code: String,
        device_name: String,
    ) -> Result<PairingResult, RostrumError> {
        let _ = (host, port, fingerprint, code, device_name);
        Err(RostrumError::unimplemented("pair_manual"))
    }

    /// Use a previously paired desktop for this session: `endpoint` and
    /// `device_token` as `PairingResult` gave them.
    pub async fn set_remote(
        &self,
        endpoint: String,
        device_token: String,
    ) -> Result<RemoteStatus, RostrumError> {
        let _ = (endpoint, device_token);
        Err(RostrumError::unimplemented("set_remote"))
    }

    /// Forget the desktop for this session (Kotlin forgets the secrets).
    pub async fn clear_remote(&self) -> Result<(), RostrumError> {
        Err(RostrumError::unimplemented("clear_remote"))
    }

    pub async fn remote_status(&self) -> Result<RemoteStatus, RostrumError> {
        Err(RostrumError::unimplemented("remote_status"))
    }

    /// The desktop's name, version and clones.
    pub async fn machine_info(&self) -> Result<MachineInfo, RostrumError> {
        Err(RostrumError::unimplemented("machine_info"))
    }

    /// The desktop clone's view of a pull request's branch. The desktop
    /// fetches first, so this can take a few seconds.
    pub async fn local_status(&self, repo: String, number: u32) -> Result<LocalStatus, RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("local_status"))
    }

    /// Run one local operation on the pull request's worktree and wait for
    /// it to finish.
    pub async fn run_local_job(
        &self,
        repo: String,
        number: u32,
        op: LocalOp,
        autostash: bool,
    ) -> Result<JobResult, RostrumError> {
        let _ = (repo, number, op, autostash);
        Err(RostrumError::unimplemented("run_local_job"))
    }

    /// Abort the rebase or merge stopped in the pull request's worktree.
    pub async fn abort_local(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let _ = (repo, number);
        Err(RostrumError::unimplemented("abort_local"))
    }

    /// Start "sync all" on the desktop over every open pull request in the
    /// feed whose repository has a clone there. Returns at once; poll
    /// `sync_all_status` for progress.
    pub async fn start_sync_all(
        &self,
        op: SyncAllOp,
        autostash: bool,
    ) -> Result<SyncRun, RostrumError> {
        let _ = (op, autostash);
        Err(RostrumError::unimplemented("start_sync_all"))
    }

    /// The latest "sync all", running or finished, or `None` if none ran.
    pub async fn sync_all_status(&self) -> Result<Option<SyncRun>, RostrumError> {
        Err(RostrumError::unimplemented("sync_all_status"))
    }

    /// tmux sessions on the desktop holding stopped operations.
    pub async fn handoffs(&self) -> Result<Vec<HandoffSession>, RostrumError> {
        Err(RostrumError::unimplemented("handoffs"))
    }

    /// Ask the desktop for its current GitHub token (when the phone's copy
    /// stopped working) and use it for this session. Persist the result.
    pub async fn refresh_github_token_from_desktop(
        &self,
    ) -> Result<DesktopGitHubToken, RostrumError> {
        Err(RostrumError::unimplemented("refresh_github_token_from_desktop"))
    }

    /// Unpair on the desktop, then forget it here. If the desktop already
    /// forgot this device, that counts as success. On any other failure the
    /// pairing is kept; `clear_remote` forgets it locally regardless.
    pub async fn unpair(&self) -> Result<(), RostrumError> {
        Err(RostrumError::unimplemented("unpair"))
    }
}
