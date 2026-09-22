//! The other work trees of the same repository.
//!
//! rostrum's own repository is a multi-worktree checkout, and so is the layout
//! it recommends: one directory per branch under a common parent. A pull
//! request's branch is therefore usually *already checked out* somewhere, and
//! acting on it means finding that directory rather than checking the branch
//! out a second time — git refuses to have one branch in two worktrees.
//!
//! `git worktree list --porcelain` is the only reliable way to ask. Walking
//! `<common>/worktrees/*/gitdir` by hand would miss the main worktree entirely
//! and would report a directory the user has since deleted as still present.
//!
//! The parser here is pure; the spawn lives in [`crate::repo`].

use std::path::PathBuf;

use crate::{
    error::GitError,
    refs::{BranchName, Oid},
};

/// One work tree, as `git worktree list --porcelain` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// The absolute work tree root — or, for a bare entry, the repository.
    pub path: PathBuf,
    /// Absent for a bare repository, which has no checkout to have a HEAD.
    pub head: Option<Oid>,
    /// The checked-out branch. `None` when detached or bare.
    pub branch: Option<BranchName>,
    pub bare: bool,
    pub detached: bool,
}

/// Parse `git worktree list --porcelain`.
///
/// Records are separated by a blank line and begin with a `worktree <path>`
/// line, followed by attribute lines: `HEAD <oid>`, `branch <ref>`, and the
/// bare words `bare` and `detached`, plus `locked [reason]` and
/// `prunable [reason]`. The last two are accepted and ignored: a locked
/// worktree is still a worktree, and the reason is prose for a human.
///
/// The porcelain format is documented as extensible — "additional attributes
/// may be added" — so an attribute this crate does not know is skipped rather
/// than rejected. A record with no `worktree` line, or a `branch` outside
/// `refs/heads/`, is a genuine surprise and is rejected.
///
/// Paths are taken verbatim from the rest of the `worktree` line. Unlike
/// `status`, porcelain worktree output does not C-quote unusual characters, so
/// a path with a space or non-ASCII in it arrives as-is.
pub fn parse_worktree_list(output: &str) -> Result<Vec<WorktreeEntry>, GitError> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }

        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            // A `worktree` line with no blank line before it is still the
            // start of a new record; git does not emit that shape, but nothing
            // is gained by losing the previous entry over it.
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(value),
                head: None,
                branch: None,
                bare: false,
                detached: false,
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            return Err(GitError::Parse {
                what: "worktree attribute before any `worktree` line",
                line: line.to_string(),
            });
        };
        match key {
            "HEAD" => entry.head = Some(Oid::parse(value)?),
            "branch" => {
                let name = value
                    .strip_prefix("refs/heads/")
                    .ok_or_else(|| GitError::Parse {
                        what: "worktree branch outside refs/heads/",
                        line: line.to_string(),
                    })?;
                entry.branch = Some(BranchName::new(name)?);
            }
            "bare" => entry.bare = true,
            "detached" => entry.detached = true,
            "locked" | "prunable" => {}
            _ => {}
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: char) -> Oid {
        Oid::parse(std::iter::repeat_n(byte, 40).collect::<String>()).expect("valid")
    }

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    /// The layout rostrum's own repository has: a main worktree and one
    /// per feature branch, as git 2.53 prints it.
    #[test]
    fn a_main_worktree_and_feature_worktrees_each_carry_their_branch() {
        let entries = parse_worktree_list(concat!(
            "worktree /home/u/rostrum/main\n",
            "HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "branch refs/heads/main\n",
            "\n",
            "worktree /home/u/rostrum/feat-branch-divergence\n",
            "HEAD bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
            "branch refs/heads/feat/branch-divergence\n",
            "\n",
            "worktree /home/u/rostrum/feat-worktree-sync\n",
            "HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "branch refs/heads/feat/worktree-sync\n",
            "\n",
        ))
        .expect("parses");

        assert_eq!(
            entries,
            vec![
                WorktreeEntry {
                    path: PathBuf::from("/home/u/rostrum/main"),
                    head: Some(oid('a')),
                    branch: Some(branch("main")),
                    bare: false,
                    detached: false,
                },
                WorktreeEntry {
                    path: PathBuf::from("/home/u/rostrum/feat-branch-divergence"),
                    head: Some(oid('b')),
                    branch: Some(branch("feat/branch-divergence")),
                    bare: false,
                    detached: false,
                },
                WorktreeEntry {
                    path: PathBuf::from("/home/u/rostrum/feat-worktree-sync"),
                    head: Some(oid('a')),
                    branch: Some(branch("feat/worktree-sync")),
                    bare: false,
                    detached: false,
                },
            ]
        );
    }

    /// Mid-rebase a worktree is detached, and that must not read as "on no
    /// branch, so free for checkout".
    #[test]
    fn a_detached_worktree_has_a_head_and_no_branch() {
        let entries = parse_worktree_list(concat!(
            "worktree /w\n",
            "HEAD cccccccccccccccccccccccccccccccccccccccc\n",
            "detached\n",
        ))
        .expect("parses");
        assert_eq!(
            entries,
            vec![WorktreeEntry {
                path: PathBuf::from("/w"),
                head: Some(oid('c')),
                branch: None,
                bare: false,
                detached: true,
            }]
        );
    }

    /// A bare main repository lists itself first, with nothing checked out.
    #[test]
    fn a_bare_entry_has_neither_head_nor_branch() {
        let entries = parse_worktree_list(concat!(
            "worktree /srv/repo.git\n",
            "bare\n",
            "\n",
            "worktree /srv/checkout\n",
            "HEAD dddddddddddddddddddddddddddddddddddddddd\n",
            "branch refs/heads/main\n",
        ))
        .expect("parses");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            WorktreeEntry {
                path: PathBuf::from("/srv/repo.git"),
                head: None,
                branch: None,
                bare: true,
                detached: false,
            }
        );
        assert_eq!(entries[1].branch, Some(branch("main")));
    }

    /// A locked worktree is still a worktree; the reason is for a human.
    #[test]
    fn a_locked_entry_is_parsed_not_rejected() {
        let entries = parse_worktree_list(concat!(
            "worktree /w\n",
            "HEAD eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\n",
            "branch refs/heads/x\n",
            "locked why not\n",
            "prunable gitdir file points to non-existent location\n",
        ))
        .expect("parses");
        assert_eq!(entries[0].branch, Some(branch("x")));
        assert!(!entries[0].bare);
    }

    /// Porcelain worktree output does not quote, so the path is everything
    /// after the first space.
    #[test]
    fn a_path_containing_a_space_survives() {
        let entries = parse_worktree_list(concat!(
            "worktree /home/u/my repos/with space\n",
            "HEAD ffffffffffffffffffffffffffffffffffffffff\n",
            "branch refs/heads/sp\n",
        ))
        .expect("parses");
        assert_eq!(
            entries[0].path,
            PathBuf::from("/home/u/my repos/with space")
        );
    }

    #[test]
    fn the_trailing_newline_is_optional() {
        let with = parse_worktree_list("worktree /w\nHEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbranch refs/heads/main\n\n")
            .expect("parses");
        let without = parse_worktree_list(
            "worktree /w\nHEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbranch refs/heads/main",
        )
        .expect("parses");
        assert_eq!(with, without);
        assert_eq!(with.len(), 1);
        assert_eq!(parse_worktree_list("").expect("parses"), vec![]);
    }

    /// A ref git wrote is not automatically a name this crate will put on a
    /// command line; the same gate as everywhere else applies.
    #[test]
    fn a_branch_name_this_crate_rejects_is_an_error_not_a_panic() {
        let err = parse_worktree_list(concat!(
            "worktree /w\n",
            "HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "branch refs/heads/a..b\n",
        ));
        assert!(
            matches!(err, Err(GitError::InvalidBranchName { .. })),
            "got {err:?}"
        );

        let err = parse_worktree_list(concat!(
            "worktree /w\n",
            "HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "branch refs/tags/v1\n",
        ));
        assert!(matches!(err, Err(GitError::Parse { .. })), "got {err:?}");
    }

    #[test]
    fn an_attribute_before_any_worktree_line_is_rejected() {
        assert!(matches!(
            parse_worktree_list("HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"),
            Err(GitError::Parse { .. })
        ));
    }
}
