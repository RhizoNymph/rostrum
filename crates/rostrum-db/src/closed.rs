//! Cached closed pull requests: each repository's last fetched window of
//! merged and closed-unmerged pull requests, so the Closed tab paints on a
//! cold start before its first fetch lands.
//!
//! Cache, with the same rules as the other lists: a row that no longer
//! decodes is logged, deleted, and reported as a miss. A table of its own,
//! because the payload is a `ClosedPull` (a pull request and its ending),
//! not a `PullRequest`, and because a pull request can sit in both lists for
//! a moment — the open list a poll behind its merge — without either write
//! replacing the other.

use chrono::Utc;
use rostrum_core::{ClosedPull, RepoId};
use sqlx::Row;
use tracing::warn;

use crate::{Db, error::DbError, types::encode_time};

impl Db {
    /// Replace the cached closed pull requests for `repo` in one
    /// transaction, keeping their order.
    pub async fn save_closed_pulls(
        &self,
        repo: &RepoId,
        closed: &[ClosedPull],
    ) -> Result<(), DbError> {
        let repo_key = repo.to_string();
        let now = encode_time(Utc::now());

        // Encode before the transaction opens, so a bad value cannot leave a
        // half-written list behind.
        let mut encoded = Vec::with_capacity(closed.len());
        for pull in closed {
            let payload = serde_json::to_string(pull).map_err(|source| DbError::Serde {
                kind: "closed pull request",
                source,
            })?;
            encoded.push((pull.number(), payload));
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM cache_closed_pull WHERE repo = ?")
            .bind(&repo_key)
            .execute(&mut *tx)
            .await?;
        for (ordinal, (number, payload)) in encoded.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO cache_closed_pull (repo, number, ordinal, payload, updated_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&repo_key)
            .bind(i64::from(number.0))
            .bind(ordinal as i64)
            .bind(&payload)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Cached closed pull requests for `repo`, in the order they were saved.
    pub async fn load_closed_pulls(&self, repo: &RepoId) -> Result<Vec<ClosedPull>, DbError> {
        let repo_key = repo.to_string();
        let rows = sqlx::query(
            "SELECT number, payload FROM cache_closed_pull WHERE repo = ? ORDER BY ordinal",
        )
        .bind(&repo_key)
        .fetch_all(&self.pool)
        .await?;

        let mut closed = Vec::with_capacity(rows.len());
        let mut corrupt = Vec::new();
        for row in rows {
            let number: i64 = row.try_get("number")?;
            let payload: String = row.try_get("payload")?;
            match serde_json::from_str::<ClosedPull>(&payload) {
                Ok(pull) => closed.push(pull),
                Err(error) => {
                    warn!(
                        repo = %repo_key,
                        number,
                        %error,
                        "discarding undecodable cached closed pull request"
                    );
                    corrupt.push(number);
                }
            }
        }

        if !corrupt.is_empty() {
            let mut tx = self.pool.begin().await?;
            for number in corrupt {
                sqlx::query("DELETE FROM cache_closed_pull WHERE repo = ? AND number = ?")
                    .bind(&repo_key)
                    .bind(number)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }

        Ok(closed)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration};
    use rostrum_core::{
        Label, MergeStateStatus, Mergeable, NodeId, PrNumber, PullEnding, PullRequest, User,
    };

    use super::*;

    fn repo(name: &str) -> RepoId {
        RepoId::new("rhizonymph", name)
    }

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid timestamp")
    }

    /// Every field the closed query fills, populated, so one that fails to
    /// round-trip is caught.
    fn pull(number: u32) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{number}")),
            title: format!("PR {number}"),
            url: format!("https://github.com/rhizonymph/rostrum/pull/{number}"),
            is_draft: number.is_multiple_of(2),
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000 + i64::from(number)),
            pushed_at: None,
            author: Some(User {
                login: "author".into(),
                avatar_url: Some("https://avatars.example/author".into()),
            }),
            head_ref: format!("branch-{number}"),
            head_sha: "abc123".into(),
            base_ref: "main".into(),
            additions: 3,
            deletions: 1,
            changed_files: 2,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: vec![Label {
                name: "bug".into(),
                color: "d73a4a".into(),
            }],
            comment_count: 4,
            checks: None,
            base_divergence: None,
            is_cross_repository: true,
        }
    }

    fn merged(number: u32) -> ClosedPull {
        ClosedPull {
            pull: pull(number),
            ending: PullEnding::Merged {
                at: at(1_800_000_000 + i64::from(number)),
            },
        }
    }

    fn closed(number: u32) -> ClosedPull {
        ClosedPull {
            pull: pull(number),
            ending: PullEnding::Closed {
                at: at(1_800_000_000 - i64::from(number)),
            },
        }
    }

    async fn db() -> Db {
        Db::open_in_memory()
            .await
            .expect("in-memory database opens")
    }

    #[tokio::test]
    async fn closed_pulls_round_trip_in_their_saved_order() {
        let db = db().await;
        let list = vec![merged(9), closed(2), merged(5)];
        db.save_closed_pulls(&repo("rostrum"), &list)
            .await
            .expect("save");
        assert_eq!(
            db.load_closed_pulls(&repo("rostrum")).await.expect("load"),
            list
        );
    }

    #[tokio::test]
    async fn saving_replaces_rather_than_appends() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_closed_pulls(&repo, &[merged(1), closed(2)])
            .await
            .expect("save");
        db.save_closed_pulls(&repo, &[closed(3)])
            .await
            .expect("save");
        assert_eq!(
            db.load_closed_pulls(&repo).await.expect("load"),
            vec![closed(3)]
        );
        db.save_closed_pulls(&repo, &[]).await.expect("clear");
        assert!(db.load_closed_pulls(&repo).await.expect("load").is_empty());
    }

    /// A pull request can be in the open list and the closed list at once —
    /// the open list a poll behind a merge — and neither write disturbs the
    /// other.
    #[tokio::test]
    async fn closed_pulls_are_scoped_per_repo_and_apart_from_open_ones() {
        let db = db().await;
        db.save_pull_requests(&repo("a"), &[pull(1)])
            .await
            .expect("save open");
        db.save_closed_pulls(&repo("a"), &[merged(1)])
            .await
            .expect("save closed");
        db.save_closed_pulls(&repo("b"), &[closed(2)])
            .await
            .expect("save closed");

        assert_eq!(
            db.load_closed_pulls(&repo("a")).await.expect("load"),
            vec![merged(1)]
        );
        assert_eq!(
            db.load_closed_pulls(&repo("b")).await.expect("load"),
            vec![closed(2)]
        );
        assert_eq!(
            db.load_pull_requests(&repo("a")).await.expect("load"),
            vec![pull(1)]
        );
    }

    #[tokio::test]
    async fn corrupt_rows_are_misses_and_are_dropped() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_closed_pulls(&repo, &[merged(1), closed(2)])
            .await
            .expect("save");
        sqlx::query("UPDATE cache_closed_pull SET payload = '{' WHERE number = 1")
            .execute(db.pool())
            .await
            .expect("corrupt");

        assert_eq!(
            db.load_closed_pulls(&repo).await.expect("load"),
            vec![closed(2)]
        );
        let remaining: i64 = sqlx::query("SELECT COUNT(*) AS n FROM cache_closed_pull")
            .fetch_one(db.pool())
            .await
            .expect("count")
            .try_get("n")
            .expect("n");
        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn pruning_reaches_the_closed_table() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_closed_pulls(&repo, &[merged(1)])
            .await
            .expect("save");
        sqlx::query("UPDATE cache_closed_pull SET updated_at = ?")
            .bind(encode_time(Utc::now() - Duration::days(30)))
            .execute(db.pool())
            .await
            .expect("backdate");
        assert_eq!(db.prune_cache(Duration::days(1)).await.expect("prune"), 1);
        assert!(db.load_closed_pulls(&repo).await.expect("load").is_empty());
    }
}
