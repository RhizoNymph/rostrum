//! Flattening application state into the feed's single row stream.
//!
//! GPUI's virtualized lists derive their visible range from their own bounds,
//! so nesting one list per repo inside an outer scroller either collapses to
//! zero height or renders every row every frame. Instead every repo and every
//! pull request is flattened into one `Vec<FeedRow>` rendered by a single
//! list, and the per-repo "container" look is reconstructed by having each row
//! draw the part of the border that belongs to it (see [`Feed::chrome`]).

use std::collections::BTreeSet;

use crate::{
    model::{LoginKey, PullRequest},
    sort::{FeedOrder, FeedSort, compare_groups, order_items, order_repos},
    stack::{FeedUnit, StackGroup, StackIx, stack_groups, units},
    state::{LoadState, RepoState},
};

/// Index into `AppState::repos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RepoIx(pub usize);

/// Index into `RepoState::prs`. Always indexes the *unfiltered* vector, so a
/// row can be resolved back to its pull request regardless of the active filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrIx(pub usize);

/// Where a pull request row sits in its stack, for the chain glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackPlace {
    Bottom,
    Middle,
    Top,
    /// The only visible member.
    Only,
}

impl StackPlace {
    fn of(ix: usize, len: usize) -> Self {
        match (ix, len) {
            (_, 0 | 1) => Self::Only,
            (0, _) => Self::Bottom,
            (ix, len) if ix + 1 == len => Self::Top,
            _ => Self::Middle,
        }
    }
}

/// A pull request row's membership of a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackSlot {
    pub stack: StackIx,
    pub place: StackPlace,
}

/// A stack group placed in the feed, with the repository it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedStack {
    pub repo: RepoIx,
    pub group: StackGroup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedRow {
    RepoHeader {
        repo: RepoIx,
    },
    /// Heads the rows of one stack: "Stack · 3 PRs", the trunk, the merge
    /// rollup, and the stack's actions. Always directly followed by the
    /// stack's visible members, bottom first.
    StackHeader {
        repo: RepoIx,
        stack: StackIx,
    },
    PrRow {
        repo: RepoIx,
        pr: PrIx,
        /// `Some` when the row is a member of the stack whose header precedes
        /// it.
        stack: Option<StackSlot>,
    },
    /// Loaded successfully, nothing to show (no open PRs, or none match).
    RepoEmpty {
        repo: RepoIx,
    },
    /// Refresh failed and there is no stale data to fall back on.
    RepoError {
        repo: RepoIx,
    },
    /// First load in flight.
    RepoLoading {
        repo: RepoIx,
    },
    /// Gap below a repo's container. Carries no chrome.
    Spacer {
        repo: RepoIx,
    },
}

impl FeedRow {
    pub fn repo(&self) -> RepoIx {
        match *self {
            Self::RepoHeader { repo }
            | Self::StackHeader { repo, .. }
            | Self::PrRow { repo, .. }
            | Self::RepoEmpty { repo }
            | Self::RepoError { repo }
            | Self::RepoLoading { repo }
            | Self::Spacer { repo } => repo,
        }
    }

    pub fn is_spacer(&self) -> bool {
        matches!(self, Self::Spacer { .. })
    }
}

/// Which portion of the container border a row is responsible for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    /// First row of a repo's run: top border and top rounding.
    Top,
    /// Interior row: side borders only.
    Middle,
    /// Last row of a run: bottom border and bottom rounding.
    Bottom,
    /// The run's only row: full border, fully rounded.
    Solo,
    /// Spacers draw nothing.
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedFilter {
    pub query: String,
    pub hide_drafts: bool,
    /// Drop repositories that have loaded successfully and have nothing to
    /// show. On by default: a feed of a dozen repositories is mostly empty
    /// headers most of the time.
    pub hide_empty_repos: bool,
    /// Logins the feed is narrowed to. Empty means every author, which is why
    /// this is a set rather than an `Option<Vec<_>>`: "none selected" and "all
    /// shown" are the same state, and giving them two representations would
    /// invite them to disagree.
    pub authors: BTreeSet<LoginKey>,
    /// Widen [`FeedFilter::authors`] from "opened by" to "waiting on" —
    /// assignee and requested reviewer as well as author. Inert while
    /// `authors` is empty, which is what lets it be a plain checkbox rather
    /// than a third selection mode.
    pub include_involved: bool,
    /// How repositories, and the items within each, are ordered.
    ///
    /// Not a filter — it never hides anything, and [`FeedFilter::is_active`]
    /// ignores it — but it lives here because it is the same kind of
    /// standing feed preference and persists through the same
    /// `feed_filter`/`absorb_filter` pair. A "clear filter" must keep it.
    pub sort: FeedSort,
}

impl Default for FeedFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            hide_drafts: false,
            hide_empty_repos: true,
            authors: BTreeSet::new(),
            include_involved: false,
            sort: FeedSort::default(),
        }
    }
}

impl FeedFilter {
    pub fn accepts(&self, pr: &PullRequest) -> bool {
        if self.hide_drafts && pr.is_draft {
            return false;
        }
        if !self.accepts_author(pr) {
            return false;
        }
        pr.matches_query(&self.query)
    }

    /// Whether the author selection lets `pr` through. An empty selection lets
    /// everything through; otherwise one selected login must match.
    fn accepts_author(&self, pr: &PullRequest) -> bool {
        if self.authors.is_empty() {
            return true;
        }
        self.authors.iter().any(|login| {
            if self.include_involved {
                pr.involves(login)
            } else {
                pr.is_authored_by(login)
            }
        })
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty() || self.hide_drafts || !self.authors.is_empty()
    }

    /// Every filter reset to its default, keeping the sort: clearing what is
    /// hidden says nothing about the order of what is shown.
    pub fn cleared(&self) -> Self {
        Self {
            sort: self.sort,
            ..Self::default()
        }
    }

    /// Add or remove a login from the selection, reporting the state it landed
    /// in so a caller can persist it without re-reading.
    pub fn toggle_author(&mut self, login: LoginKey) -> bool {
        if self.authors.remove(&login) {
            return false;
        }
        self.authors.insert(login);
        true
    }
}

/// The flattened row stream backing the feed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feed {
    rows: Vec<FeedRow>,
    hidden_repos: usize,
    /// Every stack group with a header in `rows`, indexed by [`StackIx`].
    stacks: Vec<FeedStack>,
}

impl Feed {
    pub fn rows(&self) -> &[FeedRow] {
        &self.rows
    }

    /// How many repositories `hide_empty_repos` removed from the stream.
    pub fn hidden_repos(&self) -> usize {
        self.hidden_repos
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, ix: usize) -> Option<FeedRow> {
        self.rows.get(ix).copied()
    }

    /// The stack group a [`FeedRow::StackHeader`] or [`StackSlot`] names.
    pub fn stack(&self, ix: StackIx) -> Option<&FeedStack> {
        self.stacks.get(ix.0)
    }

    pub fn stacks(&self) -> &[FeedStack] {
        &self.stacks
    }

    /// Border/rounding role of the row at `ix`, derived from whether its
    /// neighbours belong to the same repo's contiguous run.
    pub fn chrome(&self, ix: usize) -> Chrome {
        let Some(row) = self.rows.get(ix) else {
            return Chrome::None;
        };
        if row.is_spacer() {
            return Chrome::None;
        }

        let repo = row.repo();
        let continues = |neighbour: Option<&FeedRow>| {
            neighbour.is_some_and(|r| !r.is_spacer() && r.repo() == repo)
        };

        let is_top = !continues(ix.checked_sub(1).and_then(|prev| self.rows.get(prev)));
        let is_bottom = !continues(self.rows.get(ix + 1));

        match (is_top, is_bottom) {
            (true, true) => Chrome::Solo,
            (true, false) => Chrome::Top,
            (false, true) => Chrome::Bottom,
            (false, false) => Chrome::Middle,
        }
    }
}

/// Build the feed's row stream, in the order `filter.sort` asks for.
///
/// Pure: the only inputs are state and filter, which makes every invariant
/// below directly testable without a window.
///
/// Repositories appear in `filter.sort.repos` order and items within each in
/// `filter.sort.items` order. Indices stay positional — a `RepoIx` still
/// indexes `repos` — so sorting changes which order rows come in, never what
/// a row points at.
pub fn flatten(repos: &[RepoState], filter: &FeedFilter) -> Feed {
    flatten_in(repos, filter, FeedOrder::Sorted(filter.sort))
}

/// [`flatten`] in an explicit order.
///
/// [`FeedOrder::AsListed`] is for a client that orders the feed itself — the
/// Android core keeps the user's own repository order and the fetched item
/// order until it grows a sort control of its own.
pub fn flatten_in(repos: &[RepoState], filter: &FeedFilter, order: FeedOrder) -> Feed {
    let mut rows = Vec::new();
    let mut hidden_repos = 0;
    let mut stacks = Vec::new();

    let repo_order = match order {
        FeedOrder::AsListed => (0..repos.len()).map(RepoIx).collect(),
        FeedOrder::Sorted(sort) => order_repos(repos, sort.repos),
    };

    for repo_ix in repo_order {
        let repo = &repos[repo_ix.0];

        let mut visible: Vec<PrIx> = repo
            .prs
            .iter()
            .enumerate()
            .filter(|(_, pr)| filter.accepts(pr))
            .map(|(pr_ix, _)| PrIx(pr_ix))
            .collect();
        if let FeedOrder::Sorted(sort) = order {
            order_items(&repo.prs, &mut visible, sort.items);
        }

        // A repository is only hidden once it has actually loaded. One that is
        // still loading or has failed must stay visible — otherwise a broken
        // repo silently disappears instead of showing its error.
        if filter.hide_empty_repos
            && visible.is_empty()
            && matches!(repo.load, LoadState::Loaded { .. })
        {
            hidden_repos += 1;
            continue;
        }

        rows.push(FeedRow::RepoHeader { repo: repo_ix });

        if !repo.collapsed {
            if visible.is_empty() {
                rows.push(match &repo.load {
                    LoadState::Idle | LoadState::Loading if repo.prs.is_empty() => {
                        FeedRow::RepoLoading { repo: repo_ix }
                    }
                    LoadState::Failed { .. } if repo.prs.is_empty() => {
                        FeedRow::RepoError { repo: repo_ix }
                    }
                    _ => FeedRow::RepoEmpty { repo: repo_ix },
                });
            } else {
                push_units(&mut rows, &mut stacks, repo_ix, repo, &visible, order);
            }
        }

        rows.push(FeedRow::Spacer { repo: repo_ix });
    }

    Feed {
        rows,
        hidden_repos,
        stacks,
    }
}

/// One repository's pull request rows: lone pull requests as they are, and
/// each stack as a header followed by its visible members, bottom first.
///
/// A stack sorts as one unit, filed under [`compare_groups`]'s value for it:
/// the bottom member's for text keys, the newest or oldest member's for time
/// keys. `visible` arrives already in item order, and the sort is stable, so
/// lone pull requests keep exactly the order [`order_items`] gave them.
fn push_units(
    rows: &mut Vec<FeedRow>,
    stacks: &mut Vec<FeedStack>,
    repo_ix: RepoIx,
    repo: &RepoState,
    visible: &[PrIx],
    order: FeedOrder,
) {
    let groups = stack_groups(repo);
    let mut ordered = units(visible, &groups);
    if let FeedOrder::Sorted(sort) = order {
        let members = |unit: &FeedUnit| -> Vec<&PullRequest> {
            unit.members().iter().filter_map(|ix| repo.prs.get(ix.0)).collect()
        };
        ordered.sort_by(|a, b| compare_groups(&members(a), &members(b), sort.items));
    }

    for unit in ordered {
        match unit {
            FeedUnit::Single(pr) => rows.push(FeedRow::PrRow {
                repo: repo_ix,
                pr,
                stack: None,
            }),
            FeedUnit::Stack { group, visible } => {
                let stack = StackIx(stacks.len());
                stacks.push(FeedStack {
                    repo: repo_ix,
                    group: groups[group].clone(),
                });
                rows.push(FeedRow::StackHeader {
                    repo: repo_ix,
                    stack,
                });
                let len = visible.len();
                rows.extend(
                    visible
                        .into_iter()
                        .enumerate()
                        .map(|(at, pr)| FeedRow::PrRow {
                            repo: repo_ix,
                            pr,
                            stack: Some(StackSlot {
                                stack,
                                place: StackPlace::of(at, len),
                            }),
                        }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        LoginKey, MergeStateStatus, Mergeable, NodeId, PrNumber, PullRequest, RepoId, User,
    };
    use chrono::Utc;

    /// One fixed instant for every fixture pull request: the feed sorts by
    /// creation time, and `Utc::now()` per call would make the order of
    /// otherwise-identical fixtures depend on the clock.
    fn fixed_time() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
    }

    fn pr(number: u32, draft: bool) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{}", number)),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: draft,
            created_at: fixed_time(),
            updated_at: fixed_time(),
            author: None,
            head_ref: "feature".into(),
            head_sha: "abc123".into(),
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
            pushed_at: None,
        }
    }

    fn repo(name: &str, prs: Vec<PullRequest>, load: LoadState) -> RepoState {
        RepoState {
            id: name.parse::<RepoId>().expect("valid repo id"),
            prs,
            load,
            collapsed: false,
            stacks: Vec::new(),
            meta: None,
        }
    }

    fn loaded(name: &str, count: u32) -> RepoState {
        repo(
            name,
            (1..=count).map(|n| pr(n, false)).collect(),
            LoadState::Loaded { at: Utc::now() },
        )
    }

    #[test]
    fn header_then_prs_then_spacer() {
        let feed = flatten(&[loaded("a/b", 2)], &FeedFilter::default());
        assert_eq!(
            feed.rows(),
            &[
                FeedRow::RepoHeader { repo: RepoIx(0) },
                FeedRow::PrRow {
                    repo: RepoIx(0),
                    pr: PrIx(0),
                    stack: None
                },
                FeedRow::PrRow {
                    repo: RepoIx(0),
                    pr: PrIx(1),
                    stack: None
                },
                FeedRow::Spacer { repo: RepoIx(0) },
            ]
        );
    }

    /// Container chrome correctness depends on every repo's rows forming an
    /// unbroken run, headed by exactly one header.
    #[test]
    fn each_repo_forms_one_contiguous_run_with_one_header() {
        let repos = vec![loaded("a/b", 2), loaded("c/d", 1), loaded("e/f", 3)];
        let feed = flatten(&repos, &FeedFilter::default());

        for (ix, _) in repos.iter().enumerate() {
            let positions: Vec<usize> = feed
                .rows()
                .iter()
                .enumerate()
                .filter(|(_, row)| row.repo() == RepoIx(ix))
                .map(|(pos, _)| pos)
                .collect();

            assert!(!positions.is_empty());
            let span = positions[positions.len() - 1] - positions[0] + 1;
            assert_eq!(span, positions.len(), "repo {ix} rows are not contiguous");

            let headers = feed
                .rows()
                .iter()
                .filter(|row| matches!(row, FeedRow::RepoHeader { repo } if *repo == RepoIx(ix)))
                .count();
            assert_eq!(headers, 1, "repo {ix} should have exactly one header");
            assert!(matches!(
                feed.row(positions[0]),
                Some(FeedRow::RepoHeader { .. })
            ));
        }
    }

    #[test]
    fn chrome_wraps_each_run_and_skips_spacers() {
        let feed = flatten(
            &[loaded("a/b", 2), loaded("c/d", 1)],
            &FeedFilter::default(),
        );
        // header, pr, pr, spacer, header, pr, spacer
        assert_eq!(feed.chrome(0), Chrome::Top);
        assert_eq!(feed.chrome(1), Chrome::Middle);
        assert_eq!(feed.chrome(2), Chrome::Bottom);
        assert_eq!(feed.chrome(3), Chrome::None);
        assert_eq!(feed.chrome(4), Chrome::Top);
        assert_eq!(feed.chrome(5), Chrome::Bottom);
        assert_eq!(feed.chrome(6), Chrome::None);
    }

    #[test]
    fn chrome_of_out_of_range_index_is_none() {
        let feed = flatten(&[loaded("a/b", 1)], &FeedFilter::default());
        assert_eq!(feed.chrome(999), Chrome::None);
    }

    #[test]
    fn collapsed_repo_contributes_header_and_spacer_only() {
        let mut repo = loaded("a/b", 5);
        repo.collapsed = true;
        let feed = flatten(&[repo], &FeedFilter::default());
        assert_eq!(
            feed.rows(),
            &[
                FeedRow::RepoHeader { repo: RepoIx(0) },
                FeedRow::Spacer { repo: RepoIx(0) },
            ]
        );
        assert_eq!(feed.chrome(0), Chrome::Solo);
    }

    #[test]
    fn distinguishes_loading_error_and_empty() {
        let cases = [
            (LoadState::Idle, FeedRow::RepoLoading { repo: RepoIx(0) }),
            (LoadState::Loading, FeedRow::RepoLoading { repo: RepoIx(0) }),
            (
                LoadState::Loaded { at: Utc::now() },
                FeedRow::RepoEmpty { repo: RepoIx(0) },
            ),
            (
                LoadState::Failed {
                    message: "boom".into(),
                    at: Utc::now(),
                },
                FeedRow::RepoError { repo: RepoIx(0) },
            ),
        ];

        let filter = FeedFilter {
            hide_empty_repos: false,
            ..Default::default()
        };
        for (load, expected) in cases {
            let feed = flatten(&[repo("a/b", vec![], load.clone())], &filter);
            assert_eq!(feed.row(1), Some(expected), "load state {load:?}");
        }
    }

    /// A failed refresh that still has cached PRs shows the stale data rather
    /// than replacing the whole card with an error.
    #[test]
    fn failed_repo_with_stale_data_still_lists_prs() {
        let state = repo(
            "a/b",
            vec![pr(1, false)],
            LoadState::Failed {
                message: "rate limited".into(),
                at: Utc::now(),
            },
        );
        let feed = flatten(&[state], &FeedFilter::default());
        assert_eq!(
            feed.row(1),
            Some(FeedRow::PrRow {
                repo: RepoIx(0),
                pr: PrIx(0),
                stack: None
            })
        );
    }

    #[test]
    fn pr_indices_address_the_unfiltered_vector() {
        let state = repo(
            "a/b",
            vec![pr(1, true), pr(2, false), pr(3, true)],
            LoadState::Loaded { at: Utc::now() },
        );
        let filter = FeedFilter {
            hide_drafts: true,
            ..Default::default()
        };
        let feed = flatten(&[state], &filter);
        // Only PR 2 survives, and it must still be addressed as index 1.
        assert_eq!(
            feed.row(1),
            Some(FeedRow::PrRow {
                repo: RepoIx(0),
                pr: PrIx(1),
                stack: None
            })
        );
    }

    #[test]
    fn filtering_everything_out_yields_empty_not_loading() {
        let state = loaded("a/b", 3);
        let filter = FeedFilter {
            query: "nothing matches this".into(),
            hide_empty_repos: false,
            ..Default::default()
        };
        let feed = flatten(&[state], &filter);
        assert_eq!(feed.row(1), Some(FeedRow::RepoEmpty { repo: RepoIx(0) }));
    }

    #[test]
    fn empty_repos_are_hidden_by_default() {
        let feed = flatten(
            &[
                loaded("a/b", 2),
                repo("c/d", vec![], LoadState::Loaded { at: Utc::now() }),
                loaded("e/f", 1),
            ],
            &FeedFilter::default(),
        );

        assert_eq!(feed.hidden_repos(), 1);
        assert!(
            !feed.rows().iter().any(|row| row.repo() == RepoIx(1)),
            "the empty repo should contribute no rows at all"
        );
        // The surviving repos keep their own contiguous runs and chrome.
        assert_eq!(feed.chrome(0), Chrome::Top);
    }

    /// A repository that is still loading, or that failed, must stay visible —
    /// otherwise an error disappears instead of being reported.
    #[test]
    fn loading_and_failed_repos_are_never_hidden() {
        for load in [
            LoadState::Idle,
            LoadState::Loading,
            LoadState::Failed {
                message: "boom".into(),
                at: Utc::now(),
            },
        ] {
            let feed = flatten(&[repo("a/b", vec![], load.clone())], &FeedFilter::default());
            assert_eq!(feed.hidden_repos(), 0, "{load:?}");
            assert!(
                matches!(feed.row(0), Some(FeedRow::RepoHeader { .. })),
                "{load:?} should still render a header"
            );
        }
    }

    /// Hiding follows the *filtered* count, so searching narrows the feed to
    /// the repositories that actually match.
    #[test]
    fn repos_whose_prs_are_all_filtered_out_are_hidden() {
        let feed = flatten(
            &[loaded("a/b", 2)],
            &FeedFilter {
                query: "no such pull request".into(),
                ..Default::default()
            },
        );
        assert!(feed.is_empty());
        assert_eq!(feed.hidden_repos(), 1);
    }

    #[test]
    fn a_collapsed_but_non_empty_repo_is_still_shown() {
        let mut state = loaded("a/b", 3);
        state.collapsed = true;
        let feed = flatten(&[state], &FeedFilter::default());
        assert_eq!(feed.hidden_repos(), 0);
        assert_eq!(feed.len(), 2);
    }

    #[test]
    fn showing_empty_repos_restores_them() {
        let repos = [repo("a/b", vec![], LoadState::Loaded { at: Utc::now() })];
        let filter = FeedFilter {
            hide_empty_repos: false,
            ..Default::default()
        };
        let feed = flatten(&repos, &filter);
        assert_eq!(feed.hidden_repos(), 0);
        assert_eq!(feed.row(1), Some(FeedRow::RepoEmpty { repo: RepoIx(0) }));
    }

    #[test]
    fn no_repos_yields_no_rows() {
        let feed = flatten(&[], &FeedFilter::default());
        assert!(feed.is_empty());
    }

    // --- author filter ------------------------------------------------------

    fn user(login: &str) -> User {
        User {
            login: login.to_string(),
            avatar_url: None,
        }
    }

    fn authored_by(login: &str) -> PullRequest {
        PullRequest {
            author: Some(user(login)),
            ..pr(1, false)
        }
    }

    /// "Nobody selected" and "everybody shown" are the same state, so an empty
    /// set must not filter anything out.
    #[test]
    fn no_author_selected_shows_everyone() {
        let filter = FeedFilter::default();
        assert!(filter.authors.is_empty());
        assert!(filter.accepts(&authored_by("alice")));
        assert!(filter.accepts(&pr(1, false)));
        assert!(!filter.is_active());
    }

    #[test]
    fn selecting_an_author_hides_everyone_else() {
        let filter = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("alice")]),
            ..Default::default()
        };
        assert!(filter.accepts(&authored_by("Alice")));
        assert!(!filter.accepts(&authored_by("bob")));
        assert!(filter.is_active());
    }

    #[test]
    fn several_selected_authors_are_a_union() {
        let filter = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("alice"), LoginKey::new("bob")]),
            ..Default::default()
        };
        assert!(filter.accepts(&authored_by("alice")));
        assert!(filter.accepts(&authored_by("bob")));
        assert!(!filter.accepts(&authored_by("carol")));
    }

    /// The checkbox only ever widens the selection; it must never let through
    /// a pull request the narrow filter would have shown, or hide one.
    #[test]
    fn include_involved_widens_authorship_to_assignee_and_reviewer() {
        let waiting_on_me = PullRequest {
            assignees: vec![user("me")],
            ..authored_by("alice")
        };
        let review_requested = PullRequest {
            review_requests: vec![user("me")],
            ..authored_by("alice")
        };

        let narrow = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("me")]),
            ..Default::default()
        };
        assert!(!narrow.accepts(&waiting_on_me));
        assert!(!narrow.accepts(&review_requested));

        let wide = FeedFilter {
            include_involved: true,
            ..narrow.clone()
        };
        assert!(wide.accepts(&waiting_on_me));
        assert!(wide.accepts(&review_requested));
        assert!(wide.accepts(&authored_by("me")));
        assert!(!wide.accepts(&authored_by("alice")));
    }

    /// Without a selection the checkbox has nothing to widen, which is what
    /// lets it be a plain checkbox rather than a third mode.
    #[test]
    fn include_involved_is_inert_with_nothing_selected() {
        let filter = FeedFilter {
            include_involved: true,
            ..Default::default()
        };
        assert!(filter.accepts(&authored_by("anyone")));
        assert!(!filter.is_active());
    }

    /// The author filter composes with the others rather than overriding them.
    #[test]
    fn the_author_filter_intersects_the_query_and_the_draft_toggle() {
        let filter = FeedFilter {
            authors: BTreeSet::from([LoginKey::new("alice")]),
            hide_drafts: true,
            query: "PR 1".into(),
            ..Default::default()
        };

        assert!(filter.accepts(&authored_by("alice")));
        // Right author, wrong everything else.
        assert!(!filter.accepts(&PullRequest {
            is_draft: true,
            ..authored_by("alice")
        }));
        assert!(!filter.accepts(&PullRequest {
            author: Some(user("alice")),
            ..pr(2, false)
        }));
    }

    #[test]
    fn toggling_an_author_reports_the_state_it_landed_in() {
        let mut filter = FeedFilter::default();
        assert!(filter.toggle_author(LoginKey::new("alice")));
        assert_eq!(filter.authors.len(), 1);
        assert!(!filter.toggle_author(LoginKey::new("Alice")));
        assert!(filter.authors.is_empty());
    }

    /// A repository whose every pull request belongs to someone unselected is
    /// empty as far as the feed is concerned, and `hide_empty_repos` applies.
    #[test]
    fn filtering_by_author_can_empty_a_repository() {
        let state = repo(
            "a/b",
            vec![authored_by("alice")],
            LoadState::Loaded { at: Utc::now() },
        );
        let feed = flatten(
            &[state],
            &FeedFilter {
                authors: BTreeSet::from([LoginKey::new("bob")]),
                ..Default::default()
            },
        );
        assert!(feed.is_empty());
    }
}
