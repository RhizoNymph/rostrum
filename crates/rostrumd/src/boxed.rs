//! The one future alias the injectable backends share.

use std::{future::Future, pin::Pin};

/// A boxed, sendable future. Backends that tests replace ([`crate::jobs::JobRunner`],
/// [`crate::github::HandoverSource`], [`crate::tmux::SessionLister`]) return
/// one so they can sit behind `Arc<dyn …>`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
