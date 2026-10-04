//! REST requests for CI: re-runs, job logs, and a check run's own output.

use reqwest::{Method, StatusCode, header::HeaderMap};
use rostrum_core::{
    RepoId,
    ci::{Annotation, AnnotationLevel, CheckOutput, RerunTarget},
};
use serde::Deserialize;

use crate::{error::GitHubError, issues::RestCall};

/// The REST request for a re-run.
pub fn rerun_call(repo: &RepoId, target: RerunTarget) -> RestCall {
    let base = format!("/repos/{}/{}", repo.owner(), repo.name());
    let path = match target {
        RerunTarget::Job { job_id } => format!("{base}/actions/jobs/{job_id}/rerun"),
        RerunTarget::FailedJobs { run_id } => {
            format!("{base}/actions/runs/{run_id}/rerun-failed-jobs")
        }
        RerunTarget::AllJobs { run_id } => format!("{base}/actions/runs/{run_id}/rerun"),
        RerunTarget::Suite { suite_id } => format!("{base}/check-suites/{suite_id}/rerequest"),
    };
    RestCall {
        method: Method::POST,
        path,
        body: None,
    }
}

/// The path of a job's log. GitHub answers with a redirect to short-lived
/// storage, which the client follows.
pub fn job_log_path(repo: &RepoId, job_id: u64) -> String {
    format!(
        "/repos/{}/{}/actions/jobs/{job_id}/logs",
        repo.owner(),
        repo.name()
    )
}

/// Why a re-run was refused.
#[derive(Debug, thiserror::Error)]
pub enum RerunError {
    /// The token may not re-run workflows here: not a collaborator with write
    /// access, or a token without the `workflow`/`checks` permission.
    #[error("no permission to re-run this: {message}")]
    NoPermission { message: String },
    /// GitHub will not re-run this one: too old, still running, or the app
    /// does not accept re-requests.
    #[error("this cannot be re-run: {message}")]
    NotRerunnable { message: String },
    #[error("the run or job no longer exists")]
    NotFound,
    #[error(transparent)]
    Api(#[from] GitHubError),
}

/// Words GitHub uses when a 403 is about the run rather than the token.
const NOT_RERUNNABLE_HINTS: &[&str] = &[
    "retry",
    "rerun",
    "re-run",
    "rerequest",
    "re-request",
    "in progress",
    "over a month",
];

/// Turn a re-run response into success or a typed refusal.
///
/// 403 means two different things: the token lacks permission, or the run
/// is not one GitHub will re-run (most often: created more than a month ago)
/// — told apart by GitHub's message. A rate limit is neither, and goes the
/// usual way. 409 and 422 are refusals about the run; 404 is a run that is
/// gone.
pub fn classify_rerun(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> Result<(), RerunError> {
    if status.is_success() {
        return Ok(());
    }
    let message = crate::client::rest_message(body);
    let lower = message.to_lowercase();
    match status {
        StatusCode::FORBIDDEN
            if headers.get("retry-after").is_none()
                && headers
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    != Some("0") =>
        {
            if NOT_RERUNNABLE_HINTS.iter().any(|hint| lower.contains(hint)) {
                Err(RerunError::NotRerunnable { message })
            } else {
                Err(RerunError::NoPermission { message })
            }
        }
        StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY => {
            Err(RerunError::NotRerunnable { message })
        }
        StatusCode::NOT_FOUND => Err(RerunError::NotFound),
        _ => Err(RerunError::Api(
            crate::client::classify_status(status, headers, body, "re-run").unwrap_or(
                GitHubError::Unexpected {
                    status: status.as_u16(),
                    body: message,
                },
            ),
        )),
    }
}

/// `GET /repos/{o}/{r}/check-runs/{id}`, the part the detail view reads.
#[derive(Debug, Deserialize)]
pub struct CheckRunResponse {
    pub output: Option<OutputNode>,
}

#[derive(Debug, Deserialize)]
pub struct OutputNode {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub text: Option<String>,
}

/// One entry of `GET /repos/{o}/{r}/check-runs/{id}/annotations`.
#[derive(Debug, Deserialize)]
pub struct AnnotationNode {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub annotation_level: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub message: String,
}

impl AnnotationNode {
    pub fn into_domain(self) -> Annotation {
        Annotation {
            path: self.path,
            start_line: self.start_line,
            end_line: self.end_line.max(self.start_line),
            level: match self.annotation_level.as_deref() {
                Some("failure") => AnnotationLevel::Failure,
                Some("warning") => AnnotationLevel::Warning,
                _ => AnnotationLevel::Notice,
            },
            title: self.title.filter(|t| !t.trim().is_empty()),
            message: self.message,
        }
    }
}

/// A check run's output and annotations, as the detail view shows them.
/// Blank parts are absent rather than empty strings.
pub fn check_output(run: CheckRunResponse, annotations: Vec<AnnotationNode>) -> CheckOutput {
    let non_blank = |text: Option<String>| text.filter(|t| !t.trim().is_empty());
    let output = run.output;
    CheckOutput {
        title: non_blank(output.as_ref().and_then(|o| o.title.clone())),
        summary: non_blank(output.as_ref().and_then(|o| o.summary.clone())),
        text: non_blank(output.and_then(|o| o.text)),
        annotations: annotations
            .into_iter()
            .map(AnnotationNode::into_domain)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn repo() -> RepoId {
        RepoId::new("python", "cpython")
    }

    #[test]
    fn each_rerun_posts_to_its_own_endpoint_without_a_body() {
        for (target, path) in [
            (
                RerunTarget::Job {
                    job_id: 110642708500,
                },
                "/repos/python/cpython/actions/jobs/110642708500/rerun",
            ),
            (
                RerunTarget::FailedJobs {
                    run_id: 36944162426,
                },
                "/repos/python/cpython/actions/runs/36944162426/rerun-failed-jobs",
            ),
            (
                RerunTarget::AllJobs {
                    run_id: 36944162426,
                },
                "/repos/python/cpython/actions/runs/36944162426/rerun",
            ),
            (
                RerunTarget::Suite {
                    suite_id: 100084169492,
                },
                "/repos/python/cpython/check-suites/100084169492/rerequest",
            ),
        ] {
            assert_eq!(
                rerun_call(&repo(), target),
                RestCall {
                    method: Method::POST,
                    path: path.into(),
                    body: None,
                },
                "{target:?}"
            );
        }
    }

    #[test]
    fn the_log_path_names_the_job() {
        assert_eq!(
            job_log_path(&repo(), 7),
            "/repos/python/cpython/actions/jobs/7/logs"
        );
    }

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn accepted_reruns_succeed() {
        assert!(classify_rerun(StatusCode::CREATED, &HeaderMap::new(), "{}").is_ok());
        assert!(classify_rerun(StatusCode::NO_CONTENT, &HeaderMap::new(), "").is_ok());
    }

    /// 403 is a permission problem unless GitHub's message says the run is
    /// the problem.
    #[test]
    fn a_403_tells_permission_from_a_run_github_will_not_rerun() {
        let denied = classify_rerun(
            StatusCode::FORBIDDEN,
            &HeaderMap::new(),
            r#"{"message":"Must have admin rights to Repository."}"#,
        );
        assert!(
            matches!(denied, Err(RerunError::NoPermission { ref message }) if message.contains("admin"))
        );

        let stale = classify_rerun(
            StatusCode::FORBIDDEN,
            &HeaderMap::new(),
            r#"{"message":"Unable to retry this workflow run because it was created over a month ago"}"#,
        );
        assert!(matches!(stale, Err(RerunError::NotRerunnable { .. })));
    }

    #[test]
    fn a_rate_limited_403_is_neither() {
        let limited = classify_rerun(
            StatusCode::FORBIDDEN,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1800000000"),
            ]),
            r#"{"message":"API rate limit exceeded"}"#,
        );
        assert!(matches!(
            limited,
            Err(RerunError::Api(GitHubError::RateLimited { .. }))
        ));
    }

    #[test]
    fn conflicts_and_unprocessable_requests_are_not_rerunnable() {
        for status in [StatusCode::CONFLICT, StatusCode::UNPROCESSABLE_ENTITY] {
            assert!(matches!(
                classify_rerun(
                    status,
                    &HeaderMap::new(),
                    r#"{"message":"This workflow is already running"}"#
                ),
                Err(RerunError::NotRerunnable { .. })
            ));
        }
        assert!(matches!(
            classify_rerun(StatusCode::NOT_FOUND, &HeaderMap::new(), ""),
            Err(RerunError::NotFound)
        ));
        assert!(matches!(
            classify_rerun(StatusCode::UNAUTHORIZED, &HeaderMap::new(), ""),
            Err(RerunError::Api(GitHubError::Unauthorized))
        ));
    }

    #[test]
    fn decodes_a_captured_third_party_check_run() {
        let run: CheckRunResponse =
            serde_json::from_str(include_str!("../../fixtures/ci/check_run_third_party.json"))
                .expect("decodes");
        let output = check_output(run, Vec::new());
        assert_eq!(output.title.as_deref(), Some("Awaiting approvals (0/1)"));
        assert!(
            output
                .summary
                .as_deref()
                .is_some_and(|s| s.contains("team-member"))
        );
        assert!(output.annotations.is_empty());
    }

    #[test]
    fn decodes_captured_annotations() {
        let notes: Vec<AnnotationNode> =
            serde_json::from_str(include_str!("../../fixtures/ci/check_run_annotations.json"))
                .expect("decodes");
        let output = check_output(CheckRunResponse { output: None }, notes);
        assert_eq!(output.annotations.len(), 1);
        let note = &output.annotations[0];
        assert_eq!(note.level, AnnotationLevel::Failure);
        assert_eq!(note.message, "Process completed with exit code 2.");
        assert_eq!(note.location(), ".github:1579");
        assert_eq!(output.title, None);
    }
}
