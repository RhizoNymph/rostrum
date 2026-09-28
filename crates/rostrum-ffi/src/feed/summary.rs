//! One pull request as a feed row.

use rostrum_core::{LoginKey, PullRequest, RepoId, arrivals::is_review_requested_from};

use crate::{
    feed::{
        PrSummary,
        chips::{base_divergence, behind_chip, checks_role, feed_review_chip, merge_chip},
    },
    types::{LabelView, UserRef},
};

/// Everything a feed row shows. `viewer` decides "yours" and "your review is
/// requested"; with no viewer known yet both are `false`.
pub(crate) fn summarize(repo: &RepoId, pr: &PullRequest, viewer: Option<&LoginKey>) -> PrSummary {
    let merge = pr.merge_status();
    PrSummary {
        repo: repo.to_string(),
        number: pr.number.0,
        title: pr.title.clone(),
        url: pr.url.clone(),
        author: pr.author.as_ref().map(UserRef::from),
        created_at: pr.created_at.into(),
        updated_at: pr.updated_at.into(),
        is_draft: pr.is_draft,
        checks: pr.checks.map(Into::into),
        checks_role: checks_role(pr.checks),
        review_decision: pr.review_decision.map(Into::into),
        review_chip: feed_review_chip(pr.review_decision),
        merge_status: merge.into(),
        merge_chip: merge_chip(merge, pr.base_divergence),
        base_divergence: base_divergence(pr.base_divergence, &pr.base_ref),
        behind_chip: behind_chip(pr.base_divergence, &pr.base_ref),
        labels: pr.labels.iter().map(LabelView::from).collect(),
        additions: pr.additions,
        deletions: pr.deletions,
        changed_files: pr.changed_files,
        comment_count: pr.comment_count,
        review_requested: viewer.is_some_and(|viewer| is_review_requested_from(pr, viewer)),
        is_yours: viewer.is_some_and(|viewer| pr.is_authored_by(viewer)),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{
        CheckState, Divergence, Label, MergeStateStatus, Mergeable, ReviewDecision, User,
    };

    use super::*;
    use crate::{
        test_support::pull,
        types::{ColorRole, MergeStatus},
    };

    #[test]
    fn a_row_carries_what_the_feed_shows() {
        let repo: RepoId = "a/b".parse().expect("repo");
        let mut pr = pull(7);
        pr.author = Some(User {
            login: "Alice".into(),
            avatar_url: Some("https://a".into()),
        });
        pr.review_requests = vec![User {
            login: "ME".into(),
            avatar_url: None,
        }];
        pr.labels = vec![Label {
            name: "bug".into(),
            color: "d73a4a".into(),
        }];
        pr.checks = Some(CheckState::Failure);
        pr.review_decision = Some(ReviewDecision::Approved);
        pr.mergeable = Mergeable::Mergeable;
        pr.merge_state = MergeStateStatus::Behind;
        pr.base_divergence = Some(Divergence::new(1, 2));

        let me = LoginKey::new("me");
        let row = summarize(&repo, &pr, Some(&me));
        assert_eq!(row.repo, "a/b");
        assert_eq!(row.number, 7);
        assert_eq!(row.author.as_ref().map(|a| a.login.as_str()), Some("Alice"));
        assert_eq!(row.checks_role, ColorRole::Danger);
        assert_eq!(row.merge_status, MergeStatus::Behind);
        // The count replaces the plain chip.
        assert!(row.merge_chip.is_none());
        assert_eq!(row.behind_chip.map(|c| c.text), Some("↓2".to_string()));
        assert_eq!(row.review_chip.map(|c| c.text), Some("approved".to_string()));
        assert_eq!(row.labels[0].color, Some(0xFFD7_3A4A));
        assert!(row.review_requested);
        assert!(!row.is_yours);

        let alice = LoginKey::new("alice");
        let theirs = summarize(&repo, &pr, Some(&alice));
        assert!(theirs.is_yours);
        assert!(!theirs.review_requested);

        let anonymous = summarize(&repo, &pr, None);
        assert!(!anonymous.is_yours && !anonymous.review_requested);
    }
}
