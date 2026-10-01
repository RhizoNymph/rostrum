//! The stack-rewrite primitives against real repositories: a bare origin, a
//! clone of it, and scratch worktrees, all built with the `git` CLI in a
//! temporary directory.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use rostrum_git::{
    BranchName, ConflictPolicy, GitError, Oid, Outcome, PushOutcome, PushRejection, RefExpectation,
    Remote, RemoteRef, Repo, Rev,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// `main` with one commit, and two branches off it: `a` touching `a.txt`
    /// and `b` touching `b.txt`, both pushed to a bare origin and cloned.
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rostrum-git-stack-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch dir");
        let fixture = Self { root };

        let seed = fixture.path("seed");
        std::fs::create_dir_all(&seed).expect("seed dir");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        commit(&seed, "shared.txt", "one\ntwo\nthree\n");
        git(&seed, &["checkout", "-q", "-b", "a"]);
        commit(&seed, "a.txt", "a\n");
        git(&seed, &["checkout", "-q", "main"]);
        git(&seed, &["checkout", "-q", "-b", "b"]);
        commit(&seed, "b.txt", "b\n");
        git(&seed, &["checkout", "-q", "main"]);
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

    /// Commit to `branch` on the origin from a separate clone.
    fn push_to_origin(&self, branch: &str, file: &str, text: &str) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pusher = self.path(&format!("pusher-{n}"));
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
        commit(&pusher, file, text);
        git(&pusher, &["push", "-q", "origin", branch]);
    }

    fn origin_oid(&self, branch: &str) -> String {
        output(
            &self.path("origin.git"),
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    async fn repo(&self) -> Repo {
        Repo::open(self.clone()).await.expect("clone opens")
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

fn output(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}",
        dir.display()
    );
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .trim()
        .to_string()
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
    BranchName::new(name).expect("valid")
}

fn remote(name: &str) -> Rev {
    Rev::Remote(RemoteRef::origin(branch(name)))
}

async fn oid_of(repo: &Repo, rev: &Rev) -> Oid {
    repo.resolve(rev).await.expect("resolves").expect("exists")
}

#[tokio::test]
async fn resolve_answers_for_present_and_absent_refs() {
    let fixture = Fixture::new("resolve");
    let repo = fixture.repo().await;
    let a = oid_of(&repo, &remote("a")).await;
    assert_eq!(a.as_str(), fixture.origin_oid("a"));
    assert_eq!(repo.resolve(&remote("nope")).await.expect("asks"), None);
}

#[tokio::test]
async fn ancestry_is_answered_both_ways() {
    let fixture = Fixture::new("ancestry");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;
    assert!(repo.is_ancestor(&main, &a).await.expect("asks"));
    assert!(!repo.is_ancestor(&a, &main).await.expect("asks"));
    assert!(!repo.is_ancestor(&a, &b).await.expect("asks"));
    assert!(repo.is_ancestor(&a, &a).await.expect("asks"));
}

#[tokio::test]
async fn a_scratch_rebase_moves_only_the_branch_s_own_commits() {
    let fixture = Fixture::new("rebase");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;

    let scratch_path = fixture.path("scratch-b");
    let scratch = repo
        .add_scratch_worktree(&scratch_path, &b)
        .await
        .expect("worktree added");
    let outcome = scratch.rebase_scratch(&a, &main).await.expect("rebases");
    let Outcome::Completed { from, to } = outcome else {
        panic!("expected a completed rebase, got {outcome:?}");
    };
    assert_eq!(from, b);
    // `b` now sits on `a`: a's commit is an ancestor and b's file is there.
    assert!(repo.is_ancestor(&a, &to).await.expect("asks"));
    assert!(scratch_path.join("a.txt").exists());
    assert!(scratch_path.join("b.txt").exists());
    // One commit replayed, not two: main..a is not repeated.
    let count = output(
        &scratch_path,
        &["rev-list", "--count", &format!("{a}..{to}")],
    );
    assert_eq!(count, "1");

    // No branch moved: the clone's view of `b` is untouched.
    assert_eq!(oid_of(&repo, &remote("b")).await, b);

    repo.remove_scratch_worktree(&scratch_path)
        .await
        .expect("removed");
    assert!(!scratch_path.exists());
}

#[tokio::test]
async fn a_scratch_rebase_refuses_a_worktree_on_a_branch() {
    let fixture = Fixture::new("not-scratch");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    // The clone itself is on `main`, not detached.
    let err = repo.rebase_scratch(&a, &main).await.expect_err("refused");
    assert!(matches!(err, GitError::NotScratch { .. }), "{err:?}");
}

/// Two branches editing the same line of `shared.txt`.
async fn conflicting(fixture: &Fixture) -> (Repo, Oid, Oid, Oid) {
    fixture.push_to_origin("a", "shared.txt", "one\nA\nthree\n");
    fixture.push_to_origin("b", "shared.txt", "one\nB\nthree\n");
    let repo = fixture.repo().await;
    for name in ["a", "b"] {
        repo.fetch(&RemoteRef::origin(branch(name)))
            .await
            .expect("fetches");
    }
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;
    (repo, main, a, b)
}

#[tokio::test]
async fn a_conflict_under_abort_leaves_the_scratch_worktree_as_it_was() {
    let fixture = Fixture::new("conflict-abort");
    let (repo, main, a, b) = conflicting(&fixture).await;
    let path = fixture.path("scratch");
    let scratch = repo.add_scratch_worktree(&path, &b).await.expect("added");

    let outcome = scratch.rebase_scratch(&a, &main).await.expect("runs");
    let Outcome::Conflicted(conflict) = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert_eq!(
        conflict.abort_target(),
        None,
        "aborted under the default policy"
    );
    let status = scratch.status().await.expect("status");
    assert_eq!(status.in_progress, None);
    assert_eq!(status.oid(), Some(&b));
    repo.remove_scratch_worktree(&path).await.expect("removed");
}

#[tokio::test]
async fn a_conflict_under_leave_stays_for_a_handler_and_rerere_replays_its_fix() {
    let fixture = Fixture::new("conflict-leave");
    let (repo, main, a, b) = conflicting(&fixture).await;
    let repo = repo.with_conflict_policy(ConflictPolicy::Leave);

    // First attempt stops; someone resolves it by hand and continues.
    let first_path = fixture.path("scratch-1");
    let first = repo
        .add_scratch_worktree(&first_path, &b)
        .await
        .expect("added");
    let outcome = first.rebase_scratch(&a, &main).await.expect("runs");
    let Outcome::Conflicted(conflict) = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert!(conflict.abort_target().is_some(), "left in place");
    std::fs::write(first_path.join("shared.txt"), "one\nA and B\nthree\n").expect("resolve");
    git(&first_path, &["add", "shared.txt"]);
    // What a person would run; rerere records the resolution here because the
    // first run left an rr-cache behind.
    let status = Command::new("git")
        .args(["-c", "core.editor=true", "rebase", "--continue"])
        .current_dir(&first_path)
        .status()
        .expect("git runs");
    assert!(status.success());
    repo.remove_scratch_worktree(&first_path)
        .await
        .expect("removed");

    // Second attempt from scratch: rerere settles it without stopping.
    let second_path = fixture.path("scratch-2");
    let second = repo
        .add_scratch_worktree(&second_path, &b)
        .await
        .expect("added");
    let outcome = second.rebase_scratch(&a, &main).await.expect("runs");
    let Outcome::Completed { to, .. } = outcome else {
        panic!("expected rerere to finish the rebase, got {outcome:?}");
    };
    assert!(repo.is_ancestor(&a, &to).await.expect("asks"));
    let text = std::fs::read_to_string(second_path.join("shared.txt")).expect("read");
    assert_eq!(text, "one\nA and B\nthree\n");
    repo.remove_scratch_worktree(&second_path)
        .await
        .expect("removed");
}

#[tokio::test]
async fn a_leased_push_lands_when_the_remote_is_where_it_was() {
    let fixture = Fixture::new("push-ok");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;

    let path = fixture.path("scratch");
    let scratch = repo.add_scratch_worktree(&path, &b).await.expect("added");
    let Outcome::Completed { to, .. } = scratch.rebase_scratch(&a, &main).await.expect("runs")
    else {
        panic!("expected a rebase");
    };

    let outcome = repo
        .push_with_lease(&Remote::origin(), &branch("b"), &to, &b)
        .await
        .expect("pushes");
    assert_eq!(outcome, PushOutcome::Updated { forced: true });
    assert_eq!(fixture.origin_oid("b"), to.as_str());
    // git updates the tracking ref on a successful push.
    assert_eq!(oid_of(&repo, &remote("b")).await, to);

    // Pushing the same commit again is a no-op, not an error.
    let again = repo
        .push_with_lease(&Remote::origin(), &branch("b"), &to, &to)
        .await
        .expect("pushes");
    assert_eq!(again, PushOutcome::UpToDate);
}

#[tokio::test]
async fn a_leased_push_is_rejected_when_someone_pushed_in_between() {
    let fixture = Fixture::new("push-stale");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &remote("main")).await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;

    let path = fixture.path("scratch");
    let scratch = repo.add_scratch_worktree(&path, &b).await.expect("added");
    let Outcome::Completed { to, .. } = scratch.rebase_scratch(&a, &main).await.expect("runs")
    else {
        panic!("expected a rebase");
    };

    // Someone else pushes to `b` after rostrum fetched it.
    fixture.push_to_origin("b", "late.txt", "late\n");
    let theirs = fixture.origin_oid("b");

    let outcome = repo
        .push_with_lease(&Remote::origin(), &branch("b"), &to, &b)
        .await
        .expect("asks");
    assert_eq!(outcome, PushOutcome::Rejected(PushRejection::StaleLease));
    // Their commit survives.
    assert_eq!(fixture.origin_oid("b"), theirs);
}

#[tokio::test]
async fn set_branch_is_a_compare_and_swap() {
    let fixture = Fixture::new("set-branch");
    let repo = fixture.repo().await;
    let a = oid_of(&repo, &remote("a")).await;
    let b = oid_of(&repo, &remote("b")).await;

    // Create where absent.
    repo.set_branch(&branch("a"), &a, &RefExpectation::Absent)
        .await
        .expect("created");
    assert_eq!(oid_of(&repo, &Rev::Local(branch("a"))).await, a);

    // Absent no longer holds.
    assert!(
        repo.set_branch(&branch("a"), &b, &RefExpectation::Absent)
            .await
            .is_err()
    );
    // The wrong expected value is refused, and nothing moves.
    assert!(
        repo.set_branch(&branch("a"), &b, &RefExpectation::At(b.clone()))
            .await
            .is_err()
    );
    assert_eq!(oid_of(&repo, &Rev::Local(branch("a"))).await, a);
    // The right one moves it.
    repo.set_branch(&branch("a"), &b, &RefExpectation::At(a.clone()))
        .await
        .expect("moved");
    assert_eq!(oid_of(&repo, &Rev::Local(branch("a"))).await, b);
}

#[tokio::test]
async fn set_branch_never_moves_a_checked_out_branch() {
    let fixture = Fixture::new("set-checked-out");
    let repo = fixture.repo().await;
    let main = oid_of(&repo, &Rev::Local(branch("main"))).await;
    let a = oid_of(&repo, &remote("a")).await;
    let err = repo
        .set_branch(&branch("main"), &a, &RefExpectation::At(main.clone()))
        .await
        .expect_err("refused");
    assert!(matches!(err, GitError::CheckedOut { .. }), "{err:?}");
    assert_eq!(oid_of(&repo, &Rev::Local(branch("main"))).await, main);
}
