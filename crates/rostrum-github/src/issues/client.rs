//! [`GitHubClient`] methods for issues.
//!
//! Reads are GraphQL, like the pull request feed; writes are REST, through
//! the [`RestCall`]s the request types describe.

use reqwest::Method;
use rostrum_core::{IssueDetail, IssueNumber, RepoId, User};
use serde_json::json;

use crate::{
    client::{GitHubClient, MAX_PAGES, classify_label_removal, next_page_url},
    error::GitHubError,
    graphql::RateLimit,
    issues::{
        rest::{AssignableUser, CreateIssue, CreatedIssue, IssueMutation, RestCall},
        wire::{ISSUE_DETAIL, IssueDetailData, OPEN_ISSUES, OpenIssuesData},
    },
};

/// Result of one repository's issue refresh.
#[derive(Debug)]
pub struct RepoIssues {
    pub issues: Vec<rostrum_core::Issue>,
    pub rate_limit: Option<RateLimit>,
}

fn issue_resource(repo: &RepoId, number: IssueNumber) -> String {
    format!("{repo} issue {number}")
}

impl GitHubClient {
    /// Open issues for one repository, most recently updated first.
    pub async fn open_issues(&self, repo: &RepoId, limit: u32) -> Result<RepoIssues, GitHubError> {
        let resource = format!("{repo} issues");
        let data: OpenIssuesData = self
            .graphql(
                OPEN_ISSUES,
                json!({ "owner": repo.owner(), "name": repo.name(), "first": limit }),
                &resource,
            )
            .await?;
        let rate_limit = data.rate_limit.clone();
        let issues = data.into_domain().ok_or_else(|| GitHubError::NotFound {
            resource: repo.to_string(),
        })?;
        Ok(RepoIssues { issues, rate_limit })
    }

    /// The issue as it stands now, with its timeline. Works for closed issues
    /// too, which is what lets the pane offer to reopen one.
    pub async fn issue_detail(
        &self,
        repo: &RepoId,
        number: IssueNumber,
    ) -> Result<IssueDetail, GitHubError> {
        let resource = issue_resource(repo, number);
        let data: IssueDetailData = self
            .graphql(
                ISSUE_DETAIL,
                json!({ "owner": repo.owner(), "name": repo.name(), "number": number.0 }),
                &resource,
            )
            .await?;
        data.repository
            .and_then(|repository| repository.issue)
            .map(|issue| issue.into_detail())
            .ok_or(GitHubError::NotFound { resource })
    }

    /// Apply one change to an issue.
    ///
    /// A no-op mutation (an empty list to add) sends nothing. Removing a
    /// label the issue does not carry is success, as it is for pull
    /// requests: the end state the user asked for already holds.
    pub async fn mutate_issue(
        &self,
        repo: &RepoId,
        number: IssueNumber,
        mutation: &IssueMutation,
    ) -> Result<(), GitHubError> {
        if mutation.is_noop() {
            return Ok(());
        }
        let resource = issue_resource(repo, number);
        let response = self.send(mutation.call(repo, number)).await?;
        if matches!(mutation, IssueMutation::RemoveLabel(_)) {
            return match classify_label_removal(
                response.status,
                &response.headers,
                &response.body,
                &resource,
            ) {
                Some(error) => Err(error),
                None => Ok(()),
            };
        }
        response.check_status(&resource)
    }

    /// Open a new issue, answering with its number.
    pub async fn create_issue(
        &self,
        repo: &RepoId,
        request: &CreateIssue,
    ) -> Result<IssueNumber, GitHubError> {
        let resource = format!("{repo} new issue");
        let response = self.send(request.call(repo)).await?;
        response.check_status(&resource)?;
        let created: CreatedIssue =
            serde_json::from_str(&response.body).map_err(|source| GitHubError::Decode {
                context: format!("the issue created in {repo}"),
                source,
            })?;
        tracing::info!(%repo, number = created.number, url = %created.html_url, "issue created");
        Ok(IssueNumber(created.number))
    }

    /// Everyone an issue in this repository can be assigned to, following
    /// pagination to the end.
    pub async fn assignable_users(&self, repo: &RepoId) -> Result<Vec<User>, GitHubError> {
        let resource = format!("{repo} assignees");
        let mut url = format!(
            "{}/repos/{}/{}/assignees?per_page=100",
            self.rest_base,
            repo.owner(),
            repo.name()
        );
        let mut users = Vec::new();
        for _ in 0..MAX_PAGES {
            let response = self.execute(self.rest(Method::GET, &url)).await?;
            response.check_status(&resource)?;
            let page: Vec<AssignableUser> =
                serde_json::from_str(&response.body).map_err(|source| GitHubError::Decode {
                    context: format!("assignees for {repo}"),
                    source,
                })?;
            users.extend(page.into_iter().map(User::from));
            match next_page_url(&response.headers) {
                Some(next) => url = next,
                None => break,
            }
        }
        Ok(users)
    }

    async fn send(&self, call: RestCall) -> Result<crate::client::RawResponse, GitHubError> {
        let url = format!("{}{}", self.rest_base, call.path);
        let request = self.rest(call.method, &url);
        let request = match &call.body {
            Some(body) => request.json(body),
            None => request,
        };
        self.execute(request).await
    }
}
