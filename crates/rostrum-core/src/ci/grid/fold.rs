//! Folding workflows: the matrix as it is drawn.
//!
//! The grid's columns are every check across every watched repository, which
//! is far wider than a window. So each workflow ([`ColumnGroup`]) starts
//! folded into one summary column — how many of the row's checks in it
//! failed, are running or passed — and opens in place to show its checks.
//! [`Folds`] is which workflows are open; [`ColumnLayout`] is the columns
//! drawn under it, and moves a [`SlotRef`] selection over them.
//!
//! The grid itself is unchanged by folding: [`CiGrid`] still holds one cell
//! per check, and a [`SlotRef`] on a check converts to its [`CellRef`].

use std::collections::BTreeSet;

use crate::model::{PrNumber, RepoId};

use super::{CellMove, CellRef, CiGrid, ColumnGroup, GridRow};
use crate::ci::model::{CheckEntry, CheckKey, Rollup};

/// Which workflows are open, by name (`None` is apps' checks and statuses).
/// Everything not named is folded, so a workflow that first appears after a
/// refresh starts folded like the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folds {
    open: BTreeSet<Option<String>>,
}

impl Folds {
    pub fn is_open(&self, workflow: &Option<String>) -> bool {
        self.open.contains(workflow)
    }

    /// Open a folded workflow, fold an open one.
    pub fn toggle(&mut self, workflow: &Option<String>) {
        if !self.open.remove(workflow) {
            self.open.insert(workflow.clone());
        }
    }

    pub fn open(&mut self, workflow: &Option<String>) {
        self.open.insert(workflow.clone());
    }

    /// Open every workflow `grid` has.
    pub fn open_all(&mut self, grid: &CiGrid) {
        self.open
            .extend(grid.column_groups().iter().map(|g| g.workflow.clone()));
    }

    pub fn fold_all(&mut self) {
        self.open.clear();
    }

    pub fn any_open(&self) -> bool {
        !self.open.is_empty()
    }
}

/// One drawn column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShownColumn {
    /// A folded workflow: index into [`CiGrid::column_groups`].
    Workflow { group: usize },
    /// One check of an open workflow: index into [`CiGrid::columns`].
    Check { column: usize },
}

/// A drawn column, by identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnRef {
    /// A folded workflow's summary.
    Workflow(Option<String>),
    Check(CheckKey),
}

/// A drawn cell, by identity: a check, or a folded workflow's summary.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SlotRef {
    pub repo: RepoId,
    pub number: PrNumber,
    pub column: ColumnRef,
}

impl SlotRef {
    /// The check cell this is, when it is one.
    pub fn cell(&self) -> Option<CellRef> {
        match &self.column {
            ColumnRef::Check(key) => Some(CellRef {
                repo: self.repo.clone(),
                number: self.number,
                column: key.clone(),
            }),
            ColumnRef::Workflow(_) => None,
        }
    }
}

/// A folded workflow's cell: the row's checks in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupSummary {
    pub rollup: Rollup,
    /// The workflow's checks this row did not run.
    pub not_run: usize,
}

impl GridRow {
    /// This row's checks in `group`, summed.
    pub fn summary(&self, group: &ColumnGroup) -> GroupSummary {
        let cells = &self.cells[group.columns.clone()];
        GroupSummary {
            rollup: Rollup::of(cells.iter().flatten().map(|entry| entry.status)),
            not_run: cells.iter().filter(|cell| cell.is_none()).count(),
        }
    }

    /// This row's checks in `group` that are failing, then those running.
    pub fn attention<'a>(&'a self, group: &ColumnGroup) -> impl Iterator<Item = &'a CheckEntry> {
        let cells = &self.cells[group.columns.clone()];
        let failing = cells.iter().flatten().filter(|e| e.status.is_failing());
        let running = cells.iter().flatten().filter(|e| e.status.is_running());
        failing.chain(running)
    }
}

/// The columns drawn for a grid under some [`Folds`], left to right.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLayout {
    shown: Vec<ShownColumn>,
}

impl ColumnLayout {
    pub fn new(grid: &CiGrid, folds: &Folds) -> Self {
        let shown = grid
            .column_groups()
            .iter()
            .enumerate()
            .flat_map(|(ix, group)| -> Vec<ShownColumn> {
                if folds.is_open(&group.workflow) {
                    group
                        .columns
                        .clone()
                        .map(|column| ShownColumn::Check { column })
                        .collect()
                } else {
                    vec![ShownColumn::Workflow { group: ix }]
                }
            })
            .collect();
        Self { shown }
    }

    pub fn shown(&self) -> &[ShownColumn] {
        &self.shown
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// The identity of drawn column `ix`.
    pub fn column_ref(&self, grid: &CiGrid, ix: usize) -> Option<ColumnRef> {
        Some(match *self.shown.get(ix)? {
            ShownColumn::Workflow { group } => {
                ColumnRef::Workflow(grid.column_groups().get(group)?.workflow.clone())
            }
            ShownColumn::Check { column } => ColumnRef::Check(grid.columns().get(column)?.clone()),
        })
    }

    /// Where `column` is drawn. A check whose workflow is folded is drawn as
    /// that workflow's summary; an open workflow's summary is not drawn.
    pub fn index_of(&self, grid: &CiGrid, column: &ColumnRef) -> Option<usize> {
        let group = |workflow: &Option<String>| {
            grid.column_groups()
                .iter()
                .position(|g| &g.workflow == workflow)
        };
        match column {
            ColumnRef::Workflow(workflow) => {
                let group = group(workflow)?;
                self.shown
                    .iter()
                    .position(|c| *c == ShownColumn::Workflow { group })
            }
            ColumnRef::Check(key) => {
                let column = grid.columns().iter().position(|c| c == key)?;
                self.shown.iter().position(|c| match *c {
                    ShownColumn::Check { column: at } => at == column,
                    ShownColumn::Workflow { group } => {
                        grid.column_groups()[group].columns.contains(&column)
                    }
                })
            }
        }
    }

    /// `slot` as drawn now: a check whose workflow folded becomes that
    /// workflow's summary, and a summary whose workflow opened becomes the
    /// row's first failing check in it, else its first running one, else
    /// the workflow's first check. `None` when its row or column is gone.
    pub fn resolve(&self, grid: &CiGrid, slot: &SlotRef) -> Option<SlotRef> {
        let row = grid
            .rows()
            .iter()
            .find(|r| r.repo == slot.repo && r.number == slot.number)?;
        let column = match &slot.column {
            ColumnRef::Workflow(workflow) if self.index_of(grid, &slot.column).is_none() => {
                let group = grid
                    .column_groups()
                    .iter()
                    .find(|g| &g.workflow == workflow)?;
                let key = match row.attention(group).next() {
                    Some(entry) => entry.key.clone(),
                    None => grid.columns()[group.columns.start].clone(),
                };
                ColumnRef::Check(key)
            }
            other => self.column_ref(grid, self.index_of(grid, other)?)?,
        };
        Some(SlotRef {
            repo: slot.repo.clone(),
            number: slot.number,
            column,
        })
    }

    /// Where `step` lands from `from`, over the drawn columns.
    ///
    /// Left and right stay in the row; up and down move between rows,
    /// across repositories, in the same drawn column. Nothing wraps. With no
    /// selection, or one no longer in the grid, any move enters at the first
    /// drawn cell.
    pub fn step(&self, grid: &CiGrid, from: Option<&SlotRef>, step: CellMove) -> Option<SlotRef> {
        let at = from.and_then(|slot| {
            let row = grid
                .rows()
                .iter()
                .position(|r| r.repo == slot.repo && r.number == slot.number)?;
            Some((row, self.index_of(grid, &slot.column)?))
        });
        let (row, column) = match at {
            None => (0, 0),
            Some((row, column)) => {
                let last_row = grid.rows().len().saturating_sub(1);
                let last_column = self.shown.len().saturating_sub(1);
                match step {
                    CellMove::Left => (row, column.saturating_sub(1)),
                    CellMove::Right => (row, (column + 1).min(last_column)),
                    CellMove::Up => (row.saturating_sub(1), column),
                    CellMove::Down => ((row + 1).min(last_row), column),
                }
            }
        };
        let target = grid.rows().get(row)?;
        Some(SlotRef {
            repo: target.repo.clone(),
            number: target.number,
            column: self.column_ref(grid, column)?,
        })
    }
}
