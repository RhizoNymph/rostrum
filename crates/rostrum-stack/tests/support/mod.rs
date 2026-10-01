//! Fixtures shared by the stack job tests: a recording `gh` double and
//! scratch repositories with a bare origin.

#![allow(dead_code)]

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
use rostrum_git::BranchName;
use rostrum_stack::{GhOutput, GhRunner, GhStackCommand, StackJob, StackOpError};

pub static COUNTER: AtomicUsize = AtomicUsize::new(0);

// --- the gh double ----------------------------------------------------------

/// Records every command; answers `view --json` from the last `init`.
#[derive(Default)]
pub struct FakeGh {
    pub calls: Mutex<Vec<(PathBuf, RepoId, GhStackCommand)>>,
    pub fail_link: Mutex<bool>,
}

impl FakeGh {
    pub fn failing_link() -> Self {
        Self {
            fail_link: Mutex::new(true),
            ..Self::default()
        }
    }

    pub fn commands(&self) -> Vec<GhStackCommand> {
        self.calls
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, _, c)| c.clone())
            .collect()
    }

    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.commands().iter().map(GhStackCommand::argv).collect()
    }
}

pub fn ok(stdout: &str) -> GhOutput {
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
            GhStackCommand::Link { .. } | GhStackCommand::LinkExtend { .. }
                if *self.fail_link.lock().expect("lock") =>
            {
                GhOutput {
                    success: false,
                    code: Some(1),
                    stdout: String::new(),
                    stderr: "✗ failed to create stack: boom".into(),
                }
            }
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

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    /// `main`, plus one branch per `(name, parent)` with one commit adding
    /// `<name>.txt`, pushed to a bare origin and cloned.
    pub fn new(tag: &str, branches: &[(&str, &str)]) -> Self {
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

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn clone(&self) -> PathBuf {
        self.path("clone")
    }

    pub fn scratch(&self) -> PathBuf {
        self.path("scratch")
    }

    pub fn origin(&self, rev: &str) -> String {
        output(&self.path("origin.git"), &["rev-parse", rev])
    }

    pub fn origin_is_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        Command::new("git")
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .current_dir(self.path("origin.git"))
            .status()
            .expect("git runs")
            .success()
    }

    pub fn worktree_count(&self) -> usize {
        output(&self.clone(), &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    /// Commit `text` to `file` on `branch` in a separate clone of `bare`, and
    /// push it there.
    pub fn advance(&self, bare: &str, branch: &str, file: &str, text: &str) {
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

    pub fn job(&self, plan: StackPlan) -> StackJob {
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

pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

pub fn output(dir: &Path, args: &[&str]) -> String {
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

pub fn identify(dir: &Path) {
    git(dir, &["config", "user.name", "Rostrum Test"]);
    git(dir, &["config", "user.email", "test@rostrum.invalid"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

pub fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).expect("write");
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

// --- plans --------------------------------------------------------------------

pub fn repo_id() -> RepoId {
    RepoId::new("octo", "repo")
}

pub fn pull(number: u32, head: &str, base: &str) -> PullRequest {
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

pub fn plan(prs: Vec<PullRequest>, order: &[u32]) -> StackPlan {
    let state = RepoState {
        prs,
        load: LoadState::Idle,
        ..RepoState::new(repo_id())
    };
    let order: Vec<PrNumber> = order.iter().copied().map(PrNumber).collect();
    plan_stack(&state, &order, RefName::new("main").expect("valid")).expect("valid plan")
}

pub fn branch(name: &str) -> BranchName {
    BranchName::new(name).expect("valid")
}

pub fn link_argv(numbers: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["stack", "link", "--base", "main"]
        .into_iter()
        .map(String::from)
        .collect();
    argv.extend(numbers.iter().map(|n| n.to_string()));
    argv
}

pub fn init_argv(branches: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["stack", "init", "--base", "main", "--"]
        .into_iter()
        .map(String::from)
        .collect();
    argv.extend(branches.iter().map(|b| b.to_string()));
    argv
}

pub fn view_argv() -> Vec<String> {
    vec!["stack".into(), "view".into(), "--json".into()]
}
