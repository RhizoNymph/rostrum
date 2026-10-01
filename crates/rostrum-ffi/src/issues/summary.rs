//! One issue as a row.

use rostrum_core::{CloseReason, Issue, IssueState, LoginKey, RepoId};

use crate::{
    issues::{IssueCloseReason, IssueStatus, IssueSummary},
    types::{Chip, ColorRole, LabelView, UserRef},
};

impl From<CloseReason> for IssueCloseReason {
    fn from(reason: CloseReason) -> Self {
        match reason {
            CloseReason::Completed => Self::Completed,
            CloseReason::NotPlanned => Self::NotPlanned,
            CloseReason::Duplicate => Self::Duplicate,
        }
    }
}

impl From<IssueState> for IssueStatus {
    fn from(state: IssueState) -> Self {
        match state {
            IssueState::Open => Self::Open,
            IssueState::Closed(reason) => Self::Closed {
                reason: reason.map(Into::into),
            },
        }
    }
}

/// The state chip: open in the success colour, closed as completed in the
/// accent colour (GitHub's purple), closed otherwise neutral.
pub(crate) fn status_chip(state: IssueState) -> Chip {
    let role = match state {
        IssueState::Open => ColorRole::Success,
        IssueState::Closed(None | Some(CloseReason::Completed)) => ColorRole::Accent,
        IssueState::Closed(Some(CloseReason::NotPlanned | CloseReason::Duplicate)) => {
            ColorRole::Neutral
        }
    };
    Chip {
        text: state.label().to_string(),
        role,
        tooltip: None,
    }
}

/// Everything an issue row shows. `viewer` decides "yours" and "assigned to
/// you"; with no viewer known yet both are `false`.
pub(crate) fn summarize_issue(
    repo: &RepoId,
    issue: &Issue,
    viewer: Option<&LoginKey>,
) -> IssueSummary {
    IssueSummary {
        repo: repo.to_string(),
        number: issue.number.0,
        title: issue.title.clone(),
        url: issue.url.clone(),
        status: issue.state.into(),
        status_chip: status_chip(issue.state),
        author: issue.author.as_ref().map(UserRef::from),
        created_at: issue.created_at.into(),
        updated_at: issue.updated_at.into(),
        labels: issue.labels.iter().map(LabelView::from).collect(),
        assignees: issue.assignees.iter().map(UserRef::from).collect(),
        comment_count: issue.comment_count,
        milestone: issue
            .milestone
            .as_ref()
            .map(|milestone| milestone.title.clone()),
        is_yours: viewer.is_some_and(|viewer| issue.is_authored_by(viewer)),
        assigned_to_you: viewer
            .is_some_and(|viewer| issue.assignees.iter().any(|user| &user.key() == viewer)),
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{Label, Milestone, User};

    use super::*;
    use crate::test_support::issue;

    #[test]
    fn a_row_carries_what_the_issue_list_shows() {
        let repo: RepoId = "a/b".parse().expect("repo");
        let mut open = issue(7, "Alice");
        open.labels = vec![Label {
            name: "bug".into(),
            color: "d73a4a".into(),
        }];
        open.assignees = vec![User {
            login: "ME".into(),
            avatar_url: None,
        }];
        open.comment_count = 3;
        open.milestone = Some(Milestone { title: "v1".into() });

        let me = LoginKey::new("me");
        let row = summarize_issue(&repo, &open, Some(&me));
        assert_eq!(row.repo, "a/b");
        assert_eq!(row.number, 7);
        assert_eq!(row.status, IssueStatus::Open);
        assert_eq!(row.status_chip.role, ColorRole::Success);
        assert_eq!(row.labels[0].color, Some(0xFFD7_3A4A));
        assert_eq!(row.assignees[0].login, "ME");
        assert_eq!(row.comment_count, 3);
        assert_eq!(row.milestone.as_deref(), Some("v1"));
        assert!(row.assigned_to_you);
        assert!(!row.is_yours);

        let alice = summarize_issue(&repo, &open, Some(&LoginKey::new("alice")));
        assert!(alice.is_yours && !alice.assigned_to_you);
        let nobody = summarize_issue(&repo, &open, None);
        assert!(!nobody.is_yours && !nobody.assigned_to_you);
    }

    #[test]
    fn closed_issues_carry_their_reason_and_chip() {
        for (state, status, role, text) in [
            (
                IssueState::Closed(Some(CloseReason::Completed)),
                IssueStatus::Closed {
                    reason: Some(IssueCloseReason::Completed),
                },
                ColorRole::Accent,
                "closed",
            ),
            (
                IssueState::Closed(Some(CloseReason::NotPlanned)),
                IssueStatus::Closed {
                    reason: Some(IssueCloseReason::NotPlanned),
                },
                ColorRole::Neutral,
                "closed: not planned",
            ),
            (
                IssueState::Closed(None),
                IssueStatus::Closed { reason: None },
                ColorRole::Accent,
                "closed",
            ),
        ] {
            assert_eq!(IssueStatus::from(state), status);
            let chip = status_chip(state);
            assert_eq!((chip.role, chip.text.as_str()), (role, text));
        }
    }
}
