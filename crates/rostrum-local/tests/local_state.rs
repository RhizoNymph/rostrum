//! `local_state`, `run_local_job` and `abort_in_progress` against real
//! repositories built with the `git` CLI in a scratch directory.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use rostrum_core::{Divergence, PrNumber, RepoId};
use rostrum_git::{Autostash, BranchName};
use rostrum_handoff::PrMeta;
use rostrum_local::{
    LocalJob, LocalOp, LocalResult, LocalState, abort_in_progress, local_state, run_local_job,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// An origin, a clone of it, and a second worktree of the clone, removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rostrum-local-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch dir");
        let fixture = Self { root };

        let seed = fixture.path("seed");
        std::fs::create_dir_all(&seed).expect("seed dir");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        std::fs::write(seed.join("README"), "one\n").expect("write");
        git(&seed, &["add", "README"]);
        git(&seed, &["commit", "-q", "-m", "one"]);
        git(&seed, &["branch", "feature"]);
        git(
            &fixture.root,
            &["clone", "-q", "--bare", "seed", "origin.git"],
        );
        git(&fixture.root, &["clone", "-q", "origin.git", "clone"]);
        identify(&fixture.clone());
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn clone(&self) -> PathBuf {
        self.path("clone")
    }

    /// Check `feature` out in its own worktree beside the clone.
    fn check_out_feature(&self) -> PathBuf {
        let worktree = self.path("feature-wt");
        git(
            &self.clone(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                worktree.to_str().expect("utf-8"),
                "origin/feature",
            ],
        );
        worktree
    }

    /// Advance `branch` on the origin by one commit touching `file`.
    fn push_to_origin(&self, branch: &str, file: &str, text: &str) {
        let pusher = self.path(&format!("pusher-{branch}-{file}"));
        git(
            &self.root,
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn identify(dir: &Path) {
    git(dir, &["config", "user.name", "Rostrum Test"]);
    git(dir, &["config", "user.email", "test@rostrum.invalid"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).expect("write");
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name.to_string()).expect("valid branch")
}

fn job(fixture: &Fixture, op: LocalOp) -> LocalJob {
    LocalJob {
        clone: fixture.clone(),
        branch: branch("feature"),
        base: branch("main"),
        op,
        autostash: Autostash::Disabled,
        handler: None,
        pr: PrMeta {
            repo: RepoId::new("owner", "name"),
            number: PrNumber(7),
            title: "A feature".into(),
            body: String::new(),
            url: "https://github.com/owner/name/pull/7".into(),
            head_ref: "feature".into(),
            base_ref: "main".into(),
        },
    }
}

#[tokio::test]
async fn a_branch_with_no_worktree_is_not_checked_out() {
    let fixture = Fixture::new("absent");
    let state = local_state(
        &fixture.clone(),
        branch("feature"),
        Autostash::Disabled,
        None,
    )
    .await
    .expect("state");
    assert_eq!(state, LocalState::NotCheckedOut);
}

#[tokio::test]
async fn a_checked_out_branch_reports_its_own_worktree_and_drift() {
    let fixture = Fixture::new("drift");
    let worktree = fixture.check_out_feature();
    commit(&worktree, "local.txt", "unpushed\n");
    fixture.push_to_origin("feature", "remote.txt", "unpulled\n");

    let state = local_state(
        &fixture.clone(),
        branch("feature"),
        Autostash::Disabled,
        None,
    )
    .await
    .expect("state");
    let LocalState::CheckedOut(local) = state else {
        panic!("expected a checked-out branch, got {state:?}");
    };
    assert_eq!(
        local.worktree.canonicalize().expect("canonical"),
        worktree.canonicalize().expect("canonical")
    );
    assert!(local.fetched, "the fetch from a local origin succeeds");
    assert_eq!(
        local.divergence,
        Divergence {
            ahead: 1,
            behind: 1
        }
    );
    assert_eq!(local.in_progress, None);
    assert_eq!(local.handoff, None);
}

#[tokio::test]
async fn pulling_brings_the_branch_level_with_its_remote() {
    let fixture = Fixture::new("pull");
    fixture.check_out_feature();
    fixture.push_to_origin("feature", "remote.txt", "unpulled\n");

    let result = run_local_job(job(&fixture, LocalOp::PullRebase)).await;
    assert_eq!(result, LocalResult::Completed);

    let again = run_local_job(job(&fixture, LocalOp::PullRebase)).await;
    assert_eq!(again, LocalResult::UpToDate);
}

#[tokio::test]
async fn a_job_on_a_branch_with_no_worktree_says_so() {
    let fixture = Fixture::new("nojob");
    let result = run_local_job(job(&fixture, LocalOp::MergeBase)).await;
    assert_eq!(result, LocalResult::NotCheckedOut);
}

#[tokio::test]
async fn a_conflict_without_a_handler_is_aborted_and_the_worktree_left_clean() {
    let fixture = Fixture::new("conflict");
    let worktree = fixture.check_out_feature();
    commit(&worktree, "README", "feature side\n");
    fixture.push_to_origin("main", "README", "main side\n");

    let result = run_local_job(job(&fixture, LocalOp::RebaseBase)).await;
    assert!(
        matches!(result, LocalResult::Conflicted(_)),
        "expected a conflict, got {result:?}"
    );

    let state = local_state(
        &fixture.clone(),
        branch("feature"),
        Autostash::Disabled,
        None,
    )
    .await
    .expect("state");
    let LocalState::CheckedOut(local) = state else {
        panic!("expected a checked-out branch");
    };
    assert_eq!(local.in_progress, None, "the rebase was aborted");
}

#[tokio::test]
async fn aborting_with_nothing_in_progress_is_an_error() {
    let fixture = Fixture::new("abort");
    let worktree = fixture.check_out_feature();
    assert!(abort_in_progress(&worktree).await.is_err());
}
