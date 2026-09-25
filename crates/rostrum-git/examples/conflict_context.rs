//! Live check of [`Repo::conflict_context`] against a real stopped rebase.
//!
//!     cargo run -p rostrum-git --example conflict_context -- <worktree> <branch> <base>
//!     cargo run -p rostrum-git --example conflict_context -- --setup <dir>
//!
//! The first form describes whatever is stopped in `<worktree>` and prints the
//! context. It writes nothing.
//!
//! The second builds a throwaway repository under `<dir>` — which must be
//! inside the system temporary directory — with two branches that conflict,
//! starts a rebase through the crate's own [`Repo::rebase_onto`] under
//! [`ConflictPolicy::Leave`], prints the context, and then aborts the rebase and
//! removes the directory. It is the check for three claims the crate makes:
//! that `refs/heads/<branch>` is unmoved mid-rebase, that `REBASE_HEAD`
//! resolves to the commit being applied, and that the region line numbers
//! match the file on disk.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail, ensure};
use rostrum_git::{
    AbortTarget, Autostash, BranchName, ConflictBody, ConflictContext, ConflictPolicy, Outcome,
    Remote, RemoteRef, Repo,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, dir] if flag == "--setup" => setup_and_describe(Path::new(dir)).await,
        [worktree, branch, base] => {
            let repo = Repo::open(worktree)
                .await
                .with_context(|| format!("opening `{worktree}`"))?;
            let branch = BranchName::new(branch.as_str())?;
            let base = RemoteRef::new(Remote::origin(), BranchName::new(base.as_str())?);
            let context = repo.conflict_context(&branch, &base).await?;
            print_context(&context);
            Ok(())
        }
        _ => bail!("usage: conflict_context <worktree> <branch> <base> | --setup <dir>"),
    }
}

/// The file both branches edit. Line 5 is the collision; line 8 is a second,
/// non-conflicting change on the branch so the rebase has two steps.
fn numbered(lines: [&str; 10]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

async fn setup_and_describe(dir: &Path) -> Result<()> {
    let temp = std::env::temp_dir();
    ensure!(
        dir.starts_with(&temp),
        "`{}` is not under the temporary directory `{}`; refusing to write there",
        dir.display(),
        temp.display()
    );
    ensure!(
        !dir.exists(),
        "`{}` already exists; pass a fresh directory",
        dir.display()
    );
    let work = dir.join("work");
    std::fs::create_dir_all(&work)?;

    let result = build_and_describe(&work).await;
    // Whatever happened, leave nothing behind.
    if let Err(error) = std::fs::remove_dir_all(dir) {
        eprintln!("could not remove `{}`: {error}", dir.display());
    }
    result
}

async fn build_and_describe(work: &Path) -> Result<()> {
    let base = [
        "line 1", "line 2", "line 3", "line 4", "line 5", "line 6", "line 7", "line 8", "line 9",
        "line 10",
    ];
    let on_feature_1 = {
        let mut lines = base;
        lines[4] = "line 5 (feature)";
        lines
    };
    let on_feature_2 = {
        let mut lines = on_feature_1;
        lines[7] = "line 8 (feature)";
        lines
    };
    let on_main = {
        let mut lines = base;
        lines[4] = "line 5 (main)";
        lines
    };

    git(work, &["init", "-q", "-b", "main"])?;
    write_and_commit(work, &numbered(base), "base: ten lines")?;
    git(work, &["checkout", "-q", "-b", "feature"])?;
    write_and_commit(
        work,
        &numbered(on_feature_1),
        "feature: change line 5\n\nThis is the commit that will conflict.",
    )?;
    write_and_commit(work, &numbered(on_feature_2), "feature: change line 8")?;
    git(work, &["checkout", "-q", "main"])?;
    write_and_commit(work, &numbered(on_main), "main: change line 5 differently")?;
    // The tracking ref the crate rebases onto, without a network.
    git(
        work,
        &["update-ref", "refs/remotes/origin/main", "refs/heads/main"],
    )?;
    git(work, &["checkout", "-q", "feature"])?;

    let feature = BranchName::new("feature")?;
    let origin_main = RemoteRef::new(Remote::origin(), BranchName::new("main")?);
    let tip_before = git(work, &["rev-parse", "refs/heads/feature"])?;
    println!("refs/heads/feature before: {tip_before}");

    let repo = Repo::open(work)
        .await?
        .with_conflict_policy(ConflictPolicy::Leave);
    let outcome = repo
        .rebase_onto(&feature, &origin_main, Autostash::Disabled)
        .await?;
    println!("outcome: {outcome:?}\n");
    let Outcome::Conflicted(conflict) = &outcome else {
        bail!("expected a conflict, got {outcome:?}");
    };
    ensure!(
        conflict.abort_target() == Some(AbortTarget::Rebase),
        "the conflict should have been left in place"
    );

    let tip_after = git(work, &["rev-parse", "refs/heads/feature"])?;
    println!("refs/heads/feature mid-rebase: {tip_after}");
    ensure!(tip_before == tip_after, "the branch ref moved mid-rebase");
    println!("REBASE_HEAD: {}", git(work, &["rev-parse", "REBASE_HEAD"])?);
    println!("\nf.txt on disk:");
    for (index, line) in std::fs::read_to_string(work.join("f.txt"))?
        .lines()
        .enumerate()
    {
        println!("  {:>3}  {line}", index + 1);
    }

    let mut context = repo.conflict_context(&feature, &origin_main).await?;
    context.git_message = conflict.message().to_string();
    println!();
    print_context(&context);

    repo.abort(AbortTarget::Rebase).await?;
    println!(
        "\naborted; refs/heads/feature now: {}",
        git(work, &["rev-parse", "refs/heads/feature"])?
    );
    Ok(())
}

fn write_and_commit(work: &Path, contents: &str, message: &str) -> Result<()> {
    std::fs::write(work.join("f.txt"), contents)?;
    git(work, &["add", "f.txt"])?;
    git(work, &["commit", "-q", "-m", message])?;
    Ok(())
}

/// Scaffolding only: the crate deliberately has no `commit`, so the fixture is
/// built with plain git.
fn git(work: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(work)
        .args([
            "-c",
            "user.name=rostrum",
            "-c",
            "user.email=rostrum@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn print_context(context: &ConflictContext) {
    println!("operation:      {:#?}", context.operation);
    println!("branch:         {}", context.branch);
    println!("target:         {}", context.target);
    println!("head:           {}", context.head);
    println!("is_rebase:      {}", context.is_rebase());
    println!("marker sides:   {:?}", context.marker_sides());
    println!("commands:       {:#?}", context.commands());
    println!(
        "\nbranch commits ({} of {}, truncated: {}):",
        context.branch_commits.commits.len(),
        context.branch_commits.total,
        context.branch_commits.truncated()
    );
    for commit in &context.branch_commits.commits {
        println!(
            "  {} {} <{}> {}",
            commit.oid.short(),
            commit.date,
            commit.author,
            commit.subject()
        );
    }
    println!(
        "target commits ({} of {}, truncated: {}):",
        context.target_commits.commits.len(),
        context.target_commits.total,
        context.target_commits.truncated()
    );
    for commit in &context.target_commits.commits {
        println!(
            "  {} {} <{}> {}",
            commit.oid.short(),
            commit.date,
            commit.author,
            commit.subject()
        );
    }
    println!("\npaths:");
    for path in &context.paths {
        println!("  {} ({})", path.path, path.kind.describe());
        match &path.body {
            ConflictBody::Regions { regions, truncated } => {
                for region in regions {
                    println!("    lines {}..={}:", region.first_line, region.last_line);
                    for line in region.text.lines() {
                        println!("      | {line}");
                    }
                }
                println!("    truncated: {truncated}");
            }
            ConflictBody::Binary => println!("    (binary)"),
            ConflictBody::Absent { reason } => println!("    absent: {reason}"),
            ConflictBody::Omitted => println!("    (omitted by cap)"),
        }
    }
    if !context.git_message.is_empty() {
        println!("\ngit said:\n{}", context.git_message);
    }
}
