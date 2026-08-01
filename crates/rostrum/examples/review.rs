//! End-to-end check of the review data path against the live GitHub API.
//!
//!     cargo run -p rostrum --example review -- zed-industries/zed
//!
//! Fetches the most recently updated open pull request, its conversation, and
//! its diff, then parses every patch and verifies the comment anchors. Read
//! only — it never posts, merges, or closes anything.

use anyhow::{Result, anyhow};
use rostrum_core::{RepoId, Side, TimelineItem};
use rostrum_diff::{LineKind, parse_patch};
use rostrum_github::{GitHubClient, resolve_token};

#[tokio::main]
async fn main() -> Result<()> {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "zed-industries/zed".to_string());
    let repo: RepoId = arg.parse()?;

    let (token, source) = resolve_token().await?;
    println!("token via {source}\n");
    let client = GitHubClient::new(token)?;

    let wanted: Option<u32> = std::env::args().nth(2).and_then(|n| n.parse().ok());
    let listing = client.open_pull_requests(&repo, 50).await?;
    let pull = match wanted {
        Some(number) => listing
            .pull_requests
            .iter()
            .find(|pr| pr.number.0 == number)
            .ok_or_else(|| anyhow!("{repo}#{number} is not an open pull request"))?,
        None => listing
            .pull_requests
            .first()
            .ok_or_else(|| anyhow!("no open pull requests in {repo}"))?,
    };
    println!("{} {} — {}", repo, pull.number, pull.title);
    println!(
        "  {} files, +{}/-{}\n",
        pull.changed_files, pull.additions, pull.deletions
    );

    // --- conversation ------------------------------------------------------
    let conversation = client.conversation(&repo, pull.number).await?;
    let (mut comments, mut reviews, mut events) = (0, 0, 0);
    for item in &conversation.items {
        match item {
            TimelineItem::Body { .. } => {}
            TimelineItem::Comment { .. } => comments += 1,
            TimelineItem::Review { .. } => reviews += 1,
            TimelineItem::Event { .. } => events += 1,
        }
    }
    println!(
        "conversation: {} items ({comments} comments, {reviews} reviews, {events} events), \
         {} threads ({} unresolved), {} checks",
        conversation.items.len(),
        conversation.threads.len(),
        conversation.unresolved_thread_count(),
        conversation.checks.len(),
    );

    if let Some(TimelineItem::Body { body, .. }) = conversation.items.first() {
        let parsed = rostrum_md::parse(body);
        println!("  body parsed into {} markdown blocks", parsed.blocks.len());
    }
    for thread in conversation.threads.iter().take(4) {
        println!(
            "  thread {}:{:?} side={:?} resolved={} outdated={} comments={}",
            thread.path,
            thread.line,
            thread.side,
            thread.is_resolved,
            thread.is_outdated,
            thread.comments.len(),
        );
    }
    for check in conversation.checks.iter().take(3) {
        println!("  check {:<40} {:?}", check.name, check.state);
    }
    println!();

    // --- diff --------------------------------------------------------------
    let files = client.files(&repo, pull.number).await?;
    println!("diff: {} files", files.len());

    let (mut hunks, mut added, mut removed, mut context, mut anchored, mut omitted) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

    for file in &files {
        let Some(patch) = file.patch.as_deref() else {
            omitted += 1;
            continue;
        };
        let parsed = parse_patch(patch).map_err(|err| anyhow!("{}: {err}", file.filename))?;
        hunks += parsed.len();

        for hunk in &parsed {
            for line in &hunk.lines {
                match line.kind {
                    LineKind::Added => added += 1,
                    LineKind::Removed => removed += 1,
                    LineKind::Context => context += 1,
                }
                // The invariant that makes inline comments land correctly.
                if let Some(anchor) = line.anchor(&file.filename) {
                    anchored += 1;
                    let expected = match line.kind {
                        LineKind::Removed => Side::Left,
                        _ => Side::Right,
                    };
                    assert_eq!(anchor.side, expected, "wrong side for {:?}", line.kind);
                }
            }
        }
    }

    println!(
        "  {hunks} hunks, {added} added / {removed} removed / {context} context lines\n  \
         {anchored} commentable lines, {omitted} files without a patch"
    );
    println!("\nall anchors verified");
    Ok(())
}
