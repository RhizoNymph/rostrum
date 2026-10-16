//! Turning one batched answer into a result per repository.

use std::collections::HashMap;

use rostrum_core::RepoId;
use serde::Deserialize;

use crate::{
    batch::{RepoBatch, document::alias_index},
    error::{GitHubError, GraphQlError},
    graphql::{AuthorNode, GraphQlResponse, RateLimit},
};

/// A batched document's `data`: the top-level selections, and one entry per
/// repository alias. The aliases are dynamic, so they land in a map.
#[derive(Debug, Deserialize)]
pub struct BatchData<W> {
    #[serde(rename = "rateLimit", default)]
    pub rate_limit: Option<RateLimit>,
    #[serde(default)]
    pub viewer: Option<AuthorNode>,
    /// `r0..rN`, each `null` when that repository does not exist or is not
    /// visible.
    #[serde(flatten)]
    pub repos: HashMap<String, Option<W>>,
}

/// Where in a batch an error belongs.
#[derive(Debug, PartialEq, Eq)]
enum Scope {
    /// Under repository `index`'s alias.
    Repo(usize),
    /// Anywhere else (no path, `rateLimit`, `viewer`): the whole document.
    Batch,
}

fn scope(error: &GraphQlError, count: usize) -> Scope {
    error
        .path
        .as_deref()
        .and_then(|path| path.first())
        .and_then(serde_json::Value::as_str)
        .and_then(|alias| alias_index(alias, count))
        .map_or(Scope::Batch, Scope::Repo)
}

/// The error one repository's alias failed with.
///
/// Nothing but `NOT_FOUND` is the repository being gone or invisible;
/// `FORBIDDEN` is a refusal with GitHub's reason (an organisation's SSO or
/// IP allow list, typically); anything else is reported as GitHub sent it.
fn repo_error(repo: &RepoId, errors: Vec<GraphQlError>) -> GitHubError {
    if errors
        .iter()
        .all(|e| e.kind.as_deref() == Some("NOT_FOUND"))
    {
        return GitHubError::NotFound {
            resource: repo.to_string(),
        };
    }
    if let Some(forbidden) = errors
        .iter()
        .find(|e| e.kind.as_deref() == Some("FORBIDDEN"))
    {
        return GitHubError::Forbidden {
            reason: forbidden.message.clone(),
        };
    }
    GitHubError::GraphQl { errors }
}

/// Split a batched response over `repos` (the order the aliases were built
/// in) into a result per repository.
///
/// - An error outside every alias fails the whole batch.
/// - An error under a repository's alias fails that repository alone —
///   unless `excused(index, error)` says it is an expected part of the
///   answer (divergence's cross-fork `NOT_FOUND`).
/// - A repository whose alias is missing or `null` without an error is
///   `NotFound`, as the one-repository documents report it.
/// - `convert` turns a repository's wire node into its result; `None` is
///   `NotFound` too.
pub fn split_batch<W, T>(
    repos: &[RepoId],
    response: GraphQlResponse<BatchData<W>>,
    excused: impl Fn(usize, &GraphQlError) -> bool,
    convert: impl Fn(usize, W) -> Option<T>,
) -> Result<RepoBatch<T>, GitHubError> {
    let count = repos.len();
    let mut per_repo: Vec<Vec<GraphQlError>> = (0..count).map(|_| Vec::new()).collect();
    let mut whole = Vec::new();
    for error in response.errors {
        match scope(&error, count) {
            Scope::Repo(index) if excused(index, &error) => {}
            Scope::Repo(index) => per_repo[index].push(error),
            Scope::Batch => whole.push(error),
        }
    }
    if !whole.is_empty() {
        return Err(GitHubError::GraphQl { errors: whole });
    }
    let data = response.data.ok_or(GitHubError::EmptyData)?;
    let mut nodes = data.repos;

    let results = repos
        .iter()
        .zip(per_repo)
        .enumerate()
        .map(|(index, (repo, errors))| {
            let result = if errors.is_empty() {
                nodes
                    .remove(&super::document::repo_alias(index))
                    .flatten()
                    .and_then(|node| convert(index, node))
                    .ok_or_else(|| GitHubError::NotFound {
                        resource: repo.to_string(),
                    })
            } else {
                Err(repo_error(repo, errors))
            };
            (repo.clone(), result)
        })
        .collect();

    Ok(RepoBatch {
        results,
        rate_limit: data.rate_limit,
        viewer: data.viewer.and_then(AuthorNode::into_user),
    })
}
