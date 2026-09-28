//! The paired desktop (`rostrumd`): pairing, and the local worktree
//! operations it runs on the phone's behalf.
//!
//! Every call goes through `rostrum-remote`'s pinned client: the desktop's
//! self-signed certificate is trusted by fingerprint alone, and a request is
//! sent to at most one of its addresses.

mod convert;
mod refs;
mod types;

pub use types::{
    CloneInfo, DesktopGitHubToken, DesktopProbe, HandoffSession, HandoffState, InProgress,
    InProgressKind, JobOutcome, JobResult, LocalBranch, LocalOp, LocalStatus, MachineInfo,
    PairingPreview, PairingResult, RemoteStatus, SyncAllOp, SyncEntry, SyncEntryState, SyncRun,
    SyncSummary,
};

use std::sync::Arc;

use rostrum_remote::{
    API_VERSION, CertFingerprint, DeviceToken, Endpoint, Host, PairingCode, PairingOffer,
    api::{AbortRequest, JobRequest, LocalStatusRequest, PrKey, SyncAllRequest},
    client::{RemoteClient, probe},
    pairing::{GitHubHandover, PairRequest},
};

use crate::{
    engine::{
        RostrumCore,
        state::{CoreState, PullKey},
    },
    error::RostrumError,
};

/// The paired desktop for this session. The device token lives inside the
/// client, in memory only.
pub(crate) struct RemoteSession {
    client: Arc<RemoteClient>,
}

impl RemoteSession {
    fn status(&self) -> RemoteStatus {
        let endpoint = self.client.endpoint();
        RemoteStatus::Paired {
            hosts: endpoint.hosts().iter().map(ToString::to_string).collect(),
            port: endpoint.port(),
            fingerprint_short: endpoint.fingerprint().short(),
            current_host: self.client.current_host().to_string(),
        }
    }
}

impl CoreState {
    fn remote_client(&self) -> Result<Arc<RemoteClient>, RostrumError> {
        self.remote
            .as_ref()
            .map(|remote| remote.client.clone())
            .ok_or(RostrumError::NotPaired)
    }

    fn remote_status(&self) -> RemoteStatus {
        self.remote
            .as_ref()
            .map_or(RemoteStatus::NotPaired, RemoteSession::status)
    }

    /// The description of `key`, if its conversation is in memory.
    fn description(&self, key: &PullKey) -> Option<String> {
        self.conversations
            .get(key)
            .and_then(|conversation| refs::description(conversation))
    }
}

fn invalid(error: impl std::fmt::Display) -> RostrumError {
    RostrumError::invalid(error.to_string())
}

fn github_token(handover: &GitHubHandover) -> DesktopGitHubToken {
    DesktopGitHubToken {
        token: handover.token.expose().to_string(),
        source: handover.source.clone(),
        host: handover.host.clone(),
    }
}

impl RostrumCore {
    /// Check the desktop speaks this protocol, exchange the code for a device
    /// token, and make the desktop this session's remote.
    async fn pair(
        &self,
        endpoint: Endpoint,
        code: PairingCode,
        device_name: String,
    ) -> Result<PairingResult, RostrumError> {
        let device_name = device_name.trim().to_string();
        if device_name.is_empty() {
            return Err(RostrumError::invalid("name this device so the desktop can list it"));
        }
        let pairing = RemoteClient::new(endpoint.clone(), None)?;
        let hello = pairing.hello().await?;
        if hello.api_version != API_VERSION {
            return Err(RostrumError::IncompatibleDesktop {
                desktop: hello.api_version,
                supported: API_VERSION,
            });
        }
        let response = pairing
            .pair(&PairRequest {
                code,
                device_name,
            })
            .await?;
        let serialised = serde_json::to_string(&endpoint)
            .map_err(|error| RostrumError::internal(format!("could not serialise the endpoint: {error}")))?;
        let client = Arc::new(RemoteClient::new(endpoint, Some(response.token.clone()))?);
        let github = response.github.as_ref().map(github_token);
        let handed_over = github.as_ref().map(|token| token.token.clone());
        self.actor
            .try_call(move |state| {
                state.remote = Some(RemoteSession { client });
                // A handed-over token is used only when none is set: a token
                // the user pasted deliberately is not replaced behind them.
                if state.session.client().is_none()
                    && let Some(token) = handed_over
                {
                    state.session.set_token(Some(token), &state.github_api)?;
                    state.publish();
                }
                Ok(())
            })
            .await?;
        tracing::info!(machine = %hello.machine, device = %response.device, "paired");
        Ok(PairingResult {
            machine: convert::machine(response.machine),
            endpoint: serialised,
            device_id: response.device.to_string(),
            device_token: response.token.expose().to_string(),
            github,
        })
    }

    async fn remote(&self) -> Result<Arc<RemoteClient>, RostrumError> {
        self.actor.try_call(|state| state.remote_client()).await
    }

    /// The client, and the known pull request a local job addresses.
    async fn local_target(
        &self,
        key: &PullKey,
    ) -> Result<(Arc<RemoteClient>, rostrum_core::PullRequest, Option<String>), RostrumError> {
        self.ensure_hydrated().await?;
        let lookup = key.clone();
        self.actor
            .try_call(move |state| {
                Ok((
                    state.remote_client()?,
                    state.known(&lookup)?.clone(),
                    state.description(&lookup),
                ))
            })
            .await
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Read a `rostrum://pair?…` link (from a QR code) without contacting
    /// anything.
    pub fn parse_pairing_link(&self, uri: String) -> Result<PairingPreview, RostrumError> {
        let offer = PairingOffer::from_uri(&uri).map_err(invalid)?;
        Ok(convert::preview(&offer))
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
        let offer = PairingOffer::from_uri(&uri).map_err(invalid)?;
        self.pair(offer.endpoint, offer.code, device_name).await
    }

    /// Ask a desktop typed in by address who it is and which certificate it
    /// presents, trusting nothing yet.
    pub async fn probe_desktop(&self, host: String, port: u16) -> Result<DesktopProbe, RostrumError> {
        let host: Host = host.trim().parse().map_err(invalid)?;
        if port == 0 {
            return Err(RostrumError::invalid("port 0 is not a port"));
        }
        let found = probe(&[host], port).await?;
        Ok(DesktopProbe {
            machine: found.hello.machine,
            api_version: found.hello.api_version,
            compatible: found.hello.api_version == API_VERSION,
            host: found.host.to_string(),
            port,
            fingerprint: found.fingerprint.to_base64url(),
            fingerprint_short: found.fingerprint.short(),
        })
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
        let host: Host = host.trim().parse().map_err(invalid)?;
        let fingerprint = CertFingerprint::from_base64url(fingerprint.trim()).map_err(invalid)?;
        let endpoint = Endpoint::new(vec![host], port, fingerprint).map_err(invalid)?;
        let code = PairingCode::parse(&code).map_err(invalid)?;
        self.pair(endpoint, code, device_name).await
    }

    /// Use a previously paired desktop for this session: `endpoint` and
    /// `device_token` as `PairingResult` gave them.
    pub async fn set_remote(
        &self,
        endpoint: String,
        device_token: String,
    ) -> Result<RemoteStatus, RostrumError> {
        let endpoint: Endpoint = serde_json::from_str(&endpoint)
            .map_err(|error| RostrumError::invalid(format!("not a saved endpoint: {error}")))?;
        let token = DeviceToken::parse(device_token.trim()).map_err(invalid)?;
        let client = Arc::new(RemoteClient::new(endpoint, Some(token))?);
        self.actor
            .call(move |state| {
                state.remote = Some(RemoteSession { client });
                state.remote_status()
            })
            .await
    }

    /// Forget the desktop for this session (Kotlin forgets the secrets).
    pub async fn clear_remote(&self) -> Result<(), RostrumError> {
        self.actor.call(|state| state.remote = None).await
    }

    pub async fn remote_status(&self) -> Result<RemoteStatus, RostrumError> {
        self.actor.call(|state| state.remote_status()).await
    }

    /// The desktop's name, version and clones.
    pub async fn machine_info(&self) -> Result<MachineInfo, RostrumError> {
        let client = self.remote().await?;
        Ok(convert::machine(client.machine().await?))
    }

    /// The desktop clone's view of a pull request's branch. The desktop
    /// fetches first, so this can take a few seconds.
    pub async fn local_status(&self, repo: String, number: u32) -> Result<LocalStatus, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, pr, _) = self.local_target(&key).await?;
        let status = client
            .local_status(&LocalStatusRequest {
                key: PrKey {
                    repo: key.repo,
                    number: key.number,
                },
                head_ref: pr.head_ref,
            })
            .await?;
        Ok(convert::local_status(status))
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
        let key = PullKey::parse(&repo, number)?;
        let (client, pr, body) = self.local_target(&key).await?;
        let outcome = client
            .run_job(&JobRequest {
                pr: refs::pr_ref(&key.repo, &pr, body),
                op: convert::op_kind(op),
                autostash,
            })
            .await?;
        tracing::info!(repo = %key.repo, number = key.number.0, ?op, "local job finished");
        Ok(convert::job_result(outcome))
    }

    /// Abort the rebase or merge stopped in the pull request's worktree.
    pub async fn abort_local(&self, repo: String, number: u32) -> Result<(), RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, pr, _) = self.local_target(&key).await?;
        client
            .abort(&AbortRequest {
                key: PrKey {
                    repo: key.repo,
                    number: key.number,
                },
                head_ref: pr.head_ref,
            })
            .await?;
        Ok(())
    }

    /// Start "sync all" on the desktop over every open pull request in the
    /// feed whose repository has a clone there. Returns at once; poll
    /// `sync_all_status` for progress.
    pub async fn start_sync_all(
        &self,
        op: SyncAllOp,
        autostash: bool,
    ) -> Result<SyncRun, RostrumError> {
        self.ensure_hydrated().await?;
        let client = self.remote().await?;
        let machine = client.machine().await?;
        let prs = self
            .actor
            .call(move |state| {
                refs::sync_refs(&state.feed.repos, &machine.clones, |repo, pr| {
                    state.description(&PullKey {
                        repo: repo.clone(),
                        number: pr.number,
                    })
                })
            })
            .await?;
        if prs.is_empty() {
            return Err(RostrumError::invalid(
                "no open pull request in the feed has a clone on the desktop",
            ));
        }
        let run = client
            .start_sync_all(&SyncAllRequest {
                op: convert::sync_op_kind(op),
                autostash,
                prs,
            })
            .await?;
        Ok(convert::sync_run(run))
    }

    /// The latest "sync all", running or finished, or `None` if none ran.
    pub async fn sync_all_status(&self) -> Result<Option<SyncRun>, RostrumError> {
        let client = self.remote().await?;
        Ok(client.sync_all().await?.map(convert::sync_run))
    }

    /// tmux sessions on the desktop holding stopped operations.
    pub async fn handoffs(&self) -> Result<Vec<HandoffSession>, RostrumError> {
        let client = self.remote().await?;
        Ok(client
            .handoffs()
            .await?
            .into_iter()
            .map(convert::handoff)
            .collect())
    }

    /// Ask the desktop for its current GitHub token (when the phone's copy
    /// stopped working) and use it for this session. Persist the result.
    pub async fn refresh_github_token_from_desktop(
        &self,
    ) -> Result<DesktopGitHubToken, RostrumError> {
        let client = self.remote().await?;
        let handover = client.github_token().await?;
        let token = github_token(&handover);
        let apply = token.token.clone();
        self.actor
            .try_call(move |state| {
                state.session.set_token(Some(apply), &state.github_api)?;
                state.publish();
                Ok(())
            })
            .await?;
        Ok(token)
    }

    /// Unpair on the desktop, then forget it here. If the desktop already
    /// forgot this device, that counts as success. On any other failure the
    /// pairing is kept; `clear_remote` forgets it locally regardless.
    pub async fn unpair(&self) -> Result<(), RostrumError> {
        let client = self.remote().await?;
        match client.unpair().await {
            Ok(()) | Err(rostrum_remote::client::ClientError::Unauthorized) => {}
            Err(error) => return Err(error.into()),
        }
        self.actor.call(|state| state.remote = None).await?;
        tracing::info!("unpaired");
        Ok(())
    }
}
