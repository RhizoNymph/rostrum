//! Push all: plan every branch, confirm the rewrites once, run the rest.
//!
//! The confirmation model for a batch: fast-forwards need none. Every
//! branch that would be rewritten is listed together in one confirmation
//! ([`PushAllPlan::rewrites`]), and the user answers once for the batch with
//! a [`RewriteDecision`]. [`PushAllPlan::authorize`] turns that answer into
//! [`PushStep`]s: a step that runs carries the [`ConfirmedRewrite`] for its
//! own branch and oid, so [`super::run_push`] re-checks each one against the
//! remote oid on disk at the moment it runs.

use super::{ConfirmedRewrite, PlannedPush, PushPlan, PushResult, PushTarget, Refresh, plan_push};

/// Plan every entry, one at a time (they share clones' object stores and
/// locks). Each is fetched first with [`Refresh::Fetch`], so the
/// confirmation shows the remotes as they are now.
pub async fn plan_push_all<K>(entries: Vec<(K, PushTarget)>) -> PushAllPlan<K> {
    let mut planned = Vec::with_capacity(entries.len());
    for (key, target) in entries {
        let plan = plan_push(&target, Refresh::Fetch)
            .await
            .map_err(|error| error.to_string());
        planned.push(PlannedEntry { key, target, plan });
    }
    PushAllPlan { entries: planned }
}

/// One branch's plan in a batch. `plan` is `Err` when the clone could not be
/// read; that entry is reported as failed and never run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedEntry<K> {
    pub key: K,
    pub target: PushTarget,
    pub plan: Result<PushPlan, String>,
}

/// Every branch of a Push all, planned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushAllPlan<K> {
    pub entries: Vec<PlannedEntry<K>>,
}

/// The user's one answer for a batch's rewrites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteDecision {
    /// Rewrite every branch the confirmation listed, each leased on the oid
    /// it was listed with.
    RewriteListed,
    /// Push the fast-forwards only; each listed rewrite is reported as not
    /// confirmed and nothing is run for it.
    FastForwardsOnly,
}

/// What to do with one entry once the batch is authorized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushStep<K> {
    /// Run [`super::run_push`] with this confirmation (present exactly for a
    /// confirmed rewrite).
    Run {
        key: K,
        target: PushTarget,
        confirmation: Option<ConfirmedRewrite>,
    },
    /// Decided without running anything: a skip, a plan that failed, or a
    /// rewrite the user declined.
    Settled { key: K, result: PushResult },
}

impl<K> PushStep<K> {
    pub fn key(&self) -> &K {
        match self {
            Self::Run { key, .. } | Self::Settled { key, .. } => key,
        }
    }
}

impl<K> PushAllPlan<K> {
    /// The branches that would be rewritten, in batch order — exactly what
    /// the one confirmation must list.
    pub fn rewrites(&self) -> Vec<(&K, &PlannedPush)> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.plan {
                Ok(PushPlan::Ready(planned)) if planned.confirmation().is_some() => {
                    Some((&entry.key, planned))
                }
                _ => None,
            })
            .collect()
    }

    /// Whether anything needs the user's say-so before the batch runs.
    pub fn needs_confirmation(&self) -> bool {
        !self.rewrites().is_empty()
    }

    /// How many entries would actually run a push under `decision`.
    pub fn pushes(&self, decision: RewriteDecision) -> usize {
        self.entries
            .iter()
            .filter(|entry| match &entry.plan {
                Ok(PushPlan::Ready(planned)) => {
                    planned.confirmation().is_none() || decision == RewriteDecision::RewriteListed
                }
                _ => false,
            })
            .count()
    }

    /// Turn the user's answer into one step per entry, in batch order.
    pub fn authorize(self, decision: RewriteDecision) -> Vec<PushStep<K>> {
        self.entries
            .into_iter()
            .map(|entry| match entry.plan {
                Err(reason) => PushStep::Settled {
                    key: entry.key,
                    result: PushResult::Failed(reason),
                },
                Ok(PushPlan::Skip(skip)) => PushStep::Settled {
                    key: entry.key,
                    result: PushResult::Skipped(skip),
                },
                Ok(PushPlan::Ready(planned)) => match (planned.confirmation(), decision) {
                    (None, _) => PushStep::Run {
                        key: entry.key,
                        target: entry.target,
                        confirmation: None,
                    },
                    (Some(confirmation), RewriteDecision::RewriteListed) => PushStep::Run {
                        key: entry.key,
                        target: entry.target,
                        confirmation: Some(confirmation),
                    },
                    (Some(confirmation), RewriteDecision::FastForwardsOnly) => PushStep::Settled {
                        key: entry.key,
                        result: PushResult::NotConfirmed {
                            expected: confirmation.expected,
                        },
                    },
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, path::PathBuf};

    use rostrum_core::RepoId;
    use rostrum_git::{BranchName, Oid, RemoteRef};

    use super::*;
    use crate::push::{PushKind, PushSkip};

    fn oid(c: char) -> Oid {
        Oid::parse(c.to_string().repeat(40)).expect("valid")
    }

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    fn n(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("non-zero")
    }

    fn target(name: &str) -> PushTarget {
        PushTarget {
            clone: PathBuf::from("/clone"),
            branch: branch(name),
            head_repo: Some(RepoId::new("octo", "repo")),
        }
    }

    fn ready(name: &str, kind: PushKind) -> Result<PushPlan, String> {
        Ok(PushPlan::Ready(PlannedPush {
            worktree: PathBuf::from(format!("/wt/{name}")),
            branch: branch(name),
            upstream: RemoteRef::origin(branch(name)),
            local: oid('b'),
            kind,
        }))
    }

    /// One of everything: a fast-forward, two rewrites, a skip, a failure.
    fn mixed() -> PushAllPlan<u32> {
        let entry = |key: u32, name: &str, plan| PlannedEntry {
            key,
            target: target(name),
            plan,
        };
        PushAllPlan {
            entries: vec![
                entry(1, "ff", ready("ff", PushKind::FastForward { ahead: n(2) })),
                entry(
                    2,
                    "rw-a",
                    ready(
                        "rw-a",
                        PushKind::Rewrite {
                            ahead: n(1),
                            behind: n(1),
                            expected: oid('a'),
                        },
                    ),
                ),
                entry(
                    3,
                    "behind",
                    Ok(PushPlan::Skip(PushSkip::Behind { behind: n(1) })),
                ),
                entry(4, "broken", Err("could not open".into())),
                entry(
                    5,
                    "rw-c",
                    ready(
                        "rw-c",
                        PushKind::Rewrite {
                            ahead: n(3),
                            behind: n(2),
                            expected: oid('c'),
                        },
                    ),
                ),
            ],
        }
    }

    #[test]
    fn the_confirmation_lists_every_rewrite_and_nothing_else() {
        let plan = mixed();
        assert!(plan.needs_confirmation());
        let listed: Vec<(u32, String)> = plan
            .rewrites()
            .into_iter()
            .map(|(key, planned)| (*key, planned.branch.to_string()))
            .collect();
        assert_eq!(listed, vec![(2, "rw-a".into()), (5, "rw-c".into())]);
        assert_eq!(plan.pushes(RewriteDecision::RewriteListed), 3);
        assert_eq!(plan.pushes(RewriteDecision::FastForwardsOnly), 1);
    }

    #[test]
    fn a_batch_of_fast_forwards_needs_no_confirmation() {
        let plan = PushAllPlan {
            entries: vec![PlannedEntry {
                key: 1,
                target: target("ff"),
                plan: ready("ff", PushKind::FastForward { ahead: n(1) }),
            }],
        };
        assert!(!plan.needs_confirmation());
    }

    /// Confirming carries each rewrite's own branch and oid to its step, so
    /// the run re-checks that exact lease.
    #[test]
    fn confirming_runs_every_rewrite_with_its_own_lease() {
        let steps = mixed().authorize(RewriteDecision::RewriteListed);
        assert_eq!(steps.len(), 5);
        let keys: Vec<u32> = steps.iter().map(|step| *step.key()).collect();
        assert_eq!(keys, vec![1, 2, 3, 4, 5], "batch order is kept");
        assert!(matches!(
            &steps[0],
            PushStep::Run {
                confirmation: None,
                ..
            }
        ));
        assert_eq!(
            steps[1],
            PushStep::Run {
                key: 2,
                target: target("rw-a"),
                confirmation: Some(ConfirmedRewrite {
                    branch: branch("rw-a"),
                    expected: oid('a'),
                }),
            }
        );
        assert!(matches!(
            &steps[2],
            PushStep::Settled {
                result: PushResult::Skipped(PushSkip::Behind { .. }),
                ..
            }
        ));
        assert!(matches!(
            &steps[3],
            PushStep::Settled {
                result: PushResult::Failed(_),
                ..
            }
        ));
        assert!(matches!(
            &steps[4],
            PushStep::Run { confirmation: Some(ConfirmedRewrite { expected, .. }), .. } if *expected == oid('c')
        ));
    }

    /// Declining the rewrites still pushes the fast-forwards, and nothing at
    /// all is run for a declined branch.
    #[test]
    fn declining_rewrites_pushes_fast_forwards_only() {
        let steps = mixed().authorize(RewriteDecision::FastForwardsOnly);
        let run: Vec<u32> = steps
            .iter()
            .filter_map(|step| match step {
                PushStep::Run { key, .. } => Some(*key),
                PushStep::Settled { .. } => None,
            })
            .collect();
        assert_eq!(run, vec![1]);
        assert!(matches!(
            &steps[1],
            PushStep::Settled { result: PushResult::NotConfirmed { expected }, .. } if *expected == oid('a')
        ));
    }
}
