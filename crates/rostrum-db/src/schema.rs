//! Table definitions and the one migration rule that matters: the cache is
//! disposable, the drafts table is not.

use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::error::DbError;

/// Bumping this drops and recreates every cache table on next open.
pub(crate) const CACHE_SCHEMA_VERSION: &str = "1";

/// Bumping this requires writing a real migration — draft rows are user work
/// and are never dropped.
pub(crate) const DRAFT_SCHEMA_VERSION: &str = "1";

pub(crate) const CACHE_SCHEMA_VERSION_KEY: &str = "cache_schema_version";
pub(crate) const DRAFT_SCHEMA_VERSION_KEY: &str = "draft_schema_version";

/// Every disposable table, in the order they are dropped and recreated.
///
/// These names are compile-time constants; they are the only values ever
/// interpolated into SQL text in this crate.
pub(crate) const CACHE_TABLES: &[&str] =
    &["cache_pull_request", "cache_conversation", "cache_http"];

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
];

/// Bring an open database up to the current schema.
///
/// Runs as one transaction, so a failure part-way leaves the file untouched.
/// A cache version mismatch drops the cache tables; the drafts table is only
/// ever created, never dropped.
pub(crate) async fn migrate(pool: &SqlitePool) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;

    sqlx::query(CREATE_META).execute(&mut *tx).await?;
    sqlx::query(CREATE_DRAFTS).execute(&mut *tx).await?;

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
