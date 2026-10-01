//! Table definitions and the one migration rule that matters: the cache is
//! disposable, the drafts table is not.

use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::error::DbError;

/// Bumping this drops and recreates every cache table on next open.
///
/// 2: pull requests carry a GraphQL `node_id`, which rows written by version 1
/// have no value for. Dropping them costs one refresh; keeping them would leave
/// cached pull requests that cannot be converted to or from draft.
///
/// 3: pull requests carry `is_cross_repository`. Rows written by version 2
/// would decode with it `false`, so a fork's pull request could be grouped
/// into a detected stack until the first refresh; and `cache_stack` arrives.
pub(crate) const CACHE_SCHEMA_VERSION: &str = "3";

/// Bumping this requires writing a real migration — draft rows are user work
/// and are never dropped.
pub(crate) const DRAFT_SCHEMA_VERSION: &str = "1";

pub(crate) const CACHE_SCHEMA_VERSION_KEY: &str = "cache_schema_version";
pub(crate) const DRAFT_SCHEMA_VERSION_KEY: &str = "draft_schema_version";

/// Every disposable table, in the order they are dropped and recreated.
///
/// These names are compile-time constants; they are the only values ever
/// interpolated into SQL text in this crate.
pub(crate) const CACHE_TABLES: &[&str] = &[
    "cache_pull_request",
    "cache_conversation",
    "cache_http",
    "cache_repo_meta",
    "cache_stack",
];

const CREATE_META: &str = "\
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

const CREATE_DRAFTS: &str = "\
CREATE TABLE IF NOT EXISTS drafts (
    repo       TEXT    NOT NULL,
    number     INTEGER NOT NULL,
    head_sha   TEXT    NOT NULL,
    comments   TEXT    NOT NULL,
    updated_at TEXT    NOT NULL,
    PRIMARY KEY (repo, number)
)";

/// The notification check's memory of what it has already seen. Not cache:
/// it is not a copy of anything GitHub holds, and dropping it with a cache
/// schema bump would change what the next check reports. Losing it is still
/// harmless — the next check re-establishes a baseline and reports nothing —
/// so a corrupt row is discarded rather than surfaced.
const CREATE_BASELINE: &str = "\
CREATE TABLE IF NOT EXISTS notification_baseline (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    payload    TEXT    NOT NULL,
    updated_at TEXT    NOT NULL
)";

/// Statements recreating the cache from nothing. Safe to run repeatedly.
const CREATE_CACHE: &[&str] = &[
    "\
CREATE TABLE IF NOT EXISTS cache_pull_request (
    repo       TEXT    NOT NULL,
    number     INTEGER NOT NULL,
    ordinal    INTEGER NOT NULL,
    payload    TEXT    NOT NULL,
    updated_at TEXT    NOT NULL,
    PRIMARY KEY (repo, number)
)",
    "CREATE INDEX IF NOT EXISTS cache_pull_request_age ON cache_pull_request (updated_at)",
    "\
CREATE TABLE IF NOT EXISTS cache_conversation (
    repo       TEXT    NOT NULL,
    number     INTEGER NOT NULL,
    payload    TEXT    NOT NULL,
    updated_at TEXT    NOT NULL,
    PRIMARY KEY (repo, number)
)",
    "CREATE INDEX IF NOT EXISTS cache_conversation_age ON cache_conversation (updated_at)",
    "\
CREATE TABLE IF NOT EXISTS cache_http (
    url        TEXT PRIMARY KEY,
    etag       TEXT NOT NULL,
    body       TEXT NOT NULL,
    updated_at TEXT NOT NULL
)",
    "CREATE INDEX IF NOT EXISTS cache_http_age ON cache_http (updated_at)",
    // Added without a version bump: `IF NOT EXISTS` creates it on the next
    // open of an existing database, and nothing already cached is invalid.
    "\
CREATE TABLE IF NOT EXISTS cache_repo_meta (
    repo       TEXT PRIMARY KEY,
    payload    TEXT NOT NULL,
    updated_at TEXT NOT NULL
)",
    "CREATE INDEX IF NOT EXISTS cache_repo_meta_age ON cache_repo_meta (updated_at)",
    "\
CREATE TABLE IF NOT EXISTS cache_stack (
    repo       TEXT PRIMARY KEY,
    payload    TEXT NOT NULL,
    updated_at TEXT NOT NULL
)",
    "CREATE INDEX IF NOT EXISTS cache_stack_age ON cache_stack (updated_at)",
];

/// Bring an open database up to the current schema.
///
/// Runs as one transaction, so a failure part-way leaves the file untouched.
/// A cache version mismatch drops the cache tables; the drafts table is only
/// ever created, never dropped.
pub(crate) async fn migrate(pool: &SqlitePool) -> Result<(), DbError> {
    // `IMMEDIATE`: the migration reads the recorded versions before it
    // writes. A deferred transaction would take a read snapshot first and
    // then fail outright (`SQLITE_BUSY_SNAPSHOT`, which the busy timeout does
    // not retry) if another connection — a previous session still flushing
    // its last writes — committed in between. Taking the write lock up front
    // makes the open wait its turn instead.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    sqlx::query(CREATE_META).execute(&mut *tx).await?;
    sqlx::query(CREATE_DRAFTS).execute(&mut *tx).await?;
    sqlx::query(CREATE_BASELINE).execute(&mut *tx).await?;

    match read_meta(&mut tx, DRAFT_SCHEMA_VERSION_KEY).await? {
        None => write_meta(&mut tx, DRAFT_SCHEMA_VERSION_KEY, DRAFT_SCHEMA_VERSION).await?,
        Some(stored) if stored != DRAFT_SCHEMA_VERSION => {
            // Deliberately non-destructive: leave both the rows and the
            // recorded version alone so a future migration can still see it.
            warn!(
                stored = %stored,
                expected = %DRAFT_SCHEMA_VERSION,
                "draft schema version mismatch; leaving drafts untouched"
            );
        }
        Some(_) => {}
    }

    let cache_version = read_meta(&mut tx, CACHE_SCHEMA_VERSION_KEY).await?;
    if cache_version.as_deref() != Some(CACHE_SCHEMA_VERSION) {
        if let Some(stored) = &cache_version {
            info!(
                stored = %stored,
                expected = %CACHE_SCHEMA_VERSION,
                "cache schema version changed; discarding cached data"
            );
        }
        for table in CACHE_TABLES {
            sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
                .execute(&mut *tx)
                .await?;
        }
    }

    for statement in CREATE_CACHE {
        sqlx::query(statement).execute(&mut *tx).await?;
    }
    write_meta(&mut tx, CACHE_SCHEMA_VERSION_KEY, CACHE_SCHEMA_VERSION).await?;

    tx.commit().await?;
    Ok(())
}

async fn read_meta(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
) -> Result<Option<String>, DbError> {
    let row = sqlx::query("SELECT value FROM meta WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get::<String, _>("value")?)),
        None => Ok(None),
    }
}

async fn write_meta(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO meta (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
