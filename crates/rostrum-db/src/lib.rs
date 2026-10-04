//! Local SQLite store, with two deliberately separate responsibilities.
//!
//! 1. **Cache.** Pull request lists, repository metadata, conversations, and
//!    HTTP ETag/body pairs fetched from GitHub. Disposable by design: a schema
//!    bump drops it, and an undecodable row is treated as a miss rather than
//!    an error. The cost of being wrong is one extra network round-trip.
//!
//! 2. **Drafts.** Review comments the user authored locally and has never sent
//!    anywhere. There is no other copy. Draft rows survive a cache schema
//!    change, a corrupt draft surfaces as an error instead of being discarded,
//!    and [`Db::prune_cache`] never touches them.
//!
//! One further table sits between the two: the notification check's seen set
//! ([`Db::save_baseline`]). It survives cache schema changes like drafts do,
//! but it is derived state, so a corrupt row is discarded like cache.
//!
//! Domain values are stored as JSON text: the readers of this data are Rust
//! types with `serde` impls, and nothing queries across their fields.

mod baseline;
mod cache;
mod drafts;
mod error;
mod files;
mod issues;
mod repo_meta;
mod schema;
mod stacks;
mod types;

use std::{path::Path, time::Duration};

use chrono::Utc;
use sqlx::{
    ConnectOptions, Connection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

pub use error::DbError;
pub use types::{CachedResponse, DraftSet};

use types::encode_time;

/// How long a query waits for a free pooled connection. Only contention
/// between this store's own callers counts against it: creating and migrating
/// the database, which waits on the disk, happens before the pool exists.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

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

        // Create and migrate on a connection of its own, before any pool
        // exists. Creating the file and switching it to WAL each `fsync`, and
        // an `fsync` has no upper bound on a busy disk: done through the
        // pool, a slow disk turned into `PoolTimedOut` after the pool's
        // acquire timeout, though nothing was wrong. A plain connection waits
        // as long as the disk needs, and still fails on a real error.
        let mut setup = options.clone().connect().await?;
        schema::migrate(&mut setup).await?;

        let pool = Self::pool_options(4).connect_lazy_with(options);
        // Open the pool's first connection before the setup one closes:
        // closing a WAL database's last connection checkpoints it, which is
        // another `fsync` for nothing. This also surfaces a pool that cannot
        // connect here, at open, rather than on the first query.
        drop(pool.acquire().await?);
        setup.close().await?;
        Ok(Self { pool })
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

        let pool = Self::pool_options(1).connect_with(options).await?;
        let mut conn = pool.acquire().await?;
        schema::migrate(&mut conn).await?;
        Ok(Self { pool })
    }

    fn pool_options(max_connections: u32) -> SqlitePoolOptions {
        SqlitePoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .acquire_timeout(ACQUIRE_TIMEOUT)
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
