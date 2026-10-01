//! The detail header: the facts the header, the Branch tab and the action bar
//! decide on, derived once.

use rostrum_core::{
    LoginKey, PullRequest, PullState as CoreState, RepoId, arrivals::is_review_requested_from,
};

use crate::{
    detail::{DraftAction, MergeVerdict, PullHeader},
    feed::chips::{base_divergence, checks_role, header_review_chip, merge_chip, merge_role},
    types::{LabelView, PullState, UserRef},
};

/// The header of `pr`. `state` is what the last conversation fetch said; a
/// pull request known only from the feed is open, since the feed holds only
/// open ones.
pub(crate) fn header(
    repo: &RepoId,
    pr: &PullRequest,
    viewer: Option<&LoginKey>,
    state: Option<CoreState>,
) -> PullHeader {
    let merge = pr.merge_status();
    PullHeader {
        repo: repo.to_string(),
        number: pr.number.0,
        title: pr.title.clone(),
        url: pr.url.clone(),
        state: match state.unwrap_or(CoreState::Open) {
            CoreState::Open => PullState::Open,
            CoreState::Closed => PullState::Closed,
            CoreState::Merged => PullState::Merged,
        },
        is_draft: pr.is_draft,
        author: pr.author.as_ref().map(UserRef::from),
        created_at: pr.created_at.into(),
        updated_at: pr.updated_at.into(),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
        head_sha: pr.head_sha.clone(),
        labels: pr.labels.iter().map(LabelView::from).collect(),
        assignees: pr.assignees.iter().map(UserRef::from).collect(),
        review_requests: pr.review_requests.iter().map(UserRef::from).collect(),
        review_decision: pr.review_decision.map(Into::into),
        review_chip: header_review_chip(pr.review_decision),
        merge: MergeVerdict {
            status: merge.into(),
            sentence: merge.explanation().to_string(),
            blocks_merge: merge.blocks_merge(),
            role: merge_role(merge),
            chip: merge_chip(merge, pr.base_divergence),
        },
        divergence: base_divergence(pr.base_divergence, &pr.base_ref),
        checks: pr.checks.map(Into::into),
        checks_role: checks_role(pr.checks),
        changed_files: pr.changed_files,
        additions: pr.additions,
        deletions: pr.deletions,
        comment_count: pr.comment_count,
        is_yours: viewer.is_some_and(|viewer| pr.is_authored_by(viewer)),
        review_requested: viewer.is_some_and(|viewer| is_review_requested_from(pr, viewer)),
        draft_action: if pr.is_draft {
            DraftAction {
                to_draft: false,
                label: "Ready for review".into(),
            }
        } else {
            DraftAction {
                to_draft: true,
                label: "Convert to draft".into(),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{Divergence, MergeStateStatus, Mergeable};

    use super::*;
    use crate::{
        test_support::pull_by,
        types::{ColorRole, MergeStatus},
    };

    fn repo() -> RepoId {
        "a/b".parse().expect("repo")
    }

    #[test]
    fn the_verdict_says_why_and_whether_it_blocks() {
        let mut pr = pull_by(3, "alice");
        pr.mergeable = Mergeable::Mergeable;
        pr.merge_state = MergeStateStatus::Blocked;
        let header = header(&repo(), &pr, None, None);
        assert_eq!(header.merge.status, MergeStatus::Blocked);
        assert!(header.merge.blocks_merge);
        assert_eq!(header.merge.role, ColorRole::Warning);
        assert!(header.merge.sentence.contains("branch protection"));

        pr.merge_state = MergeStateStatus::Unstable;
        let unstable = super::header(&repo(), &pr, None, None);
        assert!(!unstable.merge.blocks_merge);
    }

    #[test]
    fn the_draft_action_names_the_state_it_moves_to() {
        let mut pr = pull_by(3, "alice");
        let open = header(&repo(), &pr, None, None);
        assert!(open.draft_action.to_draft);
        assert_eq!(open.draft_action.label, "Convert to draft");
        pr.is_draft = true;
        let draft = header(&repo(), &pr, None, None);
        assert!(!draft.draft_action.to_draft);
        assert_eq!(draft.merge.status, MergeStatus::Draft);
    }

    #[test]
    fn state_divergence_and_ownership() {
        let mut pr = pull_by(3, "Alice");
        pr.base_divergence = Some(Divergence::new(0, 2));
        let alice = LoginKey::new("alice");
        let header = header(&repo(), &pr, Some(&alice), Some(CoreState::Merged));
        assert_eq!(header.state, PullState::Merged);
        assert!(header.is_yours);
        let divergence = header.divergence.expect("divergence");
        assert!(divergence.fast_forwards);
        assert_eq!(divergence.behind, 2);
        assert_eq!(header.head_sha, "headsha");
    }
}
