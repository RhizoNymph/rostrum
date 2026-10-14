//! `Repo::push_branch`, `Repo::branch_upstream` and `Repo::push_urls`
//! against a bare origin in a scratch directory — never a real remote.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use rostrum_git::{
    BranchName, BranchUpstream, Oid, PushMode, PushOutcome, PushRejection, Remote, RemoteRef, Repo,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// `main` and `feature` on a bare origin, cloned.
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rostrum-git-push-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch dir");
        let fixture = Self { root };
        let seed = fixture.root.join("seed");
        std::fs::create_dir_all(&seed).expect("seed dir");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        commit(&seed, "README", "one\n");
        git(&seed, &["branch", "feature"]);
        git(
            &fixture.root,
            &["clone", "-q", "--bare", "seed", "origin.git"],
        );
        git(&fixture.root, &["clone", "-q", "origin.git", "clone"]);
        identify(&fixture.clone());
        git(&fixture.clone(), &["checkout", "-q", "feature"]);
        fixture
    }

    fn clone(&self) -> PathBuf {
        self.root.join("clone")
    }

    fn origin_oid(&self, branch: &str) -> String {
        output(
            &self.root.join("origin.git"),
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    fn head(&self) -> Oid {
        Oid::parse(output(&self.clone(), &["rev-parse", "HEAD"])).expect("oid")
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

#[tokio::test]
async fn a_fast_forward_push_moves_the_remote_branch_and_its_tracking_ref() {
    let fixture = Fixture::new("ff");
    commit(&fixture.clone(), "a.txt", "a\n");
    let head = fixture.head();
    let repo = Repo::open(fixture.clone()).await.expect("opens");

    let outcome = repo
        .push_branch(
            &Remote::origin(),
            &branch("feature"),
            &head,
            &PushMode::FastForward,
        )
        .await
        .expect("pushes");
    assert_eq!(outcome, PushOutcome::Updated { forced: false });
    assert_eq!(fixture.origin_oid("feature"), head.as_str());
    assert_eq!(
        output(
            &fixture.clone(),
            &["rev-parse", "refs/remotes/origin/feature"]
        ),
        head.as_str(),
        "git updates the tracking ref as it pushes"
    );
}

/// The fast-forward form carries no force of any kind: a commit that would
/// drop the remote's history is refused by the remote.
#[tokio::test]
async fn a_fast_forward_push_never_rewrites() {
    let fixture = Fixture::new("noforce");
    commit(&fixture.clone(), "a.txt", "a\n");
    let first = fixture.head();
    let repo = Repo::open(fixture.clone()).await.expect("opens");
    repo.push_branch(
        &Remote::origin(),
        &branch("feature"),
        &first,
        &PushMode::FastForward,
    )
    .await
    .expect("pushes");

    git(&fixture.clone(), &["reset", "-q", "--hard", "HEAD~1"]);
    commit(&fixture.clone(), "b.txt", "b\n");
    let diverged = fixture.head();
    let outcome = repo
        .push_branch(
            &Remote::origin(),
            &branch("feature"),
            &diverged,
            &PushMode::FastForward,
        )
        .await
        .expect("a verdict");
    assert_eq!(
        outcome,
        PushOutcome::Rejected(PushRejection::NonFastForward)
    );
    assert_eq!(fixture.origin_oid("feature"), first.as_str());

    // The leased form rewrites, from exactly the oid it names.
    let outcome = repo
        .push_branch(
            &Remote::origin(),
            &branch("feature"),
            &diverged,
            &PushMode::Lease {
                expected: first.clone(),
            },
        )
        .await
        .expect("a verdict");
    assert_eq!(outcome, PushOutcome::Updated { forced: true });
    assert_eq!(fixture.origin_oid("feature"), diverged.as_str());
}

/// A user's `push.followTags` must not publish a tag alongside the branch.
#[tokio::test]
async fn a_push_publishes_the_one_branch_and_no_tags() {
    let fixture = Fixture::new("tags");
    git(&fixture.clone(), &["config", "push.followTags", "true"]);
    commit(&fixture.clone(), "a.txt", "a\n");
    git(&fixture.clone(), &["tag", "-a", "-m", "v", "v-local"]);
    let head = fixture.head();
    let repo = Repo::open(fixture.clone()).await.expect("opens");
    repo.push_branch(
        &Remote::origin(),
        &branch("feature"),
        &head,
        &PushMode::FastForward,
    )
    .await
    .expect("pushes");
    let tags = output(&fixture.root.join("origin.git"), &["tag", "--list"]);
    assert_eq!(tags, "", "no tag was pushed");
}

#[tokio::test]
async fn the_configured_upstream_is_read_as_a_remote_ref() {
    let fixture = Fixture::new("upstream");
    let repo = Repo::open(fixture.clone()).await.expect("opens");
    assert_eq!(
        repo.branch_upstream(&branch("feature"))
            .await
            .expect("reads"),
        BranchUpstream::Tracking(RemoteRef::origin(branch("feature")))
    );

    git(
        &fixture.clone(),
        &["branch", "-q", "--no-track", "loose", "main"],
    );
    assert_eq!(
        repo.branch_upstream(&branch("loose")).await.expect("reads"),
        BranchUpstream::None
    );

    git(
        &fixture.clone(),
        &["branch", "-q", "--track", "local-up", "main"],
    );
    assert!(matches!(
        repo.branch_upstream(&branch("local-up"))
            .await
            .expect("reads"),
        BranchUpstream::Unusual(_)
    ));

    assert_eq!(
        repo.branch_upstream(&branch("missing"))
            .await
            .expect("reads"),
        BranchUpstream::None
    );
}

#[tokio::test]
async fn push_urls_honour_pushurl() {
    let fixture = Fixture::new("urls");
    let repo = Repo::open(fixture.clone()).await.expect("opens");
    let urls = repo.push_urls(&Remote::origin()).await.expect("reads");
    assert_eq!(urls.len(), 1);
    assert!(urls[0].ends_with("origin.git"), "{urls:?}");

    git(
        &fixture.clone(),
        &[
            "config",
            "remote.origin.pushurl",
            "git@github.com:octo/repo.git",
        ],
    );
    let urls = repo.push_urls(&Remote::origin()).await.expect("reads");
    assert_eq!(urls, vec!["git@github.com:octo/repo.git".to_string()]);
}
