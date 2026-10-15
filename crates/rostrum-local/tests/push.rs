//! `plan_push` and `run_push` against real repositories: a bare origin named
//! `octo/repo.git` and a bare fork `forker/repo.git`, both in a scratch
//! directory, never a real remote.

use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use rostrum_core::RepoId;
use rostrum_git::{BranchName, PushRejection, RemoteRef};
use rostrum_local::{
    ConfirmedRewrite, PushKind, PushPlan, PushResult, PushSkip, PushStep, PushTarget, Refresh,
    RewriteDecision, plan_push, plan_push_all, run_push,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// `octo/repo.git` with `main` and `feature`, a clone of it, and helpers to
/// move either side.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rostrum-local-push-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("octo")).expect("scratch dir");
        let fixture = Self { root };

        let seed = fixture.path("seed");
        std::fs::create_dir_all(&seed).expect("seed dir");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        commit(&seed, "README", "one\n");
        git(&seed, &["branch", "feature"]);
        git(&seed, &["branch", "other"]);
        git(
            &fixture.root,
            &["clone", "-q", "--bare", "seed", "octo/repo.git"],
        );
        git(&fixture.root, &["clone", "-q", "octo/repo.git", "clone"]);
        identify(&fixture.clone());
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn clone(&self) -> PathBuf {
        self.path("clone")
    }

    /// Check `branch` out in its own worktree, tracking `start`.
    fn worktree(&self, branch: &str, start: &str) -> PathBuf {
        let worktree = self.path(&format!("wt-{}", branch.replace('/', "-")));
        git(
            &self.clone(),
            &[
                "worktree",
                "add",
                "-q",
                "--track",
                "-b",
                branch,
                worktree.to_str().expect("utf-8"),
                start,
            ],
        );
        identify(&worktree);
        worktree
    }

    /// Advance `branch` on the bare repository `bare` by one commit.
    fn push_to(&self, bare: &str, branch: &str, file: &str) {
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
        commit(&pusher, file, &format!("{file} from elsewhere\n"));
        git(&pusher, &["push", "-q", "origin", branch]);
    }

    fn origin_oid(&self, branch: &str) -> String {
        self.oid_in("octo/repo.git", branch)
    }

    fn oid_in(&self, bare: &str, branch: &str) -> String {
        output(
            &self.path(bare),
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    fn target(&self, branch: &str) -> PushTarget {
        PushTarget {
            clone: self.clone(),
            branch: name(branch),
            head_repo: Some(RepoId::new("octo", "repo")),
        }
    }

    /// `feature` checked out with one unpushed commit, and one commit on the
    /// origin it has not seen: diverged, as after a local rebase.
    async fn diverged(&self) -> (PathBuf, ConfirmedRewrite) {
        let worktree = self.worktree("feature", "origin/feature");
        commit(&worktree, "local.txt", "local\n");
        self.push_to("octo/repo.git", "feature", "remote.txt");
        let plan = plan_push(&self.target("feature"), Refresh::Fetch)
            .await
            .expect("plans");
        let confirmation = plan.rewrite().expect("a diverged branch is a rewrite");
        assert_eq!(confirmation.expected.as_str(), self.origin_oid("feature"));
        (worktree, confirmation)
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

fn name(branch: &str) -> BranchName {
    BranchName::new(branch).expect("valid")
}

fn n(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("non-zero")
}

fn skip(plan: PushPlan) -> PushSkip {
    match plan {
        PushPlan::Skip(skip) => skip,
        PushPlan::Ready(planned) => panic!("expected a skip, got {planned:?}"),
    }
}

#[tokio::test]
async fn an_ahead_branch_is_fast_forwarded_without_confirmation() {
    let fixture = Fixture::new("ff");
    let worktree = fixture.worktree("feature", "origin/feature");
    commit(&worktree, "a.txt", "a\n");
    commit(&worktree, "b.txt", "b\n");
    let local = output(&worktree, &["rev-parse", "HEAD"]);

    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    let planned = plan.ready().expect("ready").clone();
    assert_eq!(planned.kind, PushKind::FastForward { ahead: n(2) });
    assert_eq!(planned.upstream, RemoteRef::origin(name("feature")));
    assert_eq!(planned.local.as_str(), local);
    assert_eq!(plan.rewrite(), None);

    let result = run_push(&fixture.target("feature"), None).await;
    assert_eq!(
        result,
        PushResult::Pushed {
            upstream: RemoteRef::origin(name("feature")),
            rewrote: false,
        }
    );
    assert_eq!(fixture.origin_oid("feature"), local);

    // git moved the tracking ref as it pushed, so the clone now says so
    // without another fetch.
    let after = plan_push(&fixture.target("feature"), Refresh::OnDisk)
        .await
        .expect("plans");
    assert_eq!(skip(after), PushSkip::UpToDate);
}

#[tokio::test]
async fn a_diverged_branch_is_not_pushed_without_confirmation() {
    let fixture = Fixture::new("unconfirmed");
    let (_, confirmation) = fixture.diverged().await;
    let before = fixture.origin_oid("feature");

    let result = run_push(&fixture.target("feature"), None).await;
    assert_eq!(
        result,
        PushResult::NotConfirmed {
            expected: confirmation.expected.clone()
        }
    );
    assert_eq!(
        fixture.origin_oid("feature"),
        before,
        "the remote is untouched"
    );

    // A confirmation for another branch is no confirmation.
    let elsewhere = ConfirmedRewrite {
        branch: name("other"),
        expected: confirmation.expected.clone(),
    };
    let result = run_push(&fixture.target("feature"), Some(&elsewhere)).await;
    assert!(
        matches!(result, PushResult::NotConfirmed { .. }),
        "{result:?}"
    );
    assert_eq!(fixture.origin_oid("feature"), before);
}

#[tokio::test]
async fn a_confirmed_rewrite_is_pushed_with_its_lease() {
    let fixture = Fixture::new("leased");
    let (worktree, confirmation) = fixture.diverged().await;
    let local = output(&worktree, &["rev-parse", "HEAD"]);

    let plan = plan_push(&fixture.target("feature"), Refresh::OnDisk)
        .await
        .expect("plans");
    assert!(matches!(
        plan.ready().map(|planned| &planned.kind),
        Some(PushKind::Rewrite { ahead, behind, .. }) if *ahead == n(1) && *behind == n(1)
    ));

    let result = run_push(&fixture.target("feature"), Some(&confirmation)).await;
    assert_eq!(
        result,
        PushResult::Pushed {
            upstream: RemoteRef::origin(name("feature")),
            rewrote: true,
        }
    );
    assert_eq!(fixture.origin_oid("feature"), local);
}

/// The remote moved after the user confirmed and rostrum has not fetched
/// since: the lease is what refuses, and nothing is overwritten.
#[tokio::test]
async fn a_stale_lease_is_rejected_and_the_remote_is_unchanged() {
    let fixture = Fixture::new("stale");
    let (_, confirmation) = fixture.diverged().await;
    fixture.push_to("octo/repo.git", "feature", "later.txt");
    let moved = fixture.origin_oid("feature");
    assert_ne!(moved, confirmation.expected.as_str());

    let result = run_push(&fixture.target("feature"), Some(&confirmation)).await;
    assert_eq!(result, PushResult::Rejected(PushRejection::StaleLease));
    assert_eq!(
        fixture.origin_oid("feature"),
        moved,
        "nothing was overwritten"
    );
}

/// The remote moved and the clone has fetched it: the confirmation names an
/// oid that is no longer the one a rewrite would replace, so nothing runs.
#[tokio::test]
async fn a_confirmation_for_another_oid_is_refused_before_pushing() {
    let fixture = Fixture::new("mismatch");
    let (_, confirmation) = fixture.diverged().await;
    fixture.push_to("octo/repo.git", "feature", "later.txt");
    let moved = fixture.origin_oid("feature");
    // Re-planning with a fetch is what a refreshed panel does.
    let replanned = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(
        replanned.rewrite().map(|c| c.expected.to_string()),
        Some(moved.clone())
    );

    let result = run_push(&fixture.target("feature"), Some(&confirmation)).await;
    let PushResult::ConfirmationStale { confirmed, current } = result else {
        panic!("expected a stale confirmation, got {result:?}");
    };
    assert_eq!(confirmed, confirmation.expected);
    assert_eq!(current.as_str(), moved);
    assert_eq!(fixture.origin_oid("feature"), moved);
}

#[tokio::test]
async fn a_branch_matching_its_upstream_is_up_to_date() {
    let fixture = Fixture::new("current");
    fixture.worktree("feature", "origin/feature");
    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(skip(plan), PushSkip::UpToDate);
    assert_eq!(
        run_push(&fixture.target("feature"), None).await,
        PushResult::Skipped(PushSkip::UpToDate)
    );
}

#[tokio::test]
async fn a_branch_only_behind_is_told_to_pull() {
    let fixture = Fixture::new("behind");
    fixture.worktree("feature", "origin/feature");
    fixture.push_to("octo/repo.git", "feature", "remote.txt");
    let before = fixture.origin_oid("feature");

    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(skip(plan), PushSkip::Behind { behind: n(1) });
    assert_eq!(
        run_push(&fixture.target("feature"), None).await,
        PushResult::Skipped(PushSkip::Behind { behind: n(1) })
    );
    assert_eq!(fixture.origin_oid("feature"), before);
}

#[tokio::test]
async fn a_branch_with_no_upstream_is_not_pushed() {
    let fixture = Fixture::new("noupstream");
    let worktree = fixture.path("wt-loose");
    git(
        &fixture.clone(),
        &[
            "worktree",
            "add",
            "-q",
            "--no-track",
            "-b",
            "loose",
            worktree.to_str().expect("utf-8"),
            "origin/main",
        ],
    );
    identify(&worktree);
    commit(&worktree, "x.txt", "x\n");
    let plan = plan_push(&fixture.target("loose"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(skip(plan), PushSkip::NoUpstream);
    assert_eq!(
        run_push(&fixture.target("loose"), None).await,
        PushResult::Skipped(PushSkip::NoUpstream)
    );
    assert!(
        Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "refs/heads/loose"])
            .current_dir(fixture.path("octo/repo.git"))
            .status()
            .expect("git runs")
            .code()
            == Some(1),
        "nothing was created on the remote"
    );
}

/// The PR's head is `feature`, but the local `feature` tracks `other`: it is
/// not the pull request's branch on the remote, so it is not pushed there.
#[tokio::test]
async fn a_branch_tracking_another_remote_branch_is_not_pushed() {
    let fixture = Fixture::new("elsewhere");
    let worktree = fixture.worktree("feature", "origin/other");
    commit(&worktree, "x.txt", "x\n");
    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(
        skip(plan),
        PushSkip::UpstreamIsNotHead {
            upstream: RemoteRef::origin(name("other"))
        }
    );
}

#[tokio::test]
async fn a_configured_upstream_never_fetched_has_nothing_to_compare_with() {
    let fixture = Fixture::new("unfetched");
    let worktree = fixture.worktree("feature", "origin/feature");
    commit(&worktree, "x.txt", "x\n");
    git(
        &fixture.clone(),
        &["update-ref", "-d", "refs/remotes/origin/feature"],
    );
    let plan = plan_push(&fixture.target("feature"), Refresh::OnDisk)
        .await
        .expect("plans");
    assert_eq!(
        skip(plan),
        PushSkip::NotFetched {
            upstream: RemoteRef::origin(name("feature"))
        }
    );
}

#[tokio::test]
async fn a_dirty_worktree_is_refused_and_nothing_is_pushed() {
    let fixture = Fixture::new("dirty");
    let worktree = fixture.worktree("feature", "origin/feature");
    commit(&worktree, "x.txt", "x\n");
    std::fs::write(worktree.join("README"), "edited, not committed\n").expect("write");
    let before = fixture.origin_oid("feature");

    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    let PushSkip::Blocked(reason) = skip(plan) else {
        panic!("expected a blocker");
    };
    assert!(reason.contains("uncommitted"), "{reason}");
    assert!(matches!(
        run_push(&fixture.target("feature"), None).await,
        PushResult::Skipped(PushSkip::Blocked(_))
    ));
    assert_eq!(fixture.origin_oid("feature"), before);
}

#[tokio::test]
async fn a_worktree_mid_rebase_is_refused() {
    let fixture = Fixture::new("midrebase");
    let worktree = fixture.worktree("feature", "origin/feature");
    commit(&worktree, "README", "feature side\n");
    fixture.push_to("octo/repo.git", "main", "README");
    git(&worktree, &["fetch", "-q", "origin"]);
    let status = Command::new("git")
        .args(["rebase", "origin/main"])
        .current_dir(&worktree)
        .output()
        .expect("git runs");
    assert!(!status.status.success(), "the rebase stops on a conflict");

    let plan = plan_push(&fixture.target("feature"), Refresh::OnDisk)
        .await
        .expect("plans");
    // Mid-rebase HEAD is detached, so no worktree has `feature` checked out
    // by name — either way, nothing is offered.
    assert!(
        matches!(skip(plan), PushSkip::NotCheckedOut | PushSkip::Blocked(_)),
        "a worktree mid-rebase is never pushed"
    );
    git(&worktree, &["rebase", "--abort"]);
}

#[tokio::test]
async fn a_branch_not_checked_out_is_not_pushed() {
    let fixture = Fixture::new("absent");
    let plan = plan_push(&fixture.target("feature"), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(skip(plan), PushSkip::NotCheckedOut);
}

/// A fork's branch goes to the fork, and only when its upstream remote is the
/// pull request's head repository.
#[tokio::test]
async fn a_fork_branch_is_pushed_only_to_its_head_repository() {
    let fixture = Fixture::new("fork");
    std::fs::create_dir_all(fixture.path("forker")).expect("dir");
    git(
        &fixture.root,
        &["clone", "-q", "--bare", "seed", "forker/repo.git"],
    );
    let fork_url = fixture.path("forker/repo.git");
    git(
        &fixture.clone(),
        &["remote", "add", "fork", fork_url.to_str().expect("utf-8")],
    );
    git(&fixture.clone(), &["fetch", "-q", "fork"]);
    let worktree = fixture.worktree("feature", "fork/feature");
    commit(&worktree, "x.txt", "x\n");
    let local = output(&worktree, &["rev-parse", "HEAD"]);
    let origin_before = fixture.origin_oid("feature");

    let fork_target = |head_repo: Option<RepoId>| PushTarget {
        clone: fixture.clone(),
        branch: name("feature"),
        head_repo,
    };

    // The pull request's head is in the base repository, but the branch
    // tracks the fork: not pushed anywhere.
    let plan = plan_push(
        &fork_target(Some(RepoId::new("octo", "repo"))),
        Refresh::Fetch,
    )
    .await
    .expect("plans");
    assert!(
        matches!(skip(plan), PushSkip::RemoteIsNotHeadRepository { ref remote, .. } if remote.as_str() == "fork")
    );

    // GitHub did not say where the fork lives: not pushed.
    let plan = plan_push(&fork_target(None), Refresh::Fetch)
        .await
        .expect("plans");
    assert_eq!(skip(plan), PushSkip::HeadRepositoryUnknown);

    // The head repository is the fork the branch tracks: pushed there, and
    // only there.
    let target = fork_target(Some(RepoId::new("Forker", "Repo")));
    let result = run_push(&target, None).await;
    assert_eq!(
        result,
        PushResult::Pushed {
            upstream: RemoteRef::new(
                rostrum_git::Remote::new("fork").expect("valid"),
                name("feature")
            ),
            rewrote: false,
        }
    );
    assert_eq!(fixture.oid_in("forker/repo.git", "feature"), local);
    assert_eq!(fixture.origin_oid("feature"), origin_before);
}

/// Push all over a mixed batch: the confirmation lists only the rewrite;
/// confirming pushes the fast-forward and the rewrite and runs nothing for
/// the rest.
#[tokio::test]
async fn push_all_confirms_the_rewrites_once_and_pushes_the_batch() {
    let fixture = Fixture::new("all");
    // `feature`: diverged. `other`: ahead. `main`: up to date (the clone's
    // own checkout). `ghost`: not checked out.
    let (feature_wt, _) = fixture.diverged().await;
    let other_wt = fixture.worktree("other", "origin/other");
    commit(&other_wt, "o.txt", "o\n");
    let feature_local = output(&feature_wt, &["rev-parse", "HEAD"]);
    let other_local = output(&other_wt, &["rev-parse", "HEAD"]);

    let plan = plan_push_all(vec![
        (1, fixture.target("feature")),
        (2, fixture.target("other")),
        (3, fixture.target("main")),
        (4, fixture.target("ghost")),
    ])
    .await;
    let listed: Vec<(u32, String)> = plan
        .rewrites()
        .into_iter()
        .map(|(key, planned)| (*key, planned.branch.to_string()))
        .collect();
    assert_eq!(listed, vec![(1, "feature".to_string())]);

    let mut results = Vec::new();
    for step in plan.authorize(RewriteDecision::RewriteListed) {
        let (key, result) = match step {
            PushStep::Run {
                key,
                target,
                confirmation,
            } => (key, run_push(&target, confirmation.as_ref()).await),
            PushStep::Settled { key, result } => (key, result),
        };
        results.push((key, result));
    }
    assert!(matches!(
        results[0],
        (1, PushResult::Pushed { rewrote: true, .. })
    ));
    assert!(matches!(
        results[1],
        (2, PushResult::Pushed { rewrote: false, .. })
    ));
    assert_eq!(results[2], (3, PushResult::Skipped(PushSkip::UpToDate)));
    assert_eq!(
        results[3],
        (4, PushResult::Skipped(PushSkip::NotCheckedOut))
    );
    assert_eq!(fixture.origin_oid("feature"), feature_local);
    assert_eq!(fixture.origin_oid("other"), other_local);
}

/// Declining the rewrites in a batch leaves the diverged branch's remote
/// alone and still pushes the fast-forward.
#[tokio::test]
async fn push_all_without_rewrites_pushes_only_fast_forwards() {
    let fixture = Fixture::new("allff");
    let (_, confirmation) = fixture.diverged().await;
    let other_wt = fixture.worktree("other", "origin/other");
    commit(&other_wt, "o.txt", "o\n");
    let other_local = output(&other_wt, &["rev-parse", "HEAD"]);
    let feature_before = fixture.origin_oid("feature");

    let plan = plan_push_all(vec![
        (1, fixture.target("feature")),
        (2, fixture.target("other")),
    ])
    .await;
    let mut results = Vec::new();
    for step in plan.authorize(RewriteDecision::FastForwardsOnly) {
        results.push(match step {
            PushStep::Run {
                target,
                confirmation,
                ..
            } => run_push(&target, confirmation.as_ref()).await,
            PushStep::Settled { result, .. } => result,
        });
    }
    assert_eq!(
        results[0],
        PushResult::NotConfirmed {
            expected: confirmation.expected
        }
    );
    assert!(matches!(
        results[1],
        PushResult::Pushed { rewrote: false, .. }
    ));
    assert_eq!(fixture.origin_oid("feature"), feature_before);
    assert_eq!(fixture.origin_oid("other"), other_local);
}
