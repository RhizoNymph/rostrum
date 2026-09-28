//! The state actor: one task owns [`CoreState`] and runs closures sent to it,
//! one at a time.
//!
//! Every exported method is called from Kotlin coroutines that can run
//! concurrently. Instead of guarding the state with a lock, the state lives in
//! a task and methods send it work: a closure over `&mut CoreState` whose
//! result comes back over a oneshot. A closure never awaits — network and disk
//! happen in the calling method, outside the actor, and their results are
//! sent back as another closure — so no call can hold the state across a slow
//! operation.

use std::panic::{AssertUnwindSafe, catch_unwind};

use tokio::sync::{mpsc, oneshot};

use crate::{engine::state::CoreState, error::RostrumError};

type Job = Box<dyn FnOnce(&mut CoreState) + Send>;

/// A handle for sending work to the state. Cloning it is cheap; the actor
/// stops when the last handle is dropped.
#[derive(Clone)]
pub(crate) struct Actor {
    tx: mpsc::UnboundedSender<Job>,
}

/// A handle that does not keep the actor alive, for background tasks started
/// from inside it: they must not outlive the core that started them.
#[derive(Clone)]
pub(crate) struct WeakActor {
    tx: mpsc::WeakUnboundedSender<Job>,
}

impl Actor {
    /// Start the actor on the current Tokio runtime. `build` receives a weak
    /// handle to the actor itself, for the state to start background work
    /// that reports back.
    pub(crate) fn spawn(build: impl FnOnce(WeakActor) -> CoreState) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let mut state = build(WeakActor {
            tx: tx.downgrade(),
        });
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                // A panicking closure drops its reply, so its caller gets an
                // `Internal` error; the actor itself keeps serving.
                if catch_unwind(AssertUnwindSafe(|| job(&mut state))).is_err() {
                    tracing::error!("a state update panicked");
                }
            }
            tracing::debug!("state actor stopped");
        });
        Self { tx }
    }

    /// Run `f` against the state and return what it returns.
    pub(crate) async fn call<R, F>(&self, f: F) -> Result<R, RostrumError>
    where
        F: FnOnce(&mut CoreState) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Box::new(move |state| {
                // The caller may have been cancelled; nothing to do then.
                let _ = reply.send(f(state));
            }))
            .map_err(|_| RostrumError::internal("the core has shut down"))?;
        answer
            .await
            .map_err(|_| RostrumError::internal("a state update failed"))
    }

    /// Like [`Actor::call`] for closures that can fail.
    pub(crate) async fn try_call<R, F>(&self, f: F) -> Result<R, RostrumError>
    where
        F: FnOnce(&mut CoreState) -> Result<R, RostrumError> + Send + 'static,
        R: Send + 'static,
    {
        self.call(f).await?
    }
}

impl WeakActor {
    /// A strong handle, or `None` once the core has been dropped.
    pub(crate) fn upgrade(&self) -> Option<Actor> {
        self.tx.upgrade().map(|tx| Actor { tx })
    }
}
