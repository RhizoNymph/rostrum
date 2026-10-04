//! `rostrumd`: the headless desktop half of rostrum's phone pairing.
//!
//! Two servers share one [`Daemon`]:
//!
//! - **The page server** ([`web`]), plain HTTP on `http_port`: a single
//!   self-contained page where anyone on the LAN or tailnet downloads the
//!   Android APK, and where — from this computer or over the tailnet only —
//!   pairing codes are generated and paired devices revoked.
//! - **The API** ([`api`]), HTTPS on `https_port` with a self-signed
//!   certificate the phone pins by fingerprint: every route in
//!   [`rostrum_remote::routes`], which exchange a pairing code for a device
//!   token and then drive `rostrum-local` on the user's clones.
//!
//! State is owned by two actors fed over channels rather than shared behind
//! locks: [`registry`] (pairing codes and paired devices) and [`jobs`] (which
//! clone is busy, the sync-all run, and the record of handed-off conflicts).
//!
//! See `docs/features/rostrumd.md` for the data flow end to end.

pub mod api;
pub mod app;
pub mod boxed;
pub mod config_push;
pub mod convert;
pub mod daemon;
pub mod error;
pub mod fsutil;
pub mod github;
pub mod http;
pub mod jobs;
pub mod logging;
pub mod net;
pub mod random;
pub mod registry;
pub mod rostrum_config;
pub mod server;
pub mod settings;
pub mod stacks;
pub mod state_file;
#[cfg(test)]
pub(crate) mod testkit;
pub mod tls;
pub mod tmux;
pub mod web;

pub use daemon::{Daemon, DaemonParts};
pub use error::StartupError;
pub use settings::Settings;
pub use tls::TlsIdentity;

/// `rostrumd`'s own version, reported in [`rostrum_remote::MachineInfo`].
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
