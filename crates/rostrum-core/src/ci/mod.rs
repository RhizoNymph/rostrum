//! CI checks: the PRs × checks grid, check timing, job logs, and re-runs.
//!
//! Everything here is pure — no network, no clock, no UI — so the desktop and
//! the Android core build the same grid, label the same times, read logs the
//! same way and offer the same re-runs. See `docs/features/ci_grid.md`.

pub mod grid;
pub mod log;
pub mod model;
pub mod rerun;
pub mod time;

pub use grid::{
    CellMove, CellRef, CiChecks, CiGrid, GridFilter, GridLine, GridRow, GridSection, RepoChecks,
    build_grid,
};
pub use log::{
    DEFAULT_LOG_LINES, LineKind, LineLimit, LogGroup, LogLine, LogStep, ParsedLog, parse_log,
    strip_ansi,
};
pub use model::{
    Annotation, AnnotationLevel, CheckEntry, CheckKey, CheckOutput, CheckSource, CheckStatus,
    PrChecks, Rollup, RollupState,
};
pub use rerun::{NotRerunnable, RerunTarget, mark_requeued, rerun_targets};
pub use time::{Timing, format_ago, format_duration};
