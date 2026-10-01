//! From `rostrum-stack`'s results to the job state a phone polls.
//!
//! Total functions: every `StackOutcome` variant and every error lands in
//! exactly one `StackJobState`, carrying `rostrum-stack`'s own one-line
//! summary as `detail`, so the phone shows the same sentence the desktop's
//! status line would.

use rostrum_core::StackNumber;
use rostrum_remote::{StackJobResult, StackJobState};
use rostrum_stack::{LocalTracking, StackOpError, StackOutcome};

/// A make, arrange or extend job's result.
pub fn chain_state(result: Result<StackOutcome, StackOpError>) -> StackJobState {
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => return failed(&error),
    };
    let detail = outcome.summary();
    match outcome {
        StackOutcome::Stacked(report) => StackJobState::Done {
            result: StackJobResult::Stacked {
                rewritten: report.rewritten,
                tracked: report.local == LocalTracking::Tracked,
            },
            detail,
        },
        StackOutcome::Extended { stack, report } => StackJobState::Done {
            result: StackJobResult::Extended {
                stack,
                rewritten: report.rewritten,
            },
            detail,
        },
        StackOutcome::Conflicted { number, .. } => StackJobState::Conflicted { number, detail },
        StackOutcome::HandedOff {
            number,
            session,
            worktree,
        } => StackJobState::HandedOff {
            number,
            session,
            worktree: worktree.display().to_string(),
            detail,
        },
        StackOutcome::PushRejected { pushed, .. } | StackOutcome::LinkFailed { pushed, .. } => {
            StackJobState::Failed { pushed, detail }
        }
    }
}

/// A merge's result: `gh`'s own account on success.
pub fn merge_state(stack: StackNumber, result: Result<String, StackOpError>) -> StackJobState {
    match result {
        Ok(message) => StackJobState::Done {
            result: StackJobResult::Merged { stack },
            detail: with_message(format!("Stack {stack} merged"), &message),
        },
        Err(error) => failed(&error),
    }
}

/// An unstack's result.
pub fn unstack_state(stack: StackNumber, result: Result<String, StackOpError>) -> StackJobState {
    match result {
        Ok(message) => StackJobState::Done {
            result: StackJobResult::Unstacked { stack },
            detail: with_message(format!("Stack {stack} unstacked"), &message),
        },
        Err(error) => failed(&error),
    }
}

/// An error before any outcome: by `rostrum-stack`'s invariant, nothing
/// changed, so nothing was pushed.
fn failed(error: &StackOpError) -> StackJobState {
    StackJobState::Failed {
        pushed: Vec::new(),
        detail: error.to_string(),
    }
}

fn with_message(headline: String, message: &str) -> String {
    let message = message.trim();
    if message.is_empty() {
        headline
    } else {
        format!("{headline}: {message}")
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::PrNumber;
    use rostrum_git::PushRejection;
    use rostrum_stack::StackReport;

    use super::*;

    fn seven() -> StackNumber {
        StackNumber::new(7).expect("non-zero")
    }

    fn report(local: LocalTracking) -> StackReport {
        StackReport {
            rewritten: vec![PrNumber(2)],
            local,
            notes: vec![],
        }
    }

    #[test]
    fn a_stacked_outcome_is_done_with_its_rewrites_and_tracking() {
        let state = chain_state(Ok(StackOutcome::Stacked(report(LocalTracking::Tracked))));
        let StackJobState::Done { result, detail } = state else {
            panic!("done");
        };
        assert_eq!(
            result,
            StackJobResult::Stacked {
                rewritten: vec![PrNumber(2)],
                tracked: true
            }
        );
        assert!(detail.starts_with("Stack created"), "{detail}");

        let StackJobState::Done { result, .. } = chain_state(Ok(StackOutcome::Stacked(report(
            LocalTracking::Skipped("dirty".into()),
        )))) else {
            panic!("done");
        };
        assert!(matches!(
            result,
            StackJobResult::Stacked { tracked: false, .. }
        ));
    }

    #[test]
    fn an_extension_names_its_stack() {
        let state = chain_state(Ok(StackOutcome::Extended {
            stack: seven(),
            report: report(LocalTracking::Skipped("x".into())),
        }));
        assert!(matches!(
            state,
            StackJobState::Done {
                result: StackJobResult::Extended { stack, .. },
                ..
            } if stack == seven()
        ));
    }

    #[test]
    fn stops_keep_what_the_phone_needs_to_act_on() {
        assert!(matches!(
            chain_state(Ok(StackOutcome::Conflicted {
                number: PrNumber(3),
                message: "CONFLICT".into()
            })),
            StackJobState::Conflicted {
                number: PrNumber(3),
                ..
            }
        ));
        let StackJobState::HandedOff {
            session,
            worktree,
            detail,
            ..
        } = chain_state(Ok(StackOutcome::HandedOff {
            number: PrNumber(3),
            session: "rostrum-o-r-3".into(),
            worktree: "/cache/x".into(),
        }))
        else {
            panic!("handed off");
        };
        assert_eq!(session, "rostrum-o-r-3");
        assert_eq!(worktree, "/cache/x");
        assert!(detail.contains("tmux attach -t =rostrum-o-r-3"));

        assert_eq!(
            chain_state(Ok(StackOutcome::PushRejected {
                pushed: vec![PrNumber(1)],
                number: PrNumber(2),
                reason: PushRejection::StaleLease,
            })),
            StackJobState::Failed {
                pushed: vec![PrNumber(1)],
                detail: StackOutcome::PushRejected {
                    pushed: vec![PrNumber(1)],
                    number: PrNumber(2),
                    reason: PushRejection::StaleLease,
                }
                .summary(),
            }
        );
        assert!(matches!(
            chain_state(Ok(StackOutcome::LinkFailed {
                pushed: vec![PrNumber(1), PrNumber(2)],
                message: "boom".into()
            })),
            StackJobState::Failed { pushed, .. } if pushed.len() == 2
        ));
    }

    #[test]
    fn an_error_is_a_failure_that_pushed_nothing() {
        assert_eq!(
            chain_state(Err(StackOpError::GhStackMissing)),
            StackJobState::Failed {
                pushed: vec![],
                detail: StackOpError::GhStackMissing.to_string(),
            }
        );
        assert!(matches!(
            merge_state(seven(), Err(StackOpError::GhStackMissing)),
            StackJobState::Failed { .. }
        ));
    }

    #[test]
    fn merge_and_unstack_report_gh_in_their_detail() {
        assert_eq!(
            merge_state(seven(), Ok("✓ Merged 3 pull requests".into())),
            StackJobState::Done {
                result: StackJobResult::Merged { stack: seven() },
                detail: "Stack 7 merged: ✓ Merged 3 pull requests".into(),
            }
        );
        assert_eq!(
            unstack_state(seven(), Ok("  ".into())),
            StackJobState::Done {
                result: StackJobResult::Unstacked { stack: seven() },
                detail: "Stack 7 unstacked".into(),
            }
        );
    }
}
