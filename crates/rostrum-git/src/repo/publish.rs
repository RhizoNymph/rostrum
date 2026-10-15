//! Publishing a branch: where it is configured to go, and the push itself.
//!
//! The pure halves live in [`crate::push`] (argv and verdict) and
//! [`crate::upstream`] (reading the configured upstream). What is here only
//! spawns git and hands the output to them.

use crate::{
    command::CommandKind,
    error::GitError,
    push::{PUSH_CONFIG, PushMode, PushOutcome, branch_push_args, classify_push},
    refs::{BranchName, Oid, Remote},
    upstream::{BranchUpstream, UPSTREAM_FORMAT, parse_upstream},
};

use super::{Repo, strings};

impl Repo {
    /// The upstream `branch` is configured to track, as `git push` and `git
    /// status` would read it.
    pub async fn branch_upstream(&self, branch: &BranchName) -> Result<BranchUpstream, GitError> {
        let format = format!("--format={UPSTREAM_FORMAT}");
        let qualified = branch.qualified();
        let run = self
            .run(
                &strings(["for-each-ref", format.as_str(), "--", qualified.as_str()]),
                CommandKind::Read,
            )
            .await?
            .require_success("for-each-ref")?;
        Ok(parse_upstream(&run.stdout, branch))
    }

    /// Every URL a push to `remote` goes to, after `pushurl` and
    /// `pushInsteadOf` are applied. More than one when the user configured
    /// several push URLs, which git pushes to in turn.
    pub async fn push_urls(&self, remote: &Remote) -> Result<Vec<String>, GitError> {
        let run = self
            .run(
                &strings(["remote", "get-url", "--push", "--all", remote.as_str()]),
                CommandKind::Read,
            )
            .await?
            .require_success("remote get-url")?;
        Ok(run
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Push exactly `new` to `refs/heads/<branch>` on `remote`, in `mode`.
    ///
    /// One ref per call, from an exact oid rather than from whatever a local
    /// branch points at by the time git runs. A refused push is
    /// `Ok(PushOutcome::Rejected(..))` and the remote is unchanged; a push
    /// git printed no verdict for (authentication, an unreachable remote) is
    /// an `Err` carrying stderr. `--no-verify` is never passed: a `pre-push`
    /// hook the user installed still runs.
    pub async fn push_branch(
        &self,
        remote: &Remote,
        branch: &BranchName,
        new: &Oid,
        mode: &PushMode,
    ) -> Result<PushOutcome, GitError> {
        let mut args: Vec<String> = PUSH_CONFIG.iter().map(|arg| arg.to_string()).collect();
        args.extend(branch_push_args(remote, branch, new, mode));
        let run = self.run(&args, CommandKind::Network).await?;
        let outcome = classify_push(&run.stdout, &run.stderr, run.success, run.code, branch)?;
        let expected = match mode {
            PushMode::FastForward => None,
            PushMode::Lease { expected } => Some(expected.short().to_string()),
        };
        tracing::info!(
            branch = %branch,
            remote = %remote,
            new = %new.short(),
            expected = ?expected,
            ?outcome,
            "pushed a branch"
        );
        Ok(outcome)
    }
}
