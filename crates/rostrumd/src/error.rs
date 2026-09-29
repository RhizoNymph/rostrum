//! Why the daemon could not start. Everything after startup is answered per
//! request instead (see [`crate::http::ApiFailure`]).

use std::path::PathBuf;

use crate::{
    net::listen::ListenError, settings::SettingsError, state_file::StoreError, tls::TlsError,
};

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("could not determine the home directory")]
    NoHome,
    #[error("could not determine the config directory")]
    NoConfigDir,
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("could not create {}", path.display())]
    Directory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the {server} server could not listen")]
    Listen {
        server: &'static str,
        #[source]
        source: ListenError,
    },
    #[error("the {server} server could not start")]
    Serve {
        server: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("could not wait for a shutdown signal")]
    Signal(#[source] std::io::Error),
    #[error("{0}")]
    Usage(String),
}
