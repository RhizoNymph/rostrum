//! Pushing pull request branches from the desktop's worktrees.
//!
//! The phone plans first (`plan_branch_push`, `plan_push_all`): each pull
//! request comes back as a fast-forward, a rewrite naming the remote oid it
//! would replace, or a skip with its reason. Fast-forwards push without
//! asking. A rewrite is pushed only when the phone sends back a
//! [`PushRewriteConfirmation`] for that pull request with exactly the
//! `expected` oid the plan showed — the UI must show the user every branch
//! it lists and ask once — and the desktop pushes it with
//! `--force-with-lease` on that oid. See `docs/features/remote_protocol.md`.

use std::collections::HashSet;

use rostrum_core::{PullRequest, RepoId, RepoState};
use rostrum_remote::{
    LeaseConfirmation, PrKey, PushEntry, PushOutcomeView, PushPlanEntry, PushPlanRequest,
    PushPlanView, PushRequest, PushTargetRef, api::CloneInfo,
};

use crate::{
    engine::{RostrumCore, state::PullKey},
    error::RostrumError,
    types::{Chip, ColorRole},
};

/// What pushing one pull request's branch would do.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PushPlanKind {
    NotConfigured,
    /// Only ahead of `upstream`: pushes without confirmation.
    FastForward {
        upstream: String,
        local: String,
        ahead: u32,
    },
    /// Diverged from `upstream`: pushing rewrites it, replacing `expected`.
    /// Needs a confirmation naming `expected`.
    Rewrite {
        upstream: String,
        local: String,
        expected: String,
        ahead: u32,
        behind: u32,
    },
    /// Nothing will be pushed; `quiet` for up to date / not checked out.
    Skip {
        reason: String,
        quiet: bool,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PushPlanItem {
    pub repo: String,
    pub number: u32,
    pub head_ref: String,
    pub plan: PushPlanKind,
}

/// The user's agreement to rewrite one pull request's branch on its remote,
/// given while it was at `expected` (from [`PushPlanKind::Rewrite`]).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PushRewriteConfirmation {
    pub repo: String,
    pub number: u32,
    pub expected: String,
}

/// What pushing one pull request's branch did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BranchPushOutcome {
    NotConfigured,
    Pushed {
        upstream: String,
        rewrote: bool,
    },
    Skipped {
        reason: String,
        quiet: bool,
    },
    /// Diverged, and no confirmation was sent for it.
    NotConfirmed {
        expected: String,
    },
    /// The confirmation named another oid than the one a rewrite would now
    /// replace; plan again.
    ConfirmationStale {
        confirmed: String,
        current: String,
    },
    /// The remote refused and is unchanged.
    Rejected {
        reason: String,
        stale_lease: bool,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PushItemResult {
    pub repo: String,
    pub number: u32,
    pub head_ref: String,
    pub outcome: BranchPushOutcome,
    /// One line for a snackbar.
    pub detail: String,
    /// `pushed`, `rewritten`, `skipped`, `rejected`, …; `None` when quiet.
    pub chip: Option<Chip>,
}

fn target(repo: &RepoId, pr: &PullRequest) -> PushTargetRef {
    PushTargetRef {
        key: PrKey {
            repo: repo.clone(),
            number: pr.number,
        },
        head_ref: pr.head_ref.clone(),
        head_repo: pr.head_repository_in(repo),
    }
}

/// Every open pull request in the feed whose repository has a clone on the
/// desktop — the set Push all covers, like sync-all's.
pub(crate) fn push_targets(repos: &[RepoState], clones: &[CloneInfo]) -> Vec<PushTargetRef> {
    let cloned: HashSet<&RepoId> = clones.iter().map(|clone| &clone.repo).collect();
    repos
        .iter()
        .filter(|repo| cloned.contains(&repo.id))
        .flat_map(|repo| repo.prs.iter().map(|pr| target(&repo.id, pr)))
        .collect()
}

fn plan_item(entry: PushPlanEntry) -> PushPlanItem {
    PushPlanItem {
        repo: entry.key.repo.to_string(),
        number: entry.key.number.0,
        head_ref: entry.head_ref,
        plan: match entry.plan {
            PushPlanView::NotConfigured => PushPlanKind::NotConfigured,
            PushPlanView::FastForward {
                upstream,
                local,
                ahead,
            } => PushPlanKind::FastForward {
                upstream,
                local,
                ahead,
            },
            PushPlanView::Rewrite {
                upstream,
                local,
                expected,
                ahead,
                behind,
            } => PushPlanKind::Rewrite {
                upstream,
                local,
                expected,
                ahead,
                behind,
            },
            PushPlanView::Skip { reason, quiet } => PushPlanKind::Skip { reason, quiet },
            PushPlanView::Failed { reason } => PushPlanKind::Failed { reason },
        },
    }
}

fn item_result(entry: PushEntry) -> PushItemResult {
    let detail = entry.outcome.detail();
    let role = match &entry.outcome {
        PushOutcomeView::Rejected { .. }
        | PushOutcomeView::Failed { .. }
        | PushOutcomeView::ConfirmationStale { .. } => ColorRole::Danger,
        PushOutcomeView::Pushed { .. } => ColorRole::Accent,
        _ => ColorRole::Warning,
    };
    let chip = entry.outcome.chip().map(|text| Chip {
        text: text.to_string(),
        role,
        tooltip: Some(detail.clone()),
    });
    let outcome = match entry.outcome {
        PushOutcomeView::NotConfigured => BranchPushOutcome::NotConfigured,
        PushOutcomeView::Pushed { upstream, rewrote } => {
            BranchPushOutcome::Pushed { upstream, rewrote }
        }
        PushOutcomeView::Skipped { reason, quiet } => BranchPushOutcome::Skipped { reason, quiet },
        PushOutcomeView::NotConfirmed { expected } => BranchPushOutcome::NotConfirmed { expected },
        PushOutcomeView::ConfirmationStale { confirmed, current } => {
            BranchPushOutcome::ConfirmationStale { confirmed, current }
        }
        PushOutcomeView::Rejected {
            reason,
            stale_lease,
        } => BranchPushOutcome::Rejected {
            reason,
            stale_lease,
        },
        PushOutcomeView::Failed { reason } => BranchPushOutcome::Failed { reason },
    };
    PushItemResult {
        repo: entry.key.repo.to_string(),
        number: entry.key.number.0,
        head_ref: entry.head_ref,
        outcome,
        detail,
        chip,
    }
}

fn confirmation(confirm: PushRewriteConfirmation) -> Result<LeaseConfirmation, RostrumError> {
    let key = PullKey::parse(&confirm.repo, confirm.number)?;
    Ok(LeaseConfirmation {
        key: PrKey {
            repo: key.repo,
            number: key.number,
        },
        expected: confirm.expected,
    })
}

impl RostrumCore {
    /// The Push all set, from the feed and the desktop's clones.
    async fn push_all_targets(&self) -> Result<Vec<PushTargetRef>, RostrumError> {
        self.ensure_hydrated().await?;
        let client = self.remote().await?;
        let machine = client.machine().await?;
        let targets = self
            .actor
            .call(move |state| push_targets(&state.feed.repos, &machine.clones))
            .await?;
        if targets.is_empty() {
            return Err(RostrumError::invalid(
                "no open pull request in the feed has a clone on the desktop",
            ));
        }
        Ok(targets)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// What pushing one pull request's branch would do. The desktop fetches
    /// the branch's upstream first.
    pub async fn plan_branch_push(
        &self,
        repo: String,
        number: u32,
    ) -> Result<PushPlanItem, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, pr, _) = self.local_target(&key).await?;
        let mut plan = client
            .plan_push(&PushPlanRequest {
                prs: vec![target(&key.repo, &pr)],
            })
            .await?;
        plan.entries
            .pop()
            .map(plan_item)
            .ok_or_else(|| RostrumError::invalid("the desktop answered with no plan"))
    }

    /// Push one pull request's branch. `confirm` is required for a rewrite
    /// and must carry the `expected` oid its plan showed.
    pub async fn push_branch(
        &self,
        repo: String,
        number: u32,
        confirm: Option<PushRewriteConfirmation>,
    ) -> Result<PushItemResult, RostrumError> {
        let key = PullKey::parse(&repo, number)?;
        let (client, pr, _) = self.local_target(&key).await?;
        let confirm_rewrite = confirm.map(confirmation).transpose()?.into_iter().collect();
        let mut answer = client
            .push(&PushRequest {
                prs: vec![target(&key.repo, &pr)],
                confirm_rewrite,
            })
            .await?;
        tracing::info!(repo = %key.repo, number = key.number.0, "branch push finished");
        answer
            .entries
            .pop()
            .map(item_result)
            .ok_or_else(|| RostrumError::invalid("the desktop answered with no result"))
    }

    /// Plan Push all: every open pull request in the feed whose repository
    /// has a clone on the desktop. The rewrites it lists are what the one
    /// confirmation must show.
    pub async fn plan_push_all(&self) -> Result<Vec<PushPlanItem>, RostrumError> {
        let prs = self.push_all_targets().await?;
        let client = self.remote().await?;
        let plan = client.plan_push(&PushPlanRequest { prs }).await?;
        Ok(plan.entries.into_iter().map(plan_item).collect())
    }

    /// Push all: fast-forwards always; each rewrite only with its
    /// confirmation from `confirm`. Waits for every branch.
    pub async fn push_all(
        &self,
        confirm: Vec<PushRewriteConfirmation>,
    ) -> Result<Vec<PushItemResult>, RostrumError> {
        let confirm_rewrite = confirm
            .into_iter()
            .map(confirmation)
            .collect::<Result<Vec<_>, _>>()?;
        let prs = self.push_all_targets().await?;
        let client = self.remote().await?;
        let answer = client
            .push(&PushRequest {
                prs,
                confirm_rewrite,
            })
            .await?;
        Ok(answer.entries.into_iter().map(item_result).collect())
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::PrNumber;

    use super::*;

    fn key() -> PrKey {
        PrKey {
            repo: RepoId::new("octo", "repo"),
            number: PrNumber(7),
        }
    }

    #[test]
    fn a_fork_target_carries_its_head_repository_and_a_same_repo_one_its_base() {
        let base = RepoId::new("octo", "repo");
        let mut pr = crate::test_support::pull(7);
        assert_eq!(target(&base, &pr).head_repo, Some(base.clone()));
        pr.is_cross_repository = true;
        assert_eq!(target(&base, &pr).head_repo, None);
        pr.head_repository = Some(RepoId::new("forker", "repo"));
        assert_eq!(
            target(&base, &pr).head_repo,
            Some(RepoId::new("forker", "repo"))
        );
    }

    #[test]
    fn push_results_carry_the_desktops_chip_colours() {
        let rejected = item_result(PushEntry {
            key: key(),
            head_ref: "feat".into(),
            outcome: PushOutcomeView::Rejected {
                reason: "stale".into(),
                stale_lease: true,
            },
        });
        assert_eq!(rejected.chip.map(|c| c.role), Some(ColorRole::Danger));
        assert_eq!(rejected.repo, "octo/repo");
        let pushed = item_result(PushEntry {
            key: key(),
            head_ref: "feat".into(),
            outcome: PushOutcomeView::Pushed {
                upstream: "origin/feat".into(),
                rewrote: true,
            },
        });
        assert_eq!(
            pushed.chip.as_ref().map(|c| c.text.as_str()),
            Some("rewritten")
        );
        let quiet = item_result(PushEntry {
            key: key(),
            head_ref: "feat".into(),
            outcome: PushOutcomeView::Skipped {
                reason: "up to date".into(),
                quiet: true,
            },
        });
        assert!(quiet.chip.is_none());
    }

    #[test]
    fn a_confirmation_names_its_pull_request() {
        let confirmed = confirmation(PushRewriteConfirmation {
            repo: "octo/repo".into(),
            number: 7,
            expected: "a".repeat(40),
        })
        .expect("valid");
        assert_eq!(confirmed.key, key());
        assert!(
            confirmation(PushRewriteConfirmation {
                repo: "not a repo".into(),
                number: 7,
                expected: "a".repeat(40),
            })
            .is_err()
        );
    }
}
