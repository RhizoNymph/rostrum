//! Pushing a pull request's checked-out branch to its head repository.
//!
//! Two steps, so a front-end can decide before drawing a button and confirm
//! before rewriting anything:
//!
//! 1. [`plan_push`] reads the worktree and says what a push would do: a
//!    fast-forward ([`PushKind::FastForward`]), a rewrite of a diverged
//!    branch ([`PushKind::Rewrite`], which needs the user's say-so), or
//!    nothing, with the reason ([`PushSkip`]).
//! 2. [`run_push`] plans again from what is on disk — never fetching, so the
//!    lease is the remote oid the user saw — and pushes. A rewrite runs only
//!    with a [`ConfirmedRewrite`] naming the branch and the exact remote oid
//!    it replaces; a confirmation for any other oid is refused
//!    ([`PushResult::ConfirmationStale`]) without touching the remote.
//!
//! Where the branch goes is the user's own configuration: the remote and
//! branch of its upstream (`branch.<b>.remote` / `branch.<b>.merge`), and
//! only when that remote branch is the pull request's head branch in the
//! pull request's head repository. Nothing here picks a remote.

mod batch;
mod url;

use std::{num::NonZeroU32, path::PathBuf};

use rostrum_core::{Relation, RepoId};
use rostrum_git::{
    Autostash, BranchName, BranchUpstream, GitError, Oid, Operation, PushMode, PushOutcome,
    PushRejection, Remote, RemoteRef, Repo, Rev, blockers,
};

pub use batch::{PlannedEntry, PushAllPlan, PushStep, RewriteDecision, plan_push_all};
pub use url::{repo_of_url, url_names};

/// One pull request's branch to push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushTarget {
    /// Any worktree of the clone; the one for `branch` is found from it.
    pub clone: PathBuf,
    /// The pull request's head branch — the local branch, and the remote
    /// branch it must track.
    pub branch: BranchName,
    /// The repository the head branch lives in on GitHub. `None` for a fork
    /// whose repository GitHub did not name; such a branch is never pushed.
    pub head_repo: Option<RepoId>,
}

/// Whether [`plan_push`] refreshes the tracking ref first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// Fetch the upstream branch first, so the plan describes the remote as
    /// it is now. A failed fetch is logged and the refs on disk are used.
    Fetch,
    /// Use the refs on disk. [`run_push`] always does: the lease must be the
    /// oid the user was shown.
    OnDisk,
}

/// What a push of one branch would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushPlan {
    Ready(PlannedPush),
    Skip(PushSkip),
}

impl PushPlan {
    pub fn ready(&self) -> Option<&PlannedPush> {
        match self {
            Self::Ready(planned) => Some(planned),
            Self::Skip(_) => None,
        }
    }

    /// The confirmation this plan needs before it may run, if it is a
    /// rewrite.
    pub fn rewrite(&self) -> Option<ConfirmedRewrite> {
        self.ready().and_then(PlannedPush::confirmation)
    }
}

/// A branch that can be pushed, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPush {
    /// The worktree the branch is checked out in.
    pub worktree: PathBuf,
    pub branch: BranchName,
    /// Where it goes: the branch's own upstream, already checked to be the
    /// pull request's head.
    pub upstream: RemoteRef,
    /// The commit that will be pushed: the branch tip when planned.
    pub local: Oid,
    pub kind: PushKind,
}

impl PlannedPush {
    /// The confirmation a rewrite needs, naming this branch and the remote
    /// oid it would replace. `None` for a fast-forward, which needs none.
    pub fn confirmation(&self) -> Option<ConfirmedRewrite> {
        match &self.kind {
            PushKind::FastForward { .. } => None,
            PushKind::Rewrite { expected, .. } => Some(ConfirmedRewrite {
                branch: self.branch.clone(),
                expected: expected.clone(),
            }),
        }
    }
}

/// The two pushes rostrum makes on a user's behalf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushKind {
    /// The branch only adds commits to its upstream: an ordinary push, which
    /// the remote refuses if it would drop anything.
    FastForward { ahead: NonZeroU32 },
    /// The branch and its upstream have each moved — a local rebase, say.
    /// Pushing rewrites the remote branch, leased on `expected`, the
    /// upstream oid last fetched; it needs a [`ConfirmedRewrite`].
    Rewrite {
        ahead: NonZeroU32,
        behind: NonZeroU32,
        expected: Oid,
    },
}

/// Why a branch will not be pushed. Decided before anything is attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushSkip {
    /// No worktree of the clone has the branch checked out.
    NotCheckedOut,
    /// The worktree is mid-operation, has uncommitted changes, or is not on
    /// the branch; the sentence is the [`rostrum_git::Blocker`]'s.
    Blocked(String),
    /// The branch has no upstream configured, so where it belongs is not
    /// known.
    NoUpstream,
    /// An upstream is configured but is not a plain remote branch (a local
    /// upstream, a custom refspec); carries what git printed.
    UnusualUpstream(String),
    /// The upstream is a remote branch other than the pull request's head.
    UpstreamIsNotHead { upstream: RemoteRef },
    /// A fork's pull request whose head repository GitHub did not name.
    HeadRepositoryUnknown,
    /// The upstream's remote does not push to the head repository.
    RemoteIsNotHeadRepository {
        remote: Remote,
        head_repo: RepoId,
        urls: Vec<String>,
    },
    /// The upstream's tracking ref does not exist locally: never fetched, so
    /// there is no remote oid to compare with or lease on.
    NotFetched { upstream: RemoteRef },
    /// The branch and its upstream are the same commit.
    UpToDate,
    /// The upstream has commits the branch lacks and the branch has none of
    /// its own: pull, do not push.
    Behind { behind: NonZeroU32 },
}

impl PushSkip {
    /// A sentence for a disabled button's tooltip or a skipped row.
    pub fn reason(&self) -> String {
        match self {
            Self::NotCheckedOut => "not checked out in any worktree".into(),
            Self::Blocked(reason) => reason.clone(),
            Self::NoUpstream => {
                "the branch has no upstream configured, so where to push it is not known".into()
            }
            Self::UnusualUpstream(upstream) => {
                format!(
                    "the branch's upstream `{upstream}` is not a remote branch rostrum pushes to"
                )
            }
            Self::UpstreamIsNotHead { upstream } => {
                format!("the branch tracks `{upstream}`, not the pull request's head branch")
            }
            Self::HeadRepositoryUnknown => {
                "GitHub does not say which repository this fork's branch lives in".into()
            }
            Self::RemoteIsNotHeadRepository {
                remote, head_repo, ..
            } => format!(
                "the branch's upstream remote `{remote}` does not push to {head_repo}, where the pull request's head lives"
            ),
            Self::NotFetched { upstream } => {
                format!("`{upstream}` has never been fetched, so there is nothing to compare with")
            }
            Self::UpToDate => "nothing to push: the branch matches its upstream".into(),
            Self::Behind { behind } => {
                format!(
                    "nothing to push: the branch is {behind} behind its upstream — pull instead"
                )
            }
        }
    }
}

/// The user's say-so to rewrite `branch` on its remote, given while the
/// remote was at `expected`.
///
/// A rewrite runs only when the remote oid it would replace is still exactly
/// `expected`; a confirmation for any other oid is refused, so agreeing to
/// overwrite one commit can never overwrite a different one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfirmedRewrite {
    pub branch: BranchName,
    pub expected: Oid,
}

/// What [`run_push`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushResult {
    /// The remote branch now points at the pushed commit.
    Pushed {
        upstream: RemoteRef,
        /// Whether the push replaced commits (a confirmed rewrite).
        rewrote: bool,
    },
    /// Nothing to push, or nothing that may be pushed. Nothing was run.
    Skipped(PushSkip),
    /// The branch has diverged and no confirmation was given for it.
    NotConfirmed { expected: Oid },
    /// A confirmation was given, but for a remote oid other than the one the
    /// rewrite would now replace. Nothing was pushed.
    ConfirmationStale { confirmed: Oid, current: Oid },
    /// The remote refused; it is unchanged. A stale lease is
    /// [`PushRejection::StaleLease`].
    Rejected(PushRejection),
    /// Something other than the remote refused: the clone could not be read,
    /// the push could not authenticate, it timed out.
    Failed(String),
}

impl PushResult {
    /// Short chip text, or `None` for the unremarkable outcomes.
    pub fn chip(&self) -> Option<&'static str> {
        match self {
            Self::Pushed { rewrote: false, .. } => Some("pushed"),
            Self::Pushed { rewrote: true, .. } => Some("rewritten"),
            Self::Skipped(PushSkip::NotCheckedOut | PushSkip::UpToDate) => None,
            Self::Skipped(_) => Some("skipped"),
            Self::NotConfirmed { .. } => Some("not confirmed"),
            Self::ConfirmationStale { .. } => Some("stale"),
            Self::Rejected(_) => Some("rejected"),
            Self::Failed(_) => Some("failed"),
        }
    }

    /// The sentence behind the chip, or for a banner.
    pub fn detail(&self) -> String {
        match self {
            Self::Pushed {
                upstream,
                rewrote: false,
            } => format!("pushed to {upstream}"),
            Self::Pushed {
                upstream,
                rewrote: true,
            } => format!("force-pushed (with lease) to {upstream}"),
            Self::Skipped(skip) => skip.reason(),
            Self::NotConfirmed { expected } => format!(
                "the branch has diverged from its upstream (at {}); rewriting it needs confirmation",
                expected.short()
            ),
            Self::ConfirmationStale { confirmed, current } => format!(
                "the rewrite was confirmed against {}, but the upstream is now at {}; nothing was pushed",
                confirmed.short(),
                current.short()
            ),
            Self::Rejected(rejection) => rejection.describe(),
            Self::Failed(reason) => reason.clone(),
        }
    }

    /// Whether the remote changed.
    pub fn landed(&self) -> bool {
        matches!(self, Self::Pushed { .. })
    }
}

/// Decide what pushing `target`'s branch would do.
///
/// An `Err` is a git failure reading the clone; everything that merely
/// stops a push is a [`PushPlan::Skip`].
pub async fn plan_push(target: &PushTarget, refresh: Refresh) -> Result<PushPlan, GitError> {
    let clone = Repo::open(&target.clone).await?;
    let Some(repo) = clone.worktree_for(&target.branch).await? else {
        return Ok(PushPlan::Skip(PushSkip::NotCheckedOut));
    };

    let status = repo.status().await?;
    if let Some(blocker) = blockers(
        Operation::Push,
        &status,
        Some(&target.branch),
        Autostash::Disabled,
    )
    .into_iter()
    .next()
    {
        return Ok(PushPlan::Skip(PushSkip::Blocked(blocker.reason())));
    }
    let Some(local) = status.oid().cloned() else {
        // Unreachable past the blockers (an unborn branch is one), but the
        // type does not know that.
        return Ok(PushPlan::Skip(PushSkip::Blocked(
            "the branch has no commits".into(),
        )));
    };

    let upstream = match repo.branch_upstream(&target.branch).await? {
        BranchUpstream::None => return Ok(PushPlan::Skip(PushSkip::NoUpstream)),
        BranchUpstream::Unusual(upstream) => {
            return Ok(PushPlan::Skip(PushSkip::UnusualUpstream(upstream)));
        }
        BranchUpstream::Tracking(upstream) => upstream,
    };
    if upstream.branch != target.branch {
        return Ok(PushPlan::Skip(PushSkip::UpstreamIsNotHead { upstream }));
    }

    let Some(head_repo) = &target.head_repo else {
        return Ok(PushPlan::Skip(PushSkip::HeadRepositoryUnknown));
    };
    let urls = repo.push_urls(&upstream.remote).await?;
    if urls.is_empty() || !urls.iter().all(|url| url_names(url, head_repo)) {
        return Ok(PushPlan::Skip(PushSkip::RemoteIsNotHeadRepository {
            remote: upstream.remote.clone(),
            head_repo: head_repo.clone(),
            urls,
        }));
    }

    if refresh == Refresh::Fetch
        && let Err(error) = repo.fetch(&upstream).await
    {
        tracing::debug!(%error, upstream = %upstream, "could not fetch; planning from the refs on disk");
    }

    let Some(remote_oid) = repo.resolve(&Rev::Remote(upstream.clone())).await? else {
        return Ok(PushPlan::Skip(PushSkip::NotFetched { upstream }));
    };
    let divergence = repo
        .divergence(
            &Rev::Local(target.branch.clone()),
            &Rev::Remote(upstream.clone()),
        )
        .await?;

    let kind = match divergence.relation() {
        Relation::Identical => return Ok(PushPlan::Skip(PushSkip::UpToDate)),
        Relation::Behind(behind) => return Ok(PushPlan::Skip(PushSkip::Behind { behind })),
        Relation::Ahead(ahead) => PushKind::FastForward { ahead },
        Relation::Diverged { ahead, behind } => PushKind::Rewrite {
            ahead,
            behind,
            expected: remote_oid,
        },
    };
    Ok(PushPlan::Ready(PlannedPush {
        worktree: repo.root().to_path_buf(),
        branch: target.branch.clone(),
        upstream,
        local,
        kind,
    }))
}

/// Push `target`'s branch, if that is still what [`plan_push`] would say to
/// do.
///
/// Plans again from the refs on disk (no fetch), so a rewrite is leased on
/// the oid the user confirmed. A fast-forward needs no confirmation; a
/// rewrite needs `confirmation` for exactly this branch and this oid.
///
/// Every outcome is a value, never an `Err`: in a Push all, one bad clone
/// must not stop the rest.
pub async fn run_push(target: &PushTarget, confirmation: Option<&ConfirmedRewrite>) -> PushResult {
    match push(target, confirmation).await {
        Ok(result) => result,
        Err(error) => PushResult::Failed(error.to_string()),
    }
}

async fn push(
    target: &PushTarget,
    confirmation: Option<&ConfirmedRewrite>,
) -> Result<PushResult, GitError> {
    let planned = match plan_push(target, Refresh::OnDisk).await? {
        PushPlan::Skip(skip) => return Ok(PushResult::Skipped(skip)),
        PushPlan::Ready(planned) => planned,
    };

    let mode = match &planned.kind {
        PushKind::FastForward { .. } => PushMode::FastForward,
        PushKind::Rewrite { expected, .. } => match confirmation {
            None => {
                return Ok(PushResult::NotConfirmed {
                    expected: expected.clone(),
                });
            }
            Some(confirmed) if confirmed.branch != planned.branch => {
                return Ok(PushResult::NotConfirmed {
                    expected: expected.clone(),
                });
            }
            Some(confirmed) if &confirmed.expected != expected => {
                return Ok(PushResult::ConfirmationStale {
                    confirmed: confirmed.expected.clone(),
                    current: expected.clone(),
                });
            }
            Some(_) => PushMode::Lease {
                expected: expected.clone(),
            },
        },
    };

    let repo = Repo::open(&planned.worktree).await?;
    let outcome = repo
        .push_branch(
            &planned.upstream.remote,
            &planned.upstream.branch,
            &planned.local,
            &mode,
        )
        .await?;
    Ok(match outcome {
        PushOutcome::Updated { .. } | PushOutcome::Created => PushResult::Pushed {
            upstream: planned.upstream,
            rewrote: matches!(mode, PushMode::Lease { .. }),
        },
        // The remote already had the commit: someone pushed the same thing.
        PushOutcome::UpToDate => PushResult::Skipped(PushSkip::UpToDate),
        PushOutcome::Rejected(rejection) => PushResult::Rejected(rejection),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(c: char) -> Oid {
        Oid::parse(c.to_string().repeat(40)).expect("valid")
    }

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    fn planned(kind: PushKind) -> PlannedPush {
        PlannedPush {
            worktree: PathBuf::from("/wt"),
            branch: branch("feat"),
            upstream: RemoteRef::origin(branch("feat")),
            local: oid('b'),
            kind,
        }
    }

    fn one() -> NonZeroU32 {
        NonZeroU32::new(1).expect("non-zero")
    }

    /// Only a rewrite asks for a confirmation, and the one it asks for names
    /// the branch and the remote oid it would replace.
    #[test]
    fn only_a_rewrite_needs_a_confirmation_and_it_names_the_lease() {
        assert_eq!(
            planned(PushKind::FastForward { ahead: one() }).confirmation(),
            None
        );
        let rewrite = PushPlan::Ready(planned(PushKind::Rewrite {
            ahead: one(),
            behind: one(),
            expected: oid('a'),
        }));
        assert_eq!(
            rewrite.rewrite(),
            Some(ConfirmedRewrite {
                branch: branch("feat"),
                expected: oid('a'),
            })
        );
        assert_eq!(PushPlan::Skip(PushSkip::UpToDate).rewrite(), None);
    }

    #[test]
    fn quiet_outcomes_get_no_chip_and_every_other_one_does() {
        assert_eq!(PushResult::Skipped(PushSkip::UpToDate).chip(), None);
        assert_eq!(PushResult::Skipped(PushSkip::NotCheckedOut).chip(), None);
        assert_eq!(
            PushResult::Skipped(PushSkip::NoUpstream).chip(),
            Some("skipped")
        );
        let upstream = RemoteRef::origin(branch("feat"));
        assert_eq!(
            PushResult::Pushed {
                upstream: upstream.clone(),
                rewrote: false
            }
            .chip(),
            Some("pushed")
        );
        let rewritten = PushResult::Pushed {
            upstream,
            rewrote: true,
        };
        assert_eq!(rewritten.chip(), Some("rewritten"));
        assert!(rewritten.detail().contains("with lease"));
        assert_eq!(
            PushResult::Rejected(PushRejection::StaleLease).chip(),
            Some("rejected")
        );
        let stale = PushResult::ConfirmationStale {
            confirmed: oid('a'),
            current: oid('c'),
        };
        assert_eq!(stale.chip(), Some("stale"));
        assert!(stale.detail().contains("aaaaaaa") && stale.detail().contains("ccccccc"));
        assert!(!stale.landed());
    }

    #[test]
    fn a_behind_branch_is_told_to_pull() {
        let reason = PushSkip::Behind {
            behind: NonZeroU32::new(3).expect("non-zero"),
        }
        .reason();
        assert!(reason.contains("3 behind") && reason.contains("pull"));
    }
}
