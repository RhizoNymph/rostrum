//! Pushing pull request branches from the desktop's worktrees, on a phone's
//! request.
//!
//! Two routes, mirroring the desktop's Push / Push all:
//!
//! - [`crate::routes::LOCAL_PUSH_PLAN`]: the dry run. For each pull request,
//!   what a push would do — a fast-forward, a rewrite (with the remote oid it
//!   would replace), or nothing and why ([`PushPlanView`]).
//! - [`crate::routes::LOCAL_PUSH`]: push. Fast-forwards need nothing more. A
//!   branch that has diverged from its upstream is rewritten only when the
//!   request carries a [`LeaseConfirmation`] for that pull request naming the
//!   exact remote oid the dry run showed; the desktop re-reads the branch and
//!   pushes with `--force-with-lease` on that oid. A missing confirmation is
//!   [`PushOutcomeView::NotConfirmed`], one for another oid
//!   [`PushOutcomeView::ConfirmationStale`]; neither touches the remote.
//!
//! Per-entry verdicts rather than one refusal for the batch: whether a branch
//! needs confirming is only known when the desktop reads it, and one
//! branch's answer must not stop the rest.

use rostrum_core::RepoId;
use serde::{Deserialize, Serialize};

use crate::api::PrKey;

/// One pull request's branch to plan or push.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushTargetRef {
    pub key: PrKey,
    /// The pull request's head branch: the local branch, and the remote
    /// branch its upstream must be.
    pub head_ref: String,
    /// The repository the head branch lives in (GitHub's
    /// `headRepository.nameWithOwner`): `key.repo` for a same-repository pull
    /// request, the fork otherwise. `None` for a fork GitHub did not name —
    /// that branch is never pushed.
    pub head_repo: Option<RepoId>,
}

/// `POST /api/v1/local/push/plan`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPlanRequest {
    pub prs: Vec<PushTargetRef>,
}

/// The dry run's answer, one entry per requested pull request, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPlanResponse {
    pub entries: Vec<PushPlanEntry>,
}

impl PushPlanResponse {
    /// The confirmations a push of these entries needs: one per rewrite,
    /// naming the remote oid it would replace. What the phone shows, and
    /// what it sends back once the user agrees.
    pub fn confirm_rewrite(&self) -> Vec<LeaseConfirmation> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.plan {
                PushPlanView::Rewrite { expected, .. } => Some(LeaseConfirmation {
                    key: entry.key.clone(),
                    expected: expected.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPlanEntry {
    pub key: PrKey,
    pub head_ref: String,
    pub plan: PushPlanView,
}

/// What pushing one branch would do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PushPlanView {
    /// No clone of the repository is configured on the desktop.
    NotConfigured,
    /// The branch is only ahead of `upstream` (`remote/branch`): an ordinary
    /// push. Needs no confirmation.
    FastForward {
        upstream: String,
        local: String,
        ahead: u32,
    },
    /// The branch has diverged from `upstream`: pushing rewrites it, leased
    /// on `expected` (the remote oid last fetched). Needs a
    /// [`LeaseConfirmation`] naming `expected`.
    Rewrite {
        upstream: String,
        local: String,
        expected: String,
        ahead: u32,
        behind: u32,
    },
    /// Nothing will be pushed, and why — not checked out, up to date, behind,
    /// no upstream, a dirty or mid-rebase worktree, an upstream that is not
    /// the pull request's head repository. `quiet` marks the unremarkable
    /// ones (not checked out, up to date).
    Skip { reason: String, quiet: bool },
    /// The desktop could not read the clone.
    Failed { reason: String },
}

/// The user's say-so to rewrite one pull request's branch, given while its
/// remote branch was at `expected`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseConfirmation {
    pub key: PrKey,
    /// The full remote oid the dry run showed as [`PushPlanView::Rewrite`]'s
    /// `expected`.
    pub expected: String,
}

/// `POST /api/v1/local/push`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRequest {
    pub prs: Vec<PushTargetRef>,
    /// The rewrites the user agreed to. A diverged branch without one here is
    /// not pushed. Defaulted: a request of fast-forwards can leave it out.
    #[serde(default)]
    pub confirm_rewrite: Vec<LeaseConfirmation>,
}

/// What a push did, one entry per requested pull request, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResponse {
    pub entries: Vec<PushEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushEntry {
    pub key: PrKey,
    pub head_ref: String,
    pub outcome: PushOutcomeView,
}

/// What pushing one branch did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PushOutcomeView {
    NotConfigured,
    /// The remote branch now points at the pushed commit; `rewrote` when it
    /// was a confirmed, leased rewrite.
    Pushed {
        upstream: String,
        rewrote: bool,
    },
    /// Nothing was pushed; see [`PushPlanView::Skip`].
    Skipped {
        reason: String,
        quiet: bool,
    },
    /// The branch has diverged and the request did not confirm it.
    NotConfirmed {
        expected: String,
    },
    /// The request confirmed a rewrite of another remote oid than the one
    /// the branch would now replace; nothing was pushed. Plan again.
    ConfirmationStale {
        confirmed: String,
        current: String,
    },
    /// The remote refused and is unchanged. `stale_lease` when someone
    /// pushed since the desktop last fetched.
    Rejected {
        reason: String,
        stale_lease: bool,
    },
    Failed {
        reason: String,
    },
}

impl PushOutcomeView {
    /// Chip text, or `None` for the unremarkable outcomes.
    pub fn chip(&self) -> Option<&'static str> {
        match self {
            Self::NotConfigured | Self::Skipped { quiet: true, .. } => None,
            Self::Pushed { rewrote: false, .. } => Some("pushed"),
            Self::Pushed { rewrote: true, .. } => Some("rewritten"),
            Self::Skipped { quiet: false, .. } => Some("skipped"),
            Self::NotConfirmed { .. } => Some("not confirmed"),
            Self::ConfirmationStale { .. } => Some("stale"),
            Self::Rejected { .. } => Some("rejected"),
            Self::Failed { .. } => Some("failed"),
        }
    }

    /// The sentence behind the chip.
    pub fn detail(&self) -> String {
        match self {
            Self::NotConfigured => "no clone is configured for this repository".into(),
            Self::Pushed {
                upstream,
                rewrote: false,
            } => format!("pushed to {upstream}"),
            Self::Pushed {
                upstream,
                rewrote: true,
            } => format!("force-pushed (with lease) to {upstream}"),
            Self::Skipped { reason, .. }
            | Self::Rejected { reason, .. }
            | Self::Failed { reason } => reason.clone(),
            Self::NotConfirmed { expected } => format!(
                "the branch has diverged from its upstream (at {}); rewriting it needs confirmation",
                short(expected)
            ),
            Self::ConfirmationStale { confirmed, current } => format!(
                "the rewrite was confirmed against {}, but the upstream is now at {}; nothing was pushed",
                short(confirmed),
                short(current)
            ),
        }
    }

    /// Whether the remote changed.
    pub fn landed(&self) -> bool {
        matches!(self, Self::Pushed { .. })
    }
}

fn short(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

#[cfg(test)]
mod tests {
    use rostrum_core::PrNumber;

    use super::*;

    fn key(number: u32) -> PrKey {
        PrKey {
            repo: RepoId::new("octo", "repo"),
            number: PrNumber(number),
        }
    }

    /// The dry run's rewrites are exactly what the phone confirms, each with
    /// the oid it was shown.
    #[test]
    fn the_confirmations_are_the_rewrites_with_their_oids() {
        let plan = PushPlanResponse {
            entries: vec![
                PushPlanEntry {
                    key: key(1),
                    head_ref: "ff".into(),
                    plan: PushPlanView::FastForward {
                        upstream: "origin/ff".into(),
                        local: "b".repeat(40),
                        ahead: 1,
                    },
                },
                PushPlanEntry {
                    key: key(2),
                    head_ref: "rw".into(),
                    plan: PushPlanView::Rewrite {
                        upstream: "origin/rw".into(),
                        local: "b".repeat(40),
                        expected: "a".repeat(40),
                        ahead: 1,
                        behind: 2,
                    },
                },
                PushPlanEntry {
                    key: key(3),
                    head_ref: "x".into(),
                    plan: PushPlanView::Skip {
                        reason: "nothing to push".into(),
                        quiet: true,
                    },
                },
            ],
        };
        assert_eq!(
            plan.confirm_rewrite(),
            vec![LeaseConfirmation {
                key: key(2),
                expected: "a".repeat(40),
            }]
        );
    }

    /// The wire shapes the phone will decode, pinned.
    #[test]
    fn the_wire_shapes_are_tagged_snake_case() {
        let request: PushRequest = serde_json::from_value(serde_json::json!({
            "prs": [{
                "key": { "repo": { "owner": "octo", "name": "repo" }, "number": 7 },
                "head_ref": "feat",
                "head_repo": { "owner": "forker", "name": "repo" }
            }]
        }))
        .expect("a request without confirmations decodes");
        assert!(request.confirm_rewrite.is_empty());
        assert_eq!(
            request.prs[0].head_repo,
            Some(RepoId::new("forker", "repo"))
        );

        let outcome = serde_json::to_value(PushOutcomeView::Rejected {
            reason: "stale".into(),
            stale_lease: true,
        })
        .expect("encodes");
        assert_eq!(
            outcome,
            serde_json::json!({ "kind": "rejected", "reason": "stale", "stale_lease": true })
        );
        let plan = serde_json::to_value(PushPlanView::Skip {
            reason: "r".into(),
            quiet: false,
        })
        .expect("encodes");
        assert_eq!(plan["kind"], "skip");
    }

    #[test]
    fn quiet_skips_and_unconfigured_clones_get_no_chip() {
        assert_eq!(PushOutcomeView::NotConfigured.chip(), None);
        assert_eq!(
            PushOutcomeView::Skipped {
                reason: "up to date".into(),
                quiet: true
            }
            .chip(),
            None
        );
        let stale = PushOutcomeView::ConfirmationStale {
            confirmed: "a".repeat(40),
            current: "c".repeat(40),
        };
        assert_eq!(stale.chip(), Some("stale"));
        assert!(stale.detail().contains("aaaaaaa"));
        assert!(!stale.landed());
    }
}
