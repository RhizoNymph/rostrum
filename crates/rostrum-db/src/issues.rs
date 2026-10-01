//! Cached issues: each repository's open list, and the detail of any issue
//! that was opened.
//!
//! Cache, not user data, with the same rules as the pull request tables: a
//! row that no longer decodes is logged, deleted, and reported as a miss.
//! Kept in tables of their own rather than beside pull requests because the
//! payloads are different types; a shared table would make "which type is
//! this row?" a question every reader had to get right.

use chrono::Utc;
use rostrum_core::{Issue, IssueDetail, IssueNumber, RepoId};
use sqlx::Row;
use tracing::warn;

use crate::{Db, error::DbError, types::encode_time};

impl Db {
    /// Replace the cached open issues for `repo` in one transaction.
    ///
    /// Issues absent from `issues` are removed, so an empty slice clears the
    /// repository — which is what a repository whose last issue closed should
    /// paint on the next cold start.
    pub async fn save_issues(&self, repo: &RepoId, issues: &[Issue]) -> Result<(), DbError> {
        let repo_key = repo.to_string();
        let now = encode_time(Utc::now());

        // Encode before the transaction opens, so a bad value cannot leave a
        // half-written list behind.
        let mut encoded = Vec::with_capacity(issues.len());
        for issue in issues {
            let payload = serde_json::to_string(issue).map_err(|source| DbError::Serde {
                kind: "issue",
                source,
            })?;
            encoded.push((issue.number, payload));
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM cache_issue WHERE repo = ?")
            .bind(&repo_key)
            .execute(&mut *tx)
            .await?;
        for (ordinal, (number, payload)) in encoded.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO cache_issue (repo, number, ordinal, payload, updated_at)
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

    /// Cached open issues for `repo`, in the order they were saved.
    pub async fn load_issues(&self, repo: &RepoId) -> Result<Vec<Issue>, DbError> {
        let repo_key = repo.to_string();
        let rows =
            sqlx::query("SELECT number, payload FROM cache_issue WHERE repo = ? ORDER BY ordinal")
                .bind(&repo_key)
                .fetch_all(&self.pool)
                .await?;

        let mut issues = Vec::with_capacity(rows.len());
        let mut corrupt = Vec::new();
        for row in rows {
            let number: i64 = row.try_get("number")?;
            let payload: String = row.try_get("payload")?;
            match serde_json::from_str::<Issue>(&payload) {
                Ok(issue) => issues.push(issue),
                Err(error) => {
                    warn!(repo = %repo_key, number, %error, "discarding undecodable cached issue");
                    corrupt.push(number);
                }
            }
        }

        if !corrupt.is_empty() {
            let mut tx = self.pool.begin().await?;
            for number in corrupt {
                sqlx::query("DELETE FROM cache_issue WHERE repo = ? AND number = ?")
                    .bind(&repo_key)
                    .bind(number)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        }

        Ok(issues)
    }

    /// Cache the detail of one issue.
    pub async fn save_issue_detail(
        &self,
        repo: &RepoId,
        detail: &IssueDetail,
    ) -> Result<(), DbError> {
        let payload = serde_json::to_string(detail).map_err(|source| DbError::Serde {
            kind: "issue detail",
            source,
        })?;
        sqlx::query(
            "INSERT INTO cache_issue_detail (repo, number, payload, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(repo, number) DO UPDATE SET
                 payload    = excluded.payload,
                 updated_at = excluded.updated_at",
        )
        .bind(repo.to_string())
        .bind(i64::from(detail.issue.number.0))
        .bind(&payload)
        .bind(encode_time(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The cached detail of one issue, or `None` on a miss. An undecodable
    /// row is a miss: logged, deleted, and reported absent.
    pub async fn load_issue_detail(
        &self,
        repo: &RepoId,
        number: IssueNumber,
    ) -> Result<Option<IssueDetail>, DbError> {
        let repo_key = repo.to_string();
        let row =
            sqlx::query("SELECT payload FROM cache_issue_detail WHERE repo = ? AND number = ?")
                .bind(&repo_key)
                .bind(i64::from(number.0))
                .fetch_optional(&self.pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let payload: String = row.try_get("payload")?;

        match serde_json::from_str::<IssueDetail>(&payload) {
            Ok(detail) => Ok(Some(detail)),
            Err(error) => {
                warn!(
                    repo = %repo_key,
                    number = number.0,
                    %error,
                    "discarding undecodable cached issue detail"
                );
                sqlx::query("DELETE FROM cache_issue_detail WHERE repo = ? AND number = ?")
                    .bind(&repo_key)
                    .bind(i64::from(number.0))
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
    use rostrum_core::{
        CloseReason, Conversation, EventKind, IssueState, Label, Milestone, NodeId, TimelineItem,
        User,
    };

    use super::*;

    fn repo(name: &str) -> RepoId {
        RepoId::new("rhizonymph", name)
    }

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid timestamp")
    }

    fn user(login: &str) -> User {
        User {
            login: login.into(),
            avatar_url: Some(format!("https://avatars.example/{login}")),
        }
    }

    /// Every field populated, so a field that fails to round-trip is caught.
    fn issue(number: u32) -> Issue {
        Issue {
            number: IssueNumber(number),
            node_id: NodeId(format!("I_{number}")),
            title: format!("Issue {number}"),
            url: format!("https://github.com/rhizonymph/rostrum/issues/{number}"),
            state: IssueState::Open,
            created_at: at(1_700_000_000),
            updated_at: at(1_700_000_000 + i64::from(number)),
            author: Some(user("author")),
            assignees: vec![user("assignee")],
            labels: vec![Label {
                name: "C-bug".into(),
                color: "d73a4a".into(),
            }],
            comment_count: 3,
            milestone: Some(Milestone {
                title: "1.0".into(),
            }),
        }
    }

    fn detail(number: u32) -> IssueDetail {
        let mut conversation = Conversation {
            items: vec![
                TimelineItem::Body {
                    author: Some(user("author")),
                    body: "It crashes.".into(),
                    created_at: at(1_700_000_000),
                },
                TimelineItem::Event {
                    kind: EventKind::ClosedAs(CloseReason::NotPlanned),
                    actor: Some(user("maintainer")),
                    created_at: at(1_700_000_100),
                },
                TimelineItem::Event {
                    kind: EventKind::CrossReferenced {
                        source: "a/b#9".into(),
                        title: "Fix it".into(),
                    },
                    actor: None,
                    created_at: at(1_700_000_050),
                },
            ],
            ..Default::default()
        };
        conversation.sort();
        IssueDetail {
            issue: Issue {
                state: IssueState::Closed(Some(CloseReason::NotPlanned)),
                ..issue(number)
            },
            conversation,
        }
    }

    async fn db() -> Db {
        Db::open_in_memory()
            .await
            .expect("in-memory database opens")
    }

    #[tokio::test]
    async fn issues_round_trip_in_their_saved_order() {
        let db = db().await;
        let issues = vec![issue(9), issue(2), issue(5)];
        db.save_issues(&repo("rostrum"), &issues)
            .await
            .expect("save");
        assert_eq!(
            db.load_issues(&repo("rostrum")).await.expect("load"),
            issues
        );
    }

    #[tokio::test]
    async fn saving_issues_replaces_rather_than_appends() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_issues(&repo, &[issue(1), issue(2)])
            .await
            .expect("save");
        db.save_issues(&repo, &[issue(3)]).await.expect("save");
        assert_eq!(db.load_issues(&repo).await.expect("load"), vec![issue(3)]);

        db.save_issues(&repo, &[]).await.expect("clear");
        assert!(db.load_issues(&repo).await.expect("load").is_empty());
    }

    #[tokio::test]
    async fn issues_are_scoped_per_repo_and_apart_from_pull_requests() {
        let db = db().await;
        db.save_issues(&repo("a"), &[issue(1)]).await.expect("save");
        db.save_issues(&repo("b"), &[issue(2)]).await.expect("save");
        assert_eq!(
            db.load_issues(&repo("a")).await.expect("load"),
            vec![issue(1)]
        );
        assert_eq!(
            db.load_issues(&repo("b")).await.expect("load"),
            vec![issue(2)]
        );
        assert!(
            db.load_pull_requests(&repo("a"))
                .await
                .expect("load")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn an_issue_detail_round_trips_with_its_state_and_events() {
        let db = db().await;
        let repo = repo("rostrum");
        assert_eq!(
            db.load_issue_detail(&repo, IssueNumber(4))
                .await
                .expect("load"),
            None
        );

        db.save_issue_detail(&repo, &detail(4)).await.expect("save");
        assert_eq!(
            db.load_issue_detail(&repo, IssueNumber(4))
                .await
                .expect("load"),
            Some(detail(4))
        );

        // A second save overwrites.
        let mut reopened = detail(4);
        reopened.issue.state = IssueState::Open;
        db.save_issue_detail(&repo, &reopened).await.expect("save");
        assert_eq!(
            db.load_issue_detail(&repo, IssueNumber(4))
                .await
                .expect("load")
                .map(|d| d.issue.state),
            Some(IssueState::Open)
        );
    }

    #[tokio::test]
    async fn corrupt_issue_rows_are_misses_and_are_dropped() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_issues(&repo, &[issue(1), issue(2)])
            .await
            .expect("save");
        db.save_issue_detail(&repo, &detail(1)).await.expect("save");
        sqlx::query("UPDATE cache_issue SET payload = '{' WHERE number = 1")
            .execute(db.pool())
            .await
            .expect("corrupt");
        sqlx::query("UPDATE cache_issue_detail SET payload = 'nope'")
            .execute(db.pool())
            .await
            .expect("corrupt");

        assert_eq!(db.load_issues(&repo).await.expect("load"), vec![issue(2)]);
        assert_eq!(
            db.load_issue_detail(&repo, IssueNumber(1))
                .await
                .expect("load"),
            None
        );
        // Both corrupt rows are gone, so the next read is a clean miss.
        let remaining: i64 = sqlx::query("SELECT COUNT(*) AS n FROM cache_issue_detail")
            .fetch_one(db.pool())
            .await
            .expect("count")
            .try_get("n")
            .expect("n");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn pruning_reaches_the_issue_tables() {
        let db = db().await;
        let repo = repo("rostrum");
        db.save_issues(&repo, &[issue(1)]).await.expect("save");
        db.save_issue_detail(&repo, &detail(1)).await.expect("save");
        let stale = encode_time(Utc::now() - Duration::days(30));
        for table in ["cache_issue", "cache_issue_detail"] {
            sqlx::query(&format!("UPDATE {table} SET updated_at = ?"))
                .bind(&stale)
                .execute(db.pool())
                .await
                .expect("backdate");
        }
        assert_eq!(db.prune_cache(Duration::days(1)).await.expect("prune"), 2);
        assert!(db.load_issues(&repo).await.expect("load").is_empty());
    }
}
