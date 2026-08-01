//! Local SQLite store, with two deliberately separate responsibilities.
//!
//! 1. **Cache.** Pull request lists, conversations, and HTTP ETag/body pairs
//!    fetched from GitHub. Disposable by design: a schema bump drops it, and an
//!    undecodable row is treated as a miss rather than an error. The cost of
//!    being wrong is one extra network round-trip.
//!
//! 2. **Drafts.** Review comments the user authored locally and has never sent
//!    anywhere. There is no other copy. Draft rows survive a cache schema
//!    change, a corrupt draft surfaces as an error instead of being discarded,
//!    and [`Db::prune_cache`] never touches them.
//!
//! Domain values are stored as JSON text: the readers of this data are Rust
//! types with `serde` impls, and nothing queries across their fields.

mod cache;
mod drafts;
mod error;
mod schema;
mod types;

use std::{path::Path, time::Duration};

use chrono::Utc;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

pub use error::DbError;
pub use types::{CachedResponse, DraftSet};

use types::encode_time;

/// A handle to the local store. Cheap to clone; wraps a connection pool.
#[derive(Clone, Debug)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open the database at `path`, creating the file and any missing parent
    /// directories, then migrate.
    pub async fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| DbError::Path {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        Self::from_options(options, 4).await
    }

    /// An ephemeral database, for tests.
    ///
    /// Backed by a single connection, because every connection to `:memory:`
    /// would otherwise get a private database of its own.
    pub async fn open_in_memory() -> Result<Self, DbError> {
        let options = SqliteConnectOptions::new()
            .filename(":memory:")
            .create_if_missing(true)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        Self::from_options(options, 1).await
    }

    async fn from_options(
        options: SqliteConnectOptions,
        max_connections: u32,
    ) -> Result<Self, DbError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;

        schema::migrate(&pool).await?;
        Ok(Self { pool })
    }

    /// Delete cache rows last written more than `max_age` ago.
    ///
    /// Returns the number of rows removed. Drafts are not cache and are never
    /// considered here, however old they are.
    pub async fn prune_cache(&self, max_age: chrono::Duration) -> Result<u64, DbError> {
        let Some(cutoff) = Utc::now().checked_sub_signed(max_age) else {
            tracing::warn!(?max_age, "prune horizon is out of range; pruning nothing");
            return Ok(0);
        };
        let cutoff = encode_time(cutoff);

        let mut tx = self.pool.begin().await?;
        let mut deleted = 0;
        for table in schema::CACHE_TABLES {
            let result = sqlx::query(&format!("DELETE FROM {table} WHERE updated_at < ?"))
                .bind(&cutoff)
                .execute(&mut *tx)
                .await?;
            deleted += result.rows_affected();
        }
        tx.commit().await?;
        Ok(deleted)
    }

    /// Close the pool, flushing WAL state. Optional; dropping also works.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Raw pool access, so tests can plant corrupt rows, backdate timestamps,
    /// and forge schema versions without any of that being public API.
    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests;
