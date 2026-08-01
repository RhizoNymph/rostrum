//! The single error type surfaced by this crate.

use std::path::PathBuf;

/// Everything that can go wrong talking to the local store.
///
/// Note what is *absent*: a corrupt cache row is not an error. Cache rows are
/// disposable, so an undecodable one is logged, deleted, and reported to the
/// caller as a miss. A corrupt *draft* row is the opposite — it is unsent work
/// the user typed, so it surfaces as [`DbError::CorruptDraft`] rather than
/// being quietly thrown away.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DbError {
    #[error("sqlite error")]
    Sql(#[from] sqlx::Error),

    /// A value could not be encoded to JSON on the way into the database.
    #[error("could not encode {kind} as JSON")]
    Serde {
        kind: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// A stored draft could not be decoded. Never silently discarded.
    #[error("stored drafts for {repo}#{pr} are corrupt")]
    CorruptDraft {
        repo: String,
        pr: u32,
        #[source]
        source: serde_json::Error,
    },

    /// The database file's parent directory could not be created.
    #[error("could not prepare database directory `{}`", path.display())]
    Path {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A stored timestamp was not the RFC 3339 text this crate writes.
    #[error("stored timestamp `{value}` is not valid RFC 3339")]
    Timestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
}
