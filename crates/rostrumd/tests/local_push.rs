//! Push and Push all over the real API: a scratch origin named
//! `owner/repo.git`, a clone with `feature` in its own worktree, driven from
//! `RemoteClient` through the daemon into `rostrum-local` and git. Nothing
//! here reaches a real remote.

mod common;

use std::path::{Path, PathBuf};

use common::{Harness, git};
use rostrum_core::{PrNumber, RepoId};
use rostrum_remote::{
    ApiErrorCode, LeaseConfirmation, PrKey, PushOutcomeView, PushPlanRequest, PushPlanView,
    PushRequest, PushTargetRef,
    client::{ClientError, RemoteClient},
};
use rostrumd::fsutil::ScratchDir;

struct Repos {
    scratch: ScratchDir,
}

impl Repos {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let root = scratch.path().to_path_buf();
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).expect("seed");
        std::fs::create_dir_all(root.join("owner")).expect("owner dir");
        git(&seed, &["init", "-q", "-b", "main"]);
        identify(&seed);
        commit(&seed, "README", "one\n");
        git(&seed, &["branch", "feature"]);
        git(&root, &["clone", "-q", "--bare", "seed", "owner/repo.git"]);
        git(&root, &["clone", "-q", "owner/repo.git", "clone"]);
        identify(&root.join("clone"));
        let repos = Self { scratch };
        git(
            &repos.clone_path(),
            &[
                "worktree",
                "add",
                "-q",
                "--track",
                "-b",
                "feature",
                repos.worktree().to_str().expect("utf-8"),
                "origin/feature",
            ],
        );
        identify(&repos.worktree());
        repos
    }

    fn clone_path(&self) -> PathBuf {
        self.scratch.join("clone")
    }

    fn worktree(&self) -> PathBuf {
        self.scratch.join("feature-wt")
    }

    fn origin_oid(&self, branch: &str) -> String {
        output(
            &self.scratch.join("owner/repo.git"),
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    fn local_oid(&self) -> String {
        output(&self.worktree(), &["rev-parse", "HEAD"])
    }

    fn push_to_origin(&self, branch: &str, file: &str) {
        let pusher = self.scratch.join(&format!("pusher-{branch}-{file}"));
        git(
            self.scratch.path(),
            &[
                "clone",
                "-q",
                "-b",
                branch,
                "owner/repo.git",
                pusher.to_str().expect("utf-8"),
            ],
        );
        identify(&pusher);
        commit(&pusher, file, "from elsewhere\n");
        git(&pusher, &["push", "-q", "origin", branch]);
    }

    fn config(&self) -> serde_json::Value {
        serde_json::json!({
            "clones": { "owner/repo": self.clone_path().display().to_string() }
        })
    }
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

fn output(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
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

fn key(repo: &str) -> PrKey {
    PrKey {
        repo: repo.parse().expect("repo"),
        number: PrNumber(7),
    }
}

fn target(head: &str) -> PushTargetRef {
    PushTargetRef {
        key: key("owner/repo"),
        head_ref: head.into(),
        head_repo: Some(RepoId::new("owner", "repo")),
    }
}

async fn push(client: &RemoteClient, confirm_rewrite: Vec<LeaseConfirmation>) -> PushOutcomeView {
    let mut answer = client
        .push(&PushRequest {
            prs: vec![target("feature")],
            confirm_rewrite,
        })
        .await
        .expect("push answers");
    assert_eq!(answer.entries.len(), 1);
    answer.entries.remove(0).outcome
}

#[tokio::test]
async fn an_ahead_branch_is_pushed_from_the_phone_without_confirmation() {
    let repos = Repos::new("it-push-ff");
    commit(&repos.worktree(), "a.txt", "a\n");
    let harness = Harness::start("it-push-ff-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;

    let mut elsewhere = target("feature");
    elsewhere.key = key("not/configured");
    let plan = client
        .plan_push(&PushPlanRequest {
            prs: vec![target("feature"), elsewhere],
        })
        .await
        .expect("plans");
    assert!(
        matches!(
            plan.entries[0].plan,
            PushPlanView::FastForward { ahead: 1, .. }
        ),
        "{plan:?}"
    );
    assert_eq!(plan.entries[1].plan, PushPlanView::NotConfigured);
    assert!(plan.confirm_rewrite().is_empty());

    assert_eq!(
        push(&client, Vec::new()).await,
        PushOutcomeView::Pushed {
            upstream: "origin/feature".into(),
            rewrote: false,
        }
    );
    assert_eq!(repos.origin_oid("feature"), repos.local_oid());
    harness.stop().await;
}

#[tokio::test]
async fn a_diverged_branch_is_rewritten_only_with_its_exact_confirmation() {
    let repos = Repos::new("it-push-rewrite");
    commit(&repos.worktree(), "local.txt", "local\n");
    repos.push_to_origin("feature", "remote.txt");
    let remote = repos.origin_oid("feature");
    let harness = Harness::start("it-push-rewrite-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;

    let plan = client
        .plan_push(&PushPlanRequest {
            prs: vec![target("feature")],
        })
        .await
        .expect("plans");
    let PushPlanView::Rewrite {
        expected, behind, ..
    } = &plan.entries[0].plan
    else {
        panic!("expected a rewrite, got {plan:?}");
    };
    assert_eq!(expected, &remote);
    assert_eq!(*behind, 1);

    // No confirmation: nothing is pushed.
    assert_eq!(
        push(&client, Vec::new()).await,
        PushOutcomeView::NotConfirmed {
            expected: remote.clone()
        }
    );
    assert_eq!(repos.origin_oid("feature"), remote);

    // A confirmation for some other oid: refused before pushing.
    let wrong = LeaseConfirmation {
        key: key("owner/repo"),
        expected: "0123456789abcdef0123456789abcdef01234567".into(),
    };
    assert!(matches!(
        push(&client, vec![wrong]).await,
        PushOutcomeView::ConfirmationStale { .. }
    ));
    assert_eq!(repos.origin_oid("feature"), remote);

    // Exactly what the dry run showed: rewritten, with the lease.
    assert_eq!(
        push(&client, plan.confirm_rewrite()).await,
        PushOutcomeView::Pushed {
            upstream: "origin/feature".into(),
            rewrote: true,
        }
    );
    assert_eq!(repos.origin_oid("feature"), repos.local_oid());
    harness.stop().await;
}

#[tokio::test]
async fn a_confirmation_that_cannot_be_an_oid_is_refused_outright() {
    let repos = Repos::new("it-push-bad");
    let harness = Harness::start("it-push-bad-daemon", None, Some(repos.config())).await;
    let (client, _) = harness.pair("phone").await;
    let err = client
        .push(&PushRequest {
            prs: vec![target("feature")],
            confirm_rewrite: vec![LeaseConfirmation {
                key: key("owner/repo"),
                expected: "HEAD".into(),
            }],
        })
        .await
        .expect_err("not an oid");
    assert!(
        matches!(&err, ClientError::Api(error) if error.code == ApiErrorCode::BadRequest),
        "{err:?}"
    );
    let err = client
        .push(&PushRequest {
            prs: vec![target("--force")],
            confirm_rewrite: Vec::new(),
        })
        .await
        .expect_err("not a branch");
    assert!(
        matches!(&err, ClientError::Api(error) if error.code == ApiErrorCode::BadRequest),
        "{err:?}"
    );
    harness.stop().await;
}
