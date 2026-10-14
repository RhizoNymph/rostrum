//! The pushes rostrum makes: a leased force-push of a rewritten stack branch,
//! and a user-initiated push of a checked-out pull request branch.
//!
//! rostrum's standing rule is that it never writes to a remote unasked. There
//! are two deliberate exceptions. Arranging pull requests into a stack rebases
//! a branch onto another, and the pull request only sees the rewrite once it
//! is pushed. And the Push / Push all buttons publish what a local merge or
//! rebase left ahead of the remote. Both are made as narrow as the type
//! system allows:
//!
//! - There is **no bare force**. [`push_args`] always writes
//!   `--force-with-lease=refs/heads/<branch>:<expected-oid>`, and the expected
//!   oid is a required [`Oid`], never inferred. If anyone has pushed to the
//!   branch since rostrum fetched it, the remote refuses and nothing is lost.
//! - **One branch per call**, named by a validated [`BranchName`], pushed
//!   from an exact [`Oid`] rather than from whatever a local branch points at.
//! - The verdict is read from `--porcelain` output into a [`PushOutcome`], so
//!   a rejected lease is a value the caller must handle, not prose.
//! - A push that is not a rewrite is [`PushMode::FastForward`]: no force and
//!   no lease at all, so the remote itself refuses anything but a
//!   fast-forward ([`fast_forward_push_args`]).
//!
//! The spawn lives in [`crate::Repo::push_branch`] (and
//! [`crate::Repo::push_with_lease`], its leased form); this module is pure.

use crate::{
    error::GitError,
    refs::{BranchName, Oid, Remote},
};

/// What the remote did with a leased push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// The branch now points at the pushed commit.
    Updated { forced: bool },
    /// The branch did not exist and now does.
    Created,
    /// The branch already pointed at the pushed commit.
    UpToDate,
    /// Nothing changed on the remote.
    Rejected(PushRejection),
}

impl PushOutcome {
    pub fn landed(&self) -> bool {
        !matches!(self, Self::Rejected(_))
    }
}

/// Why a push was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushRejection {
    /// The remote branch is not at the expected oid: someone pushed since it
    /// was fetched. The whole point of the lease.
    StaleLease,
    /// The remote would need a force the command did not ask for.
    NonFastForward,
    /// The remote's own hooks or rules declined, with their reason.
    RemoteRejected(String),
    /// Some other refusal, verbatim.
    Other(String),
}

impl PushRejection {
    pub fn describe(&self) -> String {
        match self {
            Self::StaleLease => {
                "the branch moved on the remote since it was fetched; nothing was overwritten"
                    .into()
            }
            Self::NonFastForward => "the remote refused a non-fast-forward update".into(),
            Self::RemoteRejected(reason) => format!("the remote rejected the push: {reason}"),
            Self::Other(reason) => format!("the push was refused: {reason}"),
        }
    }
}

/// The arguments for one leased push of `new` to `refs/heads/<branch>` on
/// `remote`, expecting the remote branch to be at `expected`.
///
/// `--no-verify` is deliberately absent: a user's `pre-push` hook is theirs
/// to run. `credential.interactive=false` keeps a credential helper from
/// waiting on a question nobody can see.
pub fn push_args(remote: &Remote, branch: &BranchName, new: &Oid, expected: &Oid) -> Vec<String> {
    let destination = branch.qualified();
    vec![
        "-c".into(),
        "credential.interactive=false".into(),
        "push".into(),
        "--porcelain".into(),
        format!("--force-with-lease={destination}:{expected}"),
        "--".into(),
        remote.as_str().into(),
        format!("{new}:{destination}"),
    ]
}

/// How a push may move the remote branch.
///
/// There is no variant for an unconditional force: a rewrite always names the
/// exact oid it expects to replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushMode {
    /// Forward only. No force and no lease: the remote refuses anything that
    /// would drop a commit it has.
    FastForward,
    /// Rewrite, but only if the remote branch is still exactly `expected` —
    /// `--force-with-lease=refs/heads/<branch>:<expected>`.
    Lease { expected: Oid },
}

/// The arguments for one fast-forward push of `new` to `refs/heads/<branch>`
/// on `remote`: the leased form's shape with the lease left out, and nothing
/// that could force.
pub fn fast_forward_push_args(remote: &Remote, branch: &BranchName, new: &Oid) -> Vec<String> {
    let destination = branch.qualified();
    vec![
        "-c".into(),
        "credential.interactive=false".into(),
        "push".into(),
        "--porcelain".into(),
        "--".into(),
        remote.as_str().into(),
        format!("{new}:{destination}"),
    ]
}

/// The arguments for one push of `new` to `refs/heads/<branch>` in `mode`.
pub fn branch_push_args(
    remote: &Remote,
    branch: &BranchName,
    new: &Oid,
    mode: &PushMode,
) -> Vec<String> {
    match mode {
        PushMode::FastForward => fast_forward_push_args(remote, branch, new),
        PushMode::Lease { expected } => push_args(remote, branch, new, expected),
    }
}

/// Configuration pinned ahead of every push, so a user's settings cannot
/// widen it: `push.followTags` would publish tags alongside the branch, and
/// `push.recurseSubmodules` (or `submodule.recurse`) would push submodule
/// commits too. Each push names one ref and publishes nothing else.
pub const PUSH_CONFIG: [&str; 4] = [
    "-c",
    "push.followTags=false",
    "-c",
    "push.recurseSubmodules=no",
];

/// Read the verdict for `refs/heads/<branch>` out of `git push --porcelain`.
///
/// Each ref gets one line, `<flag>\t<from>:<to>\t<summary>`, between a
/// `To <url>` line and `Done`. The flag is the verdict: `' '` fast-forward,
/// `'+'` forced, `'*'` new, `'='` up to date, `'!'` rejected. Rejections carry
/// their reason in parentheses: `[rejected] (stale info)` is a failed lease,
/// `[remote rejected] (...)` a hook or rule.
///
/// When git printed no line for the ref at all (an authentication failure, an
/// unreachable remote), the push did not happen and the error carries stderr.
pub fn classify_push(
    stdout: &str,
    stderr: &str,
    success: bool,
    code: Option<i32>,
    branch: &BranchName,
) -> Result<PushOutcome, GitError> {
    let destination = branch.qualified();
    let line = stdout.lines().find_map(|line| {
        let mut fields = line.splitn(3, '\t');
        let flag = fields.next()?;
        let refs = fields.next()?;
        let summary = fields.next().unwrap_or("");
        let (_, to) = refs.split_once(':')?;
        (to == destination && flag.chars().count() == 1).then(|| (flag.to_string(), summary))
    });

    let Some((flag, summary)) = line else {
        return Err(GitError::Failed {
            command: "push".into(),
            code,
            stderr: if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            },
        });
    };

    let outcome = match flag.as_str() {
        " " => PushOutcome::Updated { forced: false },
        "+" => PushOutcome::Updated { forced: true },
        "*" => PushOutcome::Created,
        "=" => PushOutcome::UpToDate,
        "!" => PushOutcome::Rejected(rejection(summary)),
        other => {
            return Err(GitError::Parse {
                what: "push porcelain flag",
                line: format!("{other}\t{summary}"),
            });
        }
    };

    // A ref line that says it landed on a push that exited non-zero would be
    // a contradiction worth stopping on rather than reporting success.
    if outcome.landed() && !success {
        return Err(GitError::Failed {
            command: "push".into(),
            code,
            stderr: stderr.trim().to_string(),
        });
    }
    Ok(outcome)
}

fn rejection(summary: &str) -> PushRejection {
    let reason = summary
        .rsplit_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap_or("")
        .trim()
        .to_string();
    if summary.starts_with("[remote rejected]") {
        return PushRejection::RemoteRejected(reason);
    }
    match reason.as_str() {
        "stale info" => PushRejection::StaleLease,
        "non-fast-forward" | "fetch first" => PushRejection::NonFastForward,
        _ => PushRejection::Other(summary.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    fn oid(c: char) -> Oid {
        Oid::parse(c.to_string().repeat(40)).expect("valid")
    }

    #[test]
    fn the_push_always_carries_an_explicit_lease_and_one_refspec() {
        let args = push_args(&Remote::origin(), &branch("feat/a"), &oid('b'), &oid('a'));
        assert_eq!(
            args,
            vec![
                "-c",
                "credential.interactive=false",
                "push",
                "--porcelain",
                &format!("--force-with-lease=refs/heads/feat/a:{}", "a".repeat(40)),
                "--",
                "origin",
                &format!("{}:refs/heads/feat/a", "b".repeat(40)),
            ]
        );
        // Never a bare force, never a lease without an expected value.
        assert!(!args.iter().any(|a| a == "--force" || a == "-f"));
        assert!(!args.iter().any(|a| a == "--force-with-lease"));
        assert!(!args.iter().any(|a| a == "--no-verify"));
        assert!(!args.iter().any(|a| a.starts_with('+')));

        // The user-initiated forms: a fast-forward carries no force and no
        // lease at all; a rewrite is exactly the leased form above.
        let ff = fast_forward_push_args(&Remote::origin(), &branch("feat/a"), &oid('b'));
        assert_eq!(
            ff,
            vec![
                "-c",
                "credential.interactive=false",
                "push",
                "--porcelain",
                "--",
                "origin",
                &format!("{}:refs/heads/feat/a", "b".repeat(40)),
            ]
        );
        assert_eq!(
            branch_push_args(
                &Remote::origin(),
                &branch("feat/a"),
                &oid('b'),
                &PushMode::FastForward
            ),
            ff
        );
        assert_eq!(
            branch_push_args(
                &Remote::origin(),
                &branch("feat/a"),
                &oid('b'),
                &PushMode::Lease { expected: oid('a') }
            ),
            args
        );
        for form in [&ff, &args] {
            assert!(!form.iter().any(|a| a == "--force" || a == "-f"));
            assert!(!form.iter().any(|a| a == "--force-with-lease"));
            assert!(!form.iter().any(|a| a.starts_with("--force-if-includes")));
            assert!(!form.iter().any(|a| a == "--no-verify"));
            assert!(!form.iter().any(|a| a.starts_with('+')));
            assert!(
                !form
                    .iter()
                    .any(|a| a == "--mirror" || a == "--all" || a == "--tags")
            );
            assert!(!form.iter().any(|a| a == "--delete" || a == "-d"));
            assert_eq!(
                form.iter()
                    .filter(|a| a.contains(':') && !a.starts_with("--"))
                    .count(),
                1
            );
        }
        assert!(!ff.iter().any(|a| a.starts_with("--force")));
        // The pinned configuration only ever narrows a push.
        assert_eq!(
            PUSH_CONFIG,
            [
                "-c",
                "push.followTags=false",
                "-c",
                "push.recurseSubmodules=no"
            ]
        );
    }

    fn porcelain(line: &str) -> String {
        format!("To /tmp/origin.git\n{line}\nDone\n")
    }

    #[test]
    fn a_forced_update_is_reported_as_such() {
        let out = porcelain("+\taaaa:refs/heads/x\t1111111...2222222 (forced update)");
        assert_eq!(
            classify_push(&out, "", true, Some(0), &branch("x")).expect("parses"),
            PushOutcome::Updated { forced: true }
        );
    }

    #[test]
    fn fast_forward_new_and_up_to_date_are_distinguished() {
        let b = branch("x");
        assert_eq!(
            classify_push(
                &porcelain(" \taaaa:refs/heads/x\t1111111..2222222"),
                "",
                true,
                Some(0),
                &b
            )
            .expect("parses"),
            PushOutcome::Updated { forced: false }
        );
        assert_eq!(
            classify_push(
                &porcelain("*\taaaa:refs/heads/x\t[new branch]"),
                "",
                true,
                Some(0),
                &b
            )
            .expect("parses"),
            PushOutcome::Created
        );
        assert_eq!(
            classify_push(
                &porcelain("=\taaaa:refs/heads/x\t[up to date]"),
                "",
                true,
                Some(0),
                &b
            )
            .expect("parses"),
            PushOutcome::UpToDate
        );
    }

    #[test]
    fn a_failed_lease_is_a_stale_lease_rejection() {
        let out = porcelain("!\taaaa:refs/heads/x\t[rejected] (stale info)");
        let outcome = classify_push(&out, "error: failed to push", false, Some(1), &branch("x"))
            .expect("parses");
        assert_eq!(outcome, PushOutcome::Rejected(PushRejection::StaleLease));
        assert!(!outcome.landed());
    }

    #[test]
    fn other_rejections_keep_their_reason() {
        let b = branch("x");
        assert_eq!(
            classify_push(
                &porcelain("!\taaaa:refs/heads/x\t[remote rejected] (pre-receive hook declined)"),
                "",
                false,
                Some(1),
                &b
            )
            .expect("parses"),
            PushOutcome::Rejected(PushRejection::RemoteRejected(
                "pre-receive hook declined".into()
            ))
        );
        assert_eq!(
            classify_push(
                &porcelain("!\taaaa:refs/heads/x\t[rejected] (fetch first)"),
                "",
                false,
                Some(1),
                &b
            )
            .expect("parses"),
            PushOutcome::Rejected(PushRejection::NonFastForward)
        );
        assert_eq!(
            classify_push(
                &porcelain("!\taaaa:refs/heads/x\t[rejected] (something new)"),
                "",
                false,
                Some(1),
                &b
            )
            .expect("parses"),
            PushOutcome::Rejected(PushRejection::Other("[rejected] (something new)".into()))
        );
    }

    #[test]
    fn only_the_named_branch_is_read() {
        let out = "To o\n!\taaaa:refs/heads/other\t[rejected] (stale info)\n+\tbbbb:refs/heads/x\t1..2 (forced update)\nDone\n";
        assert_eq!(
            classify_push(out, "", true, Some(0), &branch("x")).expect("parses"),
            PushOutcome::Updated { forced: true }
        );
        // `refs/heads/x` must not match `refs/heads/xy`.
        let out = porcelain("+\tbbbb:refs/heads/xy\t1..2 (forced update)");
        assert!(classify_push(&out, "", true, Some(0), &branch("x")).is_err());
    }

    #[test]
    fn no_line_for_the_branch_is_a_failure_carrying_stderr() {
        let Err(GitError::Failed { stderr, code, .. }) = classify_push(
            "",
            "fatal: could not read from remote repository\n",
            false,
            Some(128),
            &branch("x"),
        ) else {
            panic!("expected a failure");
        };
        assert_eq!(stderr, "fatal: could not read from remote repository");
        assert_eq!(code, Some(128));
    }

    #[test]
    fn a_landed_line_on_a_failed_exit_is_not_success() {
        let out = porcelain("+\taaaa:refs/heads/x\t1..2 (forced update)");
        assert!(classify_push(&out, "hook failed", false, Some(1), &branch("x")).is_err());
    }
}
