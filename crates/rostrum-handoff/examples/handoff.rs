//! Render a bundle from a made-up conflict, and optionally spawn a session.
//!
//!     cargo run -p rostrum-handoff --example handoff
//!     cargo run -p rostrum-handoff --example handoff -- --spawn <worktree> [command]
//!
//! Without `--spawn`, prints the rendered bundle and exits. With it, writes
//! the bundle under the cache directory and starts the tmux session exactly
//! as the app would, using `echo {context}; sleep 300` when no command is
//! given. Verify with `tmux ls`, run it a second time to see
//! `AlreadyRunning`, then `tmux kill-session -t =<session>`.
//!
//! The dummy repository is `example.org/rostrum.rs`, chosen so the session
//! name carries a `.` and the exact-match `has-session` is exercised.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use rostrum_core::{PrNumber, RepoId};
use rostrum_git::{
    BranchName, Caps, CommitList, CommitSummary, ConflictBody, ConflictContext, ConflictKind,
    ConflictRegion, ConflictedPath, Oid, StoppedOperation,
};
use rostrum_handoff::{DEFAULT_INSTRUCTIONS, Handoff, PrMeta, hand_off, render_bundle};

fn oid(seed: char) -> Oid {
    Oid::parse(std::iter::repeat_n(seed, 40).collect::<String>()).expect("40 hex digits")
}

fn commit(seed: char, subject: &str) -> CommitSummary {
    CommitSummary {
        oid: oid(seed),
        author: "Ada Lovelace".to_string(),
        date: "2026-09-21T10:00:00+00:00".to_string(),
        message: format!("{subject}\n\nLonger explanation of why.\n"),
    }
}

fn dummy_context() -> ConflictContext {
    ConflictContext {
        operation: StoppedOperation::Rebase {
            step: Some((2, 3)),
            applying: Some(commit('b', "feat: teach the parser about ranges")),
            onto: Some(oid('a')),
        },
        branch: BranchName::new("feat-ranges").expect("valid branch name"),
        target: "refs/remotes/origin/main".to_string(),
        head: oid('c'),
        paths: vec![ConflictedPath {
            path: "src/parser.rs".to_string(),
            kind: ConflictKind::BothModified,
            body: ConflictBody::Regions {
                regions: vec![ConflictRegion {
                    first_line: 10,
                    last_line: 16,
                    text: "fn parse() {\n<<<<<<< HEAD\n    old();\n=======\n    new();\n>>>>>>> bbbbbbbb (feat: teach the parser about ranges)\n}\n".to_string(),
                }],
                truncated: false,
            },
        }],
        branch_commits: CommitList {
            commits: vec![
                commit('b', "feat: teach the parser about ranges"),
                commit('d', "test: ranges"),
            ],
            total: 2,
        },
        target_commits: CommitList {
            commits: vec![commit('e', "refactor: split parse()")],
            total: 1,
        },
        git_message: "CONFLICT (content): Merge conflict in src/parser.rs\nerror: could not apply bbbbbbbb... feat: teach the parser about ranges".to_string(),
        caps: Caps::default(),
    }
}

fn dummy_pr() -> PrMeta {
    PrMeta {
        repo: RepoId::new("example.org", "rostrum.rs"),
        number: PrNumber(42),
        title: "Teach the parser about ranges".to_string(),
        url: "https://github.com/example.org/rostrum.rs/pull/42".to_string(),
        body: "Adds `a..b` to the grammar.\n\nCloses #41.".to_string(),
        head_ref: "feat-ranges".to_string(),
        base_ref: "main".to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let pr = dummy_pr();
    let context = dummy_context();

    match args.next().as_deref() {
        Some("--spawn") => {
            let worktree = PathBuf::from(args.next().context("--spawn needs a worktree path")?);
            let command = args
                .next()
                .unwrap_or_else(|| "echo {context}; sleep 300".to_string());
            let receipt = hand_off(&pr, &context, &worktree, &command, Duration::from_secs(10))
                .await
                .context("handing off")?;
            println!("session:  {}", receipt.session);
            println!("bundle:   {}", receipt.context_path.display());
            println!("spawned:  {:?}", receipt.spawned);
            println!("attach:   tmux attach -t ={}", receipt.session);
        }
        Some(other) => {
            anyhow::bail!("unknown argument `{other}`; expected `--spawn <worktree> [command]`")
        }
        None => {
            let text = render_bundle(&Handoff {
                pr: &pr,
                context: &context,
                worktree: std::path::Path::new("/home/u/src/rostrum.rs"),
                instructions: DEFAULT_INSTRUCTIONS,
            });
            print!("{text}");
        }
    }
    Ok(())
}
