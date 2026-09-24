//! The roster of people the author filter can be pointed at.
//!
//! Derived from the pull requests currently loaded rather than from anything
//! configured: the set of people with open work in the watched repositories is
//! exactly the set worth offering, and it changes on every refresh without the
//! user maintaining a list.
//!
//! Pure, so the ordering rules below are testable without a window.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};

use crate::{
    model::{LoginKey, User},
    state::RepoState,
};

/// One selectable author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorEntry {
    /// Comparison identity, and what a selection stores.
    pub key: LoginKey,
    /// Display identity — the casing GitHub returned, and the avatar.
    pub user: User,
    /// Open pull requests this person authored across every watched repository.
    /// Zero for the viewer when they have none open, and for a selection that
    /// has outlived the pull request it was made from.
    pub open_prs: usize,
    /// Most recent update across those pull requests; `None` when there are
    /// none. Drives the ordering of everyone below the viewer.
    pub latest: Option<DateTime<Utc>>,
    /// Whether this entry is the authenticated user.
    pub is_viewer: bool,
}

/// Build the author roster.
///
/// Ordering, in full:
///
/// 1. The viewer, when known — always first, and always present even with no
///    open pull requests of their own. Filtering to yourself is the reason the
///    control exists, and with `include_involved` on it is meaningful precisely
///    when you have authored nothing.
/// 2. Everyone else by most recently updated pull request, newest first.
/// 3. People with no open pull requests last — reachable only through a
///    `selected` login whose work has since been merged or closed. They are
///    kept in the roster so a stale selection can still be switched off; a
///    selection the user cannot see is a filter they cannot undo.
///
/// Ties break on the login so the row never reshuffles between two refreshes
/// that carry the same timestamps.
pub fn roster(
    repos: &[RepoState],
    viewer: Option<&User>,
    selected: &BTreeSet<LoginKey>,
) -> Vec<AuthorEntry> {
    let viewer_key = viewer.map(User::key);
    let mut by_login: HashMap<LoginKey, AuthorEntry> = HashMap::new();

    for pr in repos.iter().flat_map(|repo| &repo.prs) {
        let Some(author) = pr.author.as_ref() else {
            continue;
        };
        let key = author.key();
        if key.is_empty() {
            continue;
        }
        let entry = by_login.entry(key.clone()).or_insert_with(|| AuthorEntry {
            is_viewer: viewer_key.as_ref() == Some(&key),
            key,
            user: author.clone(),
            open_prs: 0,
            latest: None,
        });
        entry.open_prs += 1;
        entry.latest = entry.latest.max(Some(pr.updated_at));
    }

    // The viewer and any stale selection are folded in afterwards so they
    // appear with zero counts rather than not at all.
    let placeholders = viewer_key
        .iter()
        .cloned()
        .chain(selected.iter().cloned())
        .filter(|key| !key.is_empty());

    for key in placeholders {
        let is_viewer = viewer_key.as_ref() == Some(&key);
        by_login.entry(key.clone()).or_insert_with(|| AuthorEntry {
            user: match (is_viewer, viewer) {
                // Preserve the viewer's real casing and avatar; a login
                // recovered from a stale selection has neither.
                (true, Some(user)) => user.clone(),
                _ => User {
                    login: key.as_str().to_string(),
                    avatar_url: None,
                },
            },
            key,
            open_prs: 0,
            latest: None,
            is_viewer,
        });
    }

    let mut entries: Vec<AuthorEntry> = by_login.into_values().collect();
    entries.sort_by(|a, b| {
        b.is_viewer
            .cmp(&a.is_viewer)
            .then_with(|| b.latest.cmp(&a.latest))
            .then_with(|| a.key.cmp(&b.key))
    });
    entries
}

/// What the author row draws, once the roster is capped to fit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleAuthors {
    pub shown: Vec<AuthorEntry>,
    /// Authors the cap removed, for the "+N more" control. Zero when
    /// everything fits or the row is expanded.
    pub hidden: usize,
}

/// Cap the roster to `limit` chips.
///
/// The cap applies to the *unselected* tail only. The viewer leads and is never
/// cut, and neither is any selected author — a chip the user cannot see is a
/// filter they cannot switch off, and a filter you cannot switch off silently
/// hiding pull requests is the worst failure this control has.
///
/// Order is preserved, so expanding and collapsing never reshuffles the row.
pub fn visible(
    entries: Vec<AuthorEntry>,
    selected: &BTreeSet<LoginKey>,
    limit: usize,
) -> VisibleAuthors {
    let mut hidden = 0;
    let shown = entries
        .into_iter()
        .enumerate()
        .filter(|(ix, entry)| {
            if *ix < limit || selected.contains(&entry.key) {
                return true;
            }
            hidden += 1;
            false
        })
        .map(|(_, entry)| entry)
        .collect();

    VisibleAuthors { shown, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest},
        state::{LoadState, RepoState},
    };

    fn user(login: &str) -> User {
        User {
            login: login.to_string(),
            avatar_url: None,
        }
    }

    fn pr(author: Option<&str>, updated_at: &str) -> PullRequest {
        PullRequest {
            number: PrNumber(1),
            node_id: NodeId("n".into()),
            title: "t".into(),
            url: "u".into(),
            is_draft: false,
            created_at: "2026-01-01T00:00:00Z".parse().expect("valid"),
            updated_at: updated_at.parse().expect("valid"),
            author: author.map(user),
            head_ref: "h".into(),
            head_sha: "sha".into(),
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
        }
    }

    fn repo(prs: Vec<PullRequest>) -> RepoState {
        let mut state = RepoState::new("a/b".parse().expect("valid"));
        state.prs = prs;
        state.load = LoadState::Loaded {
            at: "2026-01-01T00:00:00Z".parse().expect("valid"),
        };
        state
    }

    fn logins(entries: &[AuthorEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.key.as_str()).collect()
    }

    #[test]
    fn everyone_below_the_viewer_is_ordered_by_recency() {
        let repos = [repo(vec![
            pr(Some("alice"), "2026-01-03T00:00:00Z"),
            pr(Some("bob"), "2026-01-05T00:00:00Z"),
            pr(Some("carol"), "2026-01-04T00:00:00Z"),
        ])];
        let entries = roster(&repos, None, &BTreeSet::new());
        assert_eq!(logins(&entries), ["bob", "carol", "alice"]);
    }

    /// A person's position follows their *most recent* pull request, not their
    /// oldest or their first encountered.
    #[test]
    fn an_authors_recency_is_their_newest_pull_request() {
        let repos = [repo(vec![
            pr(Some("alice"), "2026-01-01T00:00:00Z"),
            pr(Some("bob"), "2026-01-02T00:00:00Z"),
            pr(Some("alice"), "2026-01-09T00:00:00Z"),
        ])];
        let entries = roster(&repos, None, &BTreeSet::new());
        assert_eq!(logins(&entries), ["alice", "bob"]);
        assert_eq!(entries[0].open_prs, 2);
    }

    #[test]
    fn the_viewer_leads_however_stale_their_work_is() {
        let repos = [repo(vec![
            pr(Some("me"), "2020-01-01T00:00:00Z"),
            pr(Some("alice"), "2026-01-05T00:00:00Z"),
        ])];
        let entries = roster(&repos, Some(&user("me")), &BTreeSet::new());
        assert_eq!(logins(&entries), ["me", "alice"]);
        assert!(entries[0].is_viewer);
        assert!(!entries[1].is_viewer);
    }

    /// Filtering to yourself is the point of the control, and with
    /// `include_involved` it is most useful precisely when you have authored
    /// nothing.
    #[test]
    fn the_viewer_appears_with_no_pull_requests_of_their_own() {
        let repos = [repo(vec![pr(Some("alice"), "2026-01-05T00:00:00Z")])];
        let entries = roster(&repos, Some(&user("Me")), &BTreeSet::new());
        assert_eq!(logins(&entries), ["me", "alice"]);
        assert_eq!(entries[0].open_prs, 0);
        assert_eq!(entries[0].latest, None);
        // The viewer's real casing survives, because the chip shows it.
        assert_eq!(entries[0].user.login, "Me");
    }

    /// A selection whose pull requests have all been merged must stay visible,
    /// or the user is left holding a filter they cannot switch off.
    #[test]
    fn a_selection_with_no_open_work_is_kept_so_it_can_be_undone() {
        let repos = [repo(vec![pr(Some("alice"), "2026-01-05T00:00:00Z")])];
        let selected = BTreeSet::from([LoginKey::new("ghost")]);
        let entries = roster(&repos, None, &selected);
        assert_eq!(logins(&entries), ["alice", "ghost"]);
        assert_eq!(entries[1].open_prs, 0);
    }

    /// GitHub echoes whatever casing it stored, so the same person must not
    /// arrive twice under two spellings.
    #[test]
    fn logins_differing_only_in_case_are_one_person() {
        let repos = [repo(vec![
            pr(Some("Alice"), "2026-01-01T00:00:00Z"),
            pr(Some("alice"), "2026-01-02T00:00:00Z"),
        ])];
        let entries = roster(&repos, None, &BTreeSet::from([LoginKey::new("ALICE")]));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].open_prs, 2);
    }

    /// Deleted accounts and some bots come back with no author at all.
    #[test]
    fn pull_requests_without_an_author_contribute_nobody() {
        let repos = [repo(vec![pr(None, "2026-01-01T00:00:00Z")])];
        assert!(roster(&repos, None, &BTreeSet::new()).is_empty());
    }

    /// Two authors whose newest pull requests share a timestamp must not swap
    /// places from one refresh to the next.
    #[test]
    fn equal_timestamps_break_on_the_login() {
        let repos = [repo(vec![
            pr(Some("zoe"), "2026-01-05T00:00:00Z"),
            pr(Some("adam"), "2026-01-05T00:00:00Z"),
        ])];
        assert_eq!(
            logins(&roster(&repos, None, &BTreeSet::new())),
            ["adam", "zoe"]
        );
    }

    #[test]
    fn an_empty_feed_offers_nobody() {
        assert!(roster(&[], None, &BTreeSet::new()).is_empty());
    }

    // --- capping the row ----------------------------------------------------

    fn roster_of(logins: &[&str]) -> Vec<AuthorEntry> {
        logins
            .iter()
            .map(|login| AuthorEntry {
                key: LoginKey::new(login),
                user: user(login),
                open_prs: 1,
                latest: None,
                is_viewer: false,
            })
            .collect()
    }

    #[test]
    fn a_roster_within_the_limit_is_shown_whole() {
        let capped = visible(roster_of(&["a", "b"]), &BTreeSet::new(), 12);
        assert_eq!(logins(&capped.shown), ["a", "b"]);
        assert_eq!(capped.hidden, 0);
    }

    #[test]
    fn the_tail_beyond_the_limit_is_counted_not_drawn() {
        let capped = visible(roster_of(&["a", "b", "c", "d"]), &BTreeSet::new(), 2);
        assert_eq!(logins(&capped.shown), ["a", "b"]);
        assert_eq!(capped.hidden, 2);
    }

    /// The one rule that matters: a selection past the cap must still be drawn,
    /// or the user is left with a feed narrowed by a chip they cannot reach.
    #[test]
    fn a_selected_author_past_the_limit_is_still_drawn() {
        let selected = BTreeSet::from([LoginKey::new("d")]);
        let capped = visible(roster_of(&["a", "b", "c", "d"]), &selected, 2);
        assert_eq!(logins(&capped.shown), ["a", "b", "d"]);
        // `d` is drawn, so it is not among the hidden.
        assert_eq!(capped.hidden, 1);
    }

    /// Selecting someone must not reorder the row under the cursor.
    #[test]
    fn capping_preserves_the_roster_order() {
        let selected = BTreeSet::from([LoginKey::new("e"), LoginKey::new("a")]);
        let capped = visible(roster_of(&["a", "b", "c", "d", "e"]), &selected, 2);
        assert_eq!(logins(&capped.shown), ["a", "b", "e"]);
    }

    /// Expanding is "no cap", expressed as a limit nothing exceeds.
    #[test]
    fn an_unbounded_limit_hides_nobody() {
        let entries = roster_of(&["a", "b", "c"]);
        let capped = visible(entries.clone(), &BTreeSet::new(), entries.len());
        assert_eq!(capped.hidden, 0);
        assert_eq!(capped.shown.len(), 3);
    }

    #[test]
    fn a_limit_of_zero_still_draws_the_selection() {
        let selected = BTreeSet::from([LoginKey::new("b")]);
        let capped = visible(roster_of(&["a", "b"]), &selected, 0);
        assert_eq!(logins(&capped.shown), ["b"]);
        assert_eq!(capped.hidden, 1);
    }
}
