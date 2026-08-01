//! Flattening application state into the feed's single row stream.
//!
//! GPUI's virtualized lists derive their visible range from their own bounds,
//! so nesting one list per repo inside an outer scroller either collapses to
//! zero height or renders every row every frame. Instead every repo and every
//! pull request is flattened into one `Vec<FeedRow>` rendered by a single
//! list, and the per-repo "container" look is reconstructed by having each row
//! draw the part of the border that belongs to it (see [`Feed::chrome`]).

use crate::{
    model::PullRequest,
    state::{LoadState, RepoState},
};

/// Index into `AppState::repos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RepoIx(pub usize);

/// Index into `RepoState::prs`. Always indexes the *unfiltered* vector, so a
/// row can be resolved back to its pull request regardless of the active filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrIx(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedRow {
    RepoHeader {
        repo: RepoIx,
    },
    PrRow {
        repo: RepoIx,
        pr: PrIx,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedFilter {
    pub query: String,
    pub hide_drafts: bool,
}

impl FeedFilter {
    pub fn accepts(&self, pr: &PullRequest) -> bool {
        if self.hide_drafts && pr.is_draft {
            return false;
        }
        pr.matches_query(&self.query)
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty() || self.hide_drafts
    }
}

/// The flattened row stream backing the feed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feed {
    rows: Vec<FeedRow>,
}

impl Feed {
    pub fn rows(&self) -> &[FeedRow] {
        &self.rows
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

/// Build the feed's row stream.
///
/// Pure: the only inputs are state and filter, which makes every invariant
/// below directly testable without a window.
pub fn flatten(repos: &[RepoState], filter: &FeedFilter) -> Feed {
    let mut rows = Vec::new();

    for (ix, repo) in repos.iter().enumerate() {
        let repo_ix = RepoIx(ix);
        rows.push(FeedRow::RepoHeader { repo: repo_ix });

        if !repo.collapsed {
            let visible: Vec<PrIx> = repo
                .prs
                .iter()
                .enumerate()
                .filter(|(_, pr)| filter.accepts(pr))
                .map(|(pr_ix, _)| PrIx(pr_ix))
                .collect();

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
                rows.extend(
                    visible
                        .into_iter()
                        .map(|pr| FeedRow::PrRow { repo: repo_ix, pr }),
                );
            }
        }

        rows.push(FeedRow::Spacer { repo: repo_ix });
    }

    Feed { rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mergeable, PrNumber, PullRequest, RepoId};
    use chrono::Utc;

    fn pr(number: u32, draft: bool) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "feature".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            review_decision: None,
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
        }
    }

    fn repo(name: &str, prs: Vec<PullRequest>, load: LoadState) -> RepoState {
        RepoState {
            id: name.parse::<RepoId>().expect("valid repo id"),
            prs,
            load,
            collapsed: false,
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
                    pr: PrIx(0)
                },
                FeedRow::PrRow {
                    repo: RepoIx(0),
                    pr: PrIx(1)
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

        for (load, expected) in cases {
            let feed = flatten(&[repo("a/b", vec![], load.clone())], &FeedFilter::default());
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
                pr: PrIx(0)
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
                pr: PrIx(1)
            })
        );
    }

    #[test]
    fn filtering_everything_out_yields_empty_not_loading() {
        let state = loaded("a/b", 3);
        let filter = FeedFilter {
            query: "nothing matches this".into(),
            ..Default::default()
        };
        let feed = flatten(&[state], &filter);
        assert_eq!(feed.row(1), Some(FeedRow::RepoEmpty { repo: RepoIx(0) }));
    }

    #[test]
    fn no_repos_yields_no_rows() {
        let feed = flatten(&[], &FeedFilter::default());
        assert!(feed.is_empty());
    }
}
