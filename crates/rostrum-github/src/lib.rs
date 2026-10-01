//! GitHub data layer: token resolution, GraphQL reads, REST mutations.

pub mod auth;
pub mod client;
pub mod conversation;
pub mod error;
pub mod graphql;
pub mod issues;
pub mod rest;

pub use auth::{Token, resolve_token};
pub use client::GitHubClient;
pub use conversation::PULL_REQUEST_CONVERSATION;
pub use error::GitHubError;
pub use graphql::{BranchUpdateMethod, DraftState};
pub use issues::{
    Assignees, CloseAs, CommentBody, CreateIssue, DraftError, IssueDraft, IssueMutation,
    IssueStateChange, RepoIssues,
};
pub use rest::{
    AddLabels, DraftComment, IssueState, MergeMethod, MergePullRequest, PullRequestFile,
    ReviewEvent, SubmitReview,
};
