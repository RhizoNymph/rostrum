//! The Closed tab's row stream.
//!
//! Unlike the open tabs, which lay each repository out in its own container,
//! the Closed tab is one list across every repository in the order things
//! ended, newest first. Grouping by repository would break that order, so
//! there are no repository headers: each row names its own repository, and
//! [`Feed::chrome`](crate::feed::Feed::chrome) draws the whole run of closed
//! rows as a single container.
//!
//! Status that would have lived in a header — a repository still loading,
//! or one whose fetch failed — follows the list as one notice per
//! repository, each its own small container.

use crate::{
    closed::closed_order,
    feed::{Feed, FeedFilter, FeedRow, RepoIx},
    state::{LoadState, RepoState},
    tabs::FeedTab,
};

/// Build the Closed tab's rows.
///
/// - Every closed pull request the filter accepts, as a
///   [`FeedRow::ClosedRow`], in [`closed_order`], then a spacer.
/// - Then, in the repositories' own order, a [`FeedRow::RepoError`] for each
///   repository whose last closed fetch failed — even when it still holds an
///   older list, since there is no header to carry an error chip — and a
///   [`FeedRow::RepoLoading`] for each one with nothing yet and a fetch
///   pending. Each notice is followed by a spacer.
///
/// The feed's sort, collapse and "hide empty repositories" do not apply:
/// the order is the point of the tab, and there are no repository
/// containers to collapse or hide.
pub(crate) fn flatten_closed(repos: &[RepoState], filter: &FeedFilter) -> Feed {
    let order = closed_order(repos, filter);
    let mut rows: Vec<FeedRow> = order
        .iter()
        .map(|&(repo, closed)| FeedRow::ClosedRow { repo, closed })
        .collect();
    if let Some(&(last, _)) = order.last() {
        rows.push(FeedRow::Spacer { repo: last });
    }

    for (ix, repo) in repos.iter().enumerate() {
        let repo_ix = RepoIx(ix);
        let notice = match &repo.closed_load {
            LoadState::Failed { .. } => Some(FeedRow::RepoError { repo: repo_ix }),
            LoadState::Idle | LoadState::Loading if repo.closed.is_empty() => {
                Some(FeedRow::RepoLoading { repo: repo_ix })
            }
            LoadState::Idle | LoadState::Loading | LoadState::Loaded { .. } => None,
        };
        if let Some(notice) = notice {
            rows.push(notice);
            rows.push(FeedRow::Spacer { repo: repo_ix });
        }
    }

    Feed::from_parts(rows, 0, Vec::new(), FeedTab::Closed)
}
