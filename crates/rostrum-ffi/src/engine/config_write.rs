//! Settings changes: plan in the actor, write off the runtime, apply.
//!
//! `config.json` is written atomically (`rostrum_config::document::
//! write_atomic`: temporary file, `fsync`, rename). An `fsync` can take
//! seconds on a busy disk — tens of seconds on a saturated one — so it must
//! not run inside an actor closure: that would stall the actor, and with it
//! every call on the core, and block the Tokio worker it runs on. Instead:
//!
//! 1. `plan` runs in the actor against the current state and edits a copy of
//!    the config, returning whatever `apply` needs;
//! 2. the copy is written on the blocking pool;
//! 3. only if the write succeeded, `apply` runs in the actor with the new
//!    config already in place — memory never runs ahead of the disk.
//!
//! Changes are serialised by [`ConfigWrites`], held from plan to apply, so a
//! second change plans on top of the first one's result and two writes never
//! race to the rename.

use rostrum_config::Config;

use crate::{
    engine::{RostrumCore, state::CoreState},
    error::RostrumError,
};

/// One settings change at a time, from plan to apply.
#[derive(Default)]
pub(crate) struct ConfigWrites(tokio::sync::Mutex<()>);

impl RostrumCore {
    /// Change the settings: see the module docs.
    pub(crate) async fn change_config<P, T>(
        &self,
        plan: impl FnOnce(&CoreState, &mut Config) -> Result<P, RostrumError> + Send + 'static,
        apply: impl FnOnce(&mut CoreState, P) -> Result<T, RostrumError> + Send + 'static,
    ) -> Result<T, RostrumError>
    where
        P: Send + 'static,
        T: Send + 'static,
    {
        let _turn = self.config_writes.0.lock().await;
        let (next, path, planned) = self
            .actor
            .try_call(move |state| {
                let mut next = state.config.clone();
                let planned = plan(state, &mut next)?;
                Ok((next, state.config_path.clone(), planned))
            })
            .await?;
        let written = next.clone();
        tokio::task::spawn_blocking(move || written.save_to(&path)).await??;
        self.actor
            .try_call(move |state| {
                state.config = next;
                apply(state, planned)
            })
            .await
    }
}
