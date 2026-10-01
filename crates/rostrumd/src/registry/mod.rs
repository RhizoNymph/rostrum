//! Pairing codes and paired devices, owned by one task.
//!
//! The page server issues codes and revokes devices; the API redeems codes and
//! authenticates tokens. All of that goes through [`Registry`], a handle to a
//! single task that owns the [`CodeBook`] and the [`DeviceBook`] and handles
//! one command at a time. Redeeming a code and recording the device it pairs
//! is therefore one step no other request can interleave with, and the
//! devices file has exactly one writer.

pub mod codes;
pub mod devices;

use std::{net::IpAddr, time::Duration};

use chrono::{DateTime, Utc};
use rostrum_remote::{DeviceId, DeviceToken, PairingCode, TokenHash};
use tokio::sync::{mpsc, oneshot};

pub use codes::{CodeBook, RedeemError};
pub use devices::{DeviceBook, DeviceRecord, DeviceView, Superseded, clean_name};

use crate::{random::RandomError, state_file::StoreError};

/// Commands queued ahead of the registry before senders wait.
const QUEUE: usize = 64;

/// A freshly issued pairing code.
#[derive(Clone, Debug)]
pub struct IssuedCode {
    pub code: PairingCode,
    pub expires_at: DateTime<Utc>,
}

/// What a successful pairing hands back to the phone.
#[derive(Clone, Debug)]
pub struct Paired {
    pub device: DeviceId,
    pub name: String,
    pub token: DeviceToken,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Random(#[from] RandomError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the device registry has stopped")]
    Stopped,
}

#[derive(Debug, thiserror::Error)]
pub enum PairError {
    #[error(transparent)]
    Code(#[from] RedeemError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

enum Command {
    IssueCode {
        reply: oneshot::Sender<Result<IssuedCode, RegistryError>>,
    },
    Pair {
        code: PairingCode,
        name: String,
        replaces: Option<TokenHash>,
        from: IpAddr,
        reply: oneshot::Sender<Result<Paired, PairError>>,
    },
    Authenticate {
        hash: TokenHash,
        from: IpAddr,
        reply: oneshot::Sender<Option<DeviceId>>,
    },
    Devices {
        reply: oneshot::Sender<Vec<DeviceView>>,
    },
    Revoke {
        id: DeviceId,
        reply: oneshot::Sender<Result<bool, RegistryError>>,
    },
}

/// A handle to the registry task. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Registry {
    tx: mpsc::Sender<Command>,
}

impl Registry {
    /// Start the registry task. Must be called inside a Tokio runtime.
    pub fn spawn(devices: DeviceBook, code_ttl: Duration) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE);
        let actor = Actor {
            codes: CodeBook::new(code_ttl),
            devices,
        };
        tokio::spawn(actor.run(rx));
        Self { tx }
    }

    async fn ask<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, RegistryError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(make(reply))
            .await
            .map_err(|_| RegistryError::Stopped)?;
        answer.await.map_err(|_| RegistryError::Stopped)
    }

    pub async fn issue_code(&self) -> Result<IssuedCode, RegistryError> {
        self.ask(|reply| Command::IssueCode { reply }).await?
    }

    /// Redeem `code` and record a new device named `name`, replacing the
    /// device that held `replaces` — or, failing that, any device of exactly
    /// the same name (see [`DeviceBook::pair`]). Nothing is replaced unless
    /// the code is accepted.
    pub async fn pair(
        &self,
        code: PairingCode,
        name: String,
        replaces: Option<&DeviceToken>,
        from: IpAddr,
    ) -> Result<Paired, PairError> {
        let replaces = replaces.map(DeviceToken::hash);
        self.ask(|reply| Command::Pair {
            code,
            name,
            replaces,
            from,
            reply,
        })
        .await?
    }

    /// The device holding `token`, if any. Fails closed: a stopped registry
    /// authenticates nobody.
    pub async fn authenticate(&self, token: &DeviceToken, from: IpAddr) -> Option<DeviceId> {
        let hash = token.hash();
        self.ask(|reply| Command::Authenticate { hash, from, reply })
            .await
            .ok()
            .flatten()
    }

    pub async fn devices(&self) -> Result<Vec<DeviceView>, RegistryError> {
        self.ask(|reply| Command::Devices { reply }).await
    }

    /// Forget a device. `Ok(false)` when there was no such device.
    pub async fn revoke(&self, id: DeviceId) -> Result<bool, RegistryError> {
        self.ask(|reply| Command::Revoke { id, reply }).await?
    }
}

struct Actor {
    codes: CodeBook,
    devices: DeviceBook,
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::Receiver<Command>) {
        while let Some(command) = rx.recv().await {
            // A requester that gave up is not an error; drop the answer.
            match command {
                Command::IssueCode { reply } => {
                    let _ = reply.send(self.issue_code());
                }
                Command::Pair {
                    code,
                    name,
                    replaces,
                    from,
                    reply,
                } => {
                    let _ = reply.send(self.pair(&code, name, replaces.as_ref(), from));
                }
                Command::Authenticate { hash, from, reply } => {
                    let _ = reply.send(self.authenticate(&hash, from));
                }
                Command::Devices { reply } => {
                    let _ = reply.send(self.devices.views());
                }
                Command::Revoke { id, reply } => {
                    let _ = reply.send(self.revoke(&id));
                }
            }
        }
        tracing::debug!("device registry stopped");
    }

    fn issue_code(&mut self) -> Result<IssuedCode, RegistryError> {
        let code = crate::random::pairing_code()?;
        let now = tokio::time::Instant::now();
        let expires_in = self.codes.issue(code.clone(), now) - now;
        let expires_at = Utc::now()
            + chrono::TimeDelta::from_std(expires_in).unwrap_or(chrono::TimeDelta::zero());
        Ok(IssuedCode { code, expires_at })
    }

    fn pair(
        &mut self,
        code: &PairingCode,
        name: String,
        replaces: Option<&TokenHash>,
        from: IpAddr,
    ) -> Result<Paired, PairError> {
        // The code first: a request whose code is refused must not be able to
        // remove anything, whatever it says it replaces.
        self.codes.redeem(code, from, tokio::time::Instant::now())?;
        let token = crate::random::device_token().map_err(RegistryError::from)?;
        let id = crate::random::device_id().map_err(RegistryError::from)?;
        let now = Utc::now();
        let name = clean_name(&name);
        let superseded = self
            .devices
            .pair(
                DeviceRecord {
                    id: id.clone(),
                    name: name.clone(),
                    token_hash: token.hash(),
                    paired_at: now,
                    last_seen: now,
                    last_ip: from.to_canonical(),
                },
                replaces,
            )
            .map_err(RegistryError::from)?;
        tracing::info!(device = %id, name = %name, client = %from.to_canonical(), "paired a device");
        match &superseded {
            Some(Superseded::Token(old)) => tracing::info!(
                device = %id,
                replaced = %old,
                "the new pairing replaced the device whose token the phone presented"
            ),
            Some(Superseded::Name(old)) => {
                for old in old {
                    tracing::info!(
                        device = %id,
                        replaced = %old,
                        name = %name,
                        "the new pairing replaced a device of the same name"
                    );
                }
            }
            None => {}
        }
        Ok(Paired {
            device: id,
            name,
            token,
        })
    }

    fn authenticate(&mut self, hash: &TokenHash, from: IpAddr) -> Option<DeviceId> {
        let ix = self.devices.find(hash)?;
        let id = self.devices.get(ix)?.id.clone();
        if let Err(error) = self.devices.touch(ix, from, Utc::now()) {
            // Last-seen is bookkeeping; failing the request over it would
            // lock a phone out because a disk is full.
            tracing::warn!(device = %id, %error, "could not record when a device was last seen");
        }
        Some(id)
    }

    fn revoke(&mut self, id: &DeviceId) -> Result<bool, RegistryError> {
        let removed = self.devices.remove(id)?;
        if removed {
            tracing::info!(device = %id, "revoked a device");
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::ScratchDir;

    fn registry(scratch: &ScratchDir) -> Registry {
        let book = DeviceBook::load(scratch.join("devices.json")).expect("empty");
        Registry::spawn(book, Duration::from_secs(300))
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    #[tokio::test]
    async fn a_code_pairs_one_device_whose_token_then_authenticates() {
        let scratch = ScratchDir::new("registry-pair");
        let registry = registry(&scratch);
        let issued = registry.issue_code().await.expect("code");
        assert!(issued.expires_at > Utc::now());

        let paired = registry
            .pair(
                issued.code.clone(),
                " Pixel 8 ".into(),
                None,
                ip("192.168.0.9"),
            )
            .await
            .expect("pairs");
        assert_eq!(paired.name, "Pixel 8");
        assert_eq!(
            registry
                .authenticate(&paired.token, ip("192.168.0.9"))
                .await,
            Some(paired.device.clone())
        );

        let again = registry
            .pair(issued.code, "second".into(), None, ip("192.168.0.9"))
            .await;
        assert!(matches!(again, Err(PairError::Code(RedeemError::Invalid))));

        let devices = registry.devices().await.expect("devices");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, paired.device);
    }

    #[tokio::test]
    async fn a_revoked_token_no_longer_authenticates() {
        let scratch = ScratchDir::new("registry-revoke");
        let registry = registry(&scratch);
        let code = registry.issue_code().await.expect("code").code;
        let paired = registry
            .pair(code, "phone".into(), None, ip("100.64.0.20"))
            .await
            .expect("pairs");
        assert!(
            registry
                .revoke(paired.device.clone())
                .await
                .expect("revoke")
        );
        assert_eq!(
            registry
                .authenticate(&paired.token, ip("100.64.0.20"))
                .await,
            None
        );
        assert!(!registry.revoke(paired.device).await.expect("again"));
    }

    #[tokio::test]
    async fn an_unknown_token_authenticates_nobody() {
        let scratch = ScratchDir::new("registry-unknown");
        let registry = registry(&scratch);
        assert_eq!(
            registry
                .authenticate(&DeviceToken::from_bytes([4; 32]), ip("127.0.0.1"))
                .await,
            None
        );
    }

    #[tokio::test]
    async fn pairings_survive_a_restart() {
        let scratch = ScratchDir::new("registry-restart");
        let paired = {
            let registry = registry(&scratch);
            let code = registry.issue_code().await.expect("code").code;
            registry
                .pair(code, "phone".into(), None, ip("192.168.0.9"))
                .await
                .expect("pairs")
        };
        let restarted = registry(&scratch);
        assert_eq!(
            restarted
                .authenticate(&paired.token, ip("192.168.0.9"))
                .await,
            Some(paired.device)
        );
    }
}
