//! The global matrix: rows across every repository, one column set, drafts
//! out, the feed's search and authors in, and movement over the whole grid.

use chrono::Utc;

use super::*;
use crate::{
    ci::model::{CheckSource, CheckStatus, PrChecks},
    feed::{FeedFilter, StackPlace},
    model::{LoginKey, PrNumber, RepoId, User},
    state::{LoadState, RepoState},
    test_support::pull,
};

fn key(workflow: Option<&str>, name: &str) -> CheckKey {
    CheckKey::new(workflow.map(Into::into), name)
}

fn ci(name: &str) -> CheckKey {
    key(Some("CI"), name)
}

fn coverage() -> CheckKey {
    key(None, "Coverage")
}

fn entry(key: CheckKey, status: CheckStatus, job_id: u64) -> CheckEntry {
    CheckEntry {
        key,
        status,
        started_at: None,
        completed_at: None,
        details_url: None,
        source: CheckSource::Actions {
            job_id,
            run_id: 7,
            run_attempt: 1,
            suite_id: 1,
        },
    }
}

fn checks(number: u32, entries: Vec<CheckEntry>) -> PrChecks {
    PrChecks {
        number: PrNumber(number),
        head_sha: "deadbeef".into(),
        entries,
        truncated: false,
    }
}

fn repo(name: &str, numbers: &[u32]) -> RepoState {
    let mut state = RepoState::new(name.parse().expect("valid repo id"));
    state.prs = numbers.iter().copied().map(pull).collect();
    state.load = LoadState::Loaded { at: Utc::now() };
    state
}

fn id(name: &str) -> RepoId {
    name.parse().expect("valid repo id")
}

/// Two repositories that share one check and not the rest:
///
/// - a/b #1: CI/build ok, CI/test failing; #2: CI/lint running.
/// - c/d #3: CI/build ok, Coverage (an app, no workflow) neutral.
fn fixture() -> (Vec<RepoState>, CiChecks) {
    let repos = vec![repo("a/b", &[1, 2]), repo("c/d", &[3])];
    let mut checks_held = CiChecks::default();
    checks_held.loaded(
        &id("a/b"),
        vec![
            checks(
                1,
                vec![
                    entry(ci("test"), CheckStatus::Failure, 11),
                    entry(ci("build"), CheckStatus::Success, 12),
                ],
            ),
            checks(2, vec![entry(ci("lint"), CheckStatus::InProgress, 21)]),
        ],
        Utc::now(),
    );
    checks_held.loaded(
        &id("c/d"),
        vec![checks(
            3,
            vec![
                entry(ci("build"), CheckStatus::Success, 31),
                entry(coverage(), CheckStatus::Neutral, 32),
            ],
        )],
        Utc::now(),
    );
    (repos, checks_held)
}

fn grid(filter: GridFilter) -> CiGrid {
    let (repos, held) = fixture();
    build_grid(&repos, &FeedFilter::default(), &held, filter)
}

fn numbers(grid: &CiGrid) -> Vec<(String, u32)> {
    grid.rows()
        .iter()
        .map(|row| (row.repo.to_string(), row.number.0))
        .collect()
}

fn statuses(row: &GridRow) -> Vec<Option<CheckStatus>> {
    row.cells
        .iter()
        .map(|c| c.as_ref().map(|e| e.status))
        .collect()
}

/// One column set for the whole grid: the union of every repository's keys,
/// ordered workflow then name, with no workflow first.
#[test]
fn columns_are_the_union_across_every_repository_in_key_order() {
    let grid = grid(GridFilter::default());
    assert_eq!(
        grid.columns(),
        &[coverage(), ci("build"), ci("lint"), ci("test")]
    );
}

#[test]
fn columns_are_grouped_by_workflow_in_adjacent_runs() {
    let grid = grid(GridFilter::default());
    assert_eq!(
        grid.column_groups(),
        &[
            ColumnGroup {
                workflow: None,
                columns: 0..1,
            },
            ColumnGroup {
                workflow: Some("CI".into()),
                columns: 1..4,
            },
        ]
    );
}

#[test]
fn an_empty_grid_has_no_columns_and_no_groups() {
    let grid = build_grid(
        &[],
        &FeedFilter::default(),
        &CiChecks::default(),
        GridFilter::default(),
    );
    assert!(grid.columns().is_empty());
    assert!(grid.column_groups().is_empty());
    assert!(grid.rows().is_empty());
    assert!(grid.lines().is_empty());
    assert_eq!(grid.step(None, CellMove::Down), None);
}

/// Rows run across repositories in the feed's repository order, then its
/// item order, each naming its repository.
#[test]
fn rows_span_every_repository_in_feed_order() {
    let grid = grid(GridFilter::default());
    assert_eq!(
        numbers(&grid),
        vec![
            ("a/b".to_string(), 1),
            ("a/b".to_string(), 2),
            ("c/d".to_string(), 3)
        ]
    );
    assert_eq!(
        grid.lines(),
        &[
            GridLine::Row { row: 0 },
            GridLine::Row { row: 1 },
            GridLine::Row { row: 2 },
        ]
    );
}

/// Every row has one cell per global column; another repository's checks
/// read as "not run".
#[test]
fn cells_follow_the_global_columns_with_not_run_gaps() {
    let grid = grid(GridFilter::default());
    let first = &grid.rows()[0];
    assert_eq!(
        statuses(first),
        [
            None,
            Some(CheckStatus::Success),
            None,
            Some(CheckStatus::Failure)
        ]
    );
    assert_eq!(first.rollup.state(), RollupState::Failing);
    assert_eq!(first.head_sha, "deadbee");
    assert!(first.fetched);
    let other = &grid.rows()[2];
    assert_eq!(
        statuses(other),
        [
            Some(CheckStatus::Neutral),
            Some(CheckStatus::Success),
            None,
            None
        ]
    );
    assert!(
        grid.rows()
            .iter()
            .all(|row| row.cells.len() == grid.columns().len())
    );
}

/// Drafts never appear, whatever the feed's draft toggle says, and their
/// checks contribute no columns.
#[test]
fn drafts_are_excluded_even_when_the_feed_shows_them() {
    let (mut repos, held) = fixture();
    repos[0].prs[1].is_draft = true;
    let feed = FeedFilter::default();
    assert!(!feed.hide_drafts, "the feed shows drafts");
    let grid = build_grid(&repos, &feed, &held, GridFilter::default());
    assert_eq!(
        numbers(&grid),
        vec![("a/b".to_string(), 1), ("c/d".to_string(), 3)]
    );
    assert_eq!(grid.columns(), &[coverage(), ci("build"), ci("test")]);
    assert_eq!(grid.hidden(), 0, "a draft is not a hidden row");
}

/// The feed's search narrows the grid.
#[test]
fn the_feeds_search_still_applies() {
    let (repos, held) = fixture();
    let feed = FeedFilter {
        query: "PR 3".into(),
        ..Default::default()
    };
    let grid = build_grid(&repos, &feed, &held, GridFilter::default());
    assert_eq!(numbers(&grid), vec![("c/d".to_string(), 3)]);
    // Columns follow the rows the feed lets through.
    assert_eq!(grid.columns(), &[coverage(), ci("build")]);
}

/// The feed's author selection narrows the grid.
#[test]
fn the_feeds_authors_still_apply() {
    let (mut repos, held) = fixture();
    repos[0].prs[1].author = Some(User {
        login: "alice".into(),
        avatar_url: None,
    });
    let feed = FeedFilter {
        authors: [LoginKey::new("alice")].into_iter().collect(),
        ..Default::default()
    };
    let grid = build_grid(&repos, &feed, &held, GridFilter::default());
    assert_eq!(numbers(&grid), vec![("a/b".to_string(), 2)]);
}

/// The attention filter keeps failing and running rows and never shifts
/// the columns.
#[test]
fn the_attention_filter_keeps_failing_and_running_rows_without_shifting_columns() {
    let all = grid(GridFilter::default());
    let narrowed = grid(GridFilter {
        needs_attention: true,
    });
    assert_eq!(
        numbers(&narrowed),
        vec![("a/b".to_string(), 1), ("a/b".to_string(), 2)]
    );
    assert_eq!(narrowed.columns(), all.columns());
    assert_eq!(narrowed.column_groups(), all.column_groups());
    assert_eq!(narrowed.hidden(), 1);
    let cd = narrowed
        .repos()
        .iter()
        .find(|r| r.repo == id("c/d"))
        .expect("c/d still summarised");
    assert_eq!((cd.rows, cd.hidden), (0, 1));
}

#[test]
fn rows_ignore_the_feeds_collapse() {
    let (mut repos, held) = fixture();
    repos[0].collapsed = true;
    let grid = build_grid(&repos, &FeedFilter::default(), &held, GridFilter::default());
    assert_eq!(grid.rows().len(), 3);
}

#[test]
fn a_pull_request_with_no_fetched_checks_is_a_row_of_not_run() {
    let (mut repos, held) = fixture();
    repos[1].prs.push(pull(4));
    let grid = build_grid(&repos, &FeedFilter::default(), &held, GridFilter::default());
    let row = grid
        .rows()
        .iter()
        .find(|r| r.number == PrNumber(4))
        .expect("row present");
    assert_eq!(row.repo, id("c/d"));
    assert!(!row.fetched);
    assert!(row.cells.iter().all(Option::is_none));
    assert_eq!(row.rollup.state(), RollupState::Empty);
}

/// Each repository the feed shows is summarised with its checks' load state,
/// so a failure or a first load is visible without a section to put it in.
#[test]
fn every_repository_is_summarised_with_its_load_state() {
    let (mut repos, mut held) = fixture();
    repos.push(repo("e/f", &[5]));
    held.failed(&id("c/d"), "boom".into(), Utc::now());
    let grid = build_grid(&repos, &FeedFilter::default(), &held, GridFilter::default());
    let summary: Vec<(String, usize, bool)> = grid
        .repos()
        .iter()
        .map(|r| (r.repo.to_string(), r.rows, r.load.error_message().is_some()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a/b".to_string(), 2, false),
            ("c/d".to_string(), 1, true),
            ("e/f".to_string(), 1, false),
        ]
    );
    let ef = &grid.repos()[2];
    assert_eq!(ef.load, LoadState::Idle, "never fetched");
    // c/d's earlier checks stay on screen through the failure.
    assert!(grid.rows()[2].fetched);
}

fn link(number: u32, head: &str, base: &str) -> crate::model::PullRequest {
    let mut pr = pull(number);
    pr.head_ref = head.into();
    pr.base_ref = base.into();
    pr
}

/// A stack still heads its members, inside its repository's run of rows.
#[test]
fn stacks_head_their_members_within_a_repository() {
    let mut a = repo("a/b", &[]);
    a.prs = vec![link(1, "one", "main"), link(2, "two", "one")];
    let repos = vec![a, repo("c/d", &[3])];
    let (_, held) = fixture();
    let grid = build_grid(&repos, &FeedFilter::default(), &held, GridFilter::default());
    assert_eq!(
        grid.lines(),
        &[
            GridLine::Stack { members: 2 },
            GridLine::Row { row: 0 },
            GridLine::Row { row: 1 },
            GridLine::Row { row: 2 },
        ]
    );
    assert_eq!(grid.rows()[0].stack, Some(StackPlace::Bottom));
    assert_eq!(grid.rows()[1].stack, Some(StackPlace::Top));
    assert_eq!(grid.rows()[2].stack, None);
}

/// A draft in a stack drops out; the rest of the stack stays grouped.
#[test]
fn a_draft_stack_member_drops_out_of_its_stack() {
    let mut a = repo("a/b", &[]);
    a.prs = vec![
        link(1, "one", "main"),
        link(2, "two", "one"),
        link(4, "three", "two"),
    ];
    a.prs[2].is_draft = true;
    let (_, held) = fixture();
    let grid = build_grid(&[a], &FeedFilter::default(), &held, GridFilter::default());
    assert_eq!(grid.rows().len(), 2);
    assert_eq!(grid.lines()[0], GridLine::Stack { members: 2 });
}

fn cell(repo: &str, number: u32, column: CheckKey) -> CellRef {
    CellRef {
        repo: id(repo),
        number: PrNumber(number),
        column,
    }
}

#[test]
fn moving_with_no_selection_enters_at_the_first_cell() {
    let grid = grid(GridFilter::default());
    assert_eq!(
        grid.step(None, CellMove::Down),
        Some(cell("a/b", 1, coverage()))
    );
}

#[test]
fn left_and_right_stay_in_the_row_without_wrapping() {
    let grid = grid(GridFilter::default());
    let start = cell("a/b", 1, coverage());
    assert_eq!(grid.step(Some(&start), CellMove::Left), Some(start.clone()));
    assert_eq!(
        grid.step(Some(&start), CellMove::Right),
        Some(cell("a/b", 1, ci("build")))
    );
    let end = cell("a/b", 1, ci("test"));
    assert_eq!(grid.step(Some(&end), CellMove::Right), Some(end));
}

/// Up and down cross repositories in the same column, even onto a "not
/// run" cell, and stop at the edges.
#[test]
fn up_and_down_keep_the_column_across_repositories() {
    let grid = grid(GridFilter::default());
    assert_eq!(
        grid.step(Some(&cell("a/b", 2, ci("test"))), CellMove::Down),
        Some(cell("c/d", 3, ci("test")))
    );
    let bottom = cell("c/d", 3, ci("build"));
    assert_eq!(
        grid.step(Some(&bottom), CellMove::Down),
        Some(bottom.clone())
    );
    assert_eq!(
        grid.step(Some(&bottom), CellMove::Up),
        Some(cell("a/b", 2, ci("build")))
    );
    let top = cell("a/b", 1, ci("lint"));
    assert_eq!(grid.step(Some(&top), CellMove::Up), Some(top));
}

/// A selection the grid no longer holds (filtered away) re-enters at the
/// first cell.
#[test]
fn a_selection_gone_from_the_grid_re_enters_at_the_start() {
    let grid = grid(GridFilter {
        needs_attention: true,
    });
    assert_eq!(
        grid.step(Some(&cell("c/d", 3, ci("build"))), CellMove::Down),
        Some(cell("a/b", 1, coverage()))
    );
}

#[test]
fn a_cell_resolves_to_its_entry_position_and_line() {
    let grid = grid(GridFilter::default());
    let failing = cell("a/b", 1, ci("test"));
    assert_eq!(
        grid.entry(&failing).map(|e| e.status),
        Some(CheckStatus::Failure)
    );
    assert_eq!(grid.position(&failing), Some(GridPos { row: 0, column: 3 }));
    assert_eq!(grid.line_of(&failing), Some(0));
    assert_eq!(grid.entry(&cell("a/b", 1, ci("lint"))), None, "not run");
    assert_eq!(grid.entry(&cell("x/y", 1, ci("test"))), None);
    // The same number in another repository is another row.
    assert_eq!(grid.position(&cell("c/d", 1, ci("test"))), None);
}

#[test]
fn a_line_accounts_for_stack_headers() {
    let mut a = repo("a/b", &[]);
    a.prs = vec![link(1, "one", "main"), link(2, "two", "one")];
    let (_, held) = fixture();
    let grid = build_grid(&[a], &FeedFilter::default(), &held, GridFilter::default());
    assert_eq!(grid.line_of(&cell("a/b", 2, ci("build"))), Some(2));
}
