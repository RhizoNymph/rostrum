//! HTTP client for the GitHub API.

use chrono::{TimeZone, Utc};
use reqwest::{
    Client, Method, RequestBuilder, StatusCode,
    header::{ACCEPT, HeaderMap},
};
use rostrum_core::{Conversation, PrNumber, PullRequest, RepoId};
use serde_json::json;

use crate::{
    auth::Token,
    conversation::{ConversationNode, ConversationQueryData, PULL_REQUEST_CONVERSATION},
    error::GitHubError,
    graphql::{self, GraphQlResponse, PrNode, RateLimit, RepoQueryData},
    rest::{IssueState, MergeMethod, PullRequestFile, SubmitReview},
};

const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const REST_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = concat!("rostrum/", env!("CARGO_PKG_VERSION"));
/// Pinned so a future default cannot silently change response shapes.
const API_VERSION: &str = "2022-11-28";
/// At 100 entries per page this is 10,000 files, well past GitHub's own 3,000
/// file cap. It exists only so a malformed `Link` header cannot loop forever.
const MAX_PAGES: usize = 100;

/// Result of one repository refresh.
#[derive(Debug)]
pub struct RepoPullRequests {
    pub pull_requests: Vec<PullRequest>,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Clone)]
pub struct GitHubClient {
    http: Client,
    token: Token,
}

impl GitHubClient {
    pub fn new(token: Token) -> Result<Self, GitHubError> {
        let http = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { http, token })
    }

    /// Fetch open pull requests for one repository.
    pub async fn open_pull_requests(
        &self,
        repo: &RepoId,
        limit: u32,
    ) -> Result<RepoPullRequests, GitHubError> {
        let body = json!({
            "query": graphql::OPEN_PULL_REQUESTS,
            "variables": {
                "owner": repo.owner(),
                "name": repo.name(),
                "first": limit,
            },
        });

        let response = self
            .http
            .post(GRAPHQL_URL)
            .bearer_auth(self.token.as_str())
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await?;

        if let Some(err) = classify_status(status, &headers, &text, &repo.to_string()) {
            return Err(err);
        }

        let parsed: GraphQlResponse<RepoQueryData> =
            serde_json::from_str(&text).map_err(|source| GitHubError::Decode {
                context: format!("pull requests for {repo}"),
                source,
            })?;

        // A GraphQL request can return HTTP 200 and still have failed, so
        // errors are checked before `data` is trusted.
        if !parsed.errors.is_empty() {
            let all_not_found = parsed
                .errors
                .iter()
                .all(|e| e.kind.as_deref() == Some("NOT_FOUND"));
            return Err(if all_not_found {
                GitHubError::NotFound {
                    resource: repo.to_string(),
                }
            } else {
                GitHubError::GraphQl {
                    errors: parsed.errors,
                }
            });
        }

        let data = parsed.data.ok_or(GitHubError::EmptyData)?;
        let rate_limit = data.rate_limit.clone();
        let repository = data.repository.ok_or_else(|| GitHubError::NotFound {
            resource: repo.to_string(),
        })?;

        Ok(RepoPullRequests {
            pull_requests: repository
                .pull_requests
                .into_vec()
                .into_iter()
                .map(PrNode::into_domain)
                .collect(),
            rate_limit,
        })
    }

    /// Everything the detail pane shows for one pull request: description,
    /// comments, reviews, inline threads, timeline events, and checks.
    ///
    /// One GraphQL round trip; the returned [`Conversation`] is already sorted.
    pub async fn conversation(
        &self,
        repo: &RepoId,
        number: PrNumber,
    ) -> Result<Conversation, GitHubError> {
        let resource = resource_name(repo, number);
        let body = json!({
            "query": PULL_REQUEST_CONVERSATION,
            "variables": {
                "owner": repo.owner(),
                "name": repo.name(),
                "number": number.0,
            },
        });

        let response = self
            .execute(
                self.http
                    .post(GRAPHQL_URL)
                    .bearer_auth(self.token.as_str())
                    .json(&body),
            )
            .await?;
        response.check_status(&resource)?;

        let parsed: GraphQlResponse<ConversationQueryData> =
            serde_json::from_str(&response.body).map_err(|source| GitHubError::Decode {
                context: format!("conversation for {resource}"),
                source,
            })?;

        // HTTP 200 with a populated `errors` array is a normal GraphQL failure,
        // so errors are checked before `data` is trusted.
        if !parsed.errors.is_empty() {
            let all_not_found = parsed
                .errors
                .iter()
                .all(|e| e.kind.as_deref() == Some("NOT_FOUND"));
            return Err(if all_not_found {
                GitHubError::NotFound { resource }
            } else {
                GitHubError::GraphQl {
                    errors: parsed.errors,
                }
            });
        }

        parsed
            .data
            .ok_or(GitHubError::EmptyData)?
            .repository
            .and_then(|repository| repository.pull_request)
            .map(ConversationNode::into_domain)
            .ok_or(GitHubError::NotFound { resource })
    }

    /// Every changed file with its patch, following pagination to the end.
    ///
    /// GitHub caps a pull request at 3,000 files and omits the patch for binary
    /// and very large files, so an entry with no patch is expected rather than
    /// an error.
    pub async fn files(
        &self,
        repo: &RepoId,
        number: PrNumber,
    ) -> Result<Vec<PullRequestFile>, GitHubError> {
        let resource = resource_name(repo, number);
        let mut url = format!(
            "{REST_BASE}/repos/{}/{}/pulls/{}/files?per_page=100",
            repo.owner(),
            repo.name(),
            number.0
        );

        let mut files = Vec::new();
        for _ in 0..MAX_PAGES {
            let response = self.execute(self.rest(Method::GET, &url)).await?;
            response.check_status(&resource)?;

            let page: Vec<PullRequestFile> =
                serde_json::from_str(&response.body).map_err(|source| GitHubError::Decode {
                    context: format!("files for {resource}"),
                    source,
                })?;
            files.extend(page);

            match next_page_url(&response.headers) {
                Some(next) => url = next,
                None => break,
            }
        }

        Ok(files)
    }

    /// Post a top-level conversation comment (the issue comment API; inline
    /// comments go through [`Self::submit_review`] or [`Self::reply_to_thread`]).
    pub async fn add_comment(
        &self,
        repo: &RepoId,
        number: PrNumber,
        body: &str,
    ) -> Result<(), GitHubError> {
        let url = format!(
            "{REST_BASE}/repos/{}/{}/issues/{}/comments",
            repo.owner(),
            repo.name(),
            number.0
        );
        self.execute(self.rest(Method::POST, &url).json(&json!({ "body": body })))
            .await?
            .check_status(&resource_name(repo, number))
    }

    /// Submit a review with its inline comments in one request.
    ///
    /// GitHub rejects an approval or change request only when both the body and
    /// the comment list are empty; see [`SubmitReview::is_empty`].
    pub async fn submit_review(
        &self,
        repo: &RepoId,
        number: PrNumber,
        review: SubmitReview,
    ) -> Result<(), GitHubError> {
        let url = format!(
            "{REST_BASE}/repos/{}/{}/pulls/{}/reviews",
            repo.owner(),
            repo.name(),
            number.0
        );
        self.execute(self.rest(Method::POST, &url).json(&review))
            .await?
            .check_status(&resource_name(repo, number))
    }

    /// Reply into an existing inline thread.
    ///
    /// `in_reply_to` is the REST id of a comment in the thread
    /// ([`rostrum_core::ThreadComment::database_id`]); the GraphQL node id is
    /// rejected by this endpoint.
    pub async fn reply_to_thread(
        &self,
        repo: &RepoId,
        number: PrNumber,
        in_reply_to: u64,
        body: &str,
    ) -> Result<(), GitHubError> {
        let url = format!(
            "{REST_BASE}/repos/{}/{}/pulls/{}/comments/{in_reply_to}/replies",
            repo.owner(),
            repo.name(),
            number.0
        );
        self.execute(self.rest(Method::POST, &url).json(&json!({ "body": body })))
            .await?
            .check_status(&resource_name(repo, number))
    }

    /// Merge the pull request.
    ///
    /// A refusal (branch protection, failing checks, a conflict, a base branch
    /// that moved) comes back as [`GitHubError::MergeBlocked`] carrying
    /// GitHub's own explanation, not as a generic HTTP error.
    pub async fn merge(
        &self,
        repo: &RepoId,
        number: PrNumber,
        method: MergeMethod,
    ) -> Result<(), GitHubError> {
        let url = format!(
            "{REST_BASE}/repos/{}/{}/pulls/{}/merge",
            repo.owner(),
            repo.name(),
            number.0
        );
        let body = json!({ "merge_method": method.as_api_str() });

        let response = self
            .execute(self.rest(Method::PUT, &url).json(&body))
            .await?;
        if let Some(err) = classify_merge_status(response.status, &response.body) {
            return Err(err);
        }
        response.check_status(&resource_name(repo, number))
    }

    /// Close or reopen the pull request.
    pub async fn set_state(
        &self,
        repo: &RepoId,
        number: PrNumber,
        state: IssueState,
    ) -> Result<(), GitHubError> {
        let url = format!(
            "{REST_BASE}/repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            number.0
        );
        let body = json!({ "state": state.as_api_str() });
        self.execute(self.rest(Method::PATCH, &url).json(&body))
            .await?
            .check_status(&resource_name(repo, number))
    }

    /// A REST request with the auth and versioning headers already applied.
    fn rest(&self, method: Method, url: &str) -> RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(self.token.as_str())
            .header(ACCEPT, "application/vnd.github+json")
            .header("x-github-api-version", API_VERSION)
    }

    /// Send a request and read the whole response, keeping the status and
    /// headers that error classification needs.
    async fn execute(&self, request: RequestBuilder) -> Result<RawResponse, GitHubError> {
        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await?;
        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }
}

/// A response read to completion, before it is interpreted.
struct RawResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl RawResponse {
    fn check_status(&self, resource: &str) -> Result<(), GitHubError> {
        match classify_status(self.status, &self.headers, &self.body, resource) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

fn resource_name(repo: &RepoId, number: PrNumber) -> String {
    format!("{repo} {number}")
}

/// 405 and 409 from the merge endpoint both mean "GitHub will not merge this
/// right now" rather than a transport or permissions failure, so they become one
/// actionable error carrying GitHub's `message`.
fn classify_merge_status(status: StatusCode, body: &str) -> Option<GitHubError> {
    if !matches!(
        status,
        StatusCode::METHOD_NOT_ALLOWED | StatusCode::CONFLICT
    ) {
        return None;
    }
    Some(GitHubError::MergeBlocked {
        reason: rest_message(body),
    })
}

/// GitHub's REST errors put the human-readable cause in `message`; fall back to
/// the raw body when it is not JSON.
fn rest_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(|message| truncate(message, 300))
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| truncate(body, 300))
}

/// The URL of the next page, from the RFC 5988 `Link` header GitHub paginates
/// with. Following the header rather than incrementing a page counter is what
/// keeps `per_page` and cursor-based endpoints working the same way.
fn next_page_url(headers: &HeaderMap) -> Option<String> {
    let link = headers.get(reqwest::header::LINK)?.to_str().ok()?;
    for entry in link.split(',') {
        let mut parts = entry.split(';');
        let Some(url) = parts.next() else { continue };
        let is_next = parts.any(|param| matches!(param.trim(), r#"rel="next""# | "rel=next"));
        if is_next {
            let url = url.trim().trim_start_matches('<').trim_end_matches('>');
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}

/// Map a non-success HTTP status onto a structured error, distinguishing the
/// two very different meanings GitHub gives 403.
fn classify_status(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    resource: &str,
) -> Option<GitHubError> {
    if status.is_success() {
        return None;
    }

    Some(match status {
        StatusCode::UNAUTHORIZED => GitHubError::Unauthorized,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            if let Some(retry_after) = header_u64(headers, "retry-after") {
                GitHubError::SecondaryRateLimit {
                    retry_after_secs: retry_after,
                }
            } else if header_u64(headers, "x-ratelimit-remaining") == Some(0) {
                GitHubError::RateLimited {
                    reset_at: header_u64(headers, "x-ratelimit-reset")
                        .and_then(|secs| Utc.timestamp_opt(secs as i64, 0).single())
                        .unwrap_or_else(Utc::now),
                }
            } else {
                GitHubError::Forbidden {
                    reason: truncate(body, 300),
                }
            }
        }
        StatusCode::NOT_FOUND => GitHubError::NotFound {
            resource: resource.to_string(),
        },
        other => GitHubError::Unexpected {
            status: other.as_u16(),
            body: truncate(body, 300),
        },
    })
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    trimmed.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).expect("valid header"),
                HeaderValue::from_str(value).expect("valid value"),
            );
        }
        map
    }

    #[test]
    fn success_is_not_an_error() {
        assert!(classify_status(StatusCode::OK, &HeaderMap::new(), "", "a/b").is_none());
    }

    #[test]
    fn unauthorized_maps_to_token_error() {
        let err = classify_status(StatusCode::UNAUTHORIZED, &HeaderMap::new(), "", "a/b");
        assert!(matches!(err, Some(GitHubError::Unauthorized)));
    }

    /// 403 with an exhausted budget is a rate limit, not a permissions problem.
    #[test]
    fn forbidden_with_exhausted_budget_is_rate_limited() {
        let err = classify_status(
            StatusCode::FORBIDDEN,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1800000000"),
            ]),
            "",
            "a/b",
        );
        let Some(GitHubError::RateLimited { reset_at }) = err else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(reset_at.timestamp(), 1_800_000_000);
    }

    #[test]
    fn retry_after_takes_precedence_as_secondary_limit() {
        let err = classify_status(
            StatusCode::FORBIDDEN,
            &headers(&[("retry-after", "60"), ("x-ratelimit-remaining", "0")]),
            "",
            "a/b",
        );
        assert!(matches!(
            err,
            Some(GitHubError::SecondaryRateLimit {
                retry_after_secs: 60
            })
        ));
    }

    /// 403 with budget remaining is a genuine permissions/SSO failure.
    #[test]
    fn forbidden_with_budget_remaining_is_a_permissions_error() {
        let err = classify_status(
            StatusCode::FORBIDDEN,
            &headers(&[("x-ratelimit-remaining", "4000")]),
            "SAML enforcement",
            "a/b",
        );
        let Some(GitHubError::Forbidden { reason }) = err else {
            panic!("expected Forbidden, got {err:?}");
        };
        assert!(reason.contains("SAML"));
    }

    #[test]
    fn not_found_names_the_resource() {
        let err = classify_status(StatusCode::NOT_FOUND, &HeaderMap::new(), "", "a/b");
        let Some(GitHubError::NotFound { resource }) = err else {
            panic!("expected NotFound, got {err:?}");
        };
        assert_eq!(resource, "a/b");
    }

    /// A blocked merge is a normal outcome with a cause worth showing, not a
    /// generic HTTP failure.
    #[test]
    fn merge_refusals_carry_githubs_explanation() {
        for status in [StatusCode::METHOD_NOT_ALLOWED, StatusCode::CONFLICT] {
            let err = classify_merge_status(
                status,
                r#"{"message":"Required status check \"build\" is failing.","documentation_url":"..."}"#,
            );
            let Some(GitHubError::MergeBlocked { reason }) = err else {
                panic!("expected MergeBlocked for {status}, got {err:?}");
            };
            assert_eq!(reason, "Required status check \"build\" is failing.");
        }
    }

    #[test]
    fn other_merge_statuses_fall_through_to_normal_classification() {
        assert!(classify_merge_status(StatusCode::OK, "").is_none());
        assert!(classify_merge_status(StatusCode::FORBIDDEN, "").is_none());
        assert!(classify_merge_status(StatusCode::NOT_FOUND, "").is_none());
    }

    #[test]
    fn a_non_json_error_body_is_used_verbatim() {
        let err = classify_merge_status(StatusCode::CONFLICT, "<html>bad gateway</html>");
        let Some(GitHubError::MergeBlocked { reason }) = err else {
            panic!("expected MergeBlocked, got {err:?}");
        };
        assert_eq!(reason, "<html>bad gateway</html>");
    }

    #[test]
    fn merge_blocked_is_not_transient() {
        assert!(
            !GitHubError::MergeBlocked {
                reason: "conflict".into()
            }
            .is_transient()
        );
    }

    #[test]
    fn follows_the_link_header_to_the_next_page() {
        let map = headers(&[(
            "link",
            "<https://api.github.com/repositories/1/pulls/2/files?page=3>; rel=\"next\", \
             <https://api.github.com/repositories/1/pulls/2/files?page=9>; rel=\"last\"",
        )]);
        assert_eq!(
            next_page_url(&map).as_deref(),
            Some("https://api.github.com/repositories/1/pulls/2/files?page=3")
        );
    }

    /// The last page still has a `Link` header, just without a `next`.
    #[test]
    fn stops_when_no_next_relation_is_present() {
        let map = headers(&[(
            "link",
            "<https://api.github.com/repositories/1/pulls/2/files?page=1>; rel=\"first\", \
             <https://api.github.com/repositories/1/pulls/2/files?page=8>; rel=\"prev\"",
        )]);
        assert!(next_page_url(&map).is_none());
        assert!(next_page_url(&HeaderMap::new()).is_none());
    }

    #[test]
    fn resource_names_identify_the_pull_request() {
        let repo = RepoId::new("zed-industries", "zed");
        assert_eq!(resource_name(&repo, PrNumber(42)), "zed-industries/zed #42");
    }

    #[test]
    fn long_bodies_are_truncated() {
        let body = "x".repeat(1000);
        let err = classify_status(StatusCode::BAD_GATEWAY, &HeaderMap::new(), &body, "a/b");
        let Some(GitHubError::Unexpected { status, body }) = err else {
            panic!("expected Unexpected, got {err:?}");
        };
        assert_eq!(status, 502);
        assert!(body.chars().count() <= 301, "not truncated: {}", body.len());
    }
}
