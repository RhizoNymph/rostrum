//! Read-only smoke test against a real repository.
//!
//!     cargo run -p rostrum-git --example inspect -- <path> [branch] [base]
//!
//! Prints the status rostrum would read, the branch's divergence from its
//! configured upstream, and its divergence from `origin/<base>`. Nothing here
//! writes: cross-check the numbers against `git status -sb` and
//! `git rev-list --left-right --count`.

use anyhow::{Context, Result};
use rostrum_git::{Autostash, BranchName, Head, Operation, Remote, RemoteRef, Repo, Rev};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| ".".to_string());
    let branch_arg = args.next();
    let base_arg = args.next().unwrap_or_else(|| "main".to_string());

    let repo = Repo::open(&path)
        .await
        .with_context(|| format!("opening `{path}`"))?;
    println!("root:       {}", repo.root().display());
    println!("git dir:    {}", repo.git_dir().display());
    println!("common dir: {}", repo.common_dir().display());

    // Cross-check against `git worktree list`.
    println!("\nworktrees:");
    for entry in repo.worktrees().await? {
        let head = entry
            .head
            .as_ref()
            .map(|oid| oid.short().to_string())
            .unwrap_or_else(|| "-".to_string());
        let what = match (&entry.branch, entry.bare, entry.detached) {
            (Some(branch), _, _) => format!("[{branch}]"),
            (None, true, _) => "(bare)".to_string(),
            (None, _, true) => "(detached)".to_string(),
            (None, false, false) => "(?)".to_string(),
        };
        println!("  {} {head} {what}", entry.path.display());
    }

    let status = repo.status().await?;
    match &status.head {
        Head::Branch {
            name,
            oid,
            upstream,
        } => {
            println!("\nHEAD:       {name} @ {}", oid.short());
            match upstream {
                Some(upstream) => println!(
                    "upstream:   {} ({})",
                    upstream.name,
                    match upstream.divergence {
                        Some(divergence) => format!(
                            "ahead {}, behind {} -> {:?}",
                            divergence.ahead,
                            divergence.behind,
                            divergence.relation()
                        ),
                        // Configured, but its tracking ref has never been
                        // fetched into this clone.
                        None => "counts unknown".to_string(),
                    }
                ),
                None => println!("upstream:   (none)"),
            }
        }
        Head::Detached { oid } => println!("\nHEAD:       detached at {}", oid.short()),
        Head::Unborn { name } => println!("\nHEAD:       {name} (no commits yet)"),
    }

    println!(
        "worktree:   {} staged, {} unstaged, {} untracked, {} conflicted",
        status.worktree.staged,
        status.worktree.unstaged,
        status.worktree.untracked,
        status.worktree.conflicted
    );
    println!("in progress: {:?}", status.in_progress);

    let branch = match branch_arg {
        Some(name) => BranchName::new(name)?,
        None => status
            .branch()
            .cloned()
            .context("HEAD is not on a branch; pass one on the command line")?,
    };
    let base = RemoteRef::new(Remote::origin(), BranchName::new(base_arg)?);

    let local = Rev::Local(branch.clone());
    let tracking = Rev::Remote(base.clone());
    println!(
        "\n{} exists: {}",
        local.as_arg(),
        repo.ref_exists(&local).await?
    );
    println!(
        "{} exists: {}",
        tracking.as_arg(),
        repo.ref_exists(&tracking).await?
    );

    let divergence = repo.divergence(&local, &tracking).await?;
    println!(
        "\n{} vs {}: ahead {}, behind {} -> {:?}",
        local.as_arg(),
        tracking.as_arg(),
        divergence.ahead,
        divergence.behind,
        divergence.relation()
    );
    println!(
        "  fast-forwards: {}, behind: {}, identical: {}",
        divergence.fast_forwards(),
        divergence.is_behind(),
        divergence.is_identical()
    );

    for operation in [Operation::Fetch, Operation::Rebase, Operation::Merge] {
        for autostash in [Autostash::Disabled, Autostash::Enabled] {
            let preflight = repo.preflight(operation, Some(&branch), autostash).await?;
            println!(
                "\npreflight {operation} ({autostash:?}): {}",
                preflight.reason().unwrap_or_else(|| "clear".to_string())
            );
        }
    }

    println!("\nfetch refspec would be: {}", base.fetch_refspec());
    Ok(())
}
