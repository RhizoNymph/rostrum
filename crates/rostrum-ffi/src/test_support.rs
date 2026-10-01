//! Builders shared by this crate's unit tests.

use chrono::Utc;
use rostrum_core::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, User};

/// An open, non-draft pull request with every optional field empty.
pub(crate) fn pull(number: u32) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("PR {number}"),
        url: format!("https://github.com/a/b/pull/{number}"),
        is_draft: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        author: None,
        head_ref: format!("branch-{number}"),
        head_sha: "headsha".into(),
        base_ref: "main".into(),
        additions: 0,
        deletions: 0,
        changed_files: 0,
        mergeable: Mergeable::Mergeable,
        merge_state: MergeStateStatus::Clean,
        review_decision: None,
        assignees: Vec::new(),
        review_requests: Vec::new(),
        labels: Vec::new(),
        comment_count: 0,
        checks: None,
        base_divergence: None,
        is_cross_repository: false,
    }
}

/// [`pull`] opened by `login`.
pub(crate) fn pull_by(number: u32, login: &str) -> PullRequest {
    PullRequest {
        author: Some(User {
            login: login.into(),
            avatar_url: None,
        }),
        ..pull(number)
    }
}
