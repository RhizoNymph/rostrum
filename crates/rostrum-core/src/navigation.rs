//! Which screen the left pane shows: the multi-repository feed, or one
//! repository's own view.
//!
//! Navigation is state of the app, not rows of the feed: entering a
//! repository swaps the whole left pane, and leaving it puts the feed back
//! as it was. The feed's scroll position survives on its own — its list is
//! kept, merely not drawn — so what this type manages is the selection,
//! which the two screens share.

use crate::{model::RepoId, state::Selection};

/// The left pane's screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Feed,
    Repo(RepoScreen),
}

/// One repository's view, and what to restore on the way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoScreen {
    repo: RepoId,
    /// The feed's selection when the repository was entered. Restored on
    /// leaving if nothing was picked inside the repository view.
    feed_selection: Option<Selection>,
}

impl RepoScreen {
    pub fn repo(&self) -> &RepoId {
        &self.repo
    }
}

fn is_in(selection: &Selection, repo: &RepoId) -> bool {
    selection.repo() == repo
}

impl Screen {
    /// The repository being viewed, if any.
    pub fn repo(&self) -> Option<&RepoId> {
        match self {
            Self::Feed => None,
            Self::Repo(screen) => Some(&screen.repo),
        }
    }

    pub fn is_feed(&self) -> bool {
        matches!(self, Self::Feed)
    }

    /// Show `repo`'s view.
    ///
    /// A selection inside `repo` is kept — the user was looking at one of
    /// its pull requests and still is. A selection elsewhere is cleared, so
    /// the right pane shows the repository's branches rather than a pull
    /// request from another repository. Either way the feed's selection is
    /// remembered for the way back; moving from one repository to another
    /// keeps the one remembered on leaving the feed.
    pub fn enter_repo(&mut self, repo: RepoId, selection: &mut Option<Selection>) {
        if self.repo() == Some(&repo) {
            return;
        }
        let feed_selection = match std::mem::take(self) {
            Self::Feed => selection.clone(),
            Self::Repo(previous) => previous.feed_selection,
        };
        if selection.as_ref().is_some_and(|s| !is_in(s, &repo)) {
            *selection = None;
        }
        *self = Self::Repo(RepoScreen {
            repo,
            feed_selection,
        });
    }

    /// Go back to the feed.
    ///
    /// Something picked inside the repository view stays picked — it is a
    /// pull request the feed shows too, and dropping it would close the
    /// detail the user just opened. With nothing picked, the feed's own
    /// selection comes back.
    pub fn back(&mut self, selection: &mut Option<Selection>) {
        let Self::Repo(screen) = std::mem::take(self) else {
            return;
        };
        if selection.is_none() {
            *selection = screen.feed_selection;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{issue::IssueNumber, model::PrNumber};

    fn repo(raw: &str) -> RepoId {
        raw.parse().expect("valid repo id")
    }

    fn sel(raw: &str, number: u32) -> Option<Selection> {
        Some(Selection::PullRequest {
            repo: repo(raw),
            number: PrNumber(number),
        })
    }

    #[test]
    fn the_app_starts_on_the_feed() {
        assert_eq!(Screen::default(), Screen::Feed);
        assert!(Screen::default().repo().is_none());
    }

    #[test]
    fn entering_a_repository_shows_it() {
        let mut screen = Screen::Feed;
        let mut selection = None;
        screen.enter_repo(repo("a/b"), &mut selection);
        assert_eq!(screen.repo(), Some(&repo("a/b")));
        assert!(!screen.is_feed());
    }

    #[test]
    fn a_selection_in_the_entered_repository_is_kept() {
        let mut screen = Screen::Feed;
        let mut selection = sel("a/b", 4);
        screen.enter_repo(repo("a/b"), &mut selection);
        assert_eq!(selection, sel("a/b", 4));
    }

    /// An issue is as much the repository's as a pull request is.
    #[test]
    fn an_issue_selected_in_the_entered_repository_is_kept() {
        let issue = Some(Selection::Issue {
            repo: repo("a/b"),
            number: IssueNumber(12),
        });
        let mut screen = Screen::Feed;
        let mut selection = issue.clone();
        screen.enter_repo(repo("a/b"), &mut selection);
        assert_eq!(selection, issue);
    }

    #[test]
    fn a_selection_elsewhere_is_cleared_so_the_branches_show() {
        let mut screen = Screen::Feed;
        let mut selection = sel("c/d", 4);
        screen.enter_repo(repo("a/b"), &mut selection);
        assert_eq!(selection, None);
    }

    #[test]
    fn leaving_with_nothing_picked_restores_the_feed_selection() {
        let mut screen = Screen::Feed;
        let mut selection = sel("c/d", 4);
        screen.enter_repo(repo("a/b"), &mut selection);
        screen.back(&mut selection);
        assert!(screen.is_feed());
        assert_eq!(selection, sel("c/d", 4));
    }

    #[test]
    fn leaving_keeps_what_was_picked_inside_the_repository() {
        let mut screen = Screen::Feed;
        let mut selection = sel("c/d", 4);
        screen.enter_repo(repo("a/b"), &mut selection);
        selection = sel("a/b", 9);
        screen.back(&mut selection);
        assert_eq!(selection, sel("a/b", 9));
    }

    /// Going back to the branches (clearing the pick) and then leaving is
    /// "nothing picked": the feed gets its own selection back.
    #[test]
    fn a_pick_cleared_before_leaving_restores_the_feed_selection() {
        let mut screen = Screen::Feed;
        let mut selection = sel("a/b", 1);
        screen.enter_repo(repo("a/b"), &mut selection);
        selection = None;
        screen.back(&mut selection);
        assert_eq!(selection, sel("a/b", 1));
    }

    #[test]
    fn moving_between_repositories_remembers_the_feed_selection_from_before() {
        let mut screen = Screen::Feed;
        let mut selection = sel("x/y", 2);
        screen.enter_repo(repo("a/b"), &mut selection);
        screen.enter_repo(repo("c/d"), &mut selection);
        assert_eq!(screen.repo(), Some(&repo("c/d")));
        screen.back(&mut selection);
        assert_eq!(selection, sel("x/y", 2));
    }

    #[test]
    fn re_entering_the_same_repository_changes_nothing() {
        let mut screen = Screen::Feed;
        let mut selection = sel("x/y", 2);
        screen.enter_repo(repo("a/b"), &mut selection);
        selection = sel("a/b", 5);
        let before = screen.clone();
        screen.enter_repo(repo("a/b"), &mut selection);
        assert_eq!(screen, before);
        assert_eq!(selection, sel("a/b", 5));
    }

    #[test]
    fn back_on_the_feed_is_a_no_op() {
        let mut screen = Screen::Feed;
        let mut selection = sel("a/b", 1);
        screen.back(&mut selection);
        assert!(screen.is_feed());
        assert_eq!(selection, sel("a/b", 1));
    }
}
