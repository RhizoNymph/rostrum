//! What a write actually did, decided from the repository rather than from git.
//!
//! The central idea of this module is that [`classify_run`] does **not** look at
//! the exit code to decide what happened. It reads the repository's state after
//! the command and derives the answer from that. Exit codes are ambiguous — a
//! rebase whose autostash pop conflicts exits 0 while leaving unmerged paths;
//! a merge that conflicts exits 1 having done exactly what it was asked — and
//! the alternative to reading state is matching English prose, which changes
//! between git versions and would have to be defended against `LC_ALL`.

use crate::{
    error::GitError,
    preflight::{Blocker, Operation},
    refs::Oid,
    status::InProgress,
};

/// Which `--abort` clears a stopped operation.
///
/// Only ever obtained from [`Conflict::abort_target`], so `merge --abort`
/// during a rebase is not something a caller can express.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortTarget {
    Rebase,
    Merge,
}

impl AbortTarget {
    /// The subcommand to run `--abort` against.
    pub fn subcommand(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::Merge => "merge",
        }
    }
}

impl InProgress {
    /// The abort that clears this state, if this crate knows how.
    ///
    /// `Am`, `CherryPick`, `Revert` and `Bisect` return `None` on purpose: none
    /// of them can be created by the commands this crate issues, so finding one
    /// means it was already there and belongs to the user. `git rebase --abort`
    /// during a `git am` does not work, and guessing would destroy work rostrum
    /// did not start.
    pub fn abort_target(self) -> Option<AbortTarget> {
        match self {
            Self::Rebase | Self::RebaseApply => Some(AbortTarget::Rebase),
            Self::Merge => Some(AbortTarget::Merge),
            Self::Am | Self::CherryPick | Self::Revert | Self::Bisect => None,
        }
    }
}

/// A stopped operation, with git's own explanation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// A rebase stopped on a commit it could not replay.
    Rebase {
        /// The ref that was rebased onto, as passed to git.
        onto: String,
        conflicted: u32,
        message: String,
        /// Whether the automatic abort ran and succeeded — see
        /// [`crate::repo`]'s conflict policy. `false` means the sequencer state
        /// is still on disk and the caller must deal with it.
        aborted: bool,
    },
    /// A merge stopped with unmerged paths.
    Merge {
        /// The ref that was merged in, as passed to git.
        from: String,
        conflicted: u32,
        message: String,
        aborted: bool,
    },
    /// The operation itself succeeded, and then restoring the autostashed
    /// changes conflicted.
    ///
    /// Structurally distinct from the others: there is no sequencer state, so
    /// nothing to abort, and the rebase or merge is *done*. The user's changes
    /// are safe in the stash. Detecting this by state rather than by reading
    /// git's "Applying autostash resulted in conflicts" is the whole reason
    /// [`classify_run`] reads the repository instead of the output.
    AutostashPop { conflicted: u32, message: String },
}

impl Conflict {
    /// The abort that would clear this conflict, if there is anything left to
    /// clear.
    ///
    /// `None` for [`Conflict::AutostashPop`] — the operation finished and
    /// `--abort` would fail — and `None` once the automatic abort has already
    /// run, which under the default policy is the usual case.
    pub fn abort_target(&self) -> Option<AbortTarget> {
        match self {
            Self::Rebase { aborted: true, .. } | Self::Merge { aborted: true, .. } => None,
            Self::Rebase { .. } => Some(AbortTarget::Rebase),
            Self::Merge { .. } => Some(AbortTarget::Merge),
            Self::AutostashPop { .. } => None,
        }
    }

    /// git's own words, for showing to the user.
    pub fn message(&self) -> &str {
        match self {
            Self::Rebase { message, .. }
            | Self::Merge { message, .. }
            | Self::AutostashPop { message, .. } => message,
        }
    }

    pub fn conflicted(&self) -> u32 {
        match self {
            Self::Rebase { conflicted, .. }
            | Self::Merge { conflicted, .. }
            | Self::AutostashPop { conflicted, .. } => *conflicted,
        }
    }

    pub(crate) fn mark_aborted(&mut self, value: bool) {
        match self {
            Self::Rebase { aborted, .. } | Self::Merge { aborted, .. } => *aborted = value,
            Self::AutostashPop { .. } => {}
        }
    }
}

/// The result of a merge, rebase, or pull.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to do: HEAD did not move and git was content.
    AlreadyUpToDate,
    /// HEAD moved.
    Completed {
        from: Oid,
        to: Oid,
    },
    Conflicted(Conflict),
}

impl Outcome {
    pub fn is_conflicted(&self) -> bool {
        matches!(self, Self::Conflicted(_))
    }

    /// Whether the repository changed. `AlreadyUpToDate` is the only `Ok` that
    /// leaves it untouched.
    pub fn changed_anything(&self) -> bool {
        !matches!(self, Self::AlreadyUpToDate)
    }
}

/// Everything [`classify_run`] is allowed to consider.
///
/// A struct rather than eight arguments so the function stays pure and every
/// row of the decision table can be written as a literal in a test.
#[derive(Clone, Copy, Debug)]
pub struct RunReport<'a> {
    pub operation: Operation,
    /// The ref the command was pointed at, for the conflict's description.
    pub target: &'a str,
    /// Whether git exited zero. Used only to separate "nothing to do" from
    /// "declined" once the state has already ruled out a conflict.
    pub success: bool,
    pub code: Option<i32>,
    /// git's combined output, kept verbatim for the user.
    pub message: &'a str,
    pub stderr: &'a str,
    pub before: Option<&'a Oid>,
    pub after: Option<&'a Oid>,
    /// Read *after* the command ran.
    pub in_progress: Option<InProgress>,
    /// Unmerged path count read after the command ran.
    pub conflicted: u32,
}

/// Decide what a write did, from the repository state it left behind.
///
/// The table, in order:
///
/// | `in_progress`          | `conflicted` | HEAD moved | result                     |
/// |------------------------|--------------|------------|----------------------------|
/// | `Rebase`/`RebaseApply` | –            | –          | [`Conflict::Rebase`]       |
/// | `Merge`                | –            | –          | [`Conflict::Merge`]        |
/// | other                  | –            | –          | [`GitError::Refused`]      |
/// | `None`                 | > 0          | –          | [`Conflict::AutostashPop`] |
/// | `None`                 | 0            | no         | exit 0: up to date; else refusal |
/// | `None`                 | 0            | yes        | [`Outcome::Completed`]     |
///
/// Sequencer state outranks the conflict count rather than being paired with
/// it: a rebase can also stop with zero unmerged paths (a failed `--exec`, an
/// unexpected `--empty=stop`), and the caller's situation is identical either
/// way — the operation did not finish and there is state to abort.
///
/// The `other` row catches `Am`, `CherryPick`, `Revert` and `Bisect`. None can
/// be produced by the commands here, so finding one means the caller started on
/// top of someone else's operation; [`crate::Repo::preflight`] reports that as
/// a [`Blocker`], and reaching it anyway is a refusal.
pub fn classify_run(report: &RunReport<'_>) -> Result<Outcome, GitError> {
    if let Some(state) = report.in_progress {
        let conflict = match state.abort_target() {
            Some(AbortTarget::Rebase) => Conflict::Rebase {
                onto: report.target.to_string(),
                conflicted: report.conflicted,
                message: report.message.to_string(),
                aborted: false,
            },
            Some(AbortTarget::Merge) => Conflict::Merge {
                from: report.target.to_string(),
                conflicted: report.conflicted,
                message: report.message.to_string(),
                aborted: false,
            },
            None => {
                return Err(GitError::Refused {
                    operation: report.operation,
                    blocker: Blocker::InProgress(state),
                });
            }
        };
        return Ok(Outcome::Conflicted(conflict));
    }

    if report.conflicted > 0 {
        return Ok(Outcome::Conflicted(Conflict::AutostashPop {
            conflicted: report.conflicted,
            message: report.message.to_string(),
        }));
    }

    if report.before == report.after {
        return if report.success {
            Ok(Outcome::AlreadyUpToDate)
        } else {
            Err(GitError::Failed {
                command: report.operation.as_str().to_string(),
                code: report.code,
                stderr: refusal_text(report),
            })
        };
    }

    // Unreachable in practice: every caller pre-flights, and an unborn HEAD is
    // a `Blocker::UnbornBranch` there. Reported rather than unwrapped because
    // an unborn HEAD on either side has no oid to put in the outcome.
    let (Some(from), Some(to)) = (report.before, report.after) else {
        return Err(GitError::Parse {
            what: "a completed operation with an unborn HEAD on one side",
            line: report.message.to_string(),
        });
    };

    Ok(Outcome::Completed {
        from: from.clone(),
        to: to.clone(),
    })
}

/// git puts refusals on stderr, but a few (`merge` in particular) explain
/// themselves on stdout, so fall back rather than report an empty reason.
fn refusal_text(report: &RunReport<'_>) -> String {
    let stderr = report.stderr.trim();
    if stderr.is_empty() {
        report.message.trim().to_string()
    } else {
        stderr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: char) -> Oid {
        Oid::parse(std::iter::repeat_n(byte, 40).collect::<String>()).expect("valid")
    }

    fn report<'a>(
        in_progress: Option<InProgress>,
        conflicted: u32,
        before: Option<&'a Oid>,
        after: Option<&'a Oid>,
        success: bool,
    ) -> RunReport<'a> {
        RunReport {
            operation: Operation::Rebase,
            target: "refs/remotes/origin/main",
            success,
            code: if success { Some(0) } else { Some(1) },
            message: "git said something",
            stderr: "",
            before,
            after,
            in_progress,
            conflicted,
        }
    }

    #[test]
    fn a_stopped_rebase_is_a_conflict_not_an_error() {
        let a = oid('a');
        for state in [InProgress::Rebase, InProgress::RebaseApply] {
            let outcome = classify_run(&report(Some(state), 2, Some(&a), Some(&a), false))
                .expect("a conflict is a normal outcome");
            let Outcome::Conflicted(Conflict::Rebase {
                onto,
                conflicted,
                aborted,
                ..
            }) = outcome
            else {
                panic!("expected a rebase conflict for {state:?}, got {outcome:?}");
            };
            assert_eq!(onto, "refs/remotes/origin/main");
            assert_eq!(conflicted, 2);
            assert!(!aborted, "classification does not abort; the policy does");
        }
    }

    #[test]
    fn a_stopped_merge_is_a_merge_conflict() {
        let a = oid('a');
        let outcome = classify_run(&RunReport {
            operation: Operation::Merge,
            ..report(Some(InProgress::Merge), 1, Some(&a), Some(&a), false)
        })
        .expect("a conflict is a normal outcome");
        assert!(matches!(
            outcome,
            Outcome::Conflicted(Conflict::Merge { .. })
        ));
    }

    /// A rebase that stops with nothing unmerged still leaves state to abort,
    /// and the caller's situation is the same either way.
    #[test]
    fn a_sequencer_left_running_outranks_the_conflict_count() {
        let a = oid('a');
        let outcome = classify_run(&report(
            Some(InProgress::Rebase),
            0,
            Some(&a),
            Some(&a),
            true,
        ))
        .expect("classifies");
        assert!(matches!(
            outcome,
            Outcome::Conflicted(Conflict::Rebase { conflicted: 0, .. })
        ));
    }

    /// The case the whole design exists for. A conflicted autostash pop leaves
    /// unmerged paths and *no* sequencer state — and git exits 0 after a rebase
    /// and 1 after a merge, so the exit code cannot be what decides.
    #[test]
    fn an_autostash_pop_conflict_is_found_from_state_at_either_exit_code() {
        let before = oid('a');
        let after = oid('b');
        for success in [true, false] {
            let outcome = classify_run(&report(None, 3, Some(&before), Some(&after), success))
                .expect("classifies");
            let Outcome::Conflicted(Conflict::AutostashPop { conflicted, .. }) = outcome else {
                panic!("expected an autostash-pop conflict at success={success}, got {outcome:?}");
            };
            assert_eq!(conflicted, 3);
        }
    }

    /// There is no sequencer state to abort, so offering one would hand the
    /// caller a command that fails.
    #[test]
    fn an_autostash_pop_conflict_has_nothing_to_abort() {
        let conflict = Conflict::AutostashPop {
            conflicted: 1,
            message: String::new(),
        };
        assert_eq!(conflict.abort_target(), None);
    }

    /// An abort that already ran leaves nothing to abort again; one that failed
    /// leaves the state on disk, and the caller needs to be told which.
    #[test]
    fn a_conflicts_abort_target_matches_its_kind_until_it_is_aborted() {
        let mut rebase = Conflict::Rebase {
            onto: "refs/remotes/origin/main".to_string(),
            conflicted: 1,
            message: String::new(),
            aborted: false,
        };
        assert_eq!(rebase.abort_target(), Some(AbortTarget::Rebase));
        rebase.mark_aborted(true);
        assert_eq!(rebase.abort_target(), None);

        let merge = Conflict::Merge {
            from: "refs/remotes/origin/main".to_string(),
            conflicted: 1,
            message: String::new(),
            aborted: false,
        };
        assert_eq!(merge.abort_target(), Some(AbortTarget::Merge));
        assert_eq!(AbortTarget::Rebase.subcommand(), "rebase");
        assert_eq!(AbortTarget::Merge.subcommand(), "merge");
    }

    /// Someone else's `git am` or cherry-pick is not rostrum's to abort.
    #[test]
    fn a_foreign_operation_is_refused_rather_than_aborted() {
        let a = oid('a');
        for state in [
            InProgress::Am,
            InProgress::CherryPick,
            InProgress::Revert,
            InProgress::Bisect,
        ] {
            let err = classify_run(&report(Some(state), 1, Some(&a), Some(&a), false));
            let Err(GitError::Refused { blocker, .. }) = err else {
                panic!("expected a refusal for {state:?}, got {err:?}");
            };
            assert_eq!(blocker, Blocker::InProgress(state));
            assert_eq!(state.abort_target(), None);
        }
    }

    #[test]
    fn an_unmoved_head_after_a_clean_run_is_already_up_to_date() {
        let a = oid('a');
        let outcome = classify_run(&report(None, 0, Some(&a), Some(&a), true)).expect("classifies");
        assert_eq!(outcome, Outcome::AlreadyUpToDate);
        assert!(!outcome.changed_anything());
    }

    /// Nothing happened and git was unhappy: a refusal, and the repository is
    /// as it was, so it is an error.
    #[test]
    fn an_unmoved_head_after_a_failed_run_is_an_error_carrying_gits_words() {
        let a = oid('a');
        let err = classify_run(&RunReport {
            stderr: "fatal: refusing to merge unrelated histories",
            ..report(None, 0, Some(&a), Some(&a), false)
        });
        let Err(GitError::Failed { stderr, code, .. }) = err else {
            panic!("expected a failure, got {err:?}");
        };
        assert_eq!(stderr, "fatal: refusing to merge unrelated histories");
        assert_eq!(code, Some(1));
    }

    /// Some refusals explain themselves on stdout, and an empty reason is
    /// useless to a user.
    #[test]
    fn a_refusal_with_an_empty_stderr_falls_back_to_stdout() {
        let a = oid('a');
        let err = classify_run(&RunReport {
            message: "Your local changes would be overwritten",
            stderr: "   \n",
            ..report(None, 0, Some(&a), Some(&a), false)
        });
        let Err(GitError::Failed { stderr, .. }) = err else {
            panic!("expected a failure, got {err:?}");
        };
        assert_eq!(stderr, "Your local changes would be overwritten");
    }

    #[test]
    fn a_moved_head_with_a_clean_worktree_completed() {
        let before = oid('a');
        let after = oid('b');
        let outcome =
            classify_run(&report(None, 0, Some(&before), Some(&after), true)).expect("classifies");
        assert_eq!(
            outcome,
            Outcome::Completed {
                from: before,
                to: after
            }
        );
        assert!(outcome.changed_anything());
        assert!(!outcome.is_conflicted());
    }

    /// An unborn HEAD on either side has no oid to report, and pre-flight
    /// refuses to start there in the first place.
    #[test]
    fn an_unborn_head_on_either_side_is_not_a_completion() {
        let present = oid('a');
        assert!(classify_run(&report(None, 0, None, Some(&present), true)).is_err());
        assert!(classify_run(&report(None, 0, Some(&present), None, true)).is_err());
    }
}
