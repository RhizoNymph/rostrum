//! Structured logs to stdout, which systemd sends to the journal.
//!
//! `RUST_LOG` overrides the default filter. Under systemd (`JOURNAL_STREAM`
//! set) timestamps and colour are left to the journal.

use tracing_subscriber::EnvFilter;

/// rostrumd and the rostrum crates at `info`; the TLS and HTTP stacks only
/// when something is wrong.
pub const DEFAULT_FILTER: &str = "info,rustls=warn,hyper=warn,hyper_util=warn,axum_server=warn";

pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let journal = std::env::var_os("JOURNAL_STREAM").is_some();
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(!journal && std::io::IsTerminal::is_terminal(&std::io::stdout()));
    // `try_init`: a second call (tests) keeps the first subscriber.
    let _ = if journal {
        builder.without_time().try_init()
    } else {
        builder.try_init()
    };
}
