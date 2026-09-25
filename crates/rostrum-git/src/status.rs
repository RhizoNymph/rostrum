//! What the repository looks like right now, and the parsers that read it.
//!
//! One `git status --porcelain=v2 --branch -z` answers four questions at once:
//! which branch HEAD is on, how dirty the worktree is, whether anything is
//! unmerged, and how far the branch has diverged from its upstream. Doing it in
//! one spawn also means the four answers are consistent with each other, which
//! separate calls could not promise.
//!
//! Everything here is a pure function over git's output. The I/O lives in
//! [`crate::repo`].

use rostrum_core::Divergence;

use crate::{
    error::GitError,
    refs::{BranchName, Oid},
};

/// Where HEAD points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch {
        name: BranchName,
        oid: Oid,
        upstream: Option<Upstream>,
    },
    /// No branch. Normal during a rebase, and a reason to refuse to start one.
    Detached { oid: Oid },
    /// A branch that exists in HEAD but has no commit yet — a fresh `git init`,
    /// or `git checkout --orphan`. It has a name but no object id, so it cannot
    /// be an argument to anything.
    Unborn { name: BranchName },
}

impl Head {
    pub fn oid(&self) -> Option<&Oid> {
        match self {
            Self::Branch { oid, .. } | Self::Detached { oid } => Some(oid),
            Self::Unborn { .. } => None,
        }
    }

    pub fn branch(&self) -> Option<&BranchName> {
        match self {
            Self::Branch { name, .. } | Self::Unborn { name } => Some(name),
            Self::Detached { .. } => None,
        }
    }

    pub fn upstream(&self) -> Option<&Upstream> {
        match self {
            Self::Branch { upstream, .. } => upstream.as_ref(),
            _ => None,
        }
    }
}

/// The configured upstream of the current branch.
///
/// `divergence` is `None` when the upstream is configured but its tracking ref
/// does not exist locally — a branch pushed with `-u` in a clone that has never
/// fetched it back. That is emphatically not the same as having no upstream,
/// and collapsing the two would make rostrum offer a comparison against
/// nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    /// As git prints it, e.g. `origin/main`. Not split into a [`crate::RemoteRef`]:
    /// a remote name and a branch name are separated by a `/` that also appears
    /// inside branch names, so the split is ambiguous without asking git.
    pub name: String,
    pub divergence: Option<Divergence>,
}

/// How dirty the worktree is, in counts rather than a bool so the reason a
/// button is greyed out can say *why*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worktree {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

impl Worktree {
    /// Whether anything at all would stop a clean checkout.
    ///
    /// Untracked files are deliberately excluded: git does not refuse a rebase
    /// or merge because of them, and `--autostash` does not stash them either,
    /// so counting them would block operations that would have succeeded.
    pub fn is_dirty(&self) -> bool {
        self.staged > 0 || self.unstaged > 0 || self.conflicted > 0
    }

    pub fn has_conflicts(&self) -> bool {
        self.conflicted > 0
    }

    pub fn is_clean(&self) -> bool {
        *self == Self::default()
    }
}

/// A multi-step operation git has stopped in the middle of.
///
/// Derived from the presence of per-worktree state files rather than from
/// `status`, which reports only some of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InProgress {
    /// The merge-backed rebase, which is the default.
    Rebase,
    /// The patch-backed rebase (`rebase --apply`).
    RebaseApply,
    /// A genuine `git am`. It shares `rebase-apply/` with `rebase --apply` and
    /// is distinguished only by the `applying` file inside it — getting this
    /// wrong would offer a `rebase --abort` that cannot work.
    Am,
    Merge,
    CherryPick,
    Revert,
    Bisect,
}

impl InProgress {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rebase | Self::RebaseApply => "a rebase is in progress",
            Self::Am => "`git am` is in progress",
            Self::Merge => "a merge is in progress",
            Self::CherryPick => "a cherry-pick is in progress",
            Self::Revert => "a revert is in progress",
            Self::Bisect => "a bisect is in progress",
        }
    }
}

/// Which per-worktree state files exist.
///
/// Split out from the filesystem probe so the precedence rules below are a pure
/// function of seven booleans.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateFiles {
    pub rebase_merge: bool,
    pub rebase_apply: bool,
    /// `rebase-apply/applying`, present only for `git am`.
    pub rebase_applying: bool,
    pub merge_head: bool,
    pub cherry_pick_head: bool,
    pub revert_head: bool,
    pub bisect_log: bool,
}

/// Which operation the present state files describe.
///
/// Order matters. `rebase-apply/applying` must be tested before `rebase-apply`
/// or an interrupted `git am` reads as a rebase, and a rebase must be tested
/// before `MERGE_HEAD` because a conflicted rebase step writes one too.
pub fn in_progress(files: StateFiles) -> Option<InProgress> {
    if files.rebase_merge {
        return Some(InProgress::Rebase);
    }
    if files.rebase_apply {
        return Some(if files.rebase_applying {
            InProgress::Am
        } else {
            InProgress::RebaseApply
        });
    }
    if files.merge_head {
        return Some(InProgress::Merge);
    }
    if files.cherry_pick_head {
        return Some(InProgress::CherryPick);
    }
    if files.revert_head {
        return Some(InProgress::Revert);
    }
    if files.bisect_log {
        return Some(InProgress::Bisect);
    }
    None
}

/// Everything [`crate::Repo::status`] reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoStatus {
    pub head: Head,
    pub worktree: Worktree,
    pub in_progress: Option<InProgress>,
}

impl RepoStatus {
    pub fn oid(&self) -> Option<&Oid> {
        self.head.oid()
    }

    pub fn branch(&self) -> Option<&BranchName> {
        self.head.branch()
    }

    pub fn upstream(&self) -> Option<&Upstream> {
        self.head.upstream()
    }

    pub fn has_conflicts(&self) -> bool {
        self.worktree.has_conflicts()
    }
}

/// `# branch.oid` for a branch with no commits yet.
const INITIAL: &str = "(initial)";
/// `# branch.head` when HEAD is not on a branch.
const DETACHED: &str = "(detached)";

/// Parse `status --porcelain=v2 --branch -z`.
///
/// The input is the NUL-separated output with its trailing NUL removed or not;
/// empty fields are skipped either way. `-z` is not a nicety: with the default
/// `core.quotePath` git C-quotes any non-ASCII path, and a path containing a
/// literal newline would split one record into two under a line-oriented parse.
///
/// Paths are never inspected here, only counted, so it is safe for the caller
/// to have lossily decoded git's bytes: the replacement character cannot
/// introduce or remove a NUL.
pub fn parse_status_v2(output: &str) -> Result<(Head, Worktree), GitError> {
    let mut fields = output.split('\0');

    let mut oid_field: Option<&str> = None;
    let mut head_field: Option<&str> = None;
    let mut upstream_field: Option<&str> = None;
    let mut ab_field: Option<&str> = None;
    let mut worktree = Worktree::default();

    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        match record.as_bytes()[0] {
            b'#' => match record.split_once(' ') {
                Some(("#", rest)) => {
                    let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
                    match key {
                        "branch.oid" => oid_field = Some(value),
                        "branch.head" => head_field = Some(value),
                        "branch.upstream" => upstream_field = Some(value),
                        "branch.ab" => ab_field = Some(value),
                        _ => {}
                    }
                }
                _ => {
                    return Err(GitError::Parse {
                        what: "status header",
                        line: record.to_string(),
                    });
                }
            },
            b'1' => count_changed(record, &mut worktree)?,
            b'2' => {
                count_changed(record, &mut worktree)?;
                // A rename or copy carries the original path in its own NUL
                // field. Consuming it here is what stops it being read as the
                // next record.
                if fields.next().is_none() {
                    return Err(GitError::Parse {
                        what: "rename entry with no original path",
                        line: record.to_string(),
                    });
                }
            }
            b'u' => worktree.conflicted += 1,
            b'?' => worktree.untracked += 1,
            b'!' => {}
            _ => {
                return Err(GitError::Parse {
                    what: "status record",
                    line: record.to_string(),
                });
            }
        }
    }

    let head = assemble_head(oid_field, head_field, upstream_field, ab_field)?;
    Ok((head, worktree))
}

/// Tally one `1` or `2` record. `<XY>` is the second space-separated field:
/// `X` is the change staged in the index, `Y` the change in the worktree, and
/// `.` means unchanged on that side.
fn count_changed(record: &str, worktree: &mut Worktree) -> Result<(), GitError> {
    let xy = record
        .split(' ')
        .nth(1)
        .filter(|xy| xy.len() == 2)
        .ok_or(GitError::Parse {
            what: "status change record",
            line: record.to_string(),
        })?;
    let mut chars = xy.chars();
    if chars.next() != Some('.') {
        worktree.staged += 1;
    }
    if chars.next() != Some('.') {
        worktree.unstaged += 1;
    }
    Ok(())
}

fn assemble_head(
    oid_field: Option<&str>,
    head_field: Option<&str>,
    upstream_field: Option<&str>,
    ab_field: Option<&str>,
) -> Result<Head, GitError> {
    let oid_field = oid_field.ok_or(GitError::Parse {
        what: "status output with no `# branch.oid`",
        line: String::new(),
    })?;
    let head_field = head_field.ok_or(GitError::Parse {
        what: "status output with no `# branch.head`",
        line: String::new(),
    })?;

    if oid_field == INITIAL {
        if head_field == DETACHED {
            return Err(GitError::Parse {
                what: "status output claiming HEAD is both unborn and detached",
                line: format!("{oid_field} {head_field}"),
            });
        }
        return Ok(Head::Unborn {
            name: BranchName::new(head_field)?,
        });
    }

    let oid = Oid::parse(oid_field)?;
    if head_field == DETACHED {
        return Ok(Head::Detached { oid });
    }

    // `# branch.ab` is absent whenever the upstream's tracking ref is missing,
    // so the two fields are read independently: an upstream with no counts is a
    // real, reportable state.
    let upstream = upstream_field.map(|name| Upstream {
        name: name.to_string(),
        divergence: ab_field.and_then(parse_ab),
    });

    Ok(Head::Branch {
        name: BranchName::new(head_field)?,
        oid,
        upstream,
    })
}

/// Parse the value of `# branch.ab`, e.g. `+3 -4`.
///
/// Returns `None` for anything that is not two signed counts. git prints
/// `+? -?` when `status.aheadBehind` is off or the comparison could not be
/// made, and that must read as "unknown", never as "level with upstream".
pub fn parse_ab(value: &str) -> Option<Divergence> {
    let (ahead, behind) = value.trim().split_once(' ')?;
    let ahead = ahead.strip_prefix('+')?.parse().ok()?;
    let behind = behind.strip_prefix('-')?.parse().ok()?;
    Some(Divergence::new(ahead, behind))
}

/// Parse `rev-list --left-right --count <a>...<b>`, which prints two
/// tab-separated counts: commits reachable only from the left, then only from
/// the right.
pub fn parse_left_right_count(output: &str) -> Result<Divergence, GitError> {
    let invalid = || GitError::Parse {
        what: "rev-list --left-right --count output",
        line: output.trim_end().to_string(),
    };
    let (left, right) = output.trim().split_once('\t').ok_or_else(invalid)?;
    Ok(Divergence::new(
        left.trim().parse().map_err(|_| invalid())?,
        right.trim().parse().map_err(|_| invalid())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real output, with the NULs `-z` actually emits.
    fn z(records: &[&str]) -> String {
        records.join("\0")
    }

    #[test]
    fn a_clean_branch_with_an_upstream_reads_every_header() {
        let (head, worktree) = parse_status_v2(&z(&[
            "# branch.oid b87d11011148b979094156442b1e1d8d9dbed5ff",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -5",
            "",
        ]))
        .expect("parses");

        let Head::Branch {
            name,
            oid,
            upstream,
        } = head
        else {
            panic!("expected a branch, got {head:?}");
        };
        assert_eq!(name.as_str(), "main");
        assert_eq!(oid.as_str(), "b87d11011148b979094156442b1e1d8d9dbed5ff");
        assert_eq!(
            upstream,
            Some(Upstream {
                name: "origin/main".to_string(),
                divergence: Some(Divergence::new(2, 5)),
            })
        );
        assert!(worktree.is_clean());
    }

    /// The case that must not collapse: an upstream is configured, but its
    /// tracking ref has never been fetched, so git prints no `# branch.ab`.
    /// "Configured but unknown" and "no upstream at all" lead to different UI.
    #[test]
    fn an_upstream_with_no_counts_is_an_upstream_with_an_unknown_divergence() {
        let (head, _) = parse_status_v2(&z(&[
            "# branch.oid b87d11011148b979094156442b1e1d8d9dbed5ff",
            "# branch.head feat/x",
            "# branch.upstream origin/feat/x",
        ]))
        .expect("parses");

        assert_eq!(
            head.upstream(),
            Some(&Upstream {
                name: "origin/feat/x".to_string(),
                divergence: None,
            })
        );
    }

    #[test]
    fn a_branch_with_no_upstream_has_none() {
        let (head, _) = parse_status_v2(&z(&[
            "# branch.oid b87d11011148b979094156442b1e1d8d9dbed5ff",
            "# branch.head local-only",
        ]))
        .expect("parses");
        assert_eq!(head.upstream(), None);
    }

    /// `+? -?` is git saying it did not compute the counts. Reading it as
    /// `+0 -0` would tell the user their branch is level when it may not be.
    #[test]
    fn unknown_counts_are_not_zero_counts() {
        assert_eq!(parse_ab("+? -?"), None);
        assert_eq!(parse_ab(""), None);
        assert_eq!(parse_ab("+3"), None);
        assert_eq!(parse_ab("3 -4"), None);
        assert_eq!(parse_ab("+3 4"), None);
        assert_eq!(parse_ab("+0 -0"), Some(Divergence::IDENTICAL));
        assert_eq!(parse_ab("+12 -340"), Some(Divergence::new(12, 340)));
    }

    #[test]
    fn a_fresh_repository_has_an_unborn_head() {
        let (head, _) =
            parse_status_v2(&z(&["# branch.oid (initial)", "# branch.head main"])).expect("parses");
        let Head::Unborn { name } = head else {
            panic!("expected unborn, got {head:?}");
        };
        assert_eq!(name.as_str(), "main");
        assert_eq!(
            Head::Unborn {
                name: BranchName::new("main").expect("valid")
            }
            .oid(),
            None,
            "an unborn branch has no object id to pass to anything"
        );
    }

    /// HEAD is detached for the whole of a merge-backed rebase, so this is the
    /// shape `status` takes while a conflict is waiting.
    #[test]
    fn a_detached_head_has_an_oid_and_no_branch() {
        let (head, worktree) = parse_status_v2(&z(&[
            "# branch.oid 3144d3a2b3d388fd420dd627ff39e7f7bf036ac6",
            "# branch.head (detached)",
            "u UU N... 100644 100644 100644 100644 5626abf 7c30781 8c21240 f.txt",
        ]))
        .expect("parses");

        assert!(matches!(head, Head::Detached { .. }));
        assert_eq!(head.branch(), None);
        assert_eq!(worktree.conflicted, 1);
        assert!(worktree.has_conflicts());
    }

    /// A rename puts the original path in its own NUL field. Failing to consume
    /// it would parse `a.txt` as a record and reject the whole output.
    #[test]
    fn a_rename_entrys_original_path_is_consumed_not_parsed() {
        let (_, worktree) = parse_status_v2(&z(&[
            "# branch.oid d0ce0b6d30bd333410c8bec1d69438673c73097a",
            "# branch.head main",
            "2 R. N... 100644 100644 100644 7898192 7898192 R100 b.txt",
            "a.txt",
            "1 AM N... 000000 100644 100644 0000000 f2ad6c7 c.txt",
            "? untracked.txt",
            "! ignored.txt",
        ]))
        .expect("parses");

        assert_eq!(
            worktree,
            Worktree {
                // The rename is staged; `c.txt` is staged *and* modified after.
                staged: 2,
                unstaged: 1,
                untracked: 1,
                conflicted: 0,
            }
        );
        assert!(worktree.is_dirty());
    }

    /// Untracked files do not stop a rebase, and autostash does not stash them,
    /// so they must not count as dirty.
    #[test]
    fn untracked_files_alone_are_not_dirty() {
        let worktree = Worktree {
            untracked: 9,
            ..Worktree::default()
        };
        assert!(!worktree.is_dirty());
        assert!(!worktree.is_clean());
    }

    #[test]
    fn a_truncated_record_is_an_error_not_a_guess() {
        assert!(parse_status_v2(&z(&["# branch.head main"])).is_err());
        assert!(parse_status_v2(&z(&["# branch.oid (initial)"])).is_err());
        assert!(
            parse_status_v2(&z(&["# branch.oid (initial)", "# branch.head (detached)",])).is_err()
        );
        assert!(
            parse_status_v2(&z(&[
                "# branch.oid b87d11011148b979094156442b1e1d8d9dbed5ff",
                "# branch.head main",
                "2 R. N... 100644 100644 100644 7898192 7898192 R100 b.txt",
            ]))
            .is_err(),
            "a rename with no original path should not parse"
        );
        assert!(parse_status_v2("nonsense").is_err());
    }

    /// The distinction this table exists for: `rebase-apply/` with an
    /// `applying` file inside is `git am`, not a rebase, and offering
    /// `rebase --abort` for it would fail.
    #[test]
    fn rebase_apply_with_an_applying_file_is_am() {
        let base = StateFiles {
            rebase_apply: true,
            ..StateFiles::default()
        };
        assert_eq!(in_progress(base), Some(InProgress::RebaseApply));
        assert_eq!(
            in_progress(StateFiles {
                rebase_applying: true,
                ..base
            }),
            Some(InProgress::Am)
        );
    }

    /// A conflicted rebase step writes `MERGE_HEAD` as well, so the rebase
    /// directories have to win.
    #[test]
    fn a_rebase_outranks_the_merge_head_it_writes() {
        assert_eq!(
            in_progress(StateFiles {
                rebase_merge: true,
                merge_head: true,
                ..StateFiles::default()
            }),
            Some(InProgress::Rebase)
        );
    }

    #[test]
    fn each_state_file_names_its_operation() {
        let cases = [
            (
                StateFiles {
                    merge_head: true,
                    ..StateFiles::default()
                },
                InProgress::Merge,
            ),
            (
                StateFiles {
                    cherry_pick_head: true,
                    ..StateFiles::default()
                },
                InProgress::CherryPick,
            ),
            (
                StateFiles {
                    revert_head: true,
                    ..StateFiles::default()
                },
                InProgress::Revert,
            ),
            (
                StateFiles {
                    bisect_log: true,
                    ..StateFiles::default()
                },
                InProgress::Bisect,
            ),
        ];
        for (files, expected) in cases {
            assert_eq!(in_progress(files), Some(expected), "{files:?}");
        }
        assert_eq!(in_progress(StateFiles::default()), None);
    }

    #[test]
    fn left_right_counts_are_ahead_then_behind() {
        assert_eq!(
            parse_left_right_count("0\t0\n").expect("parses"),
            Divergence::IDENTICAL
        );
        assert_eq!(
            parse_left_right_count("3\t7").expect("parses"),
            Divergence::new(3, 7)
        );
        for bad in ["", "3", "3 7", "a\tb", "3\t"] {
            assert!(parse_left_right_count(bad).is_err(), "accepted `{bad}`");
        }
    }
}
