//! Building the matrix from the feed's state, its filter, and the fetched
//! checks.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    feed::{Feed, FeedFilter, FeedRow, StackPlace, flatten_tab},
    model::RepoId,
    state::{LoadState, RepoState},
    tabs::FeedTab,
};

use super::{
    super::model::{CheckEntry, CheckKey, PrChecks, Rollup},
    CiChecks, CiGrid, ColumnGroup, GridFilter, GridLine, GridRepo, GridRow,
};

/// The feed filter the grid's rows come from: the feed's search, authors
/// and sort, with drafts always out — a draft's checks are not what the grid
/// is for, whatever the feed's draft toggle shows.
fn rows_filter(feed: &FeedFilter) -> FeedFilter {
    FeedFilter {
        hide_drafts: true,
        ..feed.clone()
    }
}

/// The feed's order with collapse ignored: collapsing hides a repository's
/// rows in the feed, but the grid is where its checks are looked at.
fn feed_order(repos: &[RepoState], filter: &FeedFilter) -> Feed {
    let expanded: Vec<RepoState> = repos
        .iter()
        .map(|repo| RepoState {
            collapsed: false,
            ..repo.clone()
        })
        .collect();
    flatten_tab(&expanded, &rows_filter(filter), FeedTab::PullRequests)
}

/// One pull request the feed shows, before the grid filter.
struct Candidate<'a> {
    repo: &'a RepoState,
    pr: usize,
    /// The stack it belongs to (the feed's index) and its place there.
    stack: Option<(usize, StackPlace)>,
    checks: Option<&'a PrChecks>,
    latest: Option<BTreeMap<&'a CheckKey, &'a CheckEntry>>,
}

/// Every repository the feed shows and every pull request it lists, in the
/// feed's order, with stack sizes counted over the whole feed.
fn gather<'a>(
    repos: &'a [RepoState],
    feed: &Feed,
    checks: &'a CiChecks,
) -> (
    Vec<&'a RepoState>,
    Vec<Candidate<'a>>,
    BTreeMap<usize, usize>,
) {
    let mut shown = Vec::new();
    let mut candidates = Vec::new();
    let mut stack_sizes: BTreeMap<usize, usize> = BTreeMap::new();
    for row in feed.rows() {
        match *row {
            FeedRow::RepoHeader { repo } => shown.push(&repos[repo.0]),
            FeedRow::PrRow { repo, pr, stack } => {
                let state = &repos[repo.0];
                let pr_checks = checks.pr(&state.id, state.prs[pr.0].number);
                let stack = stack.map(|slot| {
                    *stack_sizes.entry(slot.stack.0).or_default() += 1;
                    (slot.stack.0, slot.place)
                });
                candidates.push(Candidate {
                    repo: state,
                    pr: pr.0,
                    stack,
                    checks: pr_checks,
                    latest: pr_checks.map(PrChecks::latest),
                });
            }
            _ => {}
        }
    }
    (shown, candidates, stack_sizes)
}

/// Adjacent columns of one workflow, as ranges partitioning `columns`.
fn group_columns(columns: &[CheckKey]) -> Vec<ColumnGroup> {
    let mut groups: Vec<ColumnGroup> = Vec::new();
    for (ix, column) in columns.iter().enumerate() {
        match groups.last_mut() {
            Some(group) if group.workflow == column.workflow => group.columns.end = ix + 1,
            _ => groups.push(ColumnGroup {
                workflow: column.workflow.clone(),
                columns: ix..ix + 1,
            }),
        }
    }
    groups
}

/// Build the grid from the feed's state, its filter, and the fetched checks.
///
/// Rows: every open, non-draft pull request the feed's search and author
/// selection let through, in the feed's order (collapse ignored), then
/// narrowed by `filter`. Columns: the union of the latest check keys over
/// those rows *before* `filter`, so filtering never shifts a column.
pub fn build_grid(
    repos: &[RepoState],
    feed_filter: &FeedFilter,
    checks: &CiChecks,
    filter: GridFilter,
) -> CiGrid {
    let feed = feed_order(repos, feed_filter);
    let (shown, candidates, stack_sizes) = gather(repos, &feed, checks);

    let columns: Vec<CheckKey> = candidates
        .iter()
        .filter_map(|c| c.latest.as_ref())
        .flat_map(|map| map.keys().map(|key| (*key).clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut grid = CiGrid {
        groups: group_columns(&columns),
        repos: shown
            .iter()
            .map(|state| GridRepo {
                repo: state.id.clone(),
                load: checks
                    .repo(&state.id)
                    .map(|r| r.load.clone())
                    .unwrap_or(LoadState::Idle),
                rows: 0,
                hidden: 0,
            })
            .collect(),
        ..CiGrid::default()
    };
    let summary_of: BTreeMap<&RepoId, usize> = shown
        .iter()
        .enumerate()
        .map(|(ix, state)| (&state.id, ix))
        .collect();

    let mut current_stack = None;
    for candidate in &candidates {
        let pr = &candidate.repo.prs[candidate.pr];
        let rollup = candidate
            .latest
            .as_ref()
            .map(|map| Rollup::of(map.values().map(|e| e.status)))
            .unwrap_or_default();
        let summary = summary_of.get(&candidate.repo.id).copied();
        if !filter.accepts(&rollup) {
            grid.hidden += 1;
            if let Some(ix) = summary {
                grid.repos[ix].hidden += 1;
            }
            continue;
        }
        if let Some(ix) = summary {
            grid.repos[ix].rows += 1;
        }

        let stack = candidate.stack.map(|(ix, _)| ix);
        if stack != current_stack {
            if let Some(ix) = stack {
                grid.lines.push(GridLine::Stack {
                    members: stack_sizes.get(&ix).copied().unwrap_or(0),
                });
            }
            current_stack = stack;
        }
        grid.lines.push(GridLine::Row {
            row: grid.rows.len(),
        });
        grid.rows.push(GridRow {
            repo: candidate.repo.id.clone(),
            number: pr.number,
            title: pr.title.clone(),
            head_sha: pr.head_sha.chars().take(7).collect(),
            rollup,
            cells: columns
                .iter()
                .map(|column| {
                    candidate
                        .latest
                        .as_ref()
                        .and_then(|m| m.get(column))
                        .map(|e| (*e).clone())
                })
                .collect(),
            stack: candidate.stack.map(|(_, place)| place),
            fetched: candidate.checks.is_some(),
            truncated: candidate.checks.is_some_and(|c| c.truncated),
        });
    }
    grid.columns = columns;
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(workflow: Option<&str>, name: &str) -> CheckKey {
        CheckKey::new(workflow.map(Into::into), name)
    }

    #[test]
    fn groups_partition_the_columns_by_adjacent_workflow() {
        let columns = vec![
            key(None, "Coverage"),
            key(None, "deploy"),
            key(Some("CI"), "build"),
            key(Some("Lint"), "fmt"),
            key(Some("Lint"), "clippy"),
        ];
        let groups = group_columns(&columns);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.workflow.as_deref(), g.columns.clone()))
                .collect::<Vec<_>>(),
            vec![(None, 0..2), (Some("CI"), 2..3), (Some("Lint"), 3..5)]
        );
    }

    #[test]
    fn the_rows_filter_hides_drafts_and_keeps_the_rest() {
        let feed = FeedFilter {
            query: "fix".into(),
            hide_drafts: false,
            ..Default::default()
        };
        let rows = rows_filter(&feed);
        assert!(rows.hide_drafts);
        assert_eq!(rows.query, "fix");
        assert_eq!(rows.sort, feed.sort);
    }
}
