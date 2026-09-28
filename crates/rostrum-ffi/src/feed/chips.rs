//! Chip text and colour roles, decided once for every screen.
//!
//! The rules mirror the desktop's theme and feed rows exactly, so a pull
//! request looks the same on both: only conflicts are red, protection and a
//! stale base are amber, and a chip that would repeat what another element
//! already says is left out.

use rostrum_core::{
    CheckState as CoreCheck, Divergence, MergeStatus as CoreMerge, ReviewDecision as CoreDecision,
    ReviewState as CoreReviewState,
};

use crate::{
    feed::BaseDivergence,
    types::{Chip, ColorRole},
};

/// Colour of a merge verdict. Only conflicts are danger: they are the one
/// state a branch cannot leave without someone editing code.
pub(crate) fn merge_role(status: CoreMerge) -> ColorRole {
    match status {
        CoreMerge::Conflicts => ColorRole::Danger,
        CoreMerge::Blocked | CoreMerge::Behind | CoreMerge::Unstable => ColorRole::Warning,
        CoreMerge::Ready => ColorRole::Success,
        CoreMerge::Draft => ColorRole::Draft,
        CoreMerge::Computing => ColorRole::Neutral,
    }
}

/// The merge chip: `conflict`, `behind` or `blocked`, explained in the
/// tooltip. `behind` yields to the exact count when the divergence says the
/// branch is behind — the count says the same thing with more information.
pub(crate) fn merge_chip(status: CoreMerge, divergence: Option<Divergence>) -> Option<Chip> {
    let counted = divergence.is_some_and(Divergence::is_behind);
    let text = status
        .chip()
        .filter(|_| !(counted && status == CoreMerge::Behind))?;
    Some(Chip {
        text: text.to_string(),
        role: merge_role(status),
        tooltip: Some(status.explanation().to_string()),
    })
}

/// `↓N` when the branch is behind its base; nothing when current or merely
/// ahead, which is every open pull request's normal state.
pub(crate) fn behind_chip(divergence: Option<Divergence>, base_ref: &str) -> Option<Chip> {
    let divergence = divergence.filter(|d| d.is_behind())?;
    Some(Chip {
        text: format!("↓{}", divergence.behind),
        role: ColorRole::Warning,
        tooltip: Some(format!(
            "{} commit(s) behind {base_ref}",
            divergence.behind
        )),
    })
}

/// The distance from base, with its sentence.
pub(crate) fn base_divergence(
    divergence: Option<Divergence>,
    base_ref: &str,
) -> Option<BaseDivergence> {
    let divergence = divergence?;
    let summary = match (divergence.behind, divergence.ahead) {
        (0, 0) => format!("up to date with {base_ref}"),
        (0, ahead) => format!("{ahead} commit(s) ahead of {base_ref}"),
        (behind, 0) => format!("{behind} commit(s) behind {base_ref}"),
        (behind, ahead) => format!("{behind} behind {base_ref}, {ahead} ahead"),
    };
    Some(BaseDivergence {
        behind: divergence.behind,
        ahead: divergence.ahead,
        base_ref: base_ref.to_string(),
        fast_forwards: divergence.fast_forwards(),
        summary,
    })
}

/// The feed row's review chip: a verdict worth a glance. "Review required" is
/// the default state of most open pull requests and earns nothing.
pub(crate) fn feed_review_chip(decision: Option<CoreDecision>) -> Option<Chip> {
    let (text, role) = match decision? {
        CoreDecision::Approved => ("approved", ColorRole::Success),
        CoreDecision::ChangesRequested => ("changes", ColorRole::Danger),
        CoreDecision::ReviewRequired => return None,
    };
    Some(Chip {
        text: text.to_string(),
        role,
        tooltip: None,
    })
}

/// The detail header's review chip, which has room to say more.
pub(crate) fn header_review_chip(decision: Option<CoreDecision>) -> Option<Chip> {
    let (text, role) = match decision? {
        CoreDecision::Approved => ("approved", ColorRole::Success),
        CoreDecision::ChangesRequested => ("changes requested", ColorRole::Danger),
        CoreDecision::ReviewRequired => ("review required", ColorRole::Neutral),
    };
    Some(Chip {
        text: text.to_string(),
        role,
        tooltip: None,
    })
}

/// Colour of a CI dot.
pub(crate) fn checks_role(state: Option<CoreCheck>) -> ColorRole {
    match state {
        Some(CoreCheck::Success) => ColorRole::Success,
        Some(CoreCheck::Failure | CoreCheck::Error) => ColorRole::Danger,
        Some(CoreCheck::Pending | CoreCheck::Expected) => ColorRole::Warning,
        None => ColorRole::Neutral,
    }
}

/// `success`, `failure`, … as one word, or `no status`.
pub(crate) fn checks_text(state: Option<CoreCheck>) -> &'static str {
    match state {
        Some(CoreCheck::Success) => "success",
        Some(CoreCheck::Failure) => "failure",
        Some(CoreCheck::Error) => "error",
        Some(CoreCheck::Pending) => "pending",
        Some(CoreCheck::Expected) => "expected",
        None => "no status",
    }
}

/// The chip on a submitted review in the conversation.
pub(crate) fn review_state_chip(state: CoreReviewState) -> Chip {
    let (text, role) = match state {
        CoreReviewState::Approved => ("approved", ColorRole::Success),
        CoreReviewState::ChangesRequested => ("requested changes", ColorRole::Danger),
        CoreReviewState::Commented => ("reviewed", ColorRole::Neutral),
        CoreReviewState::Dismissed => ("dismissed", ColorRole::Neutral),
        CoreReviewState::Pending => ("pending", ColorRole::Warning),
    };
    Chip {
        text: text.to_string(),
        role,
        tooltip: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_conflicts_are_danger() {
        assert_eq!(merge_role(CoreMerge::Conflicts), ColorRole::Danger);
        for amber in [CoreMerge::Blocked, CoreMerge::Behind, CoreMerge::Unstable] {
            assert_eq!(merge_role(amber), ColorRole::Warning);
        }
        assert_eq!(merge_role(CoreMerge::Ready), ColorRole::Success);
        assert_eq!(merge_role(CoreMerge::Draft), ColorRole::Draft);
        assert_eq!(merge_role(CoreMerge::Computing), ColorRole::Neutral);
    }

    #[test]
    fn merge_chips_exist_only_for_states_worth_one() {
        for (status, text) in [
            (CoreMerge::Conflicts, Some("conflict")),
            (CoreMerge::Behind, Some("behind")),
            (CoreMerge::Blocked, Some("blocked")),
            (CoreMerge::Draft, None),
            (CoreMerge::Unstable, None),
            (CoreMerge::Ready, None),
            (CoreMerge::Computing, None),
        ] {
            assert_eq!(
                merge_chip(status, None).map(|chip| chip.text),
                text.map(str::to_string),
                "{status:?}"
            );
        }
        let chip = merge_chip(CoreMerge::Blocked, None).expect("chip");
        assert_eq!(chip.role, ColorRole::Warning);
        assert_eq!(
            chip.tooltip.as_deref(),
            Some(CoreMerge::Blocked.explanation())
        );
    }

    /// The count replaces the plain `behind` chip; every other merge chip
    /// still shows next to it.
    #[test]
    fn a_known_behind_count_suppresses_only_the_behind_chip() {
        let behind = Some(Divergence::new(1, 3));
        assert!(merge_chip(CoreMerge::Behind, behind).is_none());
        assert!(merge_chip(CoreMerge::Conflicts, behind).is_some());
        // Merely ahead does not count as knowing it is behind.
        assert!(merge_chip(CoreMerge::Behind, Some(Divergence::new(2, 0))).is_some());
    }

    #[test]
    fn the_behind_chip_counts_and_names_the_base() {
        let chip = behind_chip(Some(Divergence::new(0, 4)), "main").expect("chip");
        assert_eq!(chip.text, "↓4");
        assert_eq!(chip.role, ColorRole::Warning);
        assert_eq!(chip.tooltip.as_deref(), Some("4 commit(s) behind main"));
        assert!(behind_chip(Some(Divergence::new(3, 0)), "main").is_none());
        assert!(behind_chip(None, "main").is_none());
    }

    #[test]
    fn divergence_sentences_cover_every_shape() {
        let text = |ahead, behind| {
            base_divergence(Some(Divergence::new(ahead, behind)), "main")
                .expect("some")
                .summary
        };
        assert_eq!(text(0, 0), "up to date with main");
        assert_eq!(text(2, 0), "2 commit(s) ahead of main");
        assert_eq!(text(0, 3), "3 commit(s) behind main");
        assert_eq!(text(2, 3), "3 behind main, 2 ahead");
        let fast = base_divergence(Some(Divergence::new(0, 3)), "main").expect("some");
        assert!(fast.fast_forwards);
        assert!(base_divergence(None, "main").is_none());
    }

    #[test]
    fn review_chips_differ_between_the_row_and_the_header() {
        assert_eq!(
            feed_review_chip(Some(CoreDecision::ChangesRequested)).map(|c| c.text),
            Some("changes".to_string())
        );
        assert!(feed_review_chip(Some(CoreDecision::ReviewRequired)).is_none());
        assert!(feed_review_chip(None).is_none());
        assert_eq!(
            header_review_chip(Some(CoreDecision::ReviewRequired)).map(|c| (c.text, c.role)),
            Some(("review required".to_string(), ColorRole::Neutral))
        );
        assert_eq!(
            header_review_chip(Some(CoreDecision::Approved)).map(|c| c.role),
            Some(ColorRole::Success)
        );
    }

    #[test]
    fn check_colours_and_words() {
        assert_eq!(checks_role(Some(CoreCheck::Success)), ColorRole::Success);
        assert_eq!(checks_role(Some(CoreCheck::Error)), ColorRole::Danger);
        assert_eq!(checks_role(Some(CoreCheck::Expected)), ColorRole::Warning);
        assert_eq!(checks_role(None), ColorRole::Neutral);
        assert_eq!(checks_text(Some(CoreCheck::Failure)), "failure");
        assert_eq!(checks_text(None), "no status");
    }

    #[test]
    fn review_state_chips() {
        assert_eq!(
            review_state_chip(CoreReviewState::ChangesRequested).text,
            "requested changes"
        );
        assert_eq!(
            review_state_chip(CoreReviewState::Pending).role,
            ColorRole::Warning
        );
    }
}
