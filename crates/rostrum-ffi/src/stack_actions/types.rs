//! Records for driving stacks through the paired desktop, and their
//! conversions from `rostrum-remote`'s protocol types.

use std::time::SystemTime;

use rostrum_remote::{
    RewriteBranch, StackJobKind as WireKind, StackJobResult as WireResult,
    StackJobState as WireState, StackJobStatus, StackMergeMethod as WireMethod,
    StackRewritePlan as WirePlan,
};

/// How GitHub's stack merge combines each member into the trunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StackMergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl From<StackMergeMethod> for WireMethod {
    fn from(method: StackMergeMethod) -> Self {
        match method {
            StackMergeMethod::Merge => Self::Merge,
            StackMergeMethod::Squash => Self::Squash,
            StackMergeMethod::Rebase => Self::Rebase,
        }
    }
}

/// What a dry run (or a local check) is asked about: arranging pull requests
/// into a new stack over `trunk`, or adding them to the top of stack
/// `stack`. `prs` are bottom first.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StackPlanRequest {
    Arrange {
        repo: String,
        prs: Vec<u32>,
        trunk: String,
    },
    Extend {
        repo: String,
        stack: u32,
        prs: Vec<u32>,
    },
}

/// One branch an operation would rebase and force-push (with lease).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackRewrite {
    pub number: u32,
    pub branch: String,
}

impl From<RewriteBranch> for StackRewrite {
    fn from(rewrite: RewriteBranch) -> Self {
        Self {
            number: rewrite.number.0,
            branch: rewrite.branch.as_str().to_string(),
        }
    }
}

/// The desktop's dry run: the branches an arrangement or extension would
/// rewrite, bottom first. Show them, then pass exactly their `branch` names
/// as `confirm_rewrite`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackRewritePlan {
    pub rewrites: Vec<StackRewrite>,
    /// `false`: the operation only links pull requests on GitHub.
    pub needs_rewrite: bool,
}

impl From<WirePlan> for StackRewritePlan {
    fn from(plan: WirePlan) -> Self {
        Self {
            needs_rewrite: plan.needs_rewrite(),
            rewrites: plan.rewrites.into_iter().map(Into::into).collect(),
        }
    }
}

/// The phone's own check of a request against the cached feed, with
/// `rostrum-core`'s `plan_stack` / `plan_extend` — the rules the desktop
/// applies. The desktop re-checks against GitHub as it is now, so `Valid`
/// here is a good sign, not a promise.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StackPlanCheck {
    /// Valid; `rewrites` are the branches it would rewrite as the cache sees
    /// them (empty: link only).
    Valid { rewrites: Vec<StackRewrite> },
    /// Not valid, in the core's words ("#4 is listed twice").
    Invalid { reason: String },
}

/// Whether one open pull request can be added to the top of a stack.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StackEligibility {
    /// It can. `chained`: it already builds on the stack's top, so adding it
    /// alone is a plain link with no rewrite.
    Eligible { chained: bool },
    /// It cannot, and why.
    Ineligible { reason: String },
}

/// An open pull request offered by "Add to stack", with its eligibility.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackCandidate {
    pub number: u32,
    pub title: String,
    pub eligibility: StackEligibility,
}

/// Which operation a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StackJobKind {
    Make,
    Arrange,
    Extend,
    Merge,
    Unstack,
}

impl From<WireKind> for StackJobKind {
    fn from(kind: WireKind) -> Self {
        match kind {
            WireKind::Make => Self::Make,
            WireKind::Arrange => Self::Arrange,
            WireKind::Extend => Self::Extend,
            WireKind::Merge => Self::Merge,
            WireKind::Unstack => Self::Unstack,
        }
    }
}

/// What a successful job did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StackJobResult {
    /// Made or arranged: the stack exists on GitHub. `rewritten` were rebased
    /// and force-pushed; `tracked`: `gh stack` tracks it in the desktop's
    /// clone.
    Stacked {
        rewritten: Vec<u32>,
        tracked: bool,
    },
    Extended {
        stack: u32,
        rewritten: Vec<u32>,
    },
    Merged {
        stack: u32,
    },
    Unstacked {
        stack: u32,
    },
}

impl From<WireResult> for StackJobResult {
    fn from(result: WireResult) -> Self {
        let numbers = |prs: Vec<rostrum_core::PrNumber>| prs.into_iter().map(|n| n.0).collect();
        match result {
            WireResult::Stacked { rewritten, tracked } => Self::Stacked {
                rewritten: numbers(rewritten),
                tracked,
            },
            WireResult::Extended { stack, rewritten } => Self::Extended {
                stack: stack.get(),
                rewritten: numbers(rewritten),
            },
            WireResult::Merged { stack } => Self::Merged { stack: stack.get() },
            WireResult::Unstacked { stack } => Self::Unstacked { stack: stack.get() },
        }
    }
}

/// Where a job is. Every finished state carries `detail`, the desktop's
/// one-line account, to show as-is.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StackJobState {
    /// `progress`: the current step ("Rebasing #4 (2/3)…"), once there is one.
    Running { progress: Option<String> },
    Done {
        result: StackJobResult,
        detail: String,
    },
    /// A rebase stopped on a conflict and was aborted. Nothing was pushed.
    Conflicted { number: u32, detail: String },
    /// A rebase stopped on a conflict and was left for the desktop's handler
    /// in tmux session `session`. Nothing was pushed; sending the same
    /// request again once it is resolved finishes the job.
    HandedOff {
        number: u32,
        session: String,
        worktree: String,
        detail: String,
    },
    /// It stopped. `pushed`: pull requests already force-pushed before it
    /// did; empty when nothing on GitHub changed.
    Failed { pushed: Vec<u32>, detail: String },
}

impl From<WireState> for StackJobState {
    fn from(state: WireState) -> Self {
        match state {
            WireState::Running { progress } => Self::Running { progress },
            WireState::Done { result, detail } => Self::Done {
                result: result.into(),
                detail,
            },
            WireState::Conflicted { number, detail } => Self::Conflicted {
                number: number.0,
                detail,
            },
            WireState::HandedOff {
                number,
                session,
                worktree,
                detail,
            } => Self::HandedOff {
                number: number.0,
                session,
                worktree,
                detail,
            },
            WireState::Failed { pushed, detail } => Self::Failed {
                pushed: pushed.into_iter().map(|n| n.0).collect(),
                detail,
            },
        }
    }
}

/// A stack job on the paired desktop, running or finished. Poll `stack_job`
/// with `id` until `finished`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StackJob {
    pub id: u64,
    pub repo: String,
    pub kind: StackJobKind,
    pub started_at: SystemTime,
    pub finished_at: Option<SystemTime>,
    pub finished: bool,
    pub state: StackJobState,
}

impl From<StackJobStatus> for StackJob {
    fn from(status: StackJobStatus) -> Self {
        Self {
            id: status.id.0,
            repo: status.repo.to_string(),
            kind: status.kind.into(),
            started_at: status.started_at.into(),
            finished_at: status.finished_at.map(Into::into),
            finished: status.is_finished(),
            state: status.state.into(),
        }
    }
}
