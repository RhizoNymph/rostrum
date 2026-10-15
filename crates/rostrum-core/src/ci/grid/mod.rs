//! The PRs × checks matrix: one grid over every watched repository.
//!
//! Rows are every open, non-draft pull request the feed's search and author
//! selection let through, in the feed's order — repositories as the feed
//! sorts them, then pull requests within each, stacks grouped as the feed
//! groups them — each row naming its repository. Columns are one set for
//! the whole grid: the union of [`CheckKey`]s across those rows, ordered
//! workflow then name and grouped by workflow ([`ColumnGroup`]). A check one
//! repository never runs is "not run" on every other repository's rows; that
//! is accepted for one matrix that reads the same way top to bottom.
//!
//! Like the feed, the rows are flattened into one line stream for a single
//! virtualized list, and the selection is an identity ([`CellRef`]), never an
//! index, so a refresh that reorders rows cannot move it.

mod build;
mod store;

#[cfg(test)]
mod tests;

use std::ops::Range;

use crate::{
    feed::StackPlace,
    model::{PrNumber, RepoId},
    state::LoadState,
};

use super::model::{CheckEntry, CheckKey, Rollup, RollupState};

pub use build::build_grid;
pub use store::{CiChecks, RepoChecks};

/// The grid's own narrowing, on top of the feed's search and authors.
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
    pub repo: RepoId,
    pub number: PrNumber,
    pub title: String,
    /// The head commit, seven characters.
    pub head_sha: String,
    pub rollup: Rollup,
    /// One per grid column; `None` is "not run" on this pull request.
    pub cells: Vec<Option<CheckEntry>>,
    /// Where the row sits in a stack, when the feed groups it into one.
    pub stack: Option<StackPlace>,
    /// Whether this pull request's checks have been fetched at all.
    pub fetched: bool,
    /// GitHub reported more checks than were fetched.
    pub truncated: bool,
}

/// A run of adjacent columns sharing a workflow, for a header that names the
/// workflow once above its jobs. The groups partition the columns in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnGroup {
    /// `None` for apps' checks and legacy statuses.
    pub workflow: Option<String>,
    /// Indices into [`CiGrid::columns`]; never empty.
    pub columns: Range<usize>,
}

/// A repository the feed shows, summarised: with no per-repository section,
/// this is where its checks' load state and its row counts live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridRepo {
    pub repo: RepoId,
    /// The checks' fetch state; `Idle` until first fetched.
    pub load: LoadState,
    /// Rows the grid shows for this repository.
    pub rows: usize,
    /// Rows the grid filter removed.
    pub hidden: usize,
}

/// A line of the flattened grid. The column headers are not a line: there is
/// one header for the whole grid, drawn above the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridLine {
    /// Heads a stack's member rows: "Stack · 3 PRs".
    Stack { members: usize },
    /// Index into [`CiGrid::rows`].
    Row { row: usize },
}

/// A cell, by identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub repo: RepoId,
    pub number: PrNumber,
    pub column: CheckKey,
}

/// Where a cell is drawn: indices into [`CiGrid::rows`] and
/// [`CiGrid::columns`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPos {
    pub row: usize,
    pub column: usize,
}

/// A keyboard move between cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMove {
    Up,
    Down,
    Left,
    Right,
}

/// The matrix. Built only by [`build_grid`], which keeps every row's cells
/// the length of [`CiGrid::columns`] and the groups a partition of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiGrid {
    columns: Vec<CheckKey>,
    groups: Vec<ColumnGroup>,
    rows: Vec<GridRow>,
    repos: Vec<GridRepo>,
    lines: Vec<GridLine>,
    hidden: usize,
}

impl CiGrid {
    /// Every column, ordered workflow then name.
    pub fn columns(&self) -> &[CheckKey] {
        &self.columns
    }

    pub fn column_groups(&self) -> &[ColumnGroup] {
        &self.groups
    }

    pub fn rows(&self) -> &[GridRow] {
        &self.rows
    }

    pub fn row(&self, ix: usize) -> Option<&GridRow> {
        self.rows.get(ix)
    }

    /// Each repository the feed shows, in the feed's order.
    pub fn repos(&self) -> &[GridRepo] {
        &self.repos
    }

    pub fn lines(&self) -> &[GridLine] {
        &self.lines
    }

    /// Rows the grid filter removed, across every repository.
    pub fn hidden(&self) -> usize {
        self.hidden
    }

    /// The check a cell holds, `None` when it is "not run" or gone.
    pub fn entry(&self, cell: &CellRef) -> Option<&CheckEntry> {
        let GridPos { row, column } = self.position(cell)?;
        self.rows[row].cells[column].as_ref()
    }

    /// Where a cell is.
    pub fn position(&self, cell: &CellRef) -> Option<GridPos> {
        let row = self
            .rows
            .iter()
            .position(|r| r.repo == cell.repo && r.number == cell.number)?;
        let column = self.columns.iter().position(|c| *c == cell.column)?;
        Some(GridPos { row, column })
    }

    /// The line a cell's row is drawn on, for scrolling it into view.
    pub fn line_of(&self, cell: &CellRef) -> Option<usize> {
        let GridPos { row, .. } = self.position(cell)?;
        self.lines
            .iter()
            .position(|line| *line == GridLine::Row { row })
    }

    /// The cell at a position, by identity.
    pub fn cell_at(&self, pos: GridPos) -> Option<CellRef> {
        let row = self.rows.get(pos.row)?;
        Some(CellRef {
            repo: row.repo.clone(),
            number: row.number,
            column: self.columns.get(pos.column)?.clone(),
        })
    }

    /// Where `step` lands from `from`.
    ///
    /// Left and right stay in the row; up and down move between rows,
    /// across repositories, in the same column. Nothing wraps. With no live
    /// selection, any move enters at the first cell.
    pub fn step(&self, from: Option<&CellRef>, step: CellMove) -> Option<CellRef> {
        let Some(GridPos { row, column }) = from.and_then(|cell| self.position(cell)) else {
            return self.cell_at(GridPos { row: 0, column: 0 });
        };
        let last_row = self.rows.len().saturating_sub(1);
        let last_column = self.columns.len().saturating_sub(1);
        let to = match step {
            CellMove::Left => GridPos {
                row,
                column: column.saturating_sub(1),
            },
            CellMove::Right => GridPos {
                row,
                column: (column + 1).min(last_column),
            },
            CellMove::Up => GridPos {
                row: row.saturating_sub(1),
                column,
            },
            CellMove::Down => GridPos {
                row: (row + 1).min(last_row),
                column,
            },
        };
        self.cell_at(to)
    }
}
