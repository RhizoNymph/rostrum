//! The CI view: one PRs × checks matrix over every watched repository,
//! across the whole window, with one check's log or output in a right-hand
//! pane.
//!
//! Everything this view decides is decided in `rostrum_core::ci`: the grid
//! ([`build_grid`]), cell movement ([`CiGrid::step`]), timing labels, which
//! re-runs a check offers ([`rerun_targets`]) and the optimistic flip to
//! queued, and which columns are drawn with workflows folded
//! ([`ColumnLayout`]). This module holds the entity, its keys, the horizontal scroll and
//! the re-run request.

mod hscroll;
mod log_view;
mod matrix;
mod render;

use std::{rc::Rc, time::Duration};

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ListAlignment, ListState, Subscription, Task, Window, actions, px,
};
use gpui_tokio::Tokio;
use rostrum_core::ci::{
    CellMove, CellRef, CiGrid, ColumnLayout, ColumnRef, Folds, GridFilter, RerunTarget, SlotRef,
    build_grid, rerun_targets,
};
use rostrum_github::ci::RerunError;

use crate::sync::Store;

use hscroll::HScroll;
use log_view::{LogView, LogViewEvent};

actions!(
    ci,
    [
        CellLeft,
        CellRight,
        CellUp,
        CellDown,
        OpenLog,
        Retry,
        ToggleAttention,
        ToggleWorkflow,
        ToggleAllWorkflows,
        Dismiss,
        LeaveCi,
    ]
);

/// Key context of the grid.
pub const CI_CONTEXT: &str = "CiGrid";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("h", CellLeft, Some(CI_CONTEXT)),
        KeyBinding::new("left", CellLeft, Some(CI_CONTEXT)),
        KeyBinding::new("l", CellRight, Some(CI_CONTEXT)),
        KeyBinding::new("right", CellRight, Some(CI_CONTEXT)),
        KeyBinding::new("k", CellUp, Some(CI_CONTEXT)),
        KeyBinding::new("up", CellUp, Some(CI_CONTEXT)),
        KeyBinding::new("j", CellDown, Some(CI_CONTEXT)),
        KeyBinding::new("down", CellDown, Some(CI_CONTEXT)),
        KeyBinding::new("enter", OpenLog, Some(CI_CONTEXT)),
        KeyBinding::new("r", Retry, Some(CI_CONTEXT)),
        KeyBinding::new("f", ToggleAttention, Some(CI_CONTEXT)),
        KeyBinding::new("space", ToggleWorkflow, Some(CI_CONTEXT)),
        KeyBinding::new("e", ToggleAllWorkflows, Some(CI_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CI_CONTEXT)),
        KeyBinding::new("shift-c", LeaveCi, Some(CI_CONTEXT)),
    ]);
}

/// Raised so the workspace can switch back to the feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiEvent {
    Leave,
}

/// A re-run waiting for the user to confirm it.
#[derive(Clone, Debug)]
struct PendingRerun {
    cell: CellRef,
    targets: Vec<RerunTarget>,
}

/// Width of the row-header column and of each check column, in pixels.
pub(crate) const ROW_HEADER_WIDTH: f32 = 360.;
pub(crate) const CELL_WIDTH: f32 = 132.;
/// Width of the log pane, and of the rule before it.
pub(crate) const LOG_PANE_WIDTH: f32 = 641.;

pub struct CiView {
    store: Entity<Store>,
    grid: Rc<CiGrid>,
    list: ListState,
    filter: GridFilter,
    /// Which workflows are open. Every workflow starts folded into one
    /// summary column, so the grid opens without scrolling sideways; kept
    /// for the session, not saved.
    folds: Folds,
    /// The columns drawn under `folds`.
    layout: ColumnLayout,
    /// How far the check columns (and their header) are scrolled left.
    hscroll: HScroll,
    /// Width the check columns were last drawn in, for revealing a selected
    /// column from a key press.
    columns_viewport: f32,
    selected: Option<SlotRef>,
    confirm: Option<PendingRerun>,
    /// The last re-run's outcome or refusal.
    notice: Option<(bool, String)>,
    log: Option<(Entity<LogView>, Subscription)>,
    focus: FocusHandle,
    /// Redraws once a second, so running times tick. Present only while the
    /// view is visible.
    ticker: Option<Task<()>>,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CiEvent> for CiView {}

impl Focusable for CiView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CiView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let filter = GridFilter::default();
        let grid = Rc::new(Self::build(&store, filter, cx));
        let list = ListState::new(grid.lines().len(), ListAlignment::Top, px(300.));
        let folds = Folds::default();
        let layout = ColumnLayout::new(&grid, &folds);
        Self {
            _subscriptions: vec![cx.observe(&store, |this, _, cx| this.rebuild(cx))],
            store,
            grid,
            list,
            filter,
            folds,
            layout,
            hscroll: HScroll::default(),
            columns_viewport: 0.,
            selected: None,
            confirm: None,
            notice: None,
            log: None,
            focus: cx.focus_handle(),
            ticker: None,
            tasks: Vec::new(),
        }
    }

    fn build(store: &Entity<Store>, filter: GridFilter, cx: &App) -> CiGrid {
        let store = store.read(cx);
        build_grid(&store.state.repos, &store.state.filter, &store.ci, filter)
    }

    /// Rebuild the grid when the store changes, splicing the list only when
    /// its lines changed so scroll position and measured heights survive a
    /// poll that changed only cell contents.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let grid = Self::build(&self.store, self.filter, cx);
        if grid.lines() != self.grid.lines() {
            self.list
                .splice(0..self.grid.lines().len(), grid.lines().len());
        }
        self.layout = ColumnLayout::new(&grid, &self.folds);
        self.grid = Rc::new(grid);
        cx.notify();
    }

    /// The view became visible or hidden: the store polls faster while it
    /// shows, and only then.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.set_ci_visible(visible, cx));
        self.ticker = visible.then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
    }

    /// Total width of the drawn columns.
    pub(crate) fn columns_width(&self) -> f32 {
        CELL_WIDTH * self.layout.len() as f32
    }

    pub(crate) fn select(&mut self, slot: SlotRef, cx: &mut Context<Self>) {
        let line = self
            .grid
            .rows()
            .iter()
            .position(|r| r.repo == slot.repo && r.number == slot.number)
            .and_then(|row| {
                self.grid
                    .lines()
                    .iter()
                    .position(|line| *line == rostrum_core::ci::GridLine::Row { row })
            });
        if let Some(line) = line {
            self.list.scroll_to_reveal_item(line);
        }
        if let Some(column) = self.layout.index_of(&self.grid, &slot.column) {
            self.hscroll.reveal(
                CELL_WIDTH * column as f32,
                CELL_WIDTH,
                self.columns_viewport,
            );
        }
        self.selected = Some(slot);
        self.confirm = None;
        cx.notify();
    }

    fn step(&mut self, step: CellMove, cx: &mut Context<Self>) {
        if let Some(slot) = self.layout.step(&self.grid, self.selected.as_ref(), step) {
            self.select(slot, cx);
        }
    }

    /// Re-lay the columns after the folds changed, keeping the selection on
    /// its row: on the summary its check folded into, or on the check that
    /// needs attention in the workflow it opened.
    fn refold(&mut self, cx: &mut Context<Self>) {
        self.layout = ColumnLayout::new(&self.grid, &self.folds);
        self.hscroll
            .clamp(self.columns_width(), self.columns_viewport);
        match self
            .selected
            .as_ref()
            .and_then(|slot| self.layout.resolve(&self.grid, slot))
        {
            Some(slot) => self.select(slot, cx),
            None => cx.notify(),
        }
    }

    /// Open or fold one workflow, by name.
    pub(crate) fn toggle_workflow(&mut self, workflow: &Option<String>, cx: &mut Context<Self>) {
        self.folds.toggle(workflow);
        self.refold(cx);
    }

    /// `space`: open the selected summary's workflow, or fold the selected
    /// check's.
    fn toggle_selected_workflow(&mut self, cx: &mut Context<Self>) {
        let workflow = match self.selected.as_ref().map(|slot| &slot.column) {
            Some(ColumnRef::Workflow(workflow)) => workflow.clone(),
            Some(ColumnRef::Check(key)) => key.workflow.clone(),
            None => return,
        };
        self.toggle_workflow(&workflow, cx);
    }

    /// `e`: fold everything when anything is open, else open everything.
    pub(crate) fn toggle_all_workflows(&mut self, cx: &mut Context<Self>) {
        if self.folds.any_open() {
            self.folds.fold_all();
        } else {
            self.folds.open_all(&self.grid);
        }
        self.refold(cx);
    }

    /// `enter` or a double click: a check's log, or a folded workflow
    /// opened.
    pub(crate) fn open_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected.as_ref().map(|slot| slot.column.clone()) {
            Some(ColumnRef::Workflow(workflow)) => {
                self.folds.open(&workflow);
                self.refold(cx);
            }
            Some(ColumnRef::Check(_)) => self.open_log(cx),
            None => {}
        }
    }

    pub(crate) fn open_log(&mut self, cx: &mut Context<Self>) {
        let Some(cell) = self.selected.as_ref().and_then(SlotRef::cell) else {
            return;
        };
        let Some(entry) = self.grid.entry(&cell).cloned() else {
            return;
        };
        let client = self.store.read(cx).client();
        let repo = cell.repo.clone();
        let view = cx.new(|cx| LogView::new(repo, entry, client, cx));
        let subscription = cx.subscribe(&view, |this, _, event, cx| match event {
            LogViewEvent::Close => this.close_log(cx),
        });
        tracing::debug!(repo = %cell.repo, number = cell.number.0, check = %cell.column, "opening check log");
        self.log = Some((view, subscription));
        cx.notify();
    }

    fn close_log(&mut self, cx: &mut Context<Self>) {
        self.log = None;
        cx.notify();
    }

    /// Ask to re-run the selected check: show the re-runs it offers, or why
    /// it offers none.
    fn ask_retry(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.selected.as_ref() else {
            return;
        };
        let Some(cell) = slot.cell() else {
            self.notice = Some((
                false,
                "Open the workflow (space) to re-run one of its checks".into(),
            ));
            cx.notify();
            return;
        };
        let store = self.store.read(cx);
        let (Some(entry), Some(checks)) =
            (self.grid.entry(&cell), store.ci.pr(&cell.repo, cell.number))
        else {
            self.notice = Some((false, "Nothing has run in this cell to re-run".into()));
            cx.notify();
            return;
        };
        match rerun_targets(checks, entry) {
            Ok(targets) => self.confirm = Some(PendingRerun { cell, targets }),
            Err(reason) => self.notice = Some((false, capitalised(&reason.to_string()))),
        }
        cx.notify();
    }

    /// The confirmed re-run: flip the cells to queued, send the request,
    /// and reconcile from GitHub either way.
    fn rerun(&mut self, target: RerunTarget, cx: &mut Context<Self>) {
        let Some(PendingRerun { cell, .. }) = self.confirm.take() else {
            return;
        };
        let Some(client) = self.store.read(cx).client() else {
            self.notice = Some((false, "Not authenticated".into()));
            cx.notify();
            return;
        };
        let (repo, number) = (cell.repo.clone(), cell.number);
        self.store
            .update(cx, |store, cx| store.requeue(&repo, number, target, cx));
        self.notice = Some((true, format!("{}…", target.label())));
        cx.notify();

        self.tasks.push(cx.spawn(async move |this, cx| {
            let request_repo = repo.clone();
            let result = Tokio::spawn(
                &*cx,
                async move { client.rerun(&request_repo, target).await },
            )
            .await;
            let ok = matches!(result, Ok(Ok(())));
            this.update(cx, |this, cx| {
                this.notice = Some(match result {
                    Ok(Ok(())) => (true, format!("{} requested", target.label())),
                    Ok(Err(error)) => (false, rerun_message(&error)),
                    Err(error) => (false, error.to_string()),
                });
                // A failure reconciles at once, putting the old result back.
                if !ok {
                    let repo = repo.clone();
                    this.store
                        .update(cx, |store, cx| store.refresh_ci(repo, cx));
                }
                cx.notify();
            })
            .ok();
            if ok {
                // GitHub takes a moment to create the new attempt; asking at
                // once would fetch the old result and undo the flip.
                cx.background_executor().timer(Duration::from_secs(4)).await;
                this.update(cx, |this, cx| {
                    this.store
                        .update(cx, |store, cx| store.refresh_ci(repo, cx));
                })
                .ok();
            }
        }));
    }

    fn toggle_attention(&mut self, cx: &mut Context<Self>) {
        self.filter.needs_attention = !self.filter.needs_attention;
        self.rebuild(cx);
    }

    /// `escape`: a confirmation, then the log pane, then the view itself.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.confirm.take().is_some() {
            cx.notify();
        } else if self.log.is_some() {
            self.close_log(cx);
        } else {
            cx.emit(CiEvent::Leave);
        }
    }

    fn on_left(&mut self, _: &CellLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.step(CellMove::Left, cx);
    }
    fn on_right(&mut self, _: &CellRight, _: &mut Window, cx: &mut Context<Self>) {
        self.step(CellMove::Right, cx);
    }
    fn on_up(&mut self, _: &CellUp, _: &mut Window, cx: &mut Context<Self>) {
        self.step(CellMove::Up, cx);
    }
    fn on_down(&mut self, _: &CellDown, _: &mut Window, cx: &mut Context<Self>) {
        self.step(CellMove::Down, cx);
    }
    fn on_open(&mut self, _: &OpenLog, _: &mut Window, cx: &mut Context<Self>) {
        // With a confirmation up, enter confirms its first choice.
        match self
            .confirm
            .as_ref()
            .and_then(|c| c.targets.first().copied())
        {
            Some(target) => self.rerun(target, cx),
            None => self.open_selected(cx),
        }
    }
    fn on_retry(&mut self, _: &Retry, _: &mut Window, cx: &mut Context<Self>) {
        self.ask_retry(cx);
    }
    fn on_toggle_attention(&mut self, _: &ToggleAttention, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_attention(cx);
    }
    fn on_toggle_workflow(&mut self, _: &ToggleWorkflow, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_selected_workflow(cx);
    }
    fn on_toggle_all_workflows(
        &mut self,
        _: &ToggleAllWorkflows,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_all_workflows(cx);
    }
    fn on_dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(cx);
    }
    fn on_leave(&mut self, _: &LeaveCi, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(CiEvent::Leave);
    }
}

/// What the banner says about a refused re-run.
fn rerun_message(error: &RerunError) -> String {
    match error {
        RerunError::NoPermission { message } => {
            format!("No permission to re-run this ({message})")
        }
        RerunError::NotRerunnable { message } => format!("GitHub will not re-run this: {message}"),
        RerunError::NotFound => "That run no longer exists".into(),
        RerunError::Api(error) => error.to_string(),
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::ci::NotRerunnable;

    use super::*;

    #[test]
    fn rerun_refusals_read_as_sentences() {
        assert_eq!(
            rerun_message(&RerunError::NoPermission {
                message: "Must have admin rights".into()
            }),
            "No permission to re-run this (Must have admin rights)"
        );
        assert_eq!(
            rerun_message(&RerunError::NotRerunnable {
                message: "created over a month ago".into()
            }),
            "GitHub will not re-run this: created over a month ago"
        );
        assert_eq!(
            rerun_message(&RerunError::NotFound),
            "That run no longer exists"
        );
        assert_eq!(
            capitalised(&NotRerunnable::StillRunning.to_string()),
            "The workflow run is still in progress"
        );
    }
}
