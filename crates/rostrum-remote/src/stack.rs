//! Stacks of pull requests, driven from a paired phone.
//!
//! The phone cannot run `gh`, so the desktop does: every operation here is a
//! request to `rostrumd`, which validates it against GitHub as it is now and
//! runs `rostrum-stack` on the repository's configured clone.
//!
//! Operations that can take minutes — fetching, rebasing in scratch
//! worktrees, leased pushes, `gh stack` — are **jobs**, modelled like
//! sync-all: the `POST` validates and starts the job and answers at once with
//! its [`StackJobStatus`]; `GET` [`crate::routes::stack_job`] polls it until
//! [`StackJobStatus::is_finished`].
//!
//! Rewriting history is never implicit. A request that would rebase and
//! force-push branches (Arrange, or an extension that is not already chained
//! off the stack's top) carries `confirm_rewrite`: the exact set of branches
//! the phone showed the user. The desktop computes the set itself and refuses
//! with [`crate::ApiErrorCode::RewriteNotConfirmed`] unless the two are
//! equal. [`crate::routes::STACK_PLAN`] is the dry run that tells the phone
//! what to show.

use std::{collections::BTreeSet, fmt};

use chrono::{DateTime, Utc};
use rostrum_core::{PrNumber, RefName, RepoId, StackNumber};
use serde::{Deserialize, Serialize};

/// A stack job's identity on one desktop, for polling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StackJobId(pub u64);

impl fmt::Display for StackJobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How GitHub's stack merge combines each member into the trunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackMergeMethod {
    Merge,
    Squash,
    Rebase,
}

/// `POST /api/v1/stacks/make`: make a chain whose bases already chain into a
/// stack. Nothing is rewritten; a chain that would need a rewrite is refused
/// (use [`ArrangeStackRequest`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MakeStackRequest {
    pub repo: RepoId,
    /// Bottom first.
    pub prs: Vec<PrNumber>,
    pub trunk: RefName,
}

/// `POST /api/v1/stacks/arrange`: put pull requests into a stack in this
/// order, rebasing and force-pushing (with lease) whichever branches need it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangeStackRequest {
    pub repo: RepoId,
    /// Bottom first.
    pub prs: Vec<PrNumber>,
    pub trunk: RefName,
    /// Exactly the branches the desktop will rewrite, as
    /// [`crate::routes::STACK_PLAN`] reported them; empty when none.
    pub confirm_rewrite: Vec<RefName>,
}

/// `POST /api/v1/stacks/extend`: add pull requests to the top of stack
/// `stack`. The stack's existing members are never rewritten.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendStackRequest {
    pub repo: RepoId,
    pub stack: StackNumber,
    /// Bottom first; the first builds on the stack's top.
    pub prs: Vec<PrNumber>,
    /// Exactly the branches the desktop will rewrite; empty when the
    /// additions already chain off the top.
    pub confirm_rewrite: Vec<RefName>,
}

/// `POST /api/v1/stacks/merge`: GitHub's atomic stack merge — every open
/// member or none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeStackRequest {
    pub repo: RepoId,
    pub stack: StackNumber,
    pub method: StackMergeMethod,
}

/// `POST /api/v1/stacks/unstack`: dissolve stack `stack`; its pull requests
/// stay open with their current bases.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnstackRequest {
    pub repo: RepoId,
    pub stack: StackNumber,
}

/// `POST /api/v1/stacks/plan`: which branches an arrangement or extension
/// would rewrite, without doing anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackPlanRequest {
    Arrange {
        repo: RepoId,
        prs: Vec<PrNumber>,
        trunk: RefName,
    },
    Extend {
        repo: RepoId,
        stack: StackNumber,
        prs: Vec<PrNumber>,
    },
}

/// The dry run's answer: the branches that would be rebased and
/// force-pushed, bottom first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackRewritePlan {
    pub rewrites: Vec<RewriteBranch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteBranch {
    pub number: PrNumber,
    pub branch: RefName,
}

impl StackRewritePlan {
    /// Whether anything would be rewritten. `false` means the operation only
    /// links pull requests on GitHub.
    pub fn needs_rewrite(&self) -> bool {
        !self.rewrites.is_empty()
    }

    /// The `confirm_rewrite` value that acknowledges exactly this plan.
    pub fn confirm_rewrite(&self) -> Vec<RefName> {
        self.rewrites.iter().map(|r| r.branch.clone()).collect()
    }
}

/// Whether a confirmation names exactly the branches to be rewritten. Order
/// and repetition do not matter; a missing or an extra branch does.
pub fn confirms_exactly(confirmed: &[RefName], rewrites: &[RefName]) -> bool {
    let confirmed: BTreeSet<&RefName> = confirmed.iter().collect();
    let rewrites: BTreeSet<&RefName> = rewrites.iter().collect();
    confirmed == rewrites
}

/// Which operation a job runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackJobKind {
    Make,
    Arrange,
    Extend,
    Merge,
    Unstack,
}

/// A stack job, running or finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackJobStatus {
    pub id: StackJobId,
    pub repo: RepoId,
    pub kind: StackJobKind,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub state: StackJobState,
}

impl StackJobStatus {
    pub fn is_finished(&self) -> bool {
        !matches!(self.state, StackJobState::Running { .. })
    }
}

/// Where a job is. Every finished state carries `detail`, the desktop's
/// one-line account of what happened, for showing as-is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StackJobState {
    Running {
        /// The current step, e.g. "Rebasing #4 (2/3)…", once there is one.
        progress: Option<String>,
    },
    /// It did what was asked.
    Done {
        result: StackJobResult,
        detail: String,
    },
    /// A rebase stopped on a conflict and was aborted. Nothing was pushed.
    Conflicted { number: PrNumber, detail: String },
    /// A rebase stopped on a conflict and was left for the desktop's conflict
    /// handler in tmux session `session`. Nothing was pushed; running the
    /// same request again once it is resolved finishes the job.
    HandedOff {
        number: PrNumber,
        session: String,
        worktree: String,
        detail: String,
    },
    /// It stopped. `pushed` lists pull requests whose branches were already
    /// force-pushed before it did (a rejected lease, a failed link); empty
    /// when nothing on GitHub changed.
    Failed {
        pushed: Vec<PrNumber>,
        detail: String,
    },
}

/// What a successful job did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackJobResult {
    /// Made or arranged: the stack exists on GitHub.
    Stacked {
        /// Rebased and force-pushed (with lease).
        rewritten: Vec<PrNumber>,
        /// Whether `gh stack` now tracks it in the desktop's clone.
        tracked: bool,
    },
    Extended {
        stack: StackNumber,
        rewritten: Vec<PrNumber>,
    },
    Merged {
        stack: StackNumber,
    },
    Unstacked {
        stack: StackNumber,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refname(name: &str) -> RefName {
        RefName::new(name).expect("valid")
    }

    fn seven() -> StackNumber {
        StackNumber::new(7).expect("non-zero")
    }

    #[test]
    fn requests_use_plain_json() {
        let request = ArrangeStackRequest {
            repo: RepoId::new("octo", "repo"),
            prs: vec![PrNumber(3), PrNumber(4)],
            trunk: refname("main"),
            confirm_rewrite: vec![refname("feat/b")],
        };
        let json = serde_json::to_value(&request).expect("serialises");
        assert_eq!(
            json,
            serde_json::json!({
                "repo": {"owner": "octo", "name": "repo"},
                "prs": [3, 4],
                "trunk": "main",
                "confirm_rewrite": ["feat/b"],
            })
        );
        assert_eq!(
            serde_json::from_value::<ArrangeStackRequest>(json).expect("parses"),
            request
        );

        let merge = MergeStackRequest {
            repo: RepoId::new("octo", "repo"),
            stack: seven(),
            method: StackMergeMethod::Squash,
        };
        let json = serde_json::to_value(&merge).expect("serialises");
        assert_eq!(json["stack"], 7);
        assert_eq!(json["method"], "squash");
    }

    #[test]
    fn invalid_values_do_not_parse() {
        // Stack 0 is gh-stack's "no number yet", never a stack.
        let zero = serde_json::json!({"repo": {"owner": "o", "name": "r"}, "stack": 0});
        assert!(serde_json::from_value::<UnstackRequest>(zero).is_err());
        // A trunk that would read as a flag.
        let flag = serde_json::json!({
            "repo": {"owner": "o", "name": "r"}, "prs": [1, 2], "trunk": "--force"
        });
        assert!(serde_json::from_value::<MakeStackRequest>(flag).is_err());
        let method = serde_json::json!({
            "repo": {"owner": "o", "name": "r"}, "stack": 1, "method": "octopus"
        });
        assert!(serde_json::from_value::<MergeStackRequest>(method).is_err());
    }

    #[test]
    fn a_plan_request_is_tagged_by_kind() {
        let json = serde_json::json!({
            "kind": "extend",
            "repo": {"owner": "o", "name": "r"},
            "stack": 7,
            "prs": [9],
        });
        assert_eq!(
            serde_json::from_value::<StackPlanRequest>(json).expect("parses"),
            StackPlanRequest::Extend {
                repo: RepoId::new("o", "r"),
                stack: seven(),
                prs: vec![PrNumber(9)],
            }
        );
    }

    #[test]
    fn job_states_serialise_with_a_state_tag() {
        let running = StackJobState::Running {
            progress: Some("Fetching branches…".into()),
        };
        assert_eq!(
            serde_json::to_value(&running).expect("serialises"),
            serde_json::json!({"state": "running", "progress": "Fetching branches…"})
        );
        let done = StackJobState::Done {
            result: StackJobResult::Extended {
                stack: seven(),
                rewritten: vec![PrNumber(9)],
            },
            detail: "Stack 7 extended".into(),
        };
        assert_eq!(
            serde_json::to_value(&done).expect("serialises"),
            serde_json::json!({
                "state": "done",
                "result": {"kind": "extended", "stack": 7, "rewritten": [9]},
                "detail": "Stack 7 extended",
            })
        );
        let handed = StackJobState::HandedOff {
            number: PrNumber(4),
            session: "rostrum-o-r-4".into(),
            worktree: "/cache/rostrum/stack-worktrees/x".into(),
            detail: "d".into(),
        };
        let back: StackJobState =
            serde_json::from_value(serde_json::to_value(&handed).expect("serialises"))
                .expect("parses");
        assert_eq!(back, handed);
    }

    #[test]
    fn a_status_is_finished_once_it_leaves_running() {
        let mut status = StackJobStatus {
            id: StackJobId(1),
            repo: RepoId::new("o", "r"),
            kind: StackJobKind::Arrange,
            started_at: Utc::now(),
            finished_at: None,
            state: StackJobState::Running { progress: None },
        };
        assert!(!status.is_finished());
        status.state = StackJobState::Failed {
            pushed: vec![],
            detail: "x".into(),
        };
        assert!(status.is_finished());
        let json = serde_json::to_value(&status).expect("serialises");
        assert_eq!(json["kind"], "arrange");
        assert_eq!(json["id"], 1);
    }

    #[test]
    fn the_job_route_fills_in_its_id() {
        assert_eq!(
            crate::routes::stack_job(StackJobId(42)),
            "/api/v1/stacks/jobs/42"
        );
        assert_eq!(
            crate::routes::STACK_JOB.replace("{id}", "42"),
            crate::routes::stack_job(StackJobId(42))
        );
    }

    #[test]
    fn an_unconfirmed_rewrite_is_a_409_with_its_own_code() {
        let code = crate::ApiErrorCode::RewriteNotConfirmed;
        assert_eq!(code.http_status(), 409);
        assert_eq!(
            serde_json::to_value(code).expect("serialises"),
            "rewrite_not_confirmed"
        );
    }

    #[test]
    fn a_confirmation_must_name_exactly_the_rewritten_branches() {
        let plan = StackRewritePlan {
            rewrites: vec![
                RewriteBranch {
                    number: PrNumber(2),
                    branch: refname("b"),
                },
                RewriteBranch {
                    number: PrNumber(3),
                    branch: refname("c"),
                },
            ],
        };
        assert!(plan.needs_rewrite());
        let exact = plan.confirm_rewrite();
        let rewrites = plan.confirm_rewrite();
        assert!(confirms_exactly(&exact, &rewrites));
        assert!(confirms_exactly(
            &[refname("c"), refname("b"), refname("b")],
            &rewrites
        ));
        assert!(!confirms_exactly(&[refname("b")], &rewrites), "missing one");
        assert!(
            !confirms_exactly(&[refname("a"), refname("b"), refname("c")], &rewrites),
            "an extra one"
        );
        assert!(!confirms_exactly(&[], &rewrites));
        assert!(confirms_exactly(&[], &[]));
        assert!(!StackRewritePlan { rewrites: vec![] }.needs_rewrite());
    }
}
