//! The notification check's seen set, kept between background runs.
//!
//! One row, holding a serialised [`Baseline`]. It is derived state, not user
//! work: an undecodable row is logged, deleted and reported as absent, and
//! the next check simply establishes a fresh baseline and reports nothing.

use chrono::Utc;
use rostrum_core::Baseline;
use sqlx::Row;

use crate::{Db, error::DbError, types::encode_time};

impl Db {
    /// Store the baseline, replacing the previous one.
    pub async fn save_baseline(&self, baseline: &Baseline) -> Result<(), DbError> {
        let payload = serde_json::to_string(baseline).map_err(|source| DbError::Serde {
            kind: "notification baseline",
            source,
        })?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO notification_baseline (id, payload, updated_at)
             VALUES (1, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 payload    = excluded.payload,
                 updated_at = excluded.updated_at",
        )
        .bind(&payload)
        .bind(encode_time(Utc::now()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The stored baseline, or `None` before the first check (or after a
    /// corrupt row was discarded).
    pub async fn load_baseline(&self) -> Result<Option<Baseline>, DbError> {
        let row = sqlx::query("SELECT payload FROM notification_baseline WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let payload: String = row.try_get("payload")?;
        match serde_json::from_str(&payload) {
            Ok(baseline) => Ok(Some(baseline)),
            Err(error) => {
                tracing::warn!(%error, "discarding undecodable notification baseline");
                sqlx::query("DELETE FROM notification_baseline WHERE id = 1")
                    .execute(&self.pool)
                    .await?;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rostrum_core::{
        LoadState, LoginKey, MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, RepoState,
    };

    use super::*;

    fn loaded(numbers: &[u32]) -> RepoState {
        let mut repo = RepoState::new("a/b".parse().expect("repo"));
        repo.load = LoadState::Loaded { at: Utc::now() };
        repo.prs = numbers
            .iter()
            .map(|&n| PullRequest {
                number: PrNumber(n),
                node_id: NodeId(format!("PR_{n}")),
                title: format!("PR {n}"),
                url: String::new(),
                is_draft: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                author: None,
                head_ref: "f".into(),
                head_sha: "s".into(),
                base_ref: "main".into(),
                additions: 0,
                deletions: 0,
                changed_files: 0,
                mergeable: Mergeable::Unknown,
                merge_state: MergeStateStatus::Unknown,
                review_decision: None,
                assignees: Vec::new(),
                review_requests: Vec::new(),
                labels: Vec::new(),
                comment_count: 0,
                checks: None,
                base_divergence: None,
                pushed_at: None,
            })
            .collect();
        repo
    }

    #[tokio::test]
    async fn nothing_is_stored_before_the_first_save() {
        let db = Db::open_in_memory().await.expect("db");
        assert_eq!(db.load_baseline().await.expect("load"), None);
    }

    #[tokio::test]
    async fn a_baseline_round_trips_and_keeps_reporting_correctly() {
        let db = Db::open_in_memory().await.expect("db");
        let viewer = LoginKey::new("me");
        let mut baseline = Baseline::new();
        baseline.observe(&[loaded(&[1, 2])], Some(&viewer));
        db.save_baseline(&baseline).await.expect("save");

        let mut restored = db.load_baseline().await.expect("load").expect("stored");
        assert_eq!(restored, baseline);
        let arrivals = restored.observe(&[loaded(&[1, 2, 3])], Some(&viewer));
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].number, PrNumber(3));

        // Saving again replaces the single row.
        db.save_baseline(&restored).await.expect("save");
        assert_eq!(db.load_baseline().await.expect("load"), Some(restored));
    }

    #[tokio::test]
    async fn a_corrupt_baseline_is_discarded() {
        let db = Db::open_in_memory().await.expect("db");
        sqlx::query(
            "INSERT INTO notification_baseline (id, payload, updated_at) VALUES (1, 'nope', 'x')",
        )
        .execute(db.pool())
        .await
        .expect("plant");
        assert_eq!(db.load_baseline().await.expect("load"), None);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notification_baseline")
            .fetch_one(db.pool())
            .await
            .expect("count");
        assert_eq!(remaining, 0);
    }
}
