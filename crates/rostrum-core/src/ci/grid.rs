//! The PRs × checks matrix.
//!
//! Rows are open pull requests in the feed's order — repositories and pull
//! requests as the feed sorts them, stacks grouped as the feed groups them —
//! so the grid reads as the feed with its checks spelled out. Each repository
//! is a section with its own columns: the union of check names on its pull
//! requests. Repositories rarely share check names, so one global column set
//! would be mostly empty cells; per-section headers keep every column
//! meaningful where it is drawn.
//!
//! Like the feed, the grid is flattened into one line stream for a single
//! virtualized list, and the selection is an identity ([`CellRef`]), never an
//! index, so a refresh that reorders rows cannot move it.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::{
    feed::{FeedFilter, FeedRow, StackPlace, flatten_tab},
    model::{PrNumber, RepoId},
    state::{LoadState, RepoState},
    tabs::FeedTab,
};

use super::model::{CheckEntry, CheckKey, PrChecks, Rollup, RollupState};

/// One repository's fetched checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoChecks {
    pub prs: BTreeMap<PrNumber, PrChecks>,
    pub load: LoadState,
}

impl Default for RepoChecks {
    fn default() -> Self {
        Self {
            prs: BTreeMap::new(),
            load: LoadState::Idle,
        }
    }
}

/// Every watched repository's checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiChecks {
    repos: BTreeMap<RepoId, RepoChecks>,
}

impl CiChecks {
    pub fn repo(&self, id: &RepoId) -> Option<&RepoChecks> {
        self.repos.get(id)
    }

    pub fn pr(&self, repo: &RepoId, number: PrNumber) -> Option<&PrChecks> {
        self.repos.get(repo)?.prs.get(&number)
    }

    pub fn pr_mut(&mut self, repo: &RepoId, number: PrNumber) -> Option<&mut PrChecks> {
        self.repos.get_mut(repo)?.prs.get_mut(&number)
    }

    /// A fetch is starting. Only a first load shows as loading; a refresh
    /// keeps what is on screen.
    pub fn begin(&mut self, repo: &RepoId) {
        let entry = self.repos.entry(repo.clone()).or_default();
        if entry.prs.is_empty() {
            entry.load = LoadState::Loading;
        }
    }

    /// A fetch answered: the repository's checks are replaced wholesale.
    pub fn loaded(&mut self, repo: &RepoId, checks: Vec<PrChecks>, at: DateTime<Utc>) {
        let entry = self.repos.entry(repo.clone()).or_default();
        entry.prs = checks.into_iter().map(|pr| (pr.number, pr)).collect();
        entry.load = LoadState::Loaded { at };
    }

    /// A fetch failed. Checks already held stay; the failure is recorded.
    pub fn failed(&mut self, repo: &RepoId, message: String, at: DateTime<Utc>) {
        self.repos.entry(repo.clone()).or_default().load = LoadState::Failed { message, at };
    }

    pub fn forget(&mut self, repo: &RepoId) {
        self.repos.remove(repo);
    }

    /// Whether anything anywhere is queued or running — what decides the
    /// grid's faster poll.
    pub fn any_running(&self) -> bool {
        self.repos
            .values()
            .flat_map(|repo| repo.prs.values())
            .any(|pr| pr.latest().values().any(|entry| entry.status.is_running()))
    }
}

/// The grid's own narrowing, on top of the feed's filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridFilter {
    /// Only rows with something failing or still running.
    pub needs_attention: bool,
}

impl GridFilter {
    fn accepts(&self, rollup: &Rollup) -> bool {
        !self.needs_attention
            || matches!(rollup.state(), RollupState::Failing | RollupState::Running)
    }
}

/// One pull request's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridRow {
    pub number: PrNumber,
    pub title: String,
    /// The head commit, seven characters.
    pub head_sha: String,
    pub rollup: Rollup,
    /// One per section column; `None` is "not run" on this pull request.
    pub cells: Vec<Option<CheckEntry>>,
    /// Where the row sits in a stack, when the feed groups it into one.
    pub stack: Option<StackPlace>,
    /// Whether this pull request's checks have been fetched at all.
    pub fetched: bool,
    /// GitHub reported more checks than were fetched.
    pub truncated: bool,
}

/// One repository's block of the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridSection {
    pub repo: RepoId,
    /// The union of check names on the section's pull requests, ordered by
    /// [`CheckKey`] — workflow, then job — so a workflow's jobs sit together.
    pub columns: Vec<CheckKey>,
    pub rows: Vec<GridRow>,
    pub load: LoadState,
    /// Rows the grid filter removed.
    pub hidden: usize,
}

/// A line of the flattened grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridLine {
    /// The repository's name and its column headers.
    Header {
        section: usize,
    },
    /// Heads a stack's member rows: "Stack · 3 PRs".
    Stack {
        section: usize,
        members: usize,
    },
    Row {
        section: usize,
        row: usize,
    },
    /// No rows to show: still loading, failed, or nothing open.
    Notice {
        section: usize,
    },
    Spacer,
}

/// A cell, by identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub repo: RepoId,
    pub number: PrNumber,
    pub column: CheckKey,
}

/// A keyboard move between cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMove {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiGrid {
    sections: Vec<GridSection>,
    lines: Vec<GridLine>,
}

impl CiGrid {
    pub fn sections(&self) -> &[GridSection] {
        &self.sections
    }

    pub fn lines(&self) -> &[GridLine] {
        &self.lines
    }

    pub fn section(&self, ix: usize) -> Option<&GridSection> {
        self.sections.get(ix)
    }

    /// The widest section's column count, which sizes the horizontal scroll.
    pub fn max_columns(&self) -> usize {
        self.sections
            .iter()
            .map(|s| s.columns.len())
            .max()
            .unwrap_or(0)
    }

    /// The check a cell holds, `None` when it is "not run" or gone.
    pub fn entry(&self, cell: &CellRef) -> Option<&CheckEntry> {
        let (section, row, column) = self.position(cell)?;
        self.sections[section].rows[row].cells[column].as_ref()
    }

    /// Where a cell is: section, row and column indices.
    pub fn position(&self, cell: &CellRef) -> Option<(usize, usize, usize)> {
        let section = self.sections.iter().position(|s| s.repo == cell.repo)?;
        let rows = &self.sections[section];
        let row = rows.rows.iter().position(|r| r.number == cell.number)?;
        let column = rows.columns.iter().position(|c| *c == cell.column)?;
        Some((section, row, column))
    }

    /// The line a cell's row is drawn on, for scrolling it into view.
    pub fn line_of(&self, cell: &CellRef) -> Option<usize> {
        let (section, row, _) = self.position(cell)?;
        self.lines
            .iter()
            .position(|line| *line == GridLine::Row { section, row })
    }

    fn cell_at(&self, section: usize, row: usize, column: usize) -> Option<CellRef> {
        let s = self.sections.get(section)?;
        Some(CellRef {
            repo: s.repo.clone(),
            number: s.rows.get(row)?.number,
            column: s.columns.get(column)?.clone(),
        })
    }

    /// Every row that has at least one column, in line order.
    fn navigable(&self) -> Vec<(usize, usize)> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                GridLine::Row { section, row } if !self.sections[*section].columns.is_empty() => {
                    Some((*section, *row))
                }
                _ => None,
            })
            .collect()
    }

    /// Where `step` lands from `from`.
    ///
    /// Left and right stay in the row; up and down move between rows across
    /// sections, keeping the column by name where the next section has it and
    /// clamping its position where it does not. Nothing wraps. With no live
    /// selection, any move enters at the first cell.
    pub fn step(&self, from: Option<&CellRef>, step: CellMove) -> Option<CellRef> {
        let rows = self.navigable();
        let first = rows.first()?;
        let Some((section, row, column)) = from.and_then(|cell| self.position(cell)) else {
            return self.cell_at(first.0, first.1, 0);
        };
        let width = self.sections[section].columns.len();
        match step {
            CellMove::Left => self.cell_at(section, row, column.saturating_sub(1)),
            CellMove::Right => {
                self.cell_at(section, row, (column + 1).min(width.saturating_sub(1)))
            }
            CellMove::Up | CellMove::Down => {
                let here = rows.iter().position(|r| *r == (section, row))?;
                let target = match step {
                    CellMove::Up => here.checked_sub(1).map_or(rows[here], |ix| rows[ix]),
                    _ => *rows.get(here + 1).unwrap_or(&rows[here]),
                };
                let name = &self.sections[section].columns[column];
                let columns = &self.sections[target.0].columns;
                let column = columns
                    .iter()
                    .position(|c| c == name)
                    .unwrap_or(column.min(columns.len().saturating_sub(1)));
                self.cell_at(target.0, target.1, column)
            }
        }
    }
}

/// The feed's order with collapse ignored: collapsing hides a repository's
/// rows in the feed, but the grid is where its checks are looked at.
fn feed_order(repos: &[RepoState], filter: &FeedFilter) -> crate::feed::Feed {
    let expanded: Vec<RepoState> = repos
        .iter()
        .map(|repo| RepoState {
            collapsed: false,
            ..repo.clone()
        })
        .collect();
    flatten_tab(&expanded, filter, FeedTab::PullRequests)
}

/// Build the grid from the feed's state, its filter, and the fetched checks.
pub fn build_grid(
    repos: &[RepoState],
    feed_filter: &FeedFilter,
    checks: &CiChecks,
    filter: GridFilter,
) -> CiGrid {
    let feed = feed_order(repos, feed_filter);

    // Pass 1: gather each section's pull requests, in feed order, with their
    // stack membership.
    struct Pending {
        repo: usize,
        items: Vec<(usize, Option<(usize, StackPlace)>)>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut stack_sizes: BTreeMap<usize, usize> = BTreeMap::new();
    for row in feed.rows() {
        match *row {
            FeedRow::RepoHeader { repo } => pending.push(Pending {
                repo: repo.0,
                items: Vec::new(),
            }),
            FeedRow::PrRow { pr, stack, .. } => {
                if let Some(section) = pending.last_mut() {
                    let stack = stack.map(|slot| {
                        *stack_sizes.entry(slot.stack.0).or_default() += 1;
                        (slot.stack.0, slot.place)
                    });
                    section.items.push((pr.0, stack));
                }
            }
            _ => {}
        }
    }

    let mut grid = CiGrid::default();
    for Pending { repo, items } in pending {
        let state = &repos[repo];
        let fetched = checks.repo(&state.id);

        // Columns are the union over every pull request in the section,
        // before the grid filter, so filtering never shifts them.
        let latest: Vec<Option<BTreeMap<&CheckKey, &CheckEntry>>> = items
            .iter()
            .map(|(pr, _)| {
                fetched
                    .and_then(|r| r.prs.get(&state.prs[*pr].number))
                    .map(PrChecks::latest)
            })
            .collect();
        let columns: Vec<CheckKey> = latest
            .iter()
            .flatten()
            .flat_map(|map| map.keys().map(|key| (*key).clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let mut rows = Vec::new();
        let mut stacks = Vec::new();
        let mut hidden = 0;
        for ((pr_ix, stack), latest) in items.iter().zip(&latest) {
            let pr = &state.prs[*pr_ix];
            let pr_checks = fetched.and_then(|r| r.prs.get(&pr.number));
            let rollup = latest
                .as_ref()
                .map(|map| Rollup::of(map.values().map(|e| e.status)))
                .unwrap_or_default();
            if !filter.accepts(&rollup) {
                hidden += 1;
                continue;
            }
            rows.push(GridRow {
                number: pr.number,
                title: pr.title.clone(),
                head_sha: pr.head_sha.chars().take(7).collect(),
                rollup,
                cells: columns
                    .iter()
                    .map(|column| {
                        latest
                            .as_ref()
                            .and_then(|m| m.get(column))
                            .map(|e| (*e).clone())
                    })
                    .collect(),
                stack: stack.map(|(_, place)| place),
                fetched: pr_checks.is_some(),
                truncated: pr_checks.is_some_and(|c| c.truncated),
            });
            stacks.push(stack.map(|(ix, _)| ix));
        }

        if filter.needs_attention && rows.is_empty() {
            continue;
        }
        let section = grid.sections.len();
        grid.lines.push(GridLine::Header { section });
        if rows.is_empty() {
            grid.lines.push(GridLine::Notice { section });
        }
        let mut current_stack = None;
        for (row, stack) in stacks.iter().enumerate() {
            if *stack != current_stack {
                if let Some(ix) = stack {
                    grid.lines.push(GridLine::Stack {
                        section,
                        members: stack_sizes.get(ix).copied().unwrap_or(0),
                    });
                }
                current_stack = *stack;
            }
            grid.lines.push(GridLine::Row { section, row });
        }
        grid.lines.push(GridLine::Spacer);
        grid.sections.push(GridSection {
            repo: state.id.clone(),
            columns,
            rows,
            load: fetched.map(|r| r.load.clone()).unwrap_or(LoadState::Idle),
            hidden,
        });
    }
    grid
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        ci::model::{CheckSource, CheckStatus},
        test_support::pull,
    };

    fn key(name: &str) -> CheckKey {
        CheckKey::new(Some("CI".into()), name)
    }

    fn entry(name: &str, status: CheckStatus, job_id: u64) -> CheckEntry {
        CheckEntry {
            key: key(name),
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

    /// Two repositories: a/b with #1 (build ok, test failing) and #2 (lint
    /// running); c/d with #3 (build ok).
    fn fixture() -> (Vec<RepoState>, CiChecks) {
        let repos = vec![repo("a/b", &[1, 2]), repo("c/d", &[3])];
        let mut ci = CiChecks::default();
        ci.loaded(
            &id("a/b"),
            vec![
                checks(
                    1,
                    vec![
                        entry("test", CheckStatus::Failure, 11),
                        entry("build", CheckStatus::Success, 12),
                    ],
                ),
                checks(2, vec![entry("lint", CheckStatus::InProgress, 21)]),
            ],
            Utc::now(),
        );
        ci.loaded(
            &id("c/d"),
            vec![checks(3, vec![entry("build", CheckStatus::Success, 31)])],
            Utc::now(),
        );
        (repos, ci)
    }

    fn grid(filter: GridFilter) -> CiGrid {
        let (repos, ci) = fixture();
        build_grid(&repos, &FeedFilter::default(), &ci, filter)
    }

    #[test]
    fn each_section_has_the_union_of_its_check_names_in_key_order() {
        let grid = grid(GridFilter::default());
        assert_eq!(grid.sections().len(), 2);
        assert_eq!(
            grid.sections()[0].columns,
            vec![key("build"), key("lint"), key("test")]
        );
        assert_eq!(grid.sections()[1].columns, vec![key("build")]);
        assert_eq!(grid.max_columns(), 3);
    }

    /// A column a pull request has no run for is "not run", not empty data.
    #[test]
    fn cells_follow_the_columns_with_not_run_gaps() {
        let grid = grid(GridFilter::default());
        let first = &grid.sections()[0].rows[0];
        let statuses: Vec<Option<CheckStatus>> = first
            .cells
            .iter()
            .map(|c| c.as_ref().map(|e| e.status))
            .collect();
        assert_eq!(
            statuses,
            [Some(CheckStatus::Success), None, Some(CheckStatus::Failure)]
        );
        assert_eq!(first.rollup.state(), RollupState::Failing);
        assert_eq!(first.head_sha, "deadbee");
        assert!(first.fetched);
    }

    #[test]
    fn lines_head_each_section_and_list_its_rows() {
        let grid = grid(GridFilter::default());
        assert_eq!(
            grid.lines(),
            &[
                GridLine::Header { section: 0 },
                GridLine::Row { section: 0, row: 0 },
                GridLine::Row { section: 0, row: 1 },
                GridLine::Spacer,
                GridLine::Header { section: 1 },
                GridLine::Row { section: 1, row: 0 },
                GridLine::Spacer,
            ]
        );
    }

    /// The attention filter keeps failing and running rows, drops sections
    /// left empty, and never shifts a section's columns.
    #[test]
    fn the_attention_filter_keeps_failing_and_running_rows() {
        let grid = grid(GridFilter {
            needs_attention: true,
        });
        assert_eq!(grid.sections().len(), 1);
        let section = &grid.sections()[0];
        assert_eq!(section.rows.len(), 2);
        assert_eq!(section.hidden, 0);
        assert_eq!(section.columns.len(), 3);

        let (mut repos, ci) = fixture();
        repos[0].prs.push(pull(9));
        let narrowed = build_grid(
            &repos,
            &FeedFilter::default(),
            &ci,
            GridFilter {
                needs_attention: true,
            },
        );
        // #9 has no checks fetched: nothing failing or running, so hidden.
        assert_eq!(narrowed.sections()[0].hidden, 1);
    }

    /// Rows follow the feed's order and filter, collapse aside.
    #[test]
    fn rows_follow_the_feed_but_not_its_collapse() {
        let (mut repos, ci) = fixture();
        repos[0].collapsed = true;
        let grid = build_grid(&repos, &FeedFilter::default(), &ci, GridFilter::default());
        assert_eq!(grid.sections()[0].rows.len(), 2);

        let drafts_hidden = FeedFilter {
            hide_drafts: true,
            ..Default::default()
        };
        repos[0].prs[1].is_draft = true;
        let grid = build_grid(&repos, &drafts_hidden, &ci, GridFilter::default());
        assert_eq!(grid.sections()[0].rows.len(), 1);
    }

    #[test]
    fn a_pull_request_with_no_fetched_checks_is_a_row_of_not_run() {
        let (mut repos, ci) = fixture();
        repos[1].prs.push(pull(4));
        let grid = build_grid(&repos, &FeedFilter::default(), &ci, GridFilter::default());
        let row = grid.sections()[1]
            .rows
            .iter()
            .find(|r| r.number == PrNumber(4))
            .expect("row present");
        assert!(!row.fetched);
        assert!(row.cells.iter().all(Option::is_none));
        assert_eq!(row.rollup.state(), RollupState::Empty);
    }

    #[test]
    fn a_repository_with_nothing_open_shows_a_notice() {
        let repos = vec![repo("a/b", &[])];
        let grid = build_grid(
            &repos,
            &FeedFilter {
                hide_empty_repos: false,
                ..Default::default()
            },
            &CiChecks::default(),
            GridFilter::default(),
        );
        assert_eq!(
            grid.lines(),
            &[
                GridLine::Header { section: 0 },
                GridLine::Notice { section: 0 },
                GridLine::Spacer
            ]
        );
    }

    fn cell(repo: &str, number: u32, name: &str) -> CellRef {
        CellRef {
            repo: id(repo),
            number: PrNumber(number),
            column: key(name),
        }
    }

    #[test]
    fn moving_with_no_selection_enters_at_the_first_cell() {
        let grid = grid(GridFilter::default());
        assert_eq!(
            grid.step(None, CellMove::Down),
            Some(cell("a/b", 1, "build"))
        );
    }

    #[test]
    fn left_and_right_stay_in_the_row_without_wrapping() {
        let grid = grid(GridFilter::default());
        let start = cell("a/b", 1, "build");
        assert_eq!(grid.step(Some(&start), CellMove::Left), Some(start.clone()));
        let right = grid.step(Some(&start), CellMove::Right).expect("moves");
        assert_eq!(right, cell("a/b", 1, "lint"));
        let end = cell("a/b", 1, "test");
        assert_eq!(grid.step(Some(&end), CellMove::Right), Some(end));
    }

    /// Down crosses into the next section, keeping the column by name where
    /// it exists there, and clamping where it does not.
    #[test]
    fn up_and_down_cross_sections_keeping_the_column_by_name() {
        let grid = grid(GridFilter::default());
        let down = grid.step(Some(&cell("a/b", 2, "build")), CellMove::Down);
        assert_eq!(down, Some(cell("c/d", 3, "build")));
        let clamped = grid.step(Some(&cell("a/b", 2, "test")), CellMove::Down);
        assert_eq!(clamped, Some(cell("c/d", 3, "build")));
        let bottom = cell("c/d", 3, "build");
        assert_eq!(
            grid.step(Some(&bottom), CellMove::Down),
            Some(bottom.clone())
        );
        assert_eq!(
            grid.step(Some(&bottom), CellMove::Up),
            Some(cell("a/b", 2, "build"))
        );
    }

    #[test]
    fn a_cell_resolves_to_its_entry_and_line() {
        let grid = grid(GridFilter::default());
        let failing = cell("a/b", 1, "test");
        assert_eq!(
            grid.entry(&failing).map(|e| e.status),
            Some(CheckStatus::Failure)
        );
        assert_eq!(grid.line_of(&failing), Some(1));
        assert_eq!(grid.entry(&cell("a/b", 1, "lint")), None, "not run");
        assert_eq!(grid.entry(&cell("x/y", 1, "test")), None);
    }

    #[test]
    fn running_anywhere_is_known() {
        let (_, ci) = fixture();
        assert!(ci.any_running());
        let mut quiet = CiChecks::default();
        quiet.loaded(
            &id("a/b"),
            vec![checks(1, vec![entry("t", CheckStatus::Success, 1)])],
            Utc::now(),
        );
        assert!(!quiet.any_running());
    }

    #[test]
    fn a_refresh_keeps_checks_on_screen_and_records_failures() {
        let (_, mut ci) = fixture();
        ci.begin(&id("a/b"));
        assert!(matches!(
            ci.repo(&id("a/b")).map(|r| &r.load),
            Some(LoadState::Loaded { .. })
        ));
        ci.failed(&id("a/b"), "boom".into(), Utc::now());
        assert_eq!(ci.repo(&id("a/b")).map(|r| r.prs.len()), Some(2));
        ci.begin(&id("new/repo"));
        assert_eq!(
            ci.repo(&id("new/repo")).map(|r| &r.load),
            Some(&LoadState::Loading)
        );
    }
}
