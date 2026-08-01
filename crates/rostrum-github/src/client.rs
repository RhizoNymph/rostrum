//! HTTP client for the GitHub API.

use chrono::{TimeZone, Utc};
use reqwest::{Client, StatusCode, header::HeaderMap};
use rostrum_core::{PullRequest, RepoId};
use serde_json::json;

use crate::{
    auth::Token,
    error::GitHubError,
    graphql::{self, GraphQlResponse, PrNode, RateLimit, RepoQueryData},
};

const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const USER_AGENT: &str = concat!("rostrum/", env!("CARGO_PKG_VERSION"));

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
