//! Live smoke test against the real GitHub API.
//!
//!     cargo run -p rostrum-github --example fetch -- zed-industries/zed

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
        .unwrap_or_else(|| "zed-industries/zed".to_string());
    let repo: RepoId = arg.parse()?;

    let (token, source) = resolve_token().await?;
    println!("token via {source} ({})", token.redacted());

    let client = GitHubClient::new(token)?;
    let result = client.open_pull_requests(&repo, 10).await?;

    if let Some(limit) = &result.rate_limit {
        println!(
            "rate limit: cost {}, {} remaining, resets {}",
            limit.cost, limit.remaining, limit.reset_at
        );
    }

    println!("\n{} open PRs in {repo}:\n", result.pull_requests.len());
    for pr in &result.pull_requests {
        let author = pr.author.as_ref().map_or("(unknown)", |a| a.login.as_str());
        println!(
            "  {:>6}  {:<60}  {:<12} +{}/-{}  {:?}  checks={:?}",
            pr.number.to_string(),
            pr.title.chars().take(60).collect::<String>(),
            author,
            pr.additions,
            pr.deletions,
            pr.mergeable,
            pr.checks,
        );
    }

    Ok(())
}
