//! [`GitHubClient`] methods for CI: the grid's checks, job logs, a check
//! run's output, and re-runs.

use reqwest::Method;
use rostrum_core::{
    RepoId,
    ci::{CheckOutput, PrChecks, RerunTarget},
};
use serde_json::json;

use crate::{
    ci::{
        rest::{
            AnnotationNode, CheckRunResponse, RerunError, check_output, classify_rerun,
            job_log_path, rerun_call,
        },
        wire::{CI_CHECKS, CiChecksData},
    },
    client::GitHubClient,
    error::GitHubError,
    graphql::RateLimit,
};

/// One repository's checks, as one request answered them.
#[derive(Debug)]
pub struct RepoCiChecks {
    pub checks: Vec<PrChecks>,
    pub rate_limit: Option<RateLimit>,
}

impl GitHubClient {
    /// Every open pull request's head-commit checks, in one request.
    pub async fn ci_checks(&self, repo: &RepoId, limit: u32) -> Result<RepoCiChecks, GitHubError> {
        let resource = format!("{repo} checks");
        let data: CiChecksData = self
            .graphql(
                CI_CHECKS,
                json!({ "owner": repo.owner(), "name": repo.name(), "first": limit }),
                &resource,
            )
            .await?;
        let rate_limit = data.rate_limit.clone();
        let checks = data.into_domain().ok_or_else(|| GitHubError::NotFound {
            resource: repo.to_string(),
        })?;
        Ok(RepoCiChecks { checks, rate_limit })
    }

    /// An Actions job's raw log. GitHub redirects to short-lived storage on
    /// another host; the redirect is followed, and the token is not sent on
    /// to that host.
    pub async fn job_log(&self, repo: &RepoId, job_id: u64) -> Result<String, GitHubError> {
        let url = format!("{}{}", self.rest_base, job_log_path(repo, job_id));
        let response = self.execute(self.rest(Method::GET, &url)).await?;
        response.check_status(&format!("{repo} job {job_id} log"))?;
        Ok(response.body)
    }

    /// A non-Actions check run's output and annotations.
    pub async fn check_output(
        &self,
        repo: &RepoId,
        check_run_id: u64,
    ) -> Result<CheckOutput, GitHubError> {
        let resource = format!("{repo} check run {check_run_id}");
        let base = format!(
            "{}/repos/{}/{}/check-runs/{check_run_id}",
            self.rest_base,
            repo.owner(),
            repo.name()
        );
        let run = self.execute(self.rest(Method::GET, &base)).await?;
        run.check_status(&resource)?;
        let run: CheckRunResponse =
            serde_json::from_str(&run.body).map_err(|source| GitHubError::Decode {
                context: resource.clone(),
                source,
            })?;
        let notes = self
            .execute(self.rest(Method::GET, &format!("{base}/annotations?per_page=100")))
            .await?;
        notes.check_status(&resource)?;
        let notes: Vec<AnnotationNode> =
            serde_json::from_str(&notes.body).map_err(|source| GitHubError::Decode {
                context: format!("annotations of {resource}"),
                source,
            })?;
        Ok(check_output(run, notes))
    }

    /// Ask for a re-run. Every refusal is typed; see [`RerunError`].
    pub async fn rerun(&self, repo: &RepoId, target: RerunTarget) -> Result<(), RerunError> {
        let call = rerun_call(repo, target);
        let url = format!("{}{}", self.rest_base, call.path);
        let response = self.execute(self.rest(call.method, &url)).await?;
        tracing::info!(%repo, ?target, status = response.status.as_u16(), "re-run requested");
        classify_rerun(response.status, &response.headers, &response.body)
    }
}
