//! Everything a conflict resolver needs, gathered while an operation is
//! stopped.
//!
//! rostrum has no conflict editor. Under [`ConflictPolicy::Leave`] the
//! sequencer state stays on disk and someone else — a person in a terminal, or
//! a harness rostrum hands the problem to — finishes the job. [`ConflictContext`]
//! is what they are handed: which operation stopped, on which commit, which
//! paths are unmerged and *how*, the conflicted regions themselves, and the
//! commits on each side so the intent behind both can be read.
//!
//! Three decisions here are correctness points rather than preferences.
//!
//! **The regions come from the working-tree file, not from `git diff`.**
//! During a conflict `git diff` prints *combined* format for an unmerged path,
//! and its shape depends on `merge.conflictStyle`. The resolver opens the file
//! anyway, so it is handed exactly what it will see: the file, with its
//! markers, and 1-based line numbers that match an editor's gutter.
//!
//! **The kind of conflict comes from the `u` records of `status`, not from
//! `diff --name-only --diff-filter=U`.** `UU` and `DU` both list the path, but
//! one is resolved with `git add` and the other with `git rm`; the diff filter
//! throws that distinction away.
//!
//! **Marker orientation flips between rebase and merge.** In a rebase HEAD is
//! the *target* and each replayed commit is the incoming side, so
//! `<<<<<<< HEAD` holds the base's version and `>>>>>>>` holds the branch's
//! own change; in a merge it is the reverse. Getting this backwards is the
//! single most common cause of a wrong resolution, which is why
//! [`ConflictContext::marker_sides`] is a method rather than a comment.
//!
//! Everything in this module is a pure function over a string git printed or a
//! file read from disk. The I/O lives in [`crate::repo`].
//!
//! [`ConflictPolicy::Leave`]: crate::ConflictPolicy::Leave

use crate::{
    error::GitError,
    refs::{BranchName, Oid},
};

mod regions;

pub use regions::{RegionBudget, body_from_file, extract_conflict_regions};

/// Everything a conflict resolver needs, gathered while the operation is
/// stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictContext {
    pub operation: StoppedOperation,
    /// The pull request branch (`refs/heads/<branch>`).
    pub branch: BranchName,
    /// The fully-qualified ref the operation was pointed at, e.g.
    /// `refs/remotes/origin/main`.
    pub target: String,
    /// Where HEAD is right now.
    pub head: Oid,
    pub paths: Vec<ConflictedPath>,
    /// On `refs/heads/<branch>`, not on `target`.
    pub branch_commits: CommitList,
    /// On `target`, not on `refs/heads/<branch>`.
    pub target_commits: CommitList,
    /// git's own combined output from the stop, or empty on a later load.
    ///
    /// [`Repo::conflict_context`](crate::Repo::conflict_context) reads the
    /// repository, which no longer has this text, so it always leaves the field
    /// empty; a caller holding the [`Conflict`](crate::Conflict) that was just
    /// reported fills it in from [`Conflict::message`](crate::Conflict::message).
    pub git_message: String,
    pub caps: Caps,
}

/// Which operation is waiting, and where it stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoppedOperation {
    Rebase {
        /// `(current, total)` from `rebase-merge/msgnum` and `/end`, when
        /// present.
        step: Option<(u32, u32)>,
        /// `REBASE_HEAD`; absent when the stop was not on a commit — a failed
        /// `--exec`, for instance.
        applying: Option<CommitSummary>,
        onto: Option<Oid>,
    },
    Merge {
        merging: Oid,
    },
}

/// One unmerged path and what there is to show for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictedPath {
    pub path: String,
    pub kind: ConflictKind,
    pub body: ConflictBody,
}

/// From the XY field of a porcelain-v2 `u` record.
///
/// "Us" is HEAD's side, exactly as git names it — which during a rebase is the
/// *target*, not the branch being rebased. [`ConflictContext::marker_sides`]
/// says which is which for the operation in hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// `UU`
    BothModified,
    /// `AA`
    BothAdded,
    /// `DU`
    DeletedByUs,
    /// `UD`
    DeletedByThem,
    /// `AU`
    AddedByUs,
    /// `UA`
    AddedByThem,
    /// `DD`
    BothDeleted,
}

impl ConflictKind {
    pub fn from_xy(xy: &str) -> Option<Self> {
        Some(match xy {
            "UU" => Self::BothModified,
            "AA" => Self::BothAdded,
            "DU" => Self::DeletedByUs,
            "UD" => Self::DeletedByThem,
            "AU" => Self::AddedByUs,
            "UA" => Self::AddedByThem,
            "DD" => Self::BothDeleted,
            _ => return None,
        })
    }

    /// Short human phrase: "both modified", "deleted by us", ...
    pub fn describe(self) -> &'static str {
        match self {
            Self::BothModified => "both modified",
            Self::BothAdded => "both added",
            Self::DeletedByUs => "deleted by us",
            Self::DeletedByThem => "deleted by them",
            Self::AddedByUs => "added by us",
            Self::AddedByThem => "added by them",
            Self::BothDeleted => "both deleted",
        }
    }

    /// Whether the resolver marks it with `git rm` rather than `git add`.
    pub fn resolves_by_removal(self) -> bool {
        matches!(
            self,
            Self::DeletedByUs | Self::DeletedByThem | Self::BothDeleted
        )
    }

    /// Whether git wrote a merged copy with markers into the working tree.
    /// Only the two-sided content conflicts have one; every other kind has a
    /// single side's file, or nothing, and reading it would show no markers.
    pub fn has_marked_file(self) -> bool {
        matches!(self, Self::BothModified | Self::BothAdded)
    }
}

/// What there is to show for a conflicted path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictBody {
    Regions {
        regions: Vec<ConflictRegion>,
        truncated: bool,
    },
    Binary,
    /// Nothing on disk to show (delete conflicts, or unreadable).
    Absent {
        reason: String,
    },
    /// Beyond [`Caps::max_files_with_regions`] or the total byte cap; listed by
    /// name only.
    Omitted,
}

/// A conflict block with its surrounding context, as it sits in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRegion {
    /// 1-based, inclusive.
    pub first_line: u32,
    pub last_line: u32,
    /// The lines `first_line..=last_line`, joined with `\n` and with no
    /// trailing newline. A CRLF file is normalised to LF here so the text can
    /// be quoted verbatim.
    pub text: String,
}

/// One commit, as `--format=%H%x1f%an%x1f%aI%x1f%B` prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: Oid,
    pub author: String,
    /// ISO-8601 as git's `%aI` prints it.
    pub date: String,
    /// Full message, `%B`, with the trailing newline git appends removed.
    pub message: String,
}

impl CommitSummary {
    /// First line of the message.
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
}

/// The commits on one side of the divergence, capped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitList {
    pub commits: Vec<CommitSummary>,
    /// The true count, from `rev-list --count`; never inferred from the length
    /// of `commits`, so truncation is a fact rather than a guess.
    pub total: u32,
}

impl CommitList {
    pub fn truncated(&self) -> bool {
        self.total as usize > self.commits.len()
    }
}

/// How much of the repository to put in the context.
///
/// Every cap surfaces as an explicit value in the result — [`ConflictBody::Regions`]'
/// `truncated`, [`ConflictBody::Omitted`], [`CommitList::truncated`] — rather
/// than silently shortening something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Lines of context around each conflict block.
    pub context_lines: u32,
    /// Commits listed per side.
    pub max_commits: u32,
    /// Region text per file.
    pub max_region_bytes: usize,
    /// Region text across all files.
    pub max_total_region_bytes: usize,
    /// Files whose regions are read at all.
    pub max_files_with_regions: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            context_lines: 3,
            max_commits: 50,
            max_region_bytes: 32 * 1024,
            max_total_region_bytes: 192 * 1024,
            max_files_with_regions: 40,
        }
    }
}

/// Which side of the conflict markers each ref is on. Flips between rebase
/// and merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Branch,
    Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerSides {
    /// What `<<<<<<< HEAD` contains.
    pub head_is: Side,
    /// What `>>>>>>> ...` contains.
    pub incoming_is: Side,
}

/// The commands a resolver runs, spelled out so the instructions and the
/// operation cannot drift apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commands {
    /// e.g. `git add <path>` — the resolver substitutes the path.
    pub mark_resolved: String,
    /// e.g. `git rm <path>`.
    pub mark_removed: String,
    /// e.g. `GIT_EDITOR=true git rebase --continue`.
    pub continue_: String,
    /// e.g. `git rebase --abort`.
    pub abort: String,
}

impl ConflictContext {
    pub fn is_rebase(&self) -> bool {
        matches!(self.operation, StoppedOperation::Rebase { .. })
    }

    /// Rebase: `head_is = Target`, `incoming_is = Branch`. Merge: the reverse.
    ///
    /// A rebase checks the target out and replays the branch's commits on top,
    /// so HEAD *is* the target while it runs. A merge stays on the branch and
    /// brings the target in.
    pub fn marker_sides(&self) -> MarkerSides {
        if self.is_rebase() {
            MarkerSides {
                head_is: Side::Target,
                incoming_is: Side::Branch,
            }
        } else {
            MarkerSides {
                head_is: Side::Branch,
                incoming_is: Side::Target,
            }
        }
    }

    /// The commands to finish or give up.
    ///
    /// `GIT_EDITOR=true`, where [`crate::command`] forces `false` on every git
    /// rostrum itself runs. The two are opposite on purpose: rostrum's own git
    /// must fail loudly rather than accept a message it never reviewed, but the
    /// resolver is finishing a commit whose message is already written and has
    /// to accept it non-interactively, or `--continue` opens an editor in a
    /// session nobody is watching.
    pub fn commands(&self) -> Commands {
        let subcommand = if self.is_rebase() { "rebase" } else { "merge" };
        Commands {
            mark_resolved: "git add <path>".to_string(),
            mark_removed: "git rm <path>".to_string(),
            continue_: format!("GIT_EDITOR=true git {subcommand} --continue"),
            abort: format!("git {subcommand} --abort"),
        }
    }
}

/// The `--format` every commit here is printed with: hash, author name, ISO
/// author date, raw body, separated by ASCII unit separator. `%B` can contain
/// anything but NUL, so records are separated by `-z`'s NUL and fields by
/// `\x1f`, which no commit message written by a keyboard contains.
pub const LOG_FORMAT: &str = "%H%x1f%an%x1f%aI%x1f%B";

/// Parse the `u` records of `status --porcelain=v2 -z`.
///
/// The grammar from git-status(1) is
/// `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`: ten fixed fields
/// and then the path, which may itself contain spaces, so the record is split
/// at most eleven ways. In `-z` mode the record ends at NUL and nothing in it
/// is quoted.
///
/// The output has been lossily decoded by [`crate::command`], which is fine
/// for counting but not for a path that is about to be opened: a path with a
/// replacement character in it is not the path on disk, so it is rejected
/// rather than read wrong.
pub fn parse_unmerged_v2(output: &str) -> Result<Vec<(ConflictKind, String)>, GitError> {
    let mut fields = output.split('\0');
    let mut unmerged = Vec::new();

    while let Some(record) = fields.next() {
        match record.as_bytes().first() {
            Some(b'u') => {
                let parts: Vec<&str> = record.splitn(11, ' ').collect();
                let [_, xy, _sub, _m1, _m2, _m3, _mw, _h1, _h2, _h3, path] = parts.as_slice()
                else {
                    return Err(GitError::Parse {
                        what: "unmerged status record",
                        line: record.to_string(),
                    });
                };
                let kind = ConflictKind::from_xy(xy).ok_or_else(|| GitError::Parse {
                    what: "unmerged status record kind",
                    line: record.to_string(),
                })?;
                if path.contains('\u{FFFD}') {
                    return Err(GitError::Parse {
                        what: "unmerged path that is not valid UTF-8",
                        line: record.to_string(),
                    });
                }
                unmerged.push((kind, (*path).to_string()));
            }
            // A rename carries its original path in the next NUL field, which
            // could begin with `u `. Consume it, as `parse_status_v2` does.
            Some(b'2') => {
                fields.next();
            }
            _ => {}
        }
    }

    Ok(unmerged)
}

/// Parse `rebase-merge/msgnum` and `rebase-merge/end` (or `rebase-apply/next`
/// and `/last`) into `(current, total)`. Either file being anything but a
/// number means the answer is unknown, not zero.
pub fn parse_rebase_progress(msgnum: &str, end: &str) -> Option<(u32, u32)> {
    Some((msgnum.trim().parse().ok()?, end.trim().parse().ok()?))
}

/// Parse `log -z --format=%H%x1f%an%x1f%aI%x1f%B` output.
///
/// Records are NUL-separated; a trailing NUL leaves an empty final record,
/// which is skipped. Fields are split at most four ways so a message containing
/// `\x1f` — unlikely, but not impossible — stays whole. A record with fewer
/// than four fields is a shape this crate did not ask for.
pub fn parse_log_z(output: &str) -> Result<Vec<CommitSummary>, GitError> {
    output
        .split('\0')
        .filter(|record| !record.is_empty())
        .map(parse_commit_record)
        .collect()
}

fn parse_commit_record(record: &str) -> Result<CommitSummary, GitError> {
    let mut parts = record.splitn(4, '\x1f');
    let (Some(oid), Some(author), Some(date), Some(message)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(GitError::Parse {
            what: "commit record",
            line: record.to_string(),
        });
    };
    Ok(CommitSummary {
        oid: Oid::parse(oid.trim())?,
        author: author.to_string(),
        date: date.to_string(),
        message: message.trim_end_matches('\n').to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: char) -> Oid {
        Oid::parse(std::iter::repeat_n(byte, 40).collect::<String>()).expect("valid")
    }

    fn z(records: &[&str]) -> String {
        records.join("\0")
    }

    fn u(xy: &str, path: &str) -> String {
        format!("u {xy} N... 100644 100644 100644 100644 5626abf 7c30781 8c21240 {path}")
    }

    fn context(operation: StoppedOperation) -> ConflictContext {
        ConflictContext {
            operation,
            branch: BranchName::new("feat/x").expect("valid"),
            target: "refs/remotes/origin/main".to_string(),
            head: oid('a'),
            paths: Vec::new(),
            branch_commits: CommitList {
                commits: Vec::new(),
                total: 0,
            },
            target_commits: CommitList {
                commits: Vec::new(),
                total: 0,
            },
            git_message: String::new(),
            caps: Caps::default(),
        }
    }

    fn rebase() -> ConflictContext {
        context(StoppedOperation::Rebase {
            step: None,
            applying: None,
            onto: None,
        })
    }

    fn merge() -> ConflictContext {
        context(StoppedOperation::Merge { merging: oid('b') })
    }

    /// Every XY git documents for an unmerged entry, and what each resolves
    /// with. `DU`/`UD`/`DD` need `git rm`; the rest `git add`.
    #[test]
    fn every_unmerged_kind_is_read_from_its_xy() {
        let cases = [
            ("UU", ConflictKind::BothModified, false),
            ("AA", ConflictKind::BothAdded, false),
            ("DU", ConflictKind::DeletedByUs, true),
            ("UD", ConflictKind::DeletedByThem, true),
            ("AU", ConflictKind::AddedByUs, false),
            ("UA", ConflictKind::AddedByThem, false),
            ("DD", ConflictKind::BothDeleted, true),
        ];
        for (xy, kind, removal) in cases {
            assert_eq!(ConflictKind::from_xy(xy), Some(kind), "{xy}");
            assert_eq!(kind.resolves_by_removal(), removal, "{xy}");
            let parsed = parse_unmerged_v2(&z(&[&u(xy, "f.txt")])).expect("parses");
            assert_eq!(parsed, vec![(kind, "f.txt".to_string())]);
        }
        assert!(ConflictKind::BothModified.has_marked_file());
        assert!(ConflictKind::BothAdded.has_marked_file());
        assert!(!ConflictKind::DeletedByUs.has_marked_file());
        assert!(!ConflictKind::AddedByThem.has_marked_file());
    }

    /// The path is the eleventh field and everything after it, which is what
    /// a bounded split gets right and a plain split gets wrong.
    #[test]
    fn a_path_with_spaces_is_kept_whole() {
        let parsed = parse_unmerged_v2(&z(&[&u("UU", "src/my file name.rs")])).expect("parses");
        assert_eq!(parsed[0].1, "src/my file name.rs");
    }

    /// Headers, ordinary changes, untracked files and a rename's original
    /// path — even one that starts with `u ` — are not unmerged entries.
    #[test]
    fn records_that_are_not_unmerged_are_ignored() {
        let parsed = parse_unmerged_v2(&z(&[
            "# branch.oid 3144d3a2b3d388fd420dd627ff39e7f7bf036ac6",
            "# branch.head (detached)",
            "1 .M N... 100644 100644 100644 5626abf 5626abf other.txt",
            "2 R. N... 100644 100644 100644 7898192 7898192 R100 b.txt",
            "u renamed.txt",
            "? untracked.txt",
            &u("UU", "f.txt"),
            "",
        ]))
        .expect("parses");
        assert_eq!(
            parsed,
            vec![(ConflictKind::BothModified, "f.txt".to_string())]
        );
    }

    #[test]
    fn a_garbage_kind_or_short_record_is_rejected() {
        assert!(matches!(
            parse_unmerged_v2(&u("XX", "f.txt")),
            Err(GitError::Parse { .. })
        ));
        assert!(matches!(
            parse_unmerged_v2("u UU N... f.txt"),
            Err(GitError::Parse { .. })
        ));
    }

    /// A lossily decoded path is not the path on disk; reading it would open
    /// the wrong file or none.
    #[test]
    fn a_path_that_did_not_decode_is_rejected_rather_than_opened() {
        assert!(matches!(
            parse_unmerged_v2(&u("UU", "caf\u{FFFD}.txt")),
            Err(GitError::Parse { .. })
        ));
    }

    #[test]
    fn rebase_progress_is_two_numbers_or_nothing() {
        assert_eq!(parse_rebase_progress("2\n", "5\n"), Some((2, 5)));
        assert_eq!(parse_rebase_progress("2", ""), None);
        assert_eq!(parse_rebase_progress("", "5"), None);
        assert_eq!(parse_rebase_progress("x", "5"), None);
    }

    #[test]
    fn an_empty_log_is_no_commits() {
        assert_eq!(parse_log_z("").expect("parses"), vec![]);
    }

    /// `%B` keeps paragraphs; a bounded split keeps them in the message.
    #[test]
    fn a_commit_with_a_multi_paragraph_body_is_read_whole() {
        let record = format!(
            "{}\x1fAda\x1f2026-09-21T21:32:24-07:00\x1ffirst line\n\nbody para one\n\nbody para two\n",
            "a".repeat(40)
        );
        let commits = parse_log_z(&record).expect("parses");
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].oid, oid('a'));
        assert_eq!(commits[0].author, "Ada");
        assert_eq!(commits[0].date, "2026-09-21T21:32:24-07:00");
        assert_eq!(
            commits[0].message,
            "first line\n\nbody para one\n\nbody para two"
        );
        assert_eq!(commits[0].subject(), "first line");
    }

    /// `-z` terminates every record, so the output ends in NUL and the last
    /// "record" is empty.
    #[test]
    fn a_trailing_nul_does_not_add_an_empty_commit() {
        let output = format!(
            "{}\x1fAda\x1fd1\x1fone\n\0{}\x1fBob\x1fd2\x1ftwo\n\0",
            "a".repeat(40),
            "b".repeat(40)
        );
        let commits = parse_log_z(&output).expect("parses");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[1].subject(), "two");
    }

    #[test]
    fn a_short_commit_record_is_rejected() {
        let short = format!("{}\x1fAda\x1fd1", "a".repeat(40));
        assert!(matches!(parse_log_z(&short), Err(GitError::Parse { .. })));
        assert!(parse_log_z("nonsense\x1fa\x1fb\x1fc").is_err(), "a bad oid");
    }

    #[test]
    fn a_commit_list_is_truncated_only_when_the_true_count_says_so() {
        let list = CommitList {
            commits: Vec::new(),
            total: 0,
        };
        assert!(!list.truncated());
        let list = CommitList {
            commits: Vec::new(),
            total: 3,
        };
        assert!(list.truncated());
    }

    /// The flip this module exists to spell out.
    #[test]
    fn marker_sides_flip_between_rebase_and_merge() {
        assert!(rebase().is_rebase());
        assert_eq!(
            rebase().marker_sides(),
            MarkerSides {
                head_is: Side::Target,
                incoming_is: Side::Branch,
            }
        );
        assert!(!merge().is_rebase());
        assert_eq!(
            merge().marker_sides(),
            MarkerSides {
                head_is: Side::Branch,
                incoming_is: Side::Target,
            }
        );
    }

    /// `true`, not `false`: the resolver has to accept the message.
    #[test]
    fn commands_name_the_operation_and_a_permissive_editor() {
        assert_eq!(
            rebase().commands(),
            Commands {
                mark_resolved: "git add <path>".to_string(),
                mark_removed: "git rm <path>".to_string(),
                continue_: "GIT_EDITOR=true git rebase --continue".to_string(),
                abort: "git rebase --abort".to_string(),
            }
        );
        let merge = merge().commands();
        assert_eq!(merge.continue_, "GIT_EDITOR=true git merge --continue");
        assert_eq!(merge.abort, "git merge --abort");
    }

    #[test]
    fn the_default_caps_are_the_documented_ones() {
        assert_eq!(
            Caps::default(),
            Caps {
                context_lines: 3,
                max_commits: 50,
                max_region_bytes: 32 * 1024,
                max_total_region_bytes: 192 * 1024,
                max_files_with_regions: 40,
            }
        );
    }
}
