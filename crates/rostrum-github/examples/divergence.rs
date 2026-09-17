//! Live smoke test for the branch-divergence read.
//!
//!     cargo run -p rostrum-github --example divergence -- RhizoNymph/rostrum
//!
//! Reads every open pull request in a repository and asks GitHub how far each
//! head branch has drifted from its base. The counts are the ones the detail
//! pane renders, so a mismatch with `git rev-list --left-right --count` here is
//! a mismatch in the app.
//!
//! A pull request from a fork prints `(not comparable)`: the base repository
//! cannot resolve a head ref that lives elsewhere, and GitHub answers with a
//! null comparison rather than an error. The app falls back to the local clone
//! in that case, so this is expected output, not a failure.

use anyhow::Result;
use rostrum_core::RepoId;
use rostrum_github::{GitHubClient, resolve_token};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "RhizoNymph/rostrum".to_string());
    let repo: RepoId = arg.parse()?;

    let (token, _) = resolve_token().await?;
    let client = GitHubClient::new(token)?;

    let open = client.open_pull_requests(&repo, 25).await?;
    println!(
        "\n{} open pull requests in {repo}:\n",
        open.pull_requests.len()
    );

    for pr in &open.pull_requests {
        let divergence = client.divergence(&repo, &pr.base_ref, &pr.head_ref).await?;

        match divergence {
            Some(d) => println!(
                "  {:>6}  {} -> {}\n          ahead {}, behind {}  ({:?}, fast-forwards: {})",
                pr.number.to_string(),
                pr.head_ref,
                pr.base_ref,
                d.ahead,
                d.behind,
                d.relation(),
                d.fast_forwards(),
            ),
            None => println!(
                "  {:>6}  {} -> {}\n          (not comparable; head ref is not in this repository)",
                pr.number.to_string(),
                pr.head_ref,
                pr.base_ref,
            ),
        }
    }

    println!(
        "\nCross-check one with:\n  \
         git rev-list --left-right --count refs/remotes/origin/<base>...refs/remotes/origin/<head>\n"
    );
    Ok(())
}
