//! Cached repository metadata — owner, push and creation times, stars.
//!
//! Cache like the pull request lists beside it: GitHub is the source of
//! truth, and an undecodable row is a miss. It is cached at all so a feed
//! sorted by "pushed" or "stars" opens in that order, instead of opening in
//! name order and reshuffling one repository at a time as the staggered
//! first refresh answers.

use chrono::Utc;
use rostrum_core::{RepoId, RepoMeta};
use sqlx::Row;
use tracing::warn;

use crate::{Db, error::DbError, types::encode_time};

impl Db {
    /// Record `repo`'s metadata, replacing what was there.
    pub async fn save_repo_meta(&self, repo: &RepoId, meta: &RepoMeta) -> Result<(), DbError> {
        let payload = serde_json::to_string(meta).map_err(|source| DbError::Serde {
            kind: "repository metadata",
            source,
        })?;
        sqlx::query(
            "INSERT INTO cache_repo_meta (repo, payload, updated_at)
             VALUES (?, ?, ?)
             ON CONFLICT(repo) DO UPDATE SET
                 payload    = excluded.payload,
                 updated_at = excluded.updated_at",
        )
        .bind(repo.to_string())
        .bind(&payload)
        .bind(encode_time(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `repo`'s cached metadata, or `None` on a miss. An undecodable row is
    /// a miss: it is logged, deleted, and reported as absent.
    pub async fn load_repo_meta(&self, repo: &RepoId) -> Result<Option<RepoMeta>, DbError> {
        let repo_key = repo.to_string();
        let row = sqlx::query("SELECT payload FROM cache_repo_meta WHERE repo = ?")
            .bind(&repo_key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let payload: String = row.try_get("payload")?;

        match serde_json::from_str::<RepoMeta>(&payload) {
            Ok(meta) => Ok(Some(meta)),
            Err(error) => {
                warn!(repo = %repo_key, %error, "discarding undecodable cached repository metadata");
                sqlx::query("DELETE FROM cache_repo_meta WHERE repo = ?")
                    .bind(&repo_key)
                    .execute(&self.pool)
                    .await?;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration};
    use rostrum_core::{OwnerKind, RepoOwner};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).expect("valid timestamp")
    }

    fn meta(stars: u32) -> RepoMeta {
        RepoMeta {
            owner: RepoOwner {
                login: "RhizoNymph".into(),
                kind: OwnerKind::User,
            },
            pushed_at: Some(at(30)),
            created_at: at(0),
            updated_at: at(20),
            stars,
        }
    }

    fn repo(name: &str) -> RepoId {
        RepoId::new("rhizonymph", name)
    }

    #[tokio::test]
    async fn nothing_is_cached_for_an_unseen_repository() {
        let db = Db::open_in_memory().await.expect("db");
        assert_eq!(db.load_repo_meta(&repo("a")).await.expect("load"), None);
    }

    #[tokio::test]
    async fn metadata_round_trips_and_a_save_replaces_it() {
        let db = Db::open_in_memory().await.expect("db");
        db.save_repo_meta(&repo("a"), &meta(1)).await.expect("save");
        db.save_repo_meta(&repo("b"), &meta(2)).await.expect("save");
        assert_eq!(
            db.load_repo_meta(&repo("a")).await.expect("load"),
            Some(meta(1))
        );

        db.save_repo_meta(&repo("a"), &meta(5)).await.expect("save");
        assert_eq!(
            db.load_repo_meta(&repo("a")).await.expect("load"),
            Some(meta(5))
        );
        assert_eq!(
            db.load_repo_meta(&repo("b")).await.expect("load"),
            Some(meta(2))
        );
    }

    #[tokio::test]
    async fn a_corrupt_row_is_a_miss_and_is_removed() {
        let db = Db::open_in_memory().await.expect("db");
        sqlx::query(
            "INSERT INTO cache_repo_meta (repo, payload, updated_at) VALUES ('rhizonymph/a', 'nope', 'x')",
        )
        .execute(db.pool())
        .await
        .expect("plant");
        assert_eq!(db.load_repo_meta(&repo("a")).await.expect("load"), None);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cache_repo_meta")
            .fetch_one(db.pool())
            .await
            .expect("count");
        assert_eq!(remaining, 0);
    }

    /// It is cache, so pruning stale rows reaches it like every other cache
    /// table.
    #[tokio::test]
    async fn pruning_reaches_repository_metadata() {
        let db = Db::open_in_memory().await.expect("db");
        db.save_repo_meta(&repo("a"), &meta(1)).await.expect("save");
        sqlx::query("UPDATE cache_repo_meta SET updated_at = '2000-01-01T00:00:00.000000Z'")
            .execute(db.pool())
            .await
            .expect("backdate");
        let deleted = db.prune_cache(Duration::days(1)).await.expect("prune");
        assert_eq!(deleted, 1);
        assert_eq!(db.load_repo_meta(&repo("a")).await.expect("load"), None);
    }
}
