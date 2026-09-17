//! Reading what `git fetch --porcelain --verbose` did to one tracking ref.
//!
//! `--porcelain` puts a stable, machine-readable record on **stdout**; the
//! human format goes to stderr, so the two never interleave. `--verbose` is not
//! decoration: without it git prints nothing at all for a ref that was already
//! up to date, which makes "unchanged" indistinguishable from "the refspec was
//! never processed" — and those demand opposite reactions.

use crate::{error::GitError, refs::Oid};

/// The first column of a `--porcelain` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchFlag {
    /// A space: an ordinary fast-forward.
    FastForward,
    /// `+`: a non-fast-forward update, which the `+` in the refspec allowed.
    Forced,
    /// `-`: the ref was pruned.
    Pruned,
    /// `*`: the ref is new locally.
    New,
    /// `!`: git declined the update.
    Rejected,
    /// `=`: nothing to do.
    UpToDate,
}

impl FetchFlag {
    fn from_char(ch: char) -> Option<Self> {
        Some(match ch {
            ' ' => Self::FastForward,
            '+' => Self::Forced,
            '-' => Self::Pruned,
            '*' => Self::New,
            '!' => Self::Rejected,
            '=' => Self::UpToDate,
            _ => return None,
        })
    }
}

/// One `<flag> <old> <new> <ref>` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchLine {
    pub flag: FetchFlag,
    pub old: Oid,
    pub new: Oid,
    pub reference: String,
}

/// What a fetch did to the ref that was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchOutcome {
    UpToDate,
    Updated {
        from: Oid,
        to: Oid,
        /// Whether the update rewrote history. Routine for pull request
        /// branches, and the reason the refspec carries a `+`.
        forced: bool,
    },
    /// The branch no longer exists on the remote — merged and deleted, or
    /// renamed. Not an error: it is the answer to the question, and the caller
    /// needs to stop offering to rebase onto it.
    Gone,
}

/// Parse the stdout of `git fetch --porcelain`.
///
/// The flag occupies column zero and the separator column one, so the line is
/// split at a fixed offset rather than by whitespace — a fast-forward's flag
/// *is* a space, and `split_whitespace` would swallow it.
pub fn parse_fetch_porcelain(stdout: &str) -> Result<Vec<FetchLine>, GitError> {
    let mut lines = Vec::new();
    for raw in stdout.lines() {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }

        let invalid = || GitError::Parse {
            what: "fetch --porcelain line",
            line: line.to_string(),
        };

        let mut chars = line.chars();
        let flag = chars
            .next()
            .and_then(FetchFlag::from_char)
            .ok_or_else(invalid)?;
        if chars.next() != Some(' ') {
            return Err(invalid());
        }

        let rest = &line[2..];
        let mut fields = rest.split(' ').filter(|field| !field.is_empty());
        let (Some(old), Some(new), Some(reference), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(invalid());
        };

        lines.push(FetchLine {
            flag,
            old: Oid::parse(old)?,
            new: Oid::parse(new)?,
            reference: reference.to_string(),
        });
    }
    Ok(lines)
}

/// git has no machine-readable signal for "that branch is not on the remote":
/// the refspec simply cannot be resolved and it exits 128. This is the one
/// place in the crate that reads git's prose, which is why every command runs
/// under `LC_ALL=C` — the message is fixed, and the match is on the part of it
/// that has not changed in a decade.
fn is_missing_remote_ref(stderr: &str) -> bool {
    let lowered = stderr.to_ascii_lowercase();
    lowered.contains("couldn't find remote ref") || lowered.contains("could not find remote ref")
}

/// Decide what a fetch did to `tracking_ref`.
///
/// A fetch that exits non-zero is a failure *unless* it is the specific "no
/// such branch on the remote" case, which is information rather than breakage.
pub fn classify_fetch(
    lines: &[FetchLine],
    tracking_ref: &str,
    success: bool,
    code: Option<i32>,
    stderr: &str,
) -> Result<FetchOutcome, GitError> {
    if !success {
        return if is_missing_remote_ref(stderr) {
            Ok(FetchOutcome::Gone)
        } else {
            Err(GitError::Failed {
                command: "fetch".to_string(),
                code,
                stderr: stderr.trim().to_string(),
            })
        };
    }

    // With `--verbose` every processed ref prints, so silence about the ref
    // means the refspec never reached it — a different bug from "unchanged",
    // and one that would otherwise be reported as success.
    let line = lines
        .iter()
        .find(|line| line.reference == tracking_ref)
        .ok_or_else(|| GitError::Parse {
            what: "fetch output that never mentioned the requested ref",
            line: tracking_ref.to_string(),
        })?;

    Ok(match line.flag {
        FetchFlag::UpToDate => FetchOutcome::UpToDate,
        FetchFlag::Pruned => FetchOutcome::Gone,
        FetchFlag::Rejected => {
            return Err(GitError::Failed {
                command: "fetch".to_string(),
                code,
                stderr: format!("git rejected the update to {tracking_ref}"),
            });
        }
        FetchFlag::FastForward | FetchFlag::New => FetchOutcome::Updated {
            from: line.old.clone(),
            to: line.new.clone(),
            forced: false,
        },
        FetchFlag::Forced => FetchOutcome::Updated {
            from: line.old.clone(),
            to: line.new.clone(),
            forced: true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACKING: &str = "refs/remotes/origin/main";
    const OLD: &str = "b19ada30e95dcdd0dbe0ed3b295c09361a756363";
    const NEW: &str = "a2463d3b48fea3c421d653d12260359fa6bbf03f";

    fn classify(stdout: &str) -> FetchOutcome {
        let lines = parse_fetch_porcelain(stdout).expect("parses");
        classify_fetch(&lines, TRACKING, true, Some(0), "").expect("classifies")
    }

    /// A fast-forward's flag is a literal space, so the line begins with two of
    /// them. Splitting on whitespace would lose the column entirely.
    #[test]
    fn a_fast_forward_is_flagged_by_a_leading_space() {
        assert_eq!(
            classify(&format!("  {OLD} {NEW} {TRACKING}\n")),
            FetchOutcome::Updated {
                from: Oid::parse(OLD).expect("valid"),
                to: Oid::parse(NEW).expect("valid"),
                forced: false,
            }
        );
    }

    /// Pull request branches are force-pushed as a matter of course, which is
    /// why the refspec carries a `+`; the caller wants to know it happened.
    #[test]
    fn a_forced_update_is_reported_as_forced() {
        assert_eq!(
            classify(&format!("+ {OLD} {NEW} {TRACKING}")),
            FetchOutcome::Updated {
                from: Oid::parse(OLD).expect("valid"),
                to: Oid::parse(NEW).expect("valid"),
                forced: true,
            }
        );
    }

    /// Only `--verbose` makes git print this line at all.
    #[test]
    fn an_unchanged_ref_still_prints_and_reads_as_up_to_date() {
        assert_eq!(
            classify(&format!("= {OLD} {OLD} {TRACKING}")),
            FetchOutcome::UpToDate
        );
    }

    #[test]
    fn a_new_tracking_ref_is_an_update_from_the_null_id() {
        let null = "0".repeat(40);
        let outcome = classify(&format!("* {null} {NEW} {TRACKING}"));
        let FetchOutcome::Updated { from, forced, .. } = outcome else {
            panic!("expected an update, got {outcome:?}");
        };
        assert!(from.is_null());
        assert!(!forced);
    }

    #[test]
    fn a_pruned_ref_is_gone() {
        let null = "0".repeat(40);
        assert_eq!(
            classify(&format!("- {OLD} {null} {TRACKING}")),
            FetchOutcome::Gone
        );
    }

    /// The branch was merged and deleted upstream. That is an answer, not a
    /// breakage, and the caller must stop offering to rebase onto it.
    #[test]
    fn a_branch_missing_from_the_remote_is_gone_rather_than_an_error() {
        let outcome = classify_fetch(
            &[],
            TRACKING,
            false,
            Some(128),
            "fatal: couldn't find remote ref refs/heads/feat/x\n",
        )
        .expect("a missing branch is information");
        assert_eq!(outcome, FetchOutcome::Gone);
    }

    #[test]
    fn any_other_failure_is_an_error_carrying_gits_words() {
        let err = classify_fetch(
            &[],
            TRACKING,
            false,
            Some(128),
            "fatal: 'origin' does not appear to be a git repository",
        );
        let Err(GitError::Failed { stderr, code, .. }) = err else {
            panic!("expected a failure, got {err:?}");
        };
        assert!(stderr.contains("does not appear to be a git repository"));
        assert_eq!(code, Some(128));
    }

    /// This is what `--verbose` buys: without it, a refspec that was silently
    /// dropped would be indistinguishable from one that was already current,
    /// and every divergence computed afterwards would be stale.
    #[test]
    fn a_ref_that_never_appears_in_the_output_is_an_error_not_up_to_date() {
        let err = classify_fetch(
            &parse_fetch_porcelain(&format!("= {OLD} {OLD} refs/remotes/origin/other"))
                .expect("parses"),
            TRACKING,
            true,
            Some(0),
            "",
        );
        assert!(matches!(err, Err(GitError::Parse { .. })), "{err:?}");
    }

    #[test]
    fn a_rejected_update_is_an_error() {
        let err = classify_fetch(
            &parse_fetch_porcelain(&format!("! {OLD} {NEW} {TRACKING}")).expect("parses"),
            TRACKING,
            true,
            Some(0),
            "",
        );
        assert!(matches!(err, Err(GitError::Failed { .. })), "{err:?}");
    }

    #[test]
    fn several_refs_are_parsed_and_only_the_requested_one_is_read() {
        let stdout = format!(
            "= {OLD} {OLD} refs/remotes/origin/other\n\
             + {OLD} {NEW} {TRACKING}\n"
        );
        let lines = parse_fetch_porcelain(&stdout).expect("parses");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].flag, FetchFlag::UpToDate);
        assert_eq!(lines[1].flag, FetchFlag::Forced);
        assert!(matches!(
            classify_fetch(&lines, TRACKING, true, Some(0), "").expect("classifies"),
            FetchOutcome::Updated { forced: true, .. }
        ));
    }

    #[test]
    fn malformed_output_is_rejected_rather_than_guessed_at() {
        for bad in [
            "x deadbeef deadbeef refs/remotes/origin/main",
            "=deadbeef deadbeef refs/remotes/origin/main",
            "= notanoid notanoid refs/remotes/origin/main",
            "= b19ada30e95dcdd0dbe0ed3b295c09361a756363 refs/remotes/origin/main",
        ] {
            assert!(parse_fetch_porcelain(bad).is_err(), "accepted `{bad}`");
        }
        assert!(parse_fetch_porcelain("\n\n").expect("parses").is_empty());
    }
}
