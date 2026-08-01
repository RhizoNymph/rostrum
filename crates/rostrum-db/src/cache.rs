//! The disposable half of the store.
//!
//! Everything here mirrors data that GitHub is the source of truth for. If a
//! row cannot be decoded it is dropped and reported as a miss, because the
//! worst case is one extra round-trip.

use chrono::Utc;
use rostrum_core::{Conversation, PrNumber, PullRequest, RepoId};
use sqlx::Row;
use tracing::warn;

use crate::{
    Db,
    error::DbError,
    types::{CachedResponse, encode_time},
};

impl Db {
    /// Replace the cached pull request list for `repo` in one transaction.
    ///
    /// Pull requests absent from `prs` are removed, so passing an empty slice
    /// clears the repo.
    pub async fn save_pull_requests(
        &self,
        repo: &RepoId,
        prs: &[PullRequest],
    ) -> Result<(), DbError> {
        let repo_key = repo.to_string();
        let now = encode_time(Utc::now());

        // Encode before opening the transaction so a bad value cannot leave a
        // half-written set behind.
        let mut encoded = Vec::with_capacity(prs.len());
        for pr in prs {
            let payload = serde_json::to_string(pr).map_err(|source| DbError::Serde {
                kind: "pull request",
                source,
            })?;
            encoded.push((pr.number, payload));
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM cache_pull_request WHERE repo = ?")
            .bind(&repo_key)
            .execute(&mut *tx)
            .await?;
        for (ordinal, (number, payload)) in encoded.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO cache_pull_request (repo, number, ordinal, payload, updated_at)
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

    /// Cached pull requests for `repo`, in the order they were saved.
    ///
    /// A repo that was never saved and a repo saved with an empty list are
    /// indistinguishable: both yield an empty vector.
    pub async fn load_pull_requests(&self, repo: &RepoId) -> Result<Vec<PullRequest>, DbError> {
        let repo_key = repo.to_string();
        let rows = sqlx::query(
            "SELECT number, payload FROM cache_pull_request WHERE repo = ? ORDER BY ordinal",
        )
        .bind(&repo_key)
        .fetch_all(&self.pool)
        .await?;

        let mut prs = Vec::with_capacity(rows.len());
        let mut corrupt = Vec::new();
        for row in rows {
            let number: i64 = row.try_get("number")?;
            let payload: String = row.try_get("payload")?;
            match serde_json::from_str::<PullRequest>(&payload) {
                Ok(pr) => prs.push(pr),
                Err(error) => {
                    warn!(
                        repo = %repo_key,
                        number,
                        %error,
                        "discarding undecodable cached pull request"
                    );
                    corrupt.push(number);
                }
            }
        }

        if !corrupt.is_empty() {
            let mut tx = self.pool.begin().await?;
            for number in corrupt {
                sqlx::query("DELETE FROM cache_pull_request WHERE repo = ? AND number = ?")
                    .bind(&repo_key)
                    .bind(number)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }

        Ok(prs)
    }

    /// Cache the conversation timeline for one pull request.
    pub async fn save_conversation(
        &self,
        repo: &RepoId,
        pr: PrNumber,
        conversation: &Conversation,
    ) -> Result<(), DbError> {
        let payload = serde_json::to_string(conversation).map_err(|source| DbError::Serde {
            kind: "conversation",
            source,
        })?;

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO cache_conversation (repo, number, payload, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(repo, number) DO UPDATE SET
                 payload    = excluded.payload,
                 updated_at = excluded.updated_at",
        )
        .bind(repo.to_string())
        .bind(i64::from(pr.0))
        .bind(&payload)
        .bind(encode_time(Utc::now()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The cached conversation, or `None` on a miss. An undecodable row is a
    /// miss: it is logged, deleted, and reported as absent.
    pub async fn load_conversation(
        &self,
        repo: &RepoId,
        pr: PrNumber,
    ) -> Result<Option<Conversation>, DbError> {
        let repo_key = repo.to_string();
        let row =
            sqlx::query("SELECT payload FROM cache_conversation WHERE repo = ? AND number = ?")
                .bind(&repo_key)
                .bind(i64::from(pr.0))
                .fetch_optional(&self.pool)
                .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        let payload: String = row.try_get("payload")?;

        match serde_json::from_str::<Conversation>(&payload) {
            Ok(conversation) => Ok(Some(conversation)),
            Err(error) => {
                warn!(
                    repo = %repo_key,
                    number = pr.0,
                    %error,
                    "discarding undecodable cached conversation"
                );
                sqlx::query("DELETE FROM cache_conversation WHERE repo = ? AND number = ?")
                    .bind(&repo_key)
                    .bind(i64::from(pr.0))
                    .execute(&self.pool)
                    .await?;
                Ok(None)
            }
        }
    }

    /// The stored ETag for `url` and the body it was served with.
    pub async fn load_etag(&self, url: &str) -> Result<Option<CachedResponse>, DbError> {
        let row = sqlx::query("SELECT etag, body FROM cache_http WHERE url = ?")
            .bind(url)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => Ok(Some(CachedResponse {
                etag: row.try_get("etag")?,
                body: row.try_get("body")?,
            })),
            None => Ok(None),
        }
    }

    /// Record the ETag and body of a response, replacing any previous entry.
    pub async fn save_etag(&self, url: &str, etag: &str, body: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO cache_http (url, etag, body, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(url) DO UPDATE SET
                 etag       = excluded.etag,
                 body       = excluded.body,
                 updated_at = excluded.updated_at",
        )
        .bind(url)
        .bind(etag)
        .bind(body)
        .bind(encode_time(Utc::now()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
