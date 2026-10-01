//! `run_stack_job` end to end against real git: a bare origin, a clone, and
//! scratch worktrees. `gh` is a recording double — rostrum's tests never run
//! a mutating `gh stack` command against GitHub — so every call is asserted by
//! the exact command it would have run.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::Utc;
use rostrum_core::{
    LoadState, MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, RefName, RepoId,
    RepoState, StackPlan, plan_stack,
};
use rostrum_git::{BranchName, PushRejection};
use rostrum_stack::{
    GhOutput, GhRunner, GhStackCommand, LocalNote, LocalTracking, Progress, StackJob, StackOpError,
    StackOutcome, StackProgress, run_stack_job,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

// --- the gh double ----------------------------------------------------------

/// Records every command; answers `view --json` from the last `init`.
#[derive(Default)]
struct FakeGh {
    calls: Mutex<Vec<(PathBuf, RepoId, GhStackCommand)>>,
    fail_link: Mutex<bool>,
}

impl FakeGh {
    fn failing_link() -> Self {
        Self {
            fail_link: Mutex::new(true),
            ..Self::default()
        }
    }

    fn commands(&self) -> Vec<GhStackCommand> {
        self.calls
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, _, c)| c.clone())
            .collect()
    }

    fn argvs(&self) -> Vec<Vec<String>> {
        self.commands().iter().map(GhStackCommand::argv).collect()
    }
}

fn ok(stdout: &str) -> GhOutput {
    GhOutput {
        success: true,
        code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

impl GhRunner for FakeGh {
    async fn run(
        &self,
        cwd: &Path,
        repo: &RepoId,
        command: &GhStackCommand,
    ) -> Result<GhOutput, StackOpError> {
        let last_init = {
            let mut calls = self.calls.lock().expect("lock");
            calls.push((cwd.to_path_buf(), repo.clone(), command.clone()));
            calls.iter().rev().find_map(|(_, _, c)| match c {
                GhStackCommand::Init { base, branches } => Some((base.clone(), branches.clone())),
                _ => None,
            })
        };
        Ok(match command {
            GhStackCommand::Link { .. } if *self.fail_link.lock().expect("lock") => GhOutput {
                success: false,
                code: Some(1),
                stdout: String::new(),
                stderr: "✗ failed to create stack: boom".into(),
            },
            GhStackCommand::ViewJson => match last_init {
                Some((base, branches)) => {
                    let branches: Vec<String> = branches
                        .iter()
                        .map(|b| format!(r#"{{"name":"{b}","isCurrent":false,"isMerged":false,"isQueued":false,"needsRebase":false}}"#))
                        .collect();
                    ok(&format!(
                        r#"{{"trunk":"{base}","currentBranch":"x","branches":[{}]}}"#,
                        branches.join(",")
                    ))
                }
                None => GhOutput {
                    success: false,
                    code: Some(2),
                    stdout: String::new(),
                    stderr: "✗ current branch is not part of a stack".into(),
                },
            },
            _ => ok(""),
        })
    }
}

// --- repositories -------------------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// `main`, plus one branch per `(name, parent)` with one commit adding
    /// `<name>.txt`, pushed to a bare origin and cloned.
    fn new(tag: &str, branches: &[(&str, &str)]) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rostrum-stack-job-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch dir");
        let fixture = Self { root };

        let seed = fixture.path("seed");
        std::fs::create_dir_all(&seed).expect("seed");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        commit(&seed, "shared.txt", "one\ntwo\nthree\n");
        for (name, parent) in branches {
            git(&seed, &["checkout", "-q", "-b", name, parent]);
            commit(&seed, &format!("{name}.txt"), &format!("{name}\n"));
        }
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

    fn scratch(&self) -> PathBuf {
        self.path("scratch")
    }

    fn origin(&self, rev: &str) -> String {
        output(&self.path("origin.git"), &["rev-parse", rev])
    }

    fn origin_is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        Command::new("git")
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .current_dir(self.path("origin.git"))
            .status()
            .expect("git runs")
            .success()
    }

    fn worktree_count(&self) -> usize {
        output(&self.clone(), &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    /// Commit `text` to `file` on `branch` in a separate clone of `bare`, and
    /// push it there.
    fn advance(&self, bare: &str, branch: &str, file: &str, text: &str) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pusher = self.path(&format!("pusher-{n}"));
        git(
            &self.root,
            &[
                "clone",
                "-q",
                "-b",
                branch,
                bare,
                pusher.to_str().expect("utf-8"),
            ],
        );
        identify(&pusher);
        commit(&pusher, file, text);
        git(&pusher, &["push", "-q", "origin", branch]);
    }

    fn job(&self, plan: StackPlan) -> StackJob {
        StackJob {
            clone: self.clone(),
            plan,
            handler: None,
            scratch_dir: self.scratch(),
        }
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

// --- plans --------------------------------------------------------------------

fn repo_id() -> RepoId {
    RepoId::new("octo", "repo")
}

fn pull(number: u32, head: &str, base: &str) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("PR {number}"),
        url: format!("https://github.com/octo/repo/pull/{number}"),
        is_draft: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        author: None,
        head_ref: head.into(),
        head_sha: String::new(),
        base_ref: base.into(),
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
        is_cross_repository: false,
        pushed_at: None,
    }
}

fn plan(prs: Vec<PullRequest>, order: &[u32]) -> StackPlan {
    let state = RepoState {
        prs,
        load: LoadState::Idle,
        ..RepoState::new(repo_id())
    };
    let order: Vec<PrNumber> = order.iter().copied().map(PrNumber).collect();
    plan_stack(&state, &order, RefName::new("main").expect("valid")).expect("valid plan")
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name).expect("valid")
}

fn link_argv(numbers: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["stack", "link", "--base", "main"]
        .into_iter()
        .map(String::from)
        .collect();
    argv.extend(numbers.iter().map(|n| n.to_string()));
    argv
}

fn init_argv(branches: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["stack", "init", "--base", "main", "--"]
        .into_iter()
        .map(String::from)
        .collect();
    argv.extend(branches.iter().map(|b| b.to_string()));
    argv
}

fn view_argv() -> Vec<String> {
    vec!["stack".into(), "view".into(), "--json".into()]
}

// --- tests --------------------------------------------------------------------

#[tokio::test]
async fn making_a_stack_from_an_existing_chain_rewrites_nothing() {
    let fixture = Fixture::new("make", &[("a", "main"), ("b", "a")]);
    let before = (fixture.origin("a"), fixture.origin("b"));
    let gh = FakeGh::default();
    let plan = plan(vec![pull(1, "a", "main"), pull(2, "b", "a")], &[1, 2]);
    assert!(!plan.needs_rewrite());

    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::none())
        .await
        .expect("runs");

    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert!(report.rewritten.is_empty());
    assert_eq!(report.local, LocalTracking::Tracked);
    assert_eq!(
        report.notes,
        vec![
            LocalNote::Created(branch("a")),
            LocalNote::Created(branch("b"))
        ],
        "gh stack init adopts local branches, so they are created first"
    );
    assert_eq!(
        (fixture.origin("a"), fixture.origin("b")),
        before,
        "nothing pushed"
    );
    assert_eq!(
        gh.argvs(),
        vec![link_argv(&["1", "2"]), init_argv(&["a", "b"]), view_argv()]
    );
    // Every call ran in the clone, for the repository the plan is for.
    for (cwd, repo, _) in gh.calls.lock().expect("lock").iter() {
        assert_eq!(
            cwd,
            &std::fs::canonicalize(fixture.clone()).expect("exists")
        );
        assert_eq!(repo, &repo_id());
    }
}

#[tokio::test]
async fn arranging_independent_pull_requests_rebases_and_pushes_with_a_lease() {
    let fixture = Fixture::new("arrange", &[("a", "main"), ("b", "main"), ("c", "main")]);
    let a_before = fixture.origin("a");
    let gh = FakeGh::default();
    let plan = plan(
        vec![
            pull(1, "a", "main"),
            pull(2, "b", "main"),
            pull(3, "c", "main"),
        ],
        &[1, 2, 3],
    );
    assert!(plan.needs_rewrite());

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::new(tx))
        .await
        .expect("runs");

    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert_eq!(report.rewritten, vec![PrNumber(2), PrNumber(3)]);
    assert_eq!(report.local, LocalTracking::Tracked);

    // The bottom is untouched; b sits on a, c on b, each with its own file.
    assert_eq!(fixture.origin("a"), a_before);
    assert!(fixture.origin_is_ancestor("a", "b"));
    assert!(fixture.origin_is_ancestor("b", "c"));
    assert_eq!(
        output(
            &fixture.path("origin.git"),
            &["rev-list", "--count", "b..c"]
        ),
        "1",
        "only c's own commit sits above b"
    );
    assert_eq!(
        gh.argvs(),
        vec![
            link_argv(&["1", "2", "3"]),
            init_argv(&["a", "b", "c"]),
            view_argv()
        ]
    );
    assert_eq!(fixture.worktree_count(), 1, "scratch worktrees are removed");

    let mut seen = Vec::new();
    while let Ok(progress) = rx.try_recv() {
        seen.push(progress);
    }
    assert_eq!(seen.first(), Some(&StackProgress::Fetching));
    assert!(seen.contains(&StackProgress::Pushing {
        number: PrNumber(2)
    }));
    assert!(seen.contains(&StackProgress::Pushing {
        number: PrNumber(3)
    }));
    assert_eq!(seen.last(), Some(&StackProgress::Tracking));
}

#[tokio::test]
async fn reordering_an_existing_chain_moves_only_what_changed() {
    // a ← b on GitHub; the user wants b at the bottom, then a.
    let fixture = Fixture::new("reorder", &[("a", "main"), ("b", "a")]);
    let gh = FakeGh::default();
    let plan = plan(vec![pull(1, "a", "main"), pull(2, "b", "a")], &[2, 1]);

    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert_eq!(report.rewritten, vec![PrNumber(2), PrNumber(1)]);
    assert!(fixture.origin_is_ancestor("main", "b"));
    assert!(
        !fixture.origin_is_ancestor("a", "b"),
        "b no longer carries a"
    );
    assert!(fixture.origin_is_ancestor("b", "a"));
}

#[tokio::test]
async fn a_branch_already_on_its_new_parent_is_not_pushed_again() {
    // b is built on a, but its pull request targets main: only the base on
    // GitHub needs to change, which `link` does.
    let fixture = Fixture::new("retarget", &[("a", "main"), ("b", "a")]);
    let before = fixture.origin("b");
    let gh = FakeGh::default();
    let plan = plan(vec![pull(1, "a", "main"), pull(2, "b", "main")], &[1, 2]);
    assert!(plan.needs_rewrite());

    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert!(report.rewritten.is_empty());
    assert_eq!(fixture.origin("b"), before);
    assert_eq!(gh.argvs()[0], link_argv(&["1", "2"]));
}

#[tokio::test]
async fn a_conflict_without_a_handler_aborts_and_changes_nothing() {
    let fixture = Fixture::new("conflict", &[("a", "main"), ("b", "main")]);
    // Both edit the same line.
    fixture.advance(
        fixture.path("origin.git").to_str().expect("utf-8"),
        "a",
        "shared.txt",
        "one\nA\nthree\n",
    );
    fixture.advance(
        fixture.path("origin.git").to_str().expect("utf-8"),
        "b",
        "shared.txt",
        "one\nB\nthree\n",
    );
    let before = (fixture.origin("a"), fixture.origin("b"));
    let gh = FakeGh::default();
    let plan = plan(vec![pull(1, "a", "main"), pull(2, "b", "main")], &[1, 2]);

    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Conflicted { number, message } = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert_eq!(number, PrNumber(2));
    assert!(message.contains("shared.txt"), "{message}");
    assert_eq!(
        (fixture.origin("a"), fixture.origin("b")),
        before,
        "nothing pushed"
    );
    assert!(gh.commands().is_empty(), "gh is never reached");
    assert_eq!(fixture.worktree_count(), 1, "the scratch worktree is gone");
}

#[tokio::test]
async fn a_refused_lease_stops_the_job_and_reports_what_already_landed() {
    let fixture = Fixture::new("lease", &[("a", "main"), ("b", "main"), ("c", "main")]);
    // Pushes go to a second remote where `c` has moved on since the fetch,
    // which is exactly what a teammate pushing in between looks like.
    git(
        &fixture.root,
        &["clone", "-q", "--bare", "origin.git", "pushed.git"],
    );
    fixture.advance(
        fixture.path("pushed.git").to_str().expect("utf-8"),
        "c",
        "late.txt",
        "late\n",
    );
    let theirs = output(&fixture.path("pushed.git"), &["rev-parse", "c"]);
    git(
        &fixture.clone(),
        &[
            "config",
            "remote.origin.pushurl",
            fixture.path("pushed.git").to_str().expect("utf-8"),
        ],
    );
    let gh = FakeGh::default();
    let plan = plan(
        vec![
            pull(1, "a", "main"),
            pull(2, "b", "main"),
            pull(3, "c", "main"),
        ],
        &[1, 2, 3],
    );

    let outcome = run_stack_job(fixture.job(plan), &gh, &Progress::none())
        .await
        .expect("runs");
    assert_eq!(
        outcome,
        StackOutcome::PushRejected {
            pushed: vec![PrNumber(2)],
            number: PrNumber(3),
            reason: PushRejection::StaleLease,
        }
    );
    assert_eq!(
        output(&fixture.path("pushed.git"), &["rev-parse", "c"]),
        theirs,
        "their commit survives"
    );
    assert!(
        gh.commands().is_empty(),
        "nothing is linked after a refusal"
    );
}

#[tokio::test]
async fn a_failed_link_after_pushing_is_reported_and_a_rerun_finishes_without_rewriting() {
    let fixture = Fixture::new("relink", &[("a", "main"), ("b", "main")]);
    let prs = || vec![pull(1, "a", "main"), pull(2, "b", "main")];

    let failing = FakeGh::failing_link();
    let outcome = run_stack_job(
        fixture.job(plan(prs(), &[1, 2])),
        &failing,
        &Progress::none(),
    )
    .await
    .expect("runs");
    let StackOutcome::LinkFailed { pushed, message } = outcome else {
        panic!("expected a link failure, got {outcome:?}");
    };
    assert_eq!(pushed, vec![PrNumber(2)]);
    assert!(message.contains("boom"), "{message}");
    let rewritten = fixture.origin("b");

    let gh = FakeGh::default();
    let outcome = run_stack_job(fixture.job(plan(prs(), &[1, 2])), &gh, &Progress::none())
        .await
        .expect("runs");
    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert!(report.rewritten.is_empty(), "already on its parent");
    assert_eq!(fixture.origin("b"), rewritten);
    assert_eq!(gh.argvs()[0], link_argv(&["1", "2"]));
}

#[tokio::test]
async fn a_failed_link_with_nothing_pushed_is_an_error() {
    let fixture = Fixture::new("link-error", &[("a", "main"), ("b", "a")]);
    let gh = FakeGh::failing_link();
    let err = run_stack_job(
        fixture.job(plan(vec![pull(1, "a", "main"), pull(2, "b", "a")], &[1, 2])),
        &gh,
        &Progress::none(),
    )
    .await
    .expect_err("fails");
    assert!(matches!(err, StackOpError::GhFailed { .. }), "{err:?}");
}

#[tokio::test]
async fn a_dirty_clone_is_linked_but_not_tracked() {
    let fixture = Fixture::new("dirty", &[("a", "main"), ("b", "a")]);
    std::fs::write(fixture.clone().join("shared.txt"), "edited\n").expect("dirty");
    let gh = FakeGh::default();
    let outcome = run_stack_job(
        fixture.job(plan(vec![pull(1, "a", "main"), pull(2, "b", "a")], &[1, 2])),
        &gh,
        &Progress::none(),
    )
    .await
    .expect("runs");
    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    let LocalTracking::Skipped(reason) = report.local else {
        panic!("expected tracking to be skipped");
    };
    assert!(reason.contains("uncommitted"), "{reason}");
    assert_eq!(gh.argvs(), vec![link_argv(&["1", "2"])], "no init");
}

#[tokio::test]
async fn a_checked_out_branch_is_left_alone_and_reported() {
    let fixture = Fixture::new("checked-out", &[("a", "main"), ("b", "main")]);
    let wt = fixture.path("b-wt");
    git(
        &fixture.clone(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "b",
            wt.to_str().expect("utf-8"),
            "origin/b",
        ],
    );
    let gh = FakeGh::default();
    let outcome = run_stack_job(
        fixture.job(plan(
            vec![pull(1, "a", "main"), pull(2, "b", "main")],
            &[1, 2],
        )),
        &gh,
        &Progress::none(),
    )
    .await
    .expect("runs");
    let StackOutcome::Stacked(report) = outcome else {
        panic!("expected a stack, got {outcome:?}");
    };
    assert!(
        report.notes.iter().any(|note| matches!(
            note,
            LocalNote::LeftBehind { branch: b, reason } if b.as_str() == "b" && reason.contains("checked out")
        )),
        "{:?}",
        report.notes
    );
    // The worktree's branch did not move under it.
    assert_ne!(output(&wt, &["rev-parse", "HEAD"]), fixture.origin("b"));
}

#[tokio::test]
async fn a_missing_remote_branch_is_named() {
    let fixture = Fixture::new("missing", &[("a", "main")]);
    let gh = FakeGh::default();
    let err = run_stack_job(
        fixture.job(plan(
            vec![pull(1, "a", "main"), pull(2, "gone", "a")],
            &[1, 2],
        )),
        &gh,
        &Progress::none(),
    )
    .await
    .expect_err("fails");
    assert!(
        matches!(&err, StackOpError::MissingOnRemote { branch, number } if branch == "gone" && *number == PrNumber(2)),
        "{err:?}"
    );
}
