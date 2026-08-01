//! Behavioural tests for the store.
//!
//! Almost everything runs against `open_in_memory()`. The two tests that must
//! survive a real close-and-reopen cycle (schema versioning, directory
//! creation) use a temporary file instead.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use rostrum_core::{
    CheckRun, CheckState, CommentId, Conversation, EventKind, Label, MergeStateStatus, Mergeable,
    PrNumber, PullRequest, RepoId, ReviewDecision, ReviewThread, Side, ThreadComment, ThreadId,
    TimelineItem, User,
};
use rostrum_github::DraftComment;

use crate::{Db, DbError, schema};

// --- fixtures ---------------------------------------------------------------

fn repo(name: &str) -> RepoId {
    RepoId::new("rhizonymph", name)
}

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).expect("valid timestamp")
}

fn pr(number: u32) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        title: format!("pull request {number}"),
        url: format!("https://github.com/rhizonymph/rostrum/pull/{number}"),
        is_draft: number.is_multiple_of(2),
        created_at: at(1_700_000_000),
        updated_at: at(1_700_000_500),
        author: Some(User {
            login: "octocat".into(),
            avatar_url: Some("https://example.invalid/a.png".into()),
        }),
        head_ref: "feature".into(),
        head_sha: format!("{number:040x}"),
        base_ref: "main".into(),
        additions: 12,
        deletions: 3,
        changed_files: 2,
        mergeable: Mergeable::Mergeable,
        merge_state: MergeStateStatus::Unknown,
        review_decision: Some(ReviewDecision::Approved),
        labels: vec![Label {
            name: "bug".into(),
            color: "d73a4a".into(),
        }],
        comment_count: 4,
        checks: Some(CheckState::Success),
    }
}

fn conversation() -> Conversation {
    Conversation {
        items: vec![
            TimelineItem::Body {
                author: Some(User {
                    login: "octocat".into(),
                    avatar_url: None,
                }),
                body: "the description".into(),
                created_at: at(1_700_000_000),
            },
            TimelineItem::Comment {
                id: CommentId("c1".into()),
                author: None,
                body: "looks good".into(),
                created_at: at(1_700_000_100),
            },
            TimelineItem::Event {
                kind: EventKind::Labeled { name: "bug".into() },
                actor: None,
                created_at: at(1_700_000_200),
            },
        ],
        threads: vec![ReviewThread {
            id: ThreadId("t1".into()),
            path: "src/lib.rs".into(),
            line: Some(10),
            original_line: Some(8),
            side: Side::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![ThreadComment {
                id: CommentId("tc1".into()),
                database_id: Some(99),
                author: None,
                body: "nit".into(),
                created_at: at(1_700_000_300),
            }],
        }],
        checks: vec![CheckRun {
            name: "ci".into(),
            state: Some(CheckState::Success),
            url: None,
        }],
    }
}

fn single_draft() -> DraftComment {
    DraftComment::single("src/lib.rs", 42, Side::Right, "this needs a test")
}

fn multi_line_draft() -> DraftComment {
    DraftComment {
        path: "src/db.rs".into(),
        line: 20,
        side: Side::Right,
        start_line: Some(14),
        start_side: Some(Side::Left),
        body: "this whole block".into(),
    }
}

// --- helpers ----------------------------------------------------------------

impl Db {
    /// Backdate every cache row so `prune_cache` sees it as stale.
    async fn backdate_cache(&self, at: DateTime<Utc>) {
        let stamp = crate::types::encode_time(at);
        for table in schema::CACHE_TABLES {
            sqlx::query(&format!("UPDATE {table} SET updated_at = ?"))
                .bind(&stamp)
                .execute(self.pool())
                .await
                .expect("backdating cache rows");
        }
    }

    /// Backdate every draft row, to prove pruning still leaves them alone.
    async fn backdate_drafts(&self, at: DateTime<Utc>) {
        sqlx::query("UPDATE drafts SET updated_at = ?")
            .bind(crate::types::encode_time(at))
            .execute(self.pool())
            .await
            .expect("backdating draft rows");
    }

    async fn count(&self, table: &str) -> i64 {
        use sqlx::Row;
        sqlx::query(&format!("SELECT COUNT(*) AS n FROM {table}"))
            .fetch_one(self.pool())
            .await
            .expect("counting rows")
            .try_get("n")
            .expect("count column")
    }

    async fn write_garbage(&self, sql: &str) {
        sqlx::query(sql)
            .execute(self.pool())
            .await
            .expect("planting corrupt row");
    }
}

/// A uniquely named directory under `$TMPDIR`, removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!("rostrum-db-{tag}-{unique}")),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

async fn db() -> Db {
    Db::open_in_memory()
        .await
        .expect("in-memory database opens")
}

// --- cache: pull requests ---------------------------------------------------

#[tokio::test]
async fn pull_requests_round_trip() {
    let db = db().await;
    let repo = repo("rostrum");
    let prs = vec![pr(1), pr(2), pr(3)];

    db.save_pull_requests(&repo, &prs).await.expect("save");
    let loaded = db.load_pull_requests(&repo).await.expect("load");

    assert_eq!(loaded, prs);
}

#[tokio::test]
async fn pull_requests_keep_their_saved_order() {
    let db = db().await;
    let repo = repo("rostrum");
    let prs = vec![pr(9), pr(2), pr(31), pr(4)];

    db.save_pull_requests(&repo, &prs).await.expect("save");
    let numbers: Vec<_> = db
        .load_pull_requests(&repo)
        .await
        .expect("load")
        .into_iter()
        .map(|pr| pr.number.0)
        .collect();

    assert_eq!(numbers, [9, 2, 31, 4]);
}

#[tokio::test]
async fn loading_a_repo_that_was_never_saved_yields_nothing() {
    let db = db().await;
    assert!(
        db.load_pull_requests(&repo("never-seen"))
            .await
            .expect("load")
            .is_empty()
    );
}

#[tokio::test]
async fn saving_pull_requests_replaces_rather_than_appends() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1), pr(2), pr(3)])
        .await
        .expect("first save");
    db.save_pull_requests(&repo, &[pr(4)])
        .await
        .expect("second save");

    let loaded = db.load_pull_requests(&repo).await.expect("load");
    assert_eq!(loaded, vec![pr(4)]);
}

#[tokio::test]
async fn saving_an_empty_list_clears_the_repo() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1)]).await.expect("save");
    db.save_pull_requests(&repo, &[]).await.expect("clear");

    assert!(db.load_pull_requests(&repo).await.expect("load").is_empty());
}

#[tokio::test]
async fn pull_requests_are_scoped_per_repo() {
    let db = db().await;
    let (a, b) = (repo("alpha"), repo("beta"));

    db.save_pull_requests(&a, &[pr(1), pr(2)])
        .await
        .expect("save a");
    db.save_pull_requests(&b, &[pr(7)]).await.expect("save b");
    db.save_pull_requests(&a, &[]).await.expect("clear a");

    assert!(db.load_pull_requests(&a).await.expect("load a").is_empty());
    assert_eq!(
        db.load_pull_requests(&b).await.expect("load b"),
        vec![pr(7)]
    );
}

// --- cache: conversations ---------------------------------------------------

#[tokio::test]
async fn conversation_round_trips() {
    let db = db().await;
    let repo = repo("rostrum");
    let conversation = conversation();

    db.save_conversation(&repo, PrNumber(1), &conversation)
        .await
        .expect("save");
    let loaded = db
        .load_conversation(&repo, PrNumber(1))
        .await
        .expect("load");

    assert_eq!(loaded, Some(conversation));
}

#[tokio::test]
async fn missing_conversation_is_none() {
    let db = db().await;
    assert_eq!(
        db.load_conversation(&repo("rostrum"), PrNumber(404))
            .await
            .expect("load"),
        None
    );
}

#[tokio::test]
async fn saving_a_conversation_twice_overwrites_it() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_conversation(&repo, PrNumber(1), &conversation())
        .await
        .expect("first save");
    db.save_conversation(&repo, PrNumber(1), &Conversation::default())
        .await
        .expect("second save");

    assert_eq!(
        db.load_conversation(&repo, PrNumber(1))
            .await
            .expect("load"),
        Some(Conversation::default())
    );
    assert_eq!(db.count("cache_conversation").await, 1);
}

#[tokio::test]
async fn conversations_are_keyed_by_repo_and_number() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_conversation(&repo, PrNumber(1), &conversation())
        .await
        .expect("save");

    assert!(
        db.load_conversation(&repo, PrNumber(2))
            .await
            .expect("load")
            .is_none()
    );
    assert!(
        db.load_conversation(&RepoId::new("rhizonymph", "other"), PrNumber(1))
            .await
            .expect("load")
            .is_none()
    );
}

// --- cache: etags -----------------------------------------------------------

#[tokio::test]
async fn etag_round_trips_with_its_body() {
    let db = db().await;
    let url = "https://api.github.com/repos/a/b/pulls";

    db.save_etag(url, "W/\"abc\"", "[]").await.expect("save");
    let cached = db.load_etag(url).await.expect("load").expect("present");

    assert_eq!(cached.etag, "W/\"abc\"");
    assert_eq!(cached.body, "[]");
}

#[tokio::test]
async fn saving_an_etag_again_overwrites_it() {
    let db = db().await;
    let url = "https://api.github.com/repos/a/b/pulls";

    db.save_etag(url, "one", "first").await.expect("first");
    db.save_etag(url, "two", "second").await.expect("second");

    let cached = db.load_etag(url).await.expect("load").expect("present");
    assert_eq!(cached.etag, "two");
    assert_eq!(cached.body, "second");
    assert_eq!(db.count("cache_http").await, 1);
}

#[tokio::test]
async fn unknown_url_has_no_etag() {
    let db = db().await;
    assert_eq!(
        db.load_etag("https://api.github.com/nope")
            .await
            .expect("load"),
        None
    );
}

// --- drafts -----------------------------------------------------------------

#[tokio::test]
async fn drafts_round_trip_preserving_head_sha() {
    let db = db().await;
    let repo = repo("rostrum");
    let drafts = vec![single_draft(), multi_line_draft()];

    db.save_drafts(&repo, PrNumber(7), "deadbeef", &drafts)
        .await
        .expect("save");
    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("present");

    assert_eq!(set.head_sha, "deadbeef");
    assert_eq!(set.comments, drafts);
}

#[tokio::test]
async fn multi_line_draft_anchors_survive_storage() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[multi_line_draft()])
        .await
        .expect("save");
    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("present");

    let comment = set.comments.first().expect("one comment");
    assert!(comment.is_multi_line());
    assert_eq!(comment.start_line, Some(14));
    assert_eq!(comment.start_side, Some(Side::Left));
    assert_eq!(comment.line, 20);
    assert_eq!(comment.side, Side::Right);
}

#[tokio::test]
async fn single_line_draft_has_no_range() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save");
    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("present");

    let comment = set.comments.first().expect("one comment");
    assert!(!comment.is_multi_line());
    assert_eq!(comment.start_line, None);
    assert_eq!(comment.start_side, None);
}

#[tokio::test]
async fn drafts_record_when_they_were_written() {
    let db = db().await;
    let repo = repo("rostrum");
    let before = Utc::now();

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save");
    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("present");

    assert!(set.updated_at >= before - Duration::seconds(1));
    assert!(set.updated_at <= Utc::now() + Duration::seconds(1));
}

#[tokio::test]
async fn missing_drafts_are_none() {
    let db = db().await;
    assert!(
        db.load_drafts(&repo("rostrum"), PrNumber(1))
            .await
            .expect("load")
            .is_none()
    );
}

#[tokio::test]
async fn saving_drafts_replaces_the_previous_set() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(
        &repo,
        PrNumber(7),
        "old-sha",
        &[single_draft(), multi_line_draft()],
    )
    .await
    .expect("first save");
    db.save_drafts(&repo, PrNumber(7), "new-sha", &[single_draft()])
        .await
        .expect("second save");

    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("present");
    assert_eq!(set.head_sha, "new-sha");
    assert_eq!(set.comments, vec![single_draft()]);
    assert_eq!(db.count("drafts").await, 1);
}

#[tokio::test]
async fn an_empty_draft_set_is_stored_not_removed() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save");
    db.save_drafts(&repo, PrNumber(7), "sha", &[])
        .await
        .expect("save empty");

    let set = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect("load")
        .expect("row still present");
    assert!(set.comments.is_empty());
}

#[tokio::test]
async fn clearing_drafts_removes_only_that_pull_request() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save 7");
    db.save_drafts(&repo, PrNumber(8), "sha", &[single_draft()])
        .await
        .expect("save 8");
    db.clear_drafts(&repo, PrNumber(7)).await.expect("clear");

    assert!(
        db.load_drafts(&repo, PrNumber(7))
            .await
            .expect("load 7")
            .is_none()
    );
    assert!(
        db.load_drafts(&repo, PrNumber(8))
            .await
            .expect("load 8")
            .is_some()
    );
}

#[tokio::test]
async fn clearing_absent_drafts_is_not_an_error() {
    let db = db().await;
    db.clear_drafts(&repo("rostrum"), PrNumber(1))
        .await
        .expect("clearing nothing succeeds");
}

// --- pruning ----------------------------------------------------------------

#[tokio::test]
async fn prune_cache_deletes_stale_rows_and_never_touches_drafts() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1), pr(2)])
        .await
        .expect("save prs");
    db.save_conversation(&repo, PrNumber(1), &conversation())
        .await
        .expect("save conversation");
    db.save_etag("https://api.github.com/x", "e", "b")
        .await
        .expect("save etag");
    db.save_drafts(&repo, PrNumber(1), "sha", &[single_draft()])
        .await
        .expect("save drafts");

    // Age everything, drafts included, so nothing is spared by luck.
    let long_ago = Utc::now() - Duration::days(30);
    db.backdate_cache(long_ago).await;
    db.backdate_drafts(long_ago).await;

    let deleted = db.prune_cache(Duration::days(1)).await.expect("prune");

    assert_eq!(deleted, 4, "two pull requests, one conversation, one etag");
    assert!(db.load_pull_requests(&repo).await.expect("load").is_empty());
    assert_eq!(
        db.load_conversation(&repo, PrNumber(1))
            .await
            .expect("load"),
        None
    );
    assert_eq!(
        db.load_etag("https://api.github.com/x")
            .await
            .expect("load"),
        None
    );

    // The whole point: an equally old draft is still here.
    let set = db
        .load_drafts(&repo, PrNumber(1))
        .await
        .expect("load drafts")
        .expect("drafts survive pruning");
    assert_eq!(set.comments, vec![single_draft()]);
    assert_eq!(db.count("drafts").await, 1);
}

#[tokio::test]
async fn prune_cache_keeps_fresh_rows() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1)]).await.expect("save");
    db.save_etag("https://api.github.com/x", "e", "b")
        .await
        .expect("save etag");

    let deleted = db.prune_cache(Duration::days(1)).await.expect("prune");

    assert_eq!(deleted, 0);
    assert_eq!(db.load_pull_requests(&repo).await.expect("load").len(), 1);
}

#[tokio::test]
async fn prune_cache_on_an_empty_database_is_a_no_op() {
    let db = db().await;
    assert_eq!(db.prune_cache(Duration::zero()).await.expect("prune"), 0);
}

// --- corruption -------------------------------------------------------------

#[tokio::test]
async fn corrupt_cached_pull_request_is_a_miss_and_the_row_is_dropped() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1), pr(2)])
        .await
        .expect("save");
    db.write_garbage("UPDATE cache_pull_request SET payload = '{ not json' WHERE number = 1")
        .await;

    let loaded = db.load_pull_requests(&repo).await.expect("no error");

    assert_eq!(loaded, vec![pr(2)], "the good row still decodes");
    assert_eq!(
        db.count("cache_pull_request").await,
        1,
        "the bad row was deleted"
    );
}

#[tokio::test]
async fn a_wholly_corrupt_pull_request_cache_reads_as_empty() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_pull_requests(&repo, &[pr(1)]).await.expect("save");
    db.write_garbage("UPDATE cache_pull_request SET payload = '\"a string\"'")
        .await;

    assert!(
        db.load_pull_requests(&repo)
            .await
            .expect("no error")
            .is_empty()
    );
    assert_eq!(db.count("cache_pull_request").await, 0);
}

#[tokio::test]
async fn corrupt_cached_conversation_is_a_miss_and_the_row_is_dropped() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_conversation(&repo, PrNumber(1), &conversation())
        .await
        .expect("save");
    db.write_garbage("UPDATE cache_conversation SET payload = 'nonsense'")
        .await;

    assert_eq!(
        db.load_conversation(&repo, PrNumber(1))
            .await
            .expect("no error"),
        None
    );
    assert_eq!(db.count("cache_conversation").await, 0);
}

#[tokio::test]
async fn corrupt_drafts_are_an_error_and_are_left_in_place() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save");
    db.write_garbage("UPDATE drafts SET comments = '{ not json'")
        .await;

    let error = db
        .load_drafts(&repo, PrNumber(7))
        .await
        .expect_err("corrupt drafts must not be silently discarded");

    match error {
        DbError::CorruptDraft { repo: r, pr, .. } => {
            assert_eq!(r, "rhizonymph/rostrum");
            assert_eq!(pr, 7);
        }
        other => panic!("expected CorruptDraft, got {other:?}"),
    }
    assert_eq!(db.count("drafts").await, 1, "the row was not destroyed");
}

#[tokio::test]
async fn a_corrupt_draft_timestamp_is_an_error() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(7), "sha", &[single_draft()])
        .await
        .expect("save");
    db.write_garbage("UPDATE drafts SET updated_at = 'yesterday'")
        .await;

    assert!(matches!(
        db.load_drafts(&repo, PrNumber(7)).await,
        Err(DbError::Timestamp { .. })
    ));
}

// --- schema versioning ------------------------------------------------------

#[tokio::test]
async fn a_cache_schema_bump_wipes_the_cache_and_preserves_drafts() {
    let dir = TempDir::new("schema-bump");
    let path = dir.path().join("nested/store.db");
    let repo = repo("rostrum");

    {
        let db = Db::open(&path).await.expect("open");
        db.save_pull_requests(&repo, &[pr(1), pr(2)])
            .await
            .expect("save prs");
        db.save_conversation(&repo, PrNumber(1), &conversation())
            .await
            .expect("save conversation");
        db.save_etag("https://api.github.com/x", "e", "b")
            .await
            .expect("save etag");
        db.save_drafts(&repo, PrNumber(1), "head-sha", &[single_draft()])
            .await
            .expect("save drafts");

        // Pretend the code that wrote this file predates the current schema.
        sqlx::query("UPDATE meta SET value = 'ancient' WHERE key = ?")
            .bind(schema::CACHE_SCHEMA_VERSION_KEY)
            .execute(db.pool())
            .await
            .expect("forge cache version");
        db.close().await;
    }

    let db = Db::open(&path).await.expect("reopen");

    assert!(
        db.load_pull_requests(&repo).await.expect("load").is_empty(),
        "cached pull requests are discarded"
    );
    assert_eq!(
        db.load_conversation(&repo, PrNumber(1))
            .await
            .expect("load"),
        None,
        "cached conversations are discarded"
    );
    assert_eq!(
        db.load_etag("https://api.github.com/x")
            .await
            .expect("load"),
        None,
        "cached etags are discarded"
    );

    // The whole point: unsent work survived the schema change.
    let set = db
        .load_drafts(&repo, PrNumber(1))
        .await
        .expect("load drafts")
        .expect("drafts survive a cache schema bump");
    assert_eq!(set.head_sha, "head-sha");
    assert_eq!(set.comments, vec![single_draft()]);

    db.close().await;
}

#[tokio::test]
async fn reopening_at_the_same_version_keeps_the_cache() {
    let dir = TempDir::new("same-version");
    let path = dir.path().join("store.db");
    let repo = repo("rostrum");

    {
        let db = Db::open(&path).await.expect("open");
        db.save_pull_requests(&repo, &[pr(1)]).await.expect("save");
        db.close().await;
    }

    let db = Db::open(&path).await.expect("reopen");
    assert_eq!(
        db.load_pull_requests(&repo).await.expect("load"),
        vec![pr(1)]
    );
    db.close().await;
}

#[tokio::test]
async fn open_creates_missing_parent_directories() {
    let dir = TempDir::new("mkdir");
    let path = dir.path().join("a/b/c/store.db");

    let db = Db::open(&path).await.expect("open creates the tree");
    assert!(path.exists());
    db.close().await;
}

#[tokio::test]
async fn migrating_twice_over_one_database_is_idempotent() {
    let db = db().await;
    let repo = repo("rostrum");

    db.save_drafts(&repo, PrNumber(1), "sha", &[single_draft()])
        .await
        .expect("save drafts");
    db.save_pull_requests(&repo, &[pr(1)]).await.expect("save");

    schema::migrate(db.pool()).await.expect("re-migrate");

    assert_eq!(
        db.load_pull_requests(&repo).await.expect("load"),
        vec![pr(1)]
    );
    assert!(
        db.load_drafts(&repo, PrNumber(1))
            .await
            .expect("load")
            .is_some()
    );
}
