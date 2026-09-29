//! Between `rostrum-remote`'s wire types and `rostrum-local`'s.
//!
//! `rostrum-remote` does not depend on `rostrum-local` (the phone's build must
//! not pull in the desktop's git machinery), so the server is where the two
//! meet. Every conversion is a total function here, so a new variant on either
//! side is a compile error in exactly one place.

use rostrum_git::{Autostash, BranchName, InProgress};
use rostrum_handoff::PrMeta;
use rostrum_local::{HandoffState, LocalBranch, LocalOp, LocalResult, LocalState};
use rostrum_remote::{
    HandoffStatus, InProgressKind, JobOutcome, LocalBranchStatus, LocalOpKind, LocalStatus, PrRef,
};

pub fn job_outcome(result: LocalResult) -> JobOutcome {
    match result {
        LocalResult::NotCheckedOut => JobOutcome::NotCheckedOut,
        LocalResult::UpToDate => JobOutcome::UpToDate,
        LocalResult::Completed => JobOutcome::Completed,
        LocalResult::Refused(reason) => JobOutcome::Refused { reason },
        LocalResult::Conflicted(reason) => JobOutcome::Conflicted { reason },
        LocalResult::HandedOff { session } => JobOutcome::HandedOff { session },
        LocalResult::Failed(reason) => JobOutcome::Failed { reason },
    }
}

/// [`LocalState`] is only ever about a repository with a clone; the
/// not-configured case never reaches `rostrum-local` and is answered before.
pub fn local_status(state: LocalState) -> LocalStatus {
    match state {
        LocalState::NotCheckedOut => LocalStatus::NotCheckedOut,
        LocalState::CheckedOut(branch) => LocalStatus::CheckedOut {
            branch: branch_status(branch),
        },
    }
}

fn branch_status(branch: LocalBranch) -> LocalBranchStatus {
    LocalBranchStatus {
        worktree: branch.worktree.display().to_string(),
        branch: branch.branch.as_str().to_string(),
        ahead: branch.divergence.ahead,
        behind: branch.divergence.behind,
        fetched: branch.fetched,
        blocker: branch.blocker,
        in_progress: branch.in_progress.map(in_progress_kind),
        handoff: branch.handoff.map(handoff_status),
    }
}

/// Both rebase back-ends are one rebase to the phone: the abort is the same.
pub fn in_progress_kind(in_progress: InProgress) -> InProgressKind {
    match in_progress {
        InProgress::Rebase | InProgress::RebaseApply => InProgressKind::Rebase,
        InProgress::Am => InProgressKind::Am,
        InProgress::Merge => InProgressKind::Merge,
        InProgress::CherryPick => InProgressKind::CherryPick,
        InProgress::Revert => InProgressKind::Revert,
        InProgress::Bisect => InProgressKind::Bisect,
    }
}

pub fn handoff_status(state: HandoffState) -> HandoffStatus {
    match state {
        HandoffState::Running { session } => HandoffStatus::Running { session },
        HandoffState::Gone { session } => HandoffStatus::Gone { session },
    }
}

pub fn local_op(kind: LocalOpKind) -> LocalOp {
    match kind {
        LocalOpKind::PullRebase => LocalOp::PullRebase,
        LocalOpKind::MergeRemote => LocalOp::MergeRemote,
        LocalOpKind::MergeBase => LocalOp::MergeBase,
        LocalOpKind::RebaseBase => LocalOp::RebaseBase,
    }
}

pub fn autostash(enabled: bool) -> Autostash {
    if enabled {
        Autostash::Enabled
    } else {
        Autostash::Disabled
    }
}

/// What a conflict handoff's bundle says about the pull request.
pub fn pr_meta(pr: &PrRef) -> PrMeta {
    PrMeta {
        repo: pr.key.repo.clone(),
        number: pr.key.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        body: pr.body.clone(),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
    }
}

/// Validate a branch name from the phone before it gets anywhere near an
/// argument vector. The error is `rostrum-git`'s own sentence.
pub fn branch(raw: &str) -> Result<BranchName, String> {
    BranchName::new(raw).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rostrum_core::{Divergence, PrNumber, RepoId};
    use rostrum_git::RemoteRef;
    use rostrum_remote::PrKey;

    use super::*;

    #[test]
    fn every_local_result_maps_to_its_outcome() {
        let cases = [
            (LocalResult::NotCheckedOut, JobOutcome::NotCheckedOut),
            (LocalResult::UpToDate, JobOutcome::UpToDate),
            (LocalResult::Completed, JobOutcome::Completed),
            (
                LocalResult::Refused("dirty".into()),
                JobOutcome::Refused {
                    reason: "dirty".into(),
                },
            ),
            (
                LocalResult::Conflicted("CONFLICT (content)".into()),
                JobOutcome::Conflicted {
                    reason: "CONFLICT (content)".into(),
                },
            ),
            (
                LocalResult::HandedOff {
                    session: "rostrum-a-b-1".into(),
                },
                JobOutcome::HandedOff {
                    session: "rostrum-a-b-1".into(),
                },
            ),
            (
                LocalResult::Failed("timed out".into()),
                JobOutcome::Failed {
                    reason: "timed out".into(),
                },
            ),
        ];
        for (result, outcome) in cases {
            assert_eq!(job_outcome(result), outcome);
        }
    }

    fn checked_out(in_progress: Option<InProgress>, handoff: Option<HandoffState>) -> LocalState {
        LocalState::CheckedOut(LocalBranch {
            worktree: PathBuf::from("/home/u/Code/rostrum/feat-x"),
            branch: BranchName::new("feat/x").expect("branch"),
            remote: RemoteRef::origin(BranchName::new("feat/x").expect("branch")),
            divergence: Divergence {
                ahead: 2,
                behind: 3,
            },
            fetched: false,
            blocker: Some("the worktree has uncommitted changes".into()),
            in_progress,
            handoff,
        })
    }

    #[test]
    fn a_checked_out_branch_carries_every_field_across() {
        let status = local_status(checked_out(
            Some(InProgress::Rebase),
            Some(HandoffState::Running {
                session: "rostrum-o-r-9".into(),
            }),
        ));
        assert_eq!(
            status,
            LocalStatus::CheckedOut {
                branch: LocalBranchStatus {
                    worktree: "/home/u/Code/rostrum/feat-x".into(),
                    branch: "feat/x".into(),
                    ahead: 2,
                    behind: 3,
                    fetched: false,
                    blocker: Some("the worktree has uncommitted changes".into()),
                    in_progress: Some(InProgressKind::Rebase),
                    handoff: Some(HandoffStatus::Running {
                        session: "rostrum-o-r-9".into()
                    }),
                }
            }
        );
    }

    #[test]
    fn not_checked_out_stays_not_checked_out() {
        assert_eq!(
            local_status(LocalState::NotCheckedOut),
            LocalStatus::NotCheckedOut
        );
    }

    #[test]
    fn a_gone_handoff_is_reported_as_gone() {
        let LocalStatus::CheckedOut { branch } = local_status(checked_out(
            Some(InProgress::Merge),
            Some(HandoffState::Gone {
                session: "s".into(),
            }),
        )) else {
            panic!("checked out");
        };
        assert_eq!(
            branch.handoff,
            Some(HandoffStatus::Gone {
                session: "s".into()
            })
        );
        assert_eq!(branch.in_progress, Some(InProgressKind::Merge));
    }

    #[test]
    fn both_rebase_backends_are_a_rebase_and_the_rest_map_one_to_one() {
        let cases = [
            (InProgress::Rebase, InProgressKind::Rebase),
            (InProgress::RebaseApply, InProgressKind::Rebase),
            (InProgress::Am, InProgressKind::Am),
            (InProgress::Merge, InProgressKind::Merge),
            (InProgress::CherryPick, InProgressKind::CherryPick),
            (InProgress::Revert, InProgressKind::Revert),
            (InProgress::Bisect, InProgressKind::Bisect),
        ];
        for (from, to) in cases {
            assert_eq!(in_progress_kind(from), to, "{from:?}");
        }
    }

    #[test]
    fn ops_map_one_to_one() {
        assert_eq!(local_op(LocalOpKind::PullRebase), LocalOp::PullRebase);
        assert_eq!(local_op(LocalOpKind::MergeRemote), LocalOp::MergeRemote);
        assert_eq!(local_op(LocalOpKind::MergeBase), LocalOp::MergeBase);
        assert_eq!(local_op(LocalOpKind::RebaseBase), LocalOp::RebaseBase);
    }

    #[test]
    fn a_pr_ref_becomes_the_bundles_metadata() {
        let pr = PrRef {
            key: PrKey {
                repo: RepoId::new("RhizoNymph", "rostrum"),
                number: PrNumber(12),
            },
            title: "Add rostrumd".into(),
            url: "https://github.com/RhizoNymph/rostrum/pull/12".into(),
            body: "Body".into(),
            head_ref: "feat/rostrumd".into(),
            base_ref: "main".into(),
        };
        let meta = pr_meta(&pr);
        assert_eq!(meta.repo, pr.key.repo);
        assert_eq!(meta.number, PrNumber(12));
        assert_eq!(meta.title, "Add rostrumd");
        assert_eq!(meta.url, pr.url);
        assert_eq!(meta.body, "Body");
        assert_eq!(meta.head_ref, "feat/rostrumd");
        assert_eq!(meta.base_ref, "main");
    }

    #[test]
    fn branch_names_are_validated() {
        assert!(branch("feat/x").is_ok());
        assert!(branch("--upload-pack=evil").is_err());
        assert!(branch("a..b").is_err());
        assert!(branch("").is_err());
    }

    #[test]
    fn autostash_follows_the_flag() {
        assert_eq!(autostash(true), Autostash::Enabled);
        assert_eq!(autostash(false), Autostash::Disabled);
    }
}
