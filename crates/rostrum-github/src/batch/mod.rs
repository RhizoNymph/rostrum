//! Several repositories in one GraphQL document.
//!
//! The poll used to send one document per repository per kind — open pull
//! requests, issues, CI checks, divergence — so its cost grew as repositories
//! × kinds × polls. Here each kind's per-repository selection (the same text
//! the one-repository documents use) is repeated under an alias per
//! repository, `r0: repository(owner: $o0, name: $n0) { … }`, so a chunk of
//! repositories is one request.
//!
//! GitHub costs a document by adding up the requests its connections need
//! and rounding to the nearest point (at least one), per document. Batching
//! therefore saves where one repository's share is well under a point
//! (issues and CI checks: 0.51 at 25 per repository) and costs where it is
//! just over one (pull requests: 1.26, which a lone document rounds down to
//! one). [`cost::chunk_size`] does that arithmetic per kind and picks the
//! chunk size that costs least, so the same code sends pull requests one
//! repository at a time and issues all together. Divergence (`Ref.compare`)
//! has no connections and costs one point per document however many
//! repositories it spans.
//!
//! One repository failing inside a batch — `NOT_FOUND` or `FORBIDDEN` on its
//! alias — is that repository's error; the rest are applied
//! ([`decode::split_batch`]).

pub mod cost;
pub mod decode;
pub mod divergence;
pub mod document;

mod client;

#[cfg(test)]
mod tests;

use rostrum_core::{PullRequest, RepoId, RepoMeta, User};

use crate::{error::GitHubError, graphql::RateLimit};

pub use cost::{
    BatchKind, PollShape, chunk_cost, chunk_size, chunks, poll_cost_estimate, repo_nodes,
    repo_requests, total_cost,
};
pub use document::{build_repo_batch, repo_batch_variables};

/// One batch's answer: a result per repository, in the order asked.
#[derive(Debug)]
pub struct RepoBatch<T> {
    pub results: Vec<(RepoId, Result<T, GitHubError>)>,
    /// What the whole document cost, when it selected `rateLimit`.
    pub rate_limit: Option<RateLimit>,
    /// Who the token belongs to, when the document asked.
    pub viewer: Option<User>,
}

impl<T> RepoBatch<T> {
    pub(crate) fn empty() -> Self {
        Self {
            results: Vec::new(),
            rate_limit: None,
            viewer: None,
        }
    }
}

/// One repository's open pull requests from a batch.
#[derive(Debug)]
pub struct FetchedPulls {
    pub pull_requests: Vec<PullRequest>,
    /// The repository's own facts, for sorting. `None` only if GitHub left
    /// them out.
    pub meta: Option<RepoMeta>,
}
