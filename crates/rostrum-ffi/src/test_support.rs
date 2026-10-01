//! Builders shared by this crate's unit tests.

use chrono::{DateTime, Utc};
use rostrum_core::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, User};

/// A fixed instant `number` hours into 2025, so higher numbers are newer and
/// sorted orders are deterministic.
pub(crate) fn at(number: u32) -> DateTime<Utc> {
    DateTime::from_timestamp(1_735_689_600 + i64::from(number) * 3600, 0).expect("in range")
}

/// An open, non-draft pull request with every optional field empty, created
/// and updated at [`at`]`(number)`.
pub(crate) fn pull(number: u32) -> PullRequest {
    PullRequest {
        number: PrNumber(number),
        node_id: NodeId(format!("PR_{number}")),
        title: format!("PR {number}"),
        url: format!("https://github.com/a/b/pull/{number}"),
        is_draft: false,
        created_at: at(number),
        updated_at: at(number),
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
        pushed_at: None,
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

/// An open issue opened by `login`, with every optional field empty,
/// created and updated at [`at`]`(number)`.
pub(crate) fn issue(number: u32, login: &str) -> rostrum_core::Issue {
    rostrum_core::Issue {
        number: rostrum_core::IssueNumber(number),
        node_id: NodeId(format!("I_{number}")),
        title: format!("Issue {number}"),
        url: format!("https://github.com/a/b/issues/{number}"),
        state: rostrum_core::IssueState::Open,
        created_at: at(number),
        updated_at: at(number),
        author: Some(User {
            login: login.into(),
            avatar_url: None,
        }),
        assignees: Vec::new(),
        labels: Vec::new(),
        comment_count: 0,
        milestone: None,
    }
}
