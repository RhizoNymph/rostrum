//! Builders shared by this crate's unit tests.

use chrono::Utc;

use crate::model::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest};

/// An open, non-draft pull request with every optional field empty.
pub(crate) fn pull(number: u32) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("PR {number}"),
        url: String::new(),
        is_draft: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        author: None,
        head_ref: "feature".into(),
        head_sha: "deadbeef".into(),
        base_ref: "main".into(),
        additions: 0,
        deletions: 0,
        changed_files: 0,
        mergeable: Mergeable::Unknown,
        merge_state: MergeStateStatus::Unknown,
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
