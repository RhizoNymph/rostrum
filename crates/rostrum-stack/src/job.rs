//! What a stack job is asked to do, and every way it can end.

use std::path::PathBuf;

use futures::channel::mpsc::UnboundedSender;
use rostrum_config::ConflictHandler;
use rostrum_core::{PrNumber, StackPlan};
use rostrum_git::{BranchName, PushRejection};

/// Make the pull requests of `plan` into a stack, from the clone at `clone`.
///
/// When the plan's bases already chain ("Make stack" on a detected chain)
/// nothing is rewritten: the stack is linked on GitHub and tracked in the
/// clone. Otherwise ("Arrange") each branch that needs it is rebased onto the
/// one below in a scratch worktree, every rewritten branch is pushed with a
/// lease, and then the stack is linked and tracked.
#[derive(Clone, Debug)]
pub struct StackJob {
    /// Any worktree of the clone; scratch worktrees are added beside it.
    pub clone: PathBuf,
    pub plan: StackPlan,
    /// Present ⇒ a conflict is left in its scratch worktree and handed off.
    /// Absent ⇒ it is aborted and the worktree removed.
    pub handler: Option<ConflictHandler>,
    /// Where scratch worktrees are created. Created if missing.
    pub scratch_dir: PathBuf,
}

/// Where a job has got to, for a progress line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackProgress {
    Fetching,
    Rebasing {
        number: PrNumber,
        step: usize,
        of: usize,
    },
    Pushing {
        number: PrNumber,
    },
    Linking,
    Tracking,
    Merging,
    Unstacking,
}

impl StackProgress {
    pub fn describe(&self) -> String {
        match self {
            Self::Fetching => "Fetching branches…".into(),
            Self::Rebasing { number, step, of } => format!("Rebasing {number} ({step}/{of})…"),
            Self::Pushing { number } => format!("Pushing {number} (with lease)…"),
            Self::Linking => "Linking the stack on GitHub…".into(),
            Self::Tracking => "Tracking the stack in the clone…".into(),
            Self::Merging => "Merging the stack on GitHub…".into(),
            Self::Unstacking => "Unstacking on GitHub…".into(),
        }
    }
}

/// Where progress goes. A closed receiver is not an error: the job finishes
/// whether or not anyone is still watching.
#[derive(Clone, Debug, Default)]
pub struct Progress(Option<UnboundedSender<StackProgress>>);

impl Progress {
    pub fn new(sender: UnboundedSender<StackProgress>) -> Self {
        Self(Some(sender))
    }

    pub fn none() -> Self {
        Self(None)
    }

    pub(crate) fn send(&self, progress: StackProgress) {
        tracing::debug!(?progress, "stack progress");
        if let Some(sender) = &self.0 {
            let _ = sender.unbounded_send(progress);
        }
    }
}

/// How a stack job ended. Every variant is a state the repository and
/// GitHub are actually in, so each is rendered differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackOutcome {
    /// The stack exists on GitHub.
    Stacked(StackReport),
    /// A rebase stopped on a conflict and was aborted. Nothing was pushed and
    /// no branch moved.
    Conflicted { number: PrNumber, message: String },
    /// A rebase stopped on a conflict and was left in `worktree` for the
    /// handler's tmux session. Nothing was pushed. Once it is finished there,
    /// running the job again picks the resolution up through `rerere`.
    HandedOff {
        number: PrNumber,
        session: String,
        worktree: PathBuf,
    },
    /// The remote refused a lease. The pull requests in `pushed` were already
    /// updated; nothing after `number` was tried, and nothing was linked.
    PushRejected {
        pushed: Vec<PrNumber>,
        number: PrNumber,
        reason: PushRejection,
    },
    /// Every rewritten branch was pushed, but `gh stack link` failed, so the
    /// pull requests are rebased onto each other without being a stack yet.
    /// Running the job again links them without rewriting anything.
    LinkFailed {
        pushed: Vec<PrNumber>,
        message: String,
    },
}

impl StackOutcome {
    /// One line for a status bar.
    pub fn summary(&self) -> String {
        match self {
            Self::Stacked(report) => {
                let rewritten = match report.rewritten.len() {
                    0 => String::new(),
                    n => format!("; {n} branch(es) rebased and pushed"),
                };
                let local = match &report.local {
                    LocalTracking::Tracked => "; tracked in the clone".to_string(),
                    LocalTracking::Skipped(why) => format!("; not tracked locally: {why}"),
                };
                format!("Stack created{rewritten}{local}")
            }
            Self::Conflicted { number, message } => {
                format!(
                    "Rebasing {number} stopped on a conflict and was aborted; nothing was pushed. {message}"
                )
            }
            Self::HandedOff {
                number, session, ..
            } => format!(
                "Rebasing {number} stopped on a conflict; handed off to tmux session `{session}` (tmux attach -t ={session}). Nothing was pushed; run Arrange again once it is resolved."
            ),
            Self::PushRejected {
                pushed,
                number,
                reason,
            } => format!(
                "Pushing {number} was refused: {}. Already pushed: {}.",
                reason.describe(),
                list(pushed)
            ),
            Self::LinkFailed { pushed, message } => format!(
                "Branches pushed ({}), but linking the stack failed: {message}",
                list(pushed)
            ),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Stacked(_))
    }
}

fn list(numbers: &[PrNumber]) -> String {
    if numbers.is_empty() {
        return "none".into();
    }
    numbers
        .iter()
        .map(PrNumber::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a successful job did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackReport {
    /// Pull requests whose branches were rebased and force-pushed (leased).
    pub rewritten: Vec<PrNumber>,
    pub local: LocalTracking,
    /// What happened to local branches along the way.
    pub notes: Vec<LocalNote>,
}

/// Whether `gh stack` now tracks the stack in the clone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalTracking {
    Tracked,
    /// Not tracked, and why. The stack still exists on GitHub.
    Skipped(String),
}

/// A local branch rostrum touched, or chose not to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalNote {
    /// Created from the remote so `gh stack init` could adopt it.
    Created(BranchName),
    /// Moved from the pre-rewrite commit to the rewritten one.
    Moved(BranchName),
    /// Left where it was, and now differs from the rewritten remote branch.
    LeftBehind { branch: BranchName, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_say_what_was_and_was_not_pushed() {
        let n = PrNumber(4);
        assert!(
            StackOutcome::Conflicted {
                number: n,
                message: "CONFLICT".into()
            }
            .summary()
            .contains("nothing was pushed")
        );
        let handed = StackOutcome::HandedOff {
            number: n,
            session: "rostrum-o-r-4".into(),
            worktree: "/tmp/x".into(),
        }
        .summary();
        assert!(handed.contains("tmux attach -t =rostrum-o-r-4"));
        assert!(handed.contains("Nothing was pushed"));
        let rejected = StackOutcome::PushRejected {
            pushed: vec![PrNumber(1)],
            number: PrNumber(2),
            reason: PushRejection::StaleLease,
        }
        .summary();
        assert!(rejected.contains("#2"));
        assert!(rejected.contains("#1"));
        assert!(rejected.contains("nothing was overwritten"));
    }

    #[test]
    fn a_stacked_summary_mentions_local_tracking() {
        let report = StackReport {
            rewritten: vec![PrNumber(2)],
            local: LocalTracking::Skipped("the clone has uncommitted changes".into()),
            notes: vec![],
        };
        let text = StackOutcome::Stacked(report).summary();
        assert!(text.contains("1 branch(es) rebased"));
        assert!(text.contains("not tracked locally: the clone has uncommitted changes"));
        assert!(
            StackOutcome::Stacked(StackReport {
                rewritten: vec![],
                local: LocalTracking::Tracked,
                notes: vec![],
            })
            .is_success()
        );
    }

    #[test]
    fn progress_without_a_receiver_is_silent() {
        Progress::none().send(StackProgress::Fetching);
        let (tx, rx) = futures::channel::mpsc::unbounded();
        drop(rx);
        Progress::new(tx).send(StackProgress::Linking);
    }
}
