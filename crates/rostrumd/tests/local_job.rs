//! Real local jobs over the real API: a scratch origin, a clone of it and a
//! worktree, driven from `RemoteClient` through the daemon into
//! `rostrum-local` and git.

mod common;

use std::{path::PathBuf, time::Duration};

use common::{Harness, git};
use rostrum_core::{PrNumber, RepoId};
use rostrum_remote::{
    AbortRequest, ApiErrorCode, JobOutcome, JobRequest, LocalOpKind, LocalStatus,
    LocalStatusRequest, PrKey, PrRef, SyncAllRequest, SyncEntryState,
    client::{ClientError, RemoteClient},
};
use rostrumd::fsutil::ScratchDir;

/// An origin, a clone of it, and `feature` checked out in its own worktree.
struct Repos {
    scratch: ScratchDir,
}

impl Repos {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let root = scratch.path().to_path_buf();
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).expect("seed");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        std::fs::write(seed.join("README"), "one\n").expect("write");
        git(&seed, &["add", "README"]);
        git(&seed, &["commit", "-q", "-m", "one"]);
        git(&seed, &["branch", "feature"]);
        git(&root, &["clone", "-q", "--bare", "seed", "origin.git"]);
        git(&root, &["clone", "-q", "origin.git", "clone"]);
        identify(&root.join("clone"));
        let repos = Self { scratch };
        git(
            &repos.clone_path(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                repos.worktree().to_str().expect("utf-8"),
                "origin/feature",
            ],
        );
        repos
    }

    fn clone_path(&self) -> PathBuf {
        self.scratch.join("clone")
    }

    fn worktree(&self) -> PathBuf {
        self.scratch.join("feature-wt")
    }

    /// Advance `branch` on the origin by one commit writing `file`.
    fn push_to_origin(&self, branch: &str, file: &str, text: &str) {
        let pusher = self.scratch.join(&format!("pusher-{branch}-{file}"));
        git(
            self.scratch.path(),
            &[
                "clone",
                "-q",
                "-b",
                branch,
                "origin.git",
                pusher.to_str().expect("utf-8"),
            ],
        );
        identify(&pusher);
        std::fs::write(pusher.join(file), text).expect("write");
        git(&pusher, &["add", file]);
        git(&pusher, &["commit", "-q", "-m", file]);
        git(&pusher, &["push", "-q", "origin", branch]);
    }

    fn commit_in_worktree(&self, file: &str, text: &str) {
        let worktree = self.worktree();
        std::fs::write(worktree.join(file), text).expect("write");
        git(&worktree, &["add", file]);
        git(&worktree, &["commit", "-q", "-m", file]);
    }

    fn config(&self) -> serde_json::Value {
        serde_json::json!({
            "clones": { "owner/repo": self.clone_path().display().to_string() }
        })
    }
}

fn identify(dir: &std::path::Path) {
    git(dir, &["config", "user.name", "Rostrum Test"]);
    git(dir, &["config", "user.email", "test@rostrum.invalid"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn key() -> PrKey {
    PrKey {
        repo: RepoId::new("owner", "repo"),
        number: PrNumber(7),
    }
}

fn pr(head: &str) -> PrRef {
    PrRef {
        key: key(),
        title: "A feature".into(),
        url: "https://github.com/owner/repo/pull/7".into(),
        body: String::new(),
        head_ref: head.into(),
        base_ref: "main".into(),
    }
}

async fn status(client: &RemoteClient, head: &str) -> LocalStatus {
    client
        .local_status(&LocalStatusRequest {
            key: key(),
            head_ref: head.into(),
        })
        .await
        .expect("status")
}

async fn run(client: &RemoteClient, op: LocalOpKind) -> JobOutcome {
    client
        .run_job(&JobRequest {
            pr: pr("feature"),
            op,
            autostash: false,
        })
        .await
        .expect("job")
}

#[tokio::test]
async fn a_pull_over_the_api_brings_the_worktree_level_with_its_remote() {
    let repos = Repos::new("it-job-pull");
    repos.push_to_origin("feature", "remote.txt", "unpulled\n");
    let harness = Harness::start("it-job-pull-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;

    let LocalStatus::CheckedOut { branch } = status(&client, "feature").await else {
        panic!("expected the feature branch to be checked out");
    };
    assert_eq!(
        PathBuf::from(&branch.worktree)
            .canonicalize()
            .expect("canonical"),
        repos.worktree().canonicalize().expect("canonical")
    );
    assert_eq!(branch.branch, "feature");
    assert!(branch.fetched);
    assert_eq!((branch.ahead, branch.behind), (0, 1));
    assert_eq!(branch.in_progress, None);
    assert_eq!(branch.handoff, None);

    assert_eq!(
        run(&client, LocalOpKind::PullRebase).await,
        JobOutcome::Completed
    );
    assert_eq!(
        run(&client, LocalOpKind::PullRebase).await,
        JobOutcome::UpToDate
    );
    assert!(
        repos.worktree().join("remote.txt").exists(),
        "the pull reached the worktree"
    );

    let LocalStatus::CheckedOut { branch } = status(&client, "feature").await else {
        panic!("still checked out");
    };
    assert_eq!((branch.ahead, branch.behind), (0, 0));

    harness.stop().await;
}

#[tokio::test]
async fn a_conflict_without_a_handler_is_aborted_and_there_is_then_nothing_to_abort() {
    let repos = Repos::new("it-job-conflict");
    repos.commit_in_worktree("README", "feature side\n");
    repos.push_to_origin("main", "README", "main side\n");
    let harness = Harness::start("it-job-conflict-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;

    let outcome = run(&client, LocalOpKind::RebaseBase).await;
    assert!(
        matches!(outcome, JobOutcome::Conflicted { .. }),
        "{outcome:?}"
    );

    let LocalStatus::CheckedOut { branch } = status(&client, "feature").await else {
        panic!("checked out");
    };
    assert_eq!(branch.in_progress, None, "the rebase was aborted");

    let err = client
        .abort(&AbortRequest {
            key: key(),
            head_ref: "feature".into(),
        })
        .await
        .expect_err("nothing to abort");
    assert!(
        matches!(&err, ClientError::Api(error) if error.code == ApiErrorCode::BadRequest),
        "{err:?}"
    );

    harness.stop().await;
}

#[tokio::test]
async fn a_branch_with_no_worktree_is_reported_as_such() {
    let repos = Repos::new("it-job-absent");
    let harness = Harness::start("it-job-absent-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;
    assert_eq!(
        status(&client, "main-only-elsewhere").await,
        LocalStatus::NotCheckedOut
    );
    let outcome = client
        .run_job(&JobRequest {
            pr: pr("main-only-elsewhere"),
            op: LocalOpKind::MergeBase,
            autostash: false,
        })
        .await
        .expect("job");
    assert_eq!(outcome, JobOutcome::NotCheckedOut);
    harness.stop().await;
}

#[tokio::test]
async fn sync_all_over_the_api_runs_the_real_jobs() {
    let repos = Repos::new("it-job-sync");
    repos.push_to_origin("main", "base.txt", "new on main\n");
    let harness = Harness::start("it-job-sync-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;

    let mut elsewhere = pr("feature");
    elsewhere.key.repo = RepoId::new("not", "configured");
    let started = client
        .start_sync_all(&SyncAllRequest {
            op: LocalOpKind::MergeBase,
            autostash: false,
            prs: vec![pr("feature"), elsewhere],
        })
        .await
        .expect("starts");
    assert_eq!(started.entries.len(), 2);

    let mut finished = None;
    for _ in 0..300 {
        let run = client.sync_all().await.expect("poll").expect("a run");
        if run.is_finished() {
            finished = Some(run);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let finished = finished.expect("the run finishes");
    assert_eq!(
        finished.entries[0].state,
        SyncEntryState::Done {
            outcome: JobOutcome::Completed
        }
    );
    assert_eq!(
        finished.entries[1].state,
        SyncEntryState::Done {
            outcome: JobOutcome::NotConfigured
        }
    );
    assert!(
        repos.worktree().join("base.txt").exists(),
        "main was merged in"
    );
    harness.stop().await;
}
