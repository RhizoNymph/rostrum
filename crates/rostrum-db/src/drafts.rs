//! The durable half of the store.
//!
//! A draft is a review comment the user typed that has never left this machine.
//! There is no copy of it on GitHub, so nothing here silently discards a row:
//! a draft that fails to decode is an error, and a cache schema bump does not
//! touch this table.

use chrono::Utc;
use rostrum_core::{PrNumber, RepoId, Side};
use rostrum_github::DraftComment;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    Db,
    error::DbError,
    types::{DraftSet, decode_time, encode_time},
};

/// On-disk shape of a [`DraftComment`].
///
/// `DraftComment` derives only `Serialize` — its serialised form is the GitHub
/// request payload, where absent keys are meaningful. This mirror adds the
/// `Deserialize` half without coupling the storage format to a wire format that
/// GitHub controls.
#[derive(Debug, Serialize, Deserialize)]
struct StoredDraft {
    path: String,
    line: u32,
    side: Side,
    #[serde(default)]
    start_line: Option<u32>,
    #[serde(default)]
    start_side: Option<Side>,
    body: String,
}

impl From<&DraftComment> for StoredDraft {
    fn from(comment: &DraftComment) -> Self {
        Self {
            path: comment.path.clone(),
            line: comment.line,
            side: comment.side,
            start_line: comment.start_line,
            start_side: comment.start_side,
            body: comment.body.clone(),
        }
    }
}

impl From<StoredDraft> for DraftComment {
    fn from(stored: StoredDraft) -> Self {
        Self {
            path: stored.path,
            line: stored.line,
            side: stored.side,
            start_line: stored.start_line,
            start_side: stored.start_side,
            body: stored.body,
        }
    }
}

impl Db {
    /// Persist the full set of drafts for one pull request, replacing whatever
    /// was there.
    ///
    /// An empty `drafts` slice stores an empty set rather than removing the
    /// row; use [`Db::clear_drafts`] to forget the pull request entirely.
    pub async fn save_drafts(
        &self,
        repo: &RepoId,
        pr: PrNumber,
        head_sha: &str,
        drafts: &[DraftComment],
    ) -> Result<(), DbError> {
        let stored: Vec<StoredDraft> = drafts.iter().map(StoredDraft::from).collect();
        let payload = serde_json::to_string(&stored).map_err(|source| DbError::Serde {
            kind: "drafts",
            source,
        })?;

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO drafts (repo, number, head_sha, comments, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(repo, number) DO UPDATE SET
                 head_sha   = excluded.head_sha,
                 comments   = excluded.comments,
                 updated_at = excluded.updated_at",
        )
        .bind(repo.to_string())
        .bind(i64::from(pr.0))
        .bind(head_sha)
        .bind(&payload)
        .bind(encode_time(Utc::now()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The stored drafts for one pull request, or `None` if there are none.
    ///
    /// Unlike the cache, a row that fails to decode is reported as
    /// [`DbError::CorruptDraft`] and left in place, so the user's work is not
    /// destroyed by a read.
    pub async fn load_drafts(
        &self,
        repo: &RepoId,
        pr: PrNumber,
    ) -> Result<Option<DraftSet>, DbError> {
        let repo_key = repo.to_string();
        let row = sqlx::query(
            "SELECT head_sha, comments, updated_at FROM drafts WHERE repo = ? AND number = ?",
        )
        .bind(&repo_key)
        .bind(i64::from(pr.0))
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let head_sha: String = row.try_get("head_sha")?;
        let payload: String = row.try_get("comments")?;
        let updated_at: String = row.try_get("updated_at")?;

        let stored: Vec<StoredDraft> =
            serde_json::from_str(&payload).map_err(|source| DbError::CorruptDraft {
                repo: repo_key,
                pr: pr.0,
                source,
            })?;

        Ok(Some(DraftSet {
            head_sha,
            comments: stored.into_iter().map(DraftComment::from).collect(),
            updated_at: decode_time(&updated_at)?,
        }))
    }

    /// Forget the drafts for one pull request. Succeeds when there are none.
    pub async fn clear_drafts(&self, repo: &RepoId, pr: PrNumber) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM drafts WHERE repo = ? AND number = ?")
            .bind(repo.to_string())
            .bind(i64::from(pr.0))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
