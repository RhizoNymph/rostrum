//! The CI checks document and its wire types.
//!
//! One request per repository: every open pull request's head commit with
//! its `statusCheckRollup` contexts — check runs and legacy commit statuses —
//! in one round trip. Captured live at cost 1 for six pull requests of
//! microsoft/vscode, python/cpython and kubernetes/kubernetes.

use chrono::{DateTime, Utc};
use rostrum_core::{
    PrNumber,
    ci::{CheckEntry, CheckKey, CheckSource, CheckStatus, PrChecks},
};
use serde::Deserialize;

use crate::graphql::{Connection, RateLimit};

/// Contexts fetched per head commit. A pull request with more is marked
/// [`PrChecks::truncated`] rather than paged: the grid is a glance, and a
/// hundred columns is already past what fits.
pub const CONTEXTS_PER_COMMIT: u32 = 100;

/// Open pull requests' head-commit checks for one repository, most recently
/// updated first — the same page of pull requests the feed fetches.
pub const CI_CHECKS: &str = r#"
query($owner: String!, $name: String!, $first: Int!) {
  rateLimit { cost remaining resetAt }
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number
        headRefOid
        commits(last: 1) {
          nodes {
            commit {
              oid
              statusCheckRollup {
                state
                contexts(first: 100) {
                  totalCount
                  nodes {
                    __typename
                    ... on CheckRun {
                      databaseId
                      name
                      status
                      conclusion
                      startedAt
                      completedAt
                      detailsUrl
                      checkSuite {
                        databaseId
                        app { name slug }
                        workflowRun { databaseId runNumber runAttempt workflow { name } }
                      }
                    }
                    ... on StatusContext {
                      context
                      state
                      createdAt
                      targetUrl
                      description
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiChecksData {
    pub rate_limit: Option<RateLimit>,
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<CiRepositoryNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiRepositoryNode {
    pub pull_requests: Connection<CiPullRequestNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiPullRequestNode {
    pub number: u32,
    #[serde(default)]
    pub head_ref_oid: Option<String>,
    pub commits: Option<Connection<CiCommitEdge>>,
}

#[derive(Debug, Deserialize)]
pub struct CiCommitEdge {
    pub commit: CiCommitNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiCommitNode {
    pub oid: Option<String>,
    /// `null` when nothing has reported on the commit.
    pub status_check_rollup: Option<RollupNode>,
}

#[derive(Debug, Deserialize)]
pub struct RollupNode {
    pub contexts: Option<ContextConnection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextConnection {
    #[serde(default)]
    pub total_count: u32,
    pub nodes: Option<Vec<Option<ContextNode>>>,
}

/// A `StatusCheckRollupContext`: a `CheckRun` or a `StatusContext`, read
/// structurally so a type GitHub adds later is skipped rather than failing
/// the repository.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextNode {
    #[serde(rename = "__typename", default)]
    pub typename: String,
    // CheckRun
    pub database_id: Option<u64>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub details_url: Option<String>,
    pub check_suite: Option<CheckSuiteNode>,
    // StatusContext
    pub context: Option<String>,
    pub state: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub target_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSuiteNode {
    pub database_id: Option<u64>,
    pub app: Option<AppNode>,
    pub workflow_run: Option<WorkflowRunNode>,
}

#[derive(Debug, Deserialize)]
pub struct AppNode {
    pub name: Option<String>,
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunNode {
    pub database_id: Option<u64>,
    #[serde(default)]
    pub run_attempt: Option<u32>,
    pub workflow: Option<WorkflowNode>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNode {
    pub name: Option<String>,
}

impl ContextNode {
    /// The grid's entry, or `None` for a context with nothing to key it by.
    pub fn into_domain(self) -> Option<CheckEntry> {
        match self.typename.as_str() {
            "CheckRun" => self.check_run(),
            "StatusContext" => {
                let state = self.state.as_deref()?;
                let status = CheckStatus::from_status_context(state);
                Some(CheckEntry {
                    key: CheckKey::new(None, self.context?),
                    status,
                    started_at: self.created_at,
                    completed_at: status.is_finished().then_some(self.created_at).flatten(),
                    details_url: self.target_url,
                    source: CheckSource::Status,
                })
            }
            _ => None,
        }
    }

    fn check_run(self) -> Option<CheckEntry> {
        let id = self.database_id?;
        let status =
            CheckStatus::from_check_run(self.status.as_deref()?, self.conclusion.as_deref());
        let suite = self.check_suite;
        let run = suite.as_ref().and_then(|s| s.workflow_run.as_ref());
        // An Actions job is a check run whose suite has a workflow run; its
        // check-run id is the job id the logs and re-run endpoints take.
        let (workflow, source) = match (run.and_then(|r| r.database_id), &suite) {
            (Some(run_id), Some(suite)) => (
                run.and_then(|r| r.workflow.as_ref())
                    .and_then(|w| w.name.clone()),
                CheckSource::Actions {
                    job_id: id,
                    run_id,
                    run_attempt: run.and_then(|r| r.run_attempt).unwrap_or(1),
                    suite_id: suite.database_id.unwrap_or_default(),
                },
            ),
            _ => (
                None,
                CheckSource::App {
                    check_run_id: id,
                    suite_id: suite.as_ref().and_then(|s| s.database_id),
                    app: suite
                        .as_ref()
                        .and_then(|s| s.app.as_ref())
                        .and_then(|a| a.name.clone().or_else(|| a.slug.clone()))
                        .unwrap_or_else(|| "a GitHub App".into()),
                },
            ),
        };
        Some(CheckEntry {
            key: CheckKey::new(workflow, self.name?),
            status,
            started_at: self.started_at,
            completed_at: self.completed_at,
            details_url: self.details_url,
            source,
        })
    }
}

impl CiChecksData {
    /// Every open pull request's checks, or `None` when the repository was
    /// withheld. A pull request nothing has reported on has no entries.
    pub fn into_domain(self) -> Option<Vec<PrChecks>> {
        let repository = self.repository?;
        Some(
            repository
                .pull_requests
                .into_vec()
                .into_iter()
                .map(|pr| {
                    let commit = pr
                        .commits
                        .map(Connection::into_vec)
                        .unwrap_or_default()
                        .into_iter()
                        .next()
                        .map(|edge| edge.commit);
                    let head_sha = commit
                        .as_ref()
                        .and_then(|c| c.oid.clone())
                        .or(pr.head_ref_oid)
                        .unwrap_or_default();
                    let contexts = commit
                        .and_then(|c| c.status_check_rollup)
                        .and_then(|r| r.contexts);
                    let (total, nodes) = match contexts {
                        Some(c) => (c.total_count, c.nodes.unwrap_or_default()),
                        None => (0, Vec::new()),
                    };
                    let fetched = nodes.len();
                    PrChecks {
                        number: PrNumber(pr.number),
                        head_sha,
                        entries: nodes
                            .into_iter()
                            .flatten()
                            .filter_map(ContextNode::into_domain)
                            .collect(),
                        truncated: (total as usize) > fetched,
                    }
                })
                .collect(),
        )
    }
}
