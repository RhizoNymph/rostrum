//! [`GitHubClient`] methods for batches: one request for a chunk of
//! repositories. Chunking is the caller's ([`super::chunk_size`]); each
//! method sends exactly one document, or none for an empty chunk.

use rostrum_core::{Divergence, Issue, RepoId, ci::PrChecks};

use crate::{
    batch::{
        FetchedPulls, RepoBatch,
        decode::{BatchData, split_batch},
        divergence::{
            RepoCompareNode, RepoPairs, build_divergence_document, divergence_document_variables,
            excused_pair_error, repo_divergences,
        },
        document::{build_repo_batch, repo_batch_variables},
    },
    ci::wire::{CI_CHECKS_SELECTION, CiChecksData, CiRepositoryNode},
    client::GitHubClient,
    error::GitHubError,
    graphql::{GraphQlResponse, OPEN_PULLS_SELECTION, PrNode, RepositoryNode},
    issues::wire::{IssuesRepositoryNode, OPEN_ISSUES_SELECTION, OpenIssuesData},
};

/// What the feed's batch selects once at the top: who the token belongs to.
const VIEWER: &str = "viewer { login avatarUrl }";

fn resource(kind: &str, repos: usize) -> String {
    format!("{kind} for {repos} repositories")
}

impl GitHubClient {
    /// Open pull requests for each of `repos`, in one request.
    pub async fn open_pull_requests_batch(
        &self,
        repos: &[RepoId],
        limit: u32,
    ) -> Result<RepoBatch<FetchedPulls>, GitHubError> {
        if repos.is_empty() {
            return Ok(RepoBatch::empty());
        }
        let document = build_repo_batch(OPEN_PULLS_SELECTION, repos.len(), VIEWER);
        let response: GraphQlResponse<BatchData<RepositoryNode>> = self
            .graphql_raw(
                &document,
                repo_batch_variables(repos, limit),
                &resource("open pull requests", repos.len()),
            )
            .await?;
        split_batch(
            repos,
            response,
            |_, _| false,
            |_, node| Some(fetched_pulls(node)),
        )
    }

    /// Open issues for each of `repos`, in one request.
    pub async fn open_issues_batch(
        &self,
        repos: &[RepoId],
        limit: u32,
    ) -> Result<RepoBatch<Vec<Issue>>, GitHubError> {
        if repos.is_empty() {
            return Ok(RepoBatch::empty());
        }
        let document = build_repo_batch(OPEN_ISSUES_SELECTION, repos.len(), "");
        let response: GraphQlResponse<BatchData<IssuesRepositoryNode>> = self
            .graphql_raw(
                &document,
                repo_batch_variables(repos, limit),
                &resource("open issues", repos.len()),
            )
            .await?;
        split_batch(
            repos,
            response,
            |_, _| false,
            |_, node| {
                OpenIssuesData {
                    rate_limit: None,
                    repository: Some(node),
                }
                .into_domain()
            },
        )
    }

    /// Every open pull request's head-commit checks for each of `repos`, in
    /// one request.
    pub async fn ci_checks_batch(
        &self,
        repos: &[RepoId],
        limit: u32,
    ) -> Result<RepoBatch<Vec<PrChecks>>, GitHubError> {
        if repos.is_empty() {
            return Ok(RepoBatch::empty());
        }
        let document = build_repo_batch(CI_CHECKS_SELECTION, repos.len(), "");
        let response: GraphQlResponse<BatchData<CiRepositoryNode>> = self
            .graphql_raw(
                &document,
                repo_batch_variables(repos, limit),
                &resource("checks", repos.len()),
            )
            .await?;
        split_batch(
            repos,
            response,
            |_, _| false,
            |_, node| {
                CiChecksData {
                    rate_limit: None,
                    repository: Some(node),
                }
                .into_domain()
            },
        )
    }

    /// [`Self::divergences`] for several repositories in one request.
    ///
    /// Each result is index-aligned with that repository's pairs, `None`
    /// wherever GitHub could not compare (the cross-fork case). A repository
    /// with no pairs answers an empty list without being asked.
    pub async fn divergences_batch(
        &self,
        repos: &[RepoPairs],
    ) -> Result<RepoBatch<Vec<Option<Divergence>>>, GitHubError> {
        let asked: Vec<RepoPairs> = repos
            .iter()
            .filter(|entry| !entry.pairs.is_empty())
            .cloned()
            .collect();
        let mut batch = if asked.is_empty() {
            RepoBatch::empty()
        } else {
            let ids: Vec<RepoId> = asked.iter().map(|entry| entry.repo.clone()).collect();
            let response: GraphQlResponse<BatchData<RepoCompareNode>> = self
                .graphql_raw(
                    &build_divergence_document(&asked),
                    divergence_document_variables(&asked),
                    &resource("divergence", asked.len()),
                )
                .await?;
            split_batch(
                &ids,
                response,
                |index, error| excused_pair_error(error, asked[index].pairs.len()),
                |index, node| Some(repo_divergences(node, asked[index].pairs.len())),
            )?
        };
        for entry in repos.iter().filter(|entry| entry.pairs.is_empty()) {
            batch.results.push((entry.repo.clone(), Ok(Vec::new())));
        }
        Ok(batch)
    }
}

fn fetched_pulls(node: RepositoryNode) -> FetchedPulls {
    FetchedPulls {
        meta: node.meta.into_domain(),
        pull_requests: node
            .pull_requests
            .into_vec()
            .into_iter()
            .map(PrNode::into_domain)
            .collect(),
    }
}
