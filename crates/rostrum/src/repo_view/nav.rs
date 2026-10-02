//! Keyboard movement through the repository view's two lists.
//!
//! The sidebar is pull requests above and issues below; `j`/`k` walk the
//! first and carry on into the second, as one sequence. Pure, so the edges
//! are testable without a window.

use crate::nav::Nav;

/// Which list a position is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pulls,
    Issues,
}

/// A row in one of the two lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub section: Section,
    pub index: usize,
}

impl Position {
    pub fn pull(index: usize) -> Self {
        Self {
            section: Section::Pulls,
            index,
        }
    }

    pub fn issue(index: usize) -> Self {
        Self {
            section: Section::Issues,
            index,
        }
    }

    /// Place in the combined sequence.
    fn ordinal(self, pulls: usize) -> usize {
        match self.section {
            Section::Pulls => self.index,
            Section::Issues => pulls + self.index,
        }
    }

    fn from_ordinal(ordinal: usize, pulls: usize) -> Self {
        if ordinal < pulls {
            Self::pull(ordinal)
        } else {
            Self::issue(ordinal - pulls)
        }
    }
}

/// Where `nav` lands from `current`, given the two lists' lengths.
///
/// Same rules as the feed: no wrapping, and with nothing selected (or a
/// selection that no longer exists) `Next`/`First` enter at the top and
/// `Previous`/`Last` at the bottom. `None` only when both lists are empty.
pub fn step(pulls: usize, issues: usize, current: Option<Position>, nav: Nav) -> Option<Position> {
    let total = pulls + issues;
    if total == 0 {
        return None;
    }
    let current = current
        .filter(|position| match position.section {
            Section::Pulls => position.index < pulls,
            Section::Issues => position.index < issues,
        })
        .map(|position| position.ordinal(pulls));
    let target = match (nav, current) {
        (Nav::First, _) | (Nav::Next, None) => 0,
        (Nav::Last, _) | (Nav::Previous, None) => total - 1,
        (Nav::Next, Some(at)) => (at + 1).min(total - 1),
        (Nav::Previous, Some(at)) => at.saturating_sub(1),
    };
    Some(Position::from_ordinal(target, pulls))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lists_have_nowhere_to_go() {
        for nav in [Nav::Next, Nav::Previous, Nav::First, Nav::Last] {
            assert_eq!(step(0, 0, None, nav), None, "{nav:?}");
        }
    }

    #[test]
    fn nothing_selected_enters_from_the_matching_end() {
        assert_eq!(step(3, 2, None, Nav::Next), Some(Position::pull(0)));
        assert_eq!(step(3, 2, None, Nav::First), Some(Position::pull(0)));
        assert_eq!(step(3, 2, None, Nav::Previous), Some(Position::issue(1)));
        assert_eq!(step(3, 2, None, Nav::Last), Some(Position::issue(1)));
    }

    #[test]
    fn moving_past_the_last_pull_request_continues_into_issues() {
        assert_eq!(
            step(2, 2, Some(Position::pull(1)), Nav::Next),
            Some(Position::issue(0))
        );
        assert_eq!(
            step(2, 2, Some(Position::issue(0)), Nav::Previous),
            Some(Position::pull(1))
        );
    }

    #[test]
    fn movement_does_not_wrap() {
        assert_eq!(
            step(2, 1, Some(Position::issue(0)), Nav::Next),
            Some(Position::issue(0))
        );
        assert_eq!(
            step(2, 1, Some(Position::pull(0)), Nav::Previous),
            Some(Position::pull(0))
        );
    }

    /// Until issues arrive the second list is empty, and the pull requests
    /// behave as a list of their own.
    #[test]
    fn with_no_issues_the_pull_requests_stand_alone() {
        assert_eq!(step(3, 0, None, Nav::Last), Some(Position::pull(2)));
        assert_eq!(
            step(3, 0, Some(Position::pull(2)), Nav::Next),
            Some(Position::pull(2))
        );
    }

    #[test]
    fn a_vanished_position_counts_as_nothing_selected() {
        assert_eq!(
            step(2, 0, Some(Position::pull(7)), Nav::Next),
            Some(Position::pull(0))
        );
        assert_eq!(
            step(2, 0, Some(Position::issue(0)), Nav::Previous),
            Some(Position::pull(1))
        );
    }
}
