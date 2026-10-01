//! Issues: the open list the feed shows, the detail the issue pane shows, and
//! the REST requests that change or create one.

mod client;
pub mod draft;
pub mod rest;
pub mod wire;

pub use client::RepoIssues;
pub use draft::{DraftError, IssueDraft};
pub use rest::{
    AssignableUser, Assignees, CloseAs, CommentBody, CreateIssue, CreatedIssue, EmptyComment,
    IssueMutation, IssueStateChange, RestCall,
};
pub use wire::{ISSUE_DETAIL, OPEN_ISSUES};
