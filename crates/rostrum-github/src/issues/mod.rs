//! Issues: the open list the feed shows, the detail the issue pane shows, and
//! the REST requests that change or create one.

mod client;
pub mod draft;
pub mod edit;
pub mod rest;
pub mod wire;

pub use client::{RepoIssues, issue_variables};
pub use edit::{EditCheck, EditError, IssueEditor};
pub use draft::{DraftError, IssueDraft};
pub use rest::{
    AssignableUser, Assignees, CloseAs, IssueEdit, CommentBody, CreateIssue, CreatedIssue, EmptyComment,
    IssueMutation, IssueStateChange, RestCall,
};
pub use wire::{ISSUE_DETAIL, OPEN_ISSUES};
