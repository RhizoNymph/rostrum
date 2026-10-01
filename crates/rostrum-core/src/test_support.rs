//! Builders shared by this crate's unit tests.

use chrono::{DateTime, Utc};

use crate::model::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest};

/// One fixed instant for every fixture: the feed sorts by creation time, and
/// `Utc::now()` per call would order otherwise-identical fixtures by the
/// clock.
pub(crate) fn fixed_time() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
}

/// An open, non-draft pull request with every optional field empty.
pub(crate) fn pull(number: u32) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("PR {number}"),
        url: String::new(),
        is_draft: false,
        created_at: fixed_time(),
        updated_at: fixed_time(),
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
        pushed_at: None,
    }
}

/// An open issue with every optional field empty.
pub(crate) fn issue(number: u32) -> crate::issue::Issue {
    use crate::issue::{Issue, IssueNumber, IssueState};
    Issue {
        number: IssueNumber(number),
        node_id: NodeId(format!("I_{number}")),
        title: format!("Issue {number}"),
        url: String::new(),
        state: IssueState::Open,
        created_at: fixed_time(),
        updated_at: fixed_time(),
        author: None,
        assignees: Vec::new(),
        labels: Vec::new(),
        comment_count: 0,
        milestone: None,
    }
}
