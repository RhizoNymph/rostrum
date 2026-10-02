//! Pushing this phone's shareable settings to the paired desktop.
//!
//! The desktop keeps a revision of its shareable settings. A push names the
//! revision the user previewed (`DesktopConfigPreview::revision`); if the
//! desktop's settings have changed since, nothing is written and the answer
//! is `Changed` with the desktop's settings as they are now and the new
//! difference, so the phone can show it before the user tries again. The
//! desktop writes only the shareable keys, merged into its file, and its app
//! reloads them (`docs/features/remote_protocol.md`).

use rostrum_remote::{ConfigPush, ConfigPushOutcome, ConfigRevision};

use crate::{
    engine::RostrumCore,
    error::RostrumError,
    remote::config::{DesktopConfigPreview, revised_preview, shareable},
};

/// How a push ended.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConfigPushResult {
    /// Written. `desktop` is the desktop's settings now, with its new
    /// revision; `push_changes` is empty unless the desktop clamped
    /// something.
    Applied { desktop: DesktopConfigPreview },
    /// Not written: the desktop's settings changed since `base`. `desktop`
    /// is how they are now, with the new revision and what pushing would
    /// change now.
    Changed { desktop: DesktopConfigPreview },
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Replace the desktop's shareable settings — repositories, pull
    /// requests and issues per repository, the feed filter, autostash, both
    /// sorts and the trunks — with this phone's, provided they are still at
    /// `base`, the revision from `desktop_config`.
    pub async fn push_config_to_desktop(
        &self,
        base: String,
    ) -> Result<ConfigPushResult, RostrumError> {
        let base = base.trim().to_string();
        if base.is_empty() {
            return Err(RostrumError::invalid(
                "a push needs the revision the preview was made against",
            ));
        }
        let client = self.remote().await?;
        let config = self.actor.call(|state| shareable(&state.config)).await?;
        let push = ConfigPush {
            config,
            base: Some(ConfigRevision(base.clone())),
        };
        // The name first: once the push is written, nothing after it should
        // be able to fail the call.
        let machine = client.machine().await?;
        let outcome = client.push_config(&push).await?;
        let (applied, desktop) = match outcome {
            ConfigPushOutcome::Applied(desktop) => (true, desktop),
            ConfigPushOutcome::Changed(desktop) => (false, desktop),
        };
        tracing::info!(
            %base,
            revision = %desktop.revision,
            applied,
            "pushed settings to the desktop"
        );
        let preview = self
            .actor
            .call(move |state| revised_preview(&machine.name, &desktop, &state.config))
            .await?;
        Ok(if applied {
            ConfigPushResult::Applied { desktop: preview }
        } else {
            ConfigPushResult::Changed { desktop: preview }
        })
    }
}
