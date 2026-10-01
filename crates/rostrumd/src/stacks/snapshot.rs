//! A repository's open pull requests and stacks, fetched fresh for one
//! request.
//!
//! Validation must be against GitHub as it is now — a pull request merged a
//! minute ago is not open, a stack made from the desktop is a stack — so
//! every stack request fetches both, with the desktop's token
//! (`gh auth token`, then `$GITHUB_TOKEN`). The token is never logged.

use rostrum_core::{RepoId, RepoState};
use rostrum_github::{GitHubClient, GitHubError, RepoStacks, resolve_token};

use crate::boxed::BoxFuture;

/// The most pull requests one GraphQL page returns. A stack member beyond the
/// newest hundred open pull requests reads as not open.
pub const PULL_REQUEST_LIMIT: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error(
        "this computer has no GitHub token: run `gh auth login`, or set GITHUB_TOKEN for rostrumd"
    )]
    NoToken,
    #[error("could not read {repo} from GitHub: {source}")]
    GitHub {
        repo: RepoId,
        #[source]
        source: GitHubError,
    },
}

/// Where repository snapshots come from. The daemon uses [`GitHubSnapshots`];
/// tests supply a fixed state.
pub trait RepoSnapshots: Send + Sync {
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoState, SnapshotError>>;
}

/// GitHub's GraphQL feed query for the pull requests, its Stacks API for the
/// stacks.
pub struct GitHubSnapshots;

impl RepoSnapshots for GitHubSnapshots {
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoState, SnapshotError>> {
        Box::pin(async move {
            let (token, _) = resolve_token().await.map_err(|_| SnapshotError::NoToken)?;
            let github = |source| SnapshotError::GitHub {
                repo: repo.clone(),
                source,
            };
            let client = GitHubClient::new(token).map_err(github)?;
            let pulls = client
                .open_pull_requests(repo, PULL_REQUEST_LIMIT)
                .await
                .map_err(github)?;
            let stacks = match client.stacks(repo).await.map_err(github)? {
                RepoStacks::Available(stacks) => stacks,
                // Stacked pull requests are not enabled: there are none.
                RepoStacks::Unavailable => Vec::new(),
            };
            Ok(RepoState {
                prs: pulls.pull_requests,
                stacks,
                meta: pulls.meta,
                ..RepoState::new(repo.clone())
            })
        })
    }
}
