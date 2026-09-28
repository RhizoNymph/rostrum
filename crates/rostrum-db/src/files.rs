//! A pull request's changed files, cached per head commit.
//!
//! A diff is a pure function of its head commit, so the head sha is a
//! stronger validator than an HTTP ETag: while the head has not moved the
//! diff cannot have changed, and no request is needed at all. The entry
//! lives in `cache_http` as a validator/payload pair — which is what a
//! conditional cache entry is, whoever issued the validator.

use rostrum_core::{PrNumber, RepoId};
use rostrum_github::PullRequestFile;

use crate::{Db, error::DbError};

/// The `cache_http` key a pull request's files are stored under.
fn files_key(repo: &RepoId, number: PrNumber) -> String {
    format!("files:{repo}{number}")
}

impl Db {
    /// Cache the changed files of `repo`/`number` as of `head_sha`, replacing
    /// whatever was stored for an older head. A blank sha is not a validator,
    /// so nothing is stored.
    pub async fn save_pull_request_files(
        &self,
        repo: &RepoId,
        number: PrNumber,
        head_sha: &str,
        files: &[PullRequestFile],
    ) -> Result<(), DbError> {
        if head_sha.is_empty() {
            return Ok(());
        }
        let body = serde_json::to_string(files).map_err(|source| DbError::Serde {
            kind: "pull request files",
            source,
        })?;
        self.save_etag(&files_key(repo, number), head_sha, &body)
            .await
    }

    /// The cached files of `repo`/`number`, but only if they were stored for
    /// exactly `head_sha`. A different head, a blank sha, or a row that no
    /// longer decodes is a miss.
    pub async fn load_pull_request_files(
        &self,
        repo: &RepoId,
        number: PrNumber,
        head_sha: &str,
    ) -> Result<Option<Vec<PullRequestFile>>, DbError> {
        if head_sha.is_empty() {
            return Ok(None);
        }
        let Some(cached) = self.load_etag(&files_key(repo, number)).await? else {
            return Ok(None);
        };
        if cached.etag != head_sha {
            return Ok(None);
        }
        match serde_json::from_str(&cached.body) {
            Ok(files) => Ok(Some(files)),
            Err(error) => {
                tracing::warn!(%repo, number = number.0, %error, "discarding undecodable cached files");
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PullRequestFile {
        PullRequestFile {
            filename: name.into(),
            previous_filename: None,
            status: "modified".into(),
            additions: 1,
            deletions: 2,
            patch: Some("@@ -1 +1 @@\n-a\n+b".into()),
        }
    }

    fn repo() -> RepoId {
        RepoId::new("rhizonymph", "rostrum")
    }

    /// The key is the one the desktop has always written, so a cache written
    /// by either reads back in the other.
    #[test]
    fn the_key_matches_the_desktop_format() {
        assert_eq!(files_key(&repo(), PrNumber(12)), "files:rhizonymph/rostrum#12");
    }

    #[tokio::test]
    async fn files_round_trip_for_the_same_head() {
        let db = Db::open_in_memory().await.expect("db");
        let files = vec![file("a.rs"), file("b.rs")];
        db.save_pull_request_files(&repo(), PrNumber(1), "abc", &files)
            .await
            .expect("save");
        let loaded = db
            .load_pull_request_files(&repo(), PrNumber(1), "abc")
            .await
            .expect("load");
        assert_eq!(loaded, Some(files));
    }

    #[tokio::test]
    async fn a_moved_head_is_a_miss() {
        let db = Db::open_in_memory().await.expect("db");
        db.save_pull_request_files(&repo(), PrNumber(1), "abc", &[file("a.rs")])
            .await
            .expect("save");
        let loaded = db
            .load_pull_request_files(&repo(), PrNumber(1), "def")
            .await
            .expect("load");
        assert_eq!(loaded, None);
        // And another pull request's entry is a different key entirely.
        let other = db
            .load_pull_request_files(&repo(), PrNumber(2), "abc")
            .await
            .expect("load");
        assert_eq!(other, None);
    }

    #[tokio::test]
    async fn a_blank_head_neither_stores_nor_matches() {
        let db = Db::open_in_memory().await.expect("db");
        db.save_pull_request_files(&repo(), PrNumber(1), "", &[file("a.rs")])
            .await
            .expect("save");
        assert_eq!(
            db.load_etag(&files_key(&repo(), PrNumber(1)))
                .await
                .expect("load"),
            None
        );
        assert_eq!(
            db.load_pull_request_files(&repo(), PrNumber(1), "")
                .await
                .expect("load"),
            None
        );
    }

    #[tokio::test]
    async fn an_undecodable_entry_is_a_miss() {
        let db = Db::open_in_memory().await.expect("db");
        db.save_etag(&files_key(&repo(), PrNumber(1)), "abc", "not json")
            .await
            .expect("save");
        assert_eq!(
            db.load_pull_request_files(&repo(), PrNumber(1), "abc")
                .await
                .expect("load"),
            None
        );
    }
}
