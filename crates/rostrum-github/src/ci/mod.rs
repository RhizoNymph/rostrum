//! CI checks over the API: the grid's per-repository document, job logs, a
//! check run's own output, and re-runs. The rules — statuses, the grid,
//! timing, log parsing, re-run eligibility — are pure and live in
//! `rostrum_core::ci`.

mod client;
pub mod rest;
pub mod wire;

#[cfg(test)]
mod tests;

pub use client::RepoCiChecks;
pub use rest::{RerunError, classify_rerun, job_log_path, rerun_call};
pub use wire::{CI_CHECKS, CONTEXTS_PER_COMMIT};
