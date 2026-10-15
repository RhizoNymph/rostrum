//! The latest batch verdict for one pull request, from whichever batch ran
//! last: a local sync ("Pull all", …) or a Push all.

use rostrum_local::{LocalResult, PushResult, PushSkip};

/// How a row's chip should be coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictTone {
    /// Something the user must look at: a conflict, a failure, a refusal by
    /// the remote.
    Danger,
    /// Handed to someone else, or a remote branch that changed.
    Accent,
    /// Worth a note, not an alarm.
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncVerdict<'a> {
    Local(&'a LocalResult),
    Push(&'a PushResult),
}

impl SyncVerdict<'_> {
    /// Chip text, or `None` for the unremarkable outcomes.
    pub fn chip(&self) -> Option<&'static str> {
        match self {
            Self::Local(result) => result.chip(),
            Self::Push(result) => result.chip(),
        }
    }

    /// The sentence behind the chip.
    pub fn detail(&self) -> String {
        match self {
            Self::Local(result) => result.detail(),
            Self::Push(result) => result.detail(),
        }
    }

    pub fn tone(&self) -> VerdictTone {
        match self {
            Self::Local(LocalResult::Conflicted(_) | LocalResult::Failed(_)) => VerdictTone::Danger,
            Self::Local(LocalResult::HandedOff { .. }) => VerdictTone::Accent,
            Self::Local(
                LocalResult::Refused(_)
                | LocalResult::NotCheckedOut
                | LocalResult::UpToDate
                | LocalResult::Completed,
            ) => VerdictTone::Warning,
            Self::Push(
                PushResult::Rejected(_)
                | PushResult::Failed(_)
                | PushResult::ConfirmationStale { .. },
            ) => VerdictTone::Danger,
            Self::Push(PushResult::Pushed { .. }) => VerdictTone::Accent,
            Self::Push(
                PushResult::NotConfirmed { .. }
                | PushResult::Skipped(
                    PushSkip::NotCheckedOut
                    | PushSkip::Blocked(_)
                    | PushSkip::NoUpstream
                    | PushSkip::UnusualUpstream(_)
                    | PushSkip::UpstreamIsNotHead { .. }
                    | PushSkip::HeadRepositoryUnknown
                    | PushSkip::RemoteIsNotHeadRepository { .. }
                    | PushSkip::NotFetched { .. }
                    | PushSkip::UpToDate
                    | PushSkip::Behind { .. },
                ),
            ) => VerdictTone::Warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use rostrum_git::PushRejection;

    use super::*;

    #[test]
    fn a_rejected_push_is_an_alarm_and_a_skip_is_a_note() {
        let rejected = PushResult::Rejected(PushRejection::StaleLease);
        assert_eq!(SyncVerdict::Push(&rejected).tone(), VerdictTone::Danger);
        let skipped = PushResult::Skipped(PushSkip::NoUpstream);
        assert_eq!(SyncVerdict::Push(&skipped).tone(), VerdictTone::Warning);
        assert_eq!(SyncVerdict::Push(&skipped).chip(), Some("skipped"));
        let conflicted = LocalResult::Conflicted("c".into());
        assert_eq!(SyncVerdict::Local(&conflicted).tone(), VerdictTone::Danger);
        assert_eq!(SyncVerdict::Local(&conflicted).chip(), Some("conflict"));
    }
}
