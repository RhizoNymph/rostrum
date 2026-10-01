//! The cached list of each repository's stacks, so the feed can paint stacks
//! grouped on a cold start, before the Stacks API has answered.
//!
//! Cache, not record: GitHub is the source of truth, every refresh replaces
//! the row, and an undecodable row is a miss.

use chrono::Utc;
use rostrum_core::{RepoId, Stack};
use sqlx::Row;
use tracing::warn;

use crate::{Db, error::DbError, types::encode_time};

impl Db {
    /// Replace the cached stacks for `repo`. An empty slice is stored too: "no
    /// stacks" is an answer worth remembering across a restart.
    pub async fn save_stacks(&self, repo: &RepoId, stacks: &[Stack]) -> Result<(), DbError> {
        let payload = serde_json::to_string(stacks).map_err(|source| DbError::Serde {
            kind: "stacks",
            source,
        })?;
        sqlx::query(
            "INSERT INTO cache_stack (repo, payload, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(repo) DO UPDATE SET payload = excluded.payload,
                                             updated_at = excluded.updated_at",
        )
        .bind(repo.to_string())
        .bind(payload)
        .bind(encode_time(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The cached stacks for `repo`; empty when nothing was cached or the row
    /// could not be decoded (in which case it is deleted).
    ///
    /// Stacks cached under another repository's key — which only a corrupted
    /// file could produce — are dropped, so a stack can never be grouped into
    /// the wrong repository.
    pub async fn load_stacks(&self, repo: &RepoId) -> Result<Vec<Stack>, DbError> {
        let key = repo.to_string();
        let row = sqlx::query("SELECT payload FROM cache_stack WHERE repo = ?")
            .bind(&key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(Vec::new());
        };
        let payload: String = row.try_get("payload")?;
        match serde_json::from_str::<Vec<Stack>>(&payload) {
            Ok(stacks) => Ok(stacks.into_iter().filter(|s| &s.repo == repo).collect()),
            Err(error) => {
                warn!(repo = %key, %error, "discarding undecodable cached stacks");
                sqlx::query("DELETE FROM cache_stack WHERE repo = ?")
                    .bind(&key)
                    .execute(&self.pool)
                    .await?;
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{PrNumber, RefName, StackMembers, StackNumber};

    use super::*;

    fn repo(name: &str) -> RepoId {
        RepoId::new("o", name)
    }

    fn stack(repo: &RepoId, number: u32, members: &[u32]) -> Stack {
        Stack {
            repo: repo.clone(),
            number: StackNumber::new(number),
            trunk: RefName::new("main").expect("valid"),
            members: StackMembers::new(members.iter().copied().map(PrNumber).collect())
                .expect("valid"),
        }
    }

    #[tokio::test]
    async fn stacks_round_trip_per_repository() {
        let db = Db::open_in_memory().await.expect("open");
        let a = repo("a");
        let b = repo("b");
        db.save_stacks(&a, &[stack(&a, 1, &[1, 2]), stack(&a, 2, &[5, 4, 3])])
            .await
            .expect("save");
        db.save_stacks(&b, &[]).await.expect("save");

        assert_eq!(
            db.load_stacks(&a).await.expect("load"),
            vec![stack(&a, 1, &[1, 2]), stack(&a, 2, &[5, 4, 3])]
        );
        assert!(db.load_stacks(&b).await.expect("load").is_empty());
        assert!(
            db.load_stacks(&repo("never"))
                .await
                .expect("load")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn saving_replaces_the_previous_list() {
        let db = Db::open_in_memory().await.expect("open");
        let a = repo("a");
        db.save_stacks(&a, &[stack(&a, 1, &[1, 2])])
            .await
            .expect("save");
        db.save_stacks(&a, &[stack(&a, 3, &[7, 8])])
            .await
            .expect("save");
        assert_eq!(
            db.load_stacks(&a).await.expect("load"),
            vec![stack(&a, 3, &[7, 8])]
        );
    }

    #[tokio::test]
    async fn an_undecodable_row_is_a_miss_and_is_removed() {
        let db = Db::open_in_memory().await.expect("open");
        let a = repo("a");
        db.save_stacks(&a, &[stack(&a, 1, &[1, 2])])
            .await
            .expect("save");
        // Members that violate the invariant must not decode into a stack.
        sqlx::query("UPDATE cache_stack SET payload = ?")
            .bind(r#"[{"repo":"o/a","number":1,"trunk":"main","members":[]}]"#)
            .execute(&db.pool)
            .await
            .expect("corrupt");
        assert!(db.load_stacks(&a).await.expect("load").is_empty());
        let left: i64 = sqlx::query("SELECT COUNT(*) AS n FROM cache_stack")
            .fetch_one(&db.pool)
            .await
            .expect("count")
            .try_get("n")
            .expect("n");
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn a_stack_for_another_repository_is_never_returned() {
        let db = Db::open_in_memory().await.expect("open");
        let a = repo("a");
        let b = repo("b");
        db.save_stacks(&a, &[stack(&b, 1, &[1, 2]), stack(&a, 2, &[3, 4])])
            .await
            .expect("save");
        assert_eq!(
            db.load_stacks(&a).await.expect("load"),
            vec![stack(&a, 2, &[3, 4])]
        );
    }

    #[tokio::test]
    async fn stacks_are_cache_and_go_with_a_prune() {
        let db = Db::open_in_memory().await.expect("open");
        let a = repo("a");
        db.save_stacks(&a, &[stack(&a, 1, &[1, 2])])
            .await
            .expect("save");
        db.prune_cache(chrono::Duration::seconds(-1))
            .await
            .expect("prune");
        assert!(db.load_stacks(&a).await.expect("load").is_empty());
    }
}
