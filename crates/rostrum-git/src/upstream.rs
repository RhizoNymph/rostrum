//! Which remote branch a local branch is configured to track.
//!
//! A push must go where the user's own configuration says the branch belongs,
//! never to a remote rostrum guessed. `git for-each-ref` with
//! `%(upstream:remotename)` and `%(upstream:remoteref)` answers that from
//! `branch.<name>.remote` and `branch.<name>.merge` exactly as `git push`
//! and `git status` read them; [`parse_upstream`] turns the answer into a
//! [`BranchUpstream`].
//!
//! The spawn lives in [`crate::Repo::branch_upstream`]; this module is pure.

use crate::refs::{BranchName, Remote, RemoteRef};

/// The format [`crate::Repo::branch_upstream`] asks `for-each-ref` for: the
/// ref's own name (patterns match by prefix, so it is checked), the upstream's
/// remote name, the remote branch it merges, and the local tracking ref.
pub const UPSTREAM_FORMAT: &str =
    "%(refname)%00%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream)";

/// A local branch's configured upstream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchUpstream {
    /// No upstream is configured.
    None,
    /// The branch tracks `remote_ref`, and its tracking ref is the ordinary
    /// `refs/remotes/<remote>/<branch>`.
    Tracking(RemoteRef),
    /// An upstream is configured but it is not a remote branch rostrum can
    /// name safely: a local upstream (`branch.<b>.remote = .`), a merge ref
    /// outside `refs/heads/`, a custom fetch refspec mapping it somewhere
    /// unusual, or a name git accepts and this crate does not. Carries what
    /// git printed, for the reason shown to the user.
    Unusual(String),
}

/// Read [`UPSTREAM_FORMAT`] output for `branch`.
///
/// Lines for other refs (a pattern `refs/heads/a` also matches
/// `refs/heads/a/b`) are ignored; no line for the branch at all is
/// [`BranchUpstream::None`].
pub fn parse_upstream(stdout: &str, branch: &BranchName) -> BranchUpstream {
    let qualified = branch.qualified();
    let Some(line) = stdout.lines().find_map(|line| {
        let mut fields = line.split('\0');
        (fields.next() == Some(qualified.as_str())).then(|| fields.collect::<Vec<_>>())
    }) else {
        return BranchUpstream::None;
    };
    let [remote, merge, tracking] = line.as_slice() else {
        return BranchUpstream::Unusual(line.join(" "));
    };
    if remote.is_empty() && merge.is_empty() && tracking.is_empty() {
        return BranchUpstream::None;
    }
    let described = || {
        if tracking.is_empty() {
            format!("{remote} {merge}")
        } else {
            (*tracking).to_string()
        }
    };
    let Ok(remote) = Remote::new(*remote) else {
        return BranchUpstream::Unusual(described());
    };
    let Some(Ok(merged)) = merge.strip_prefix("refs/heads/").map(BranchName::new) else {
        return BranchUpstream::Unusual(described());
    };
    let remote_ref = RemoteRef::new(remote, merged);
    if *tracking != remote_ref.tracking_ref() {
        return BranchUpstream::Unusual(described());
    }
    BranchUpstream::Tracking(remote_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> BranchName {
        BranchName::new(name).expect("valid")
    }

    #[test]
    fn a_tracked_remote_branch_is_named_as_a_remote_ref() {
        let out = "refs/heads/feat/x\0fork\0refs/heads/feat/x\0refs/remotes/fork/feat/x\n";
        assert_eq!(
            parse_upstream(out, &branch("feat/x")),
            BranchUpstream::Tracking(RemoteRef::new(
                Remote::new("fork").expect("valid"),
                branch("feat/x")
            ))
        );
    }

    /// A branch tracking a differently named remote branch is still a remote
    /// ref; the caller decides whether that is the pull request's head.
    #[test]
    fn the_merged_branch_may_differ_from_the_local_name() {
        let out =
            "refs/heads/local\0origin\0refs/heads/remote-name\0refs/remotes/origin/remote-name\n";
        assert_eq!(
            parse_upstream(out, &branch("local")),
            BranchUpstream::Tracking(RemoteRef::origin(branch("remote-name")))
        );
    }

    #[test]
    fn no_upstream_and_no_line_are_both_none() {
        assert_eq!(
            parse_upstream("refs/heads/x\0\0\0\n", &branch("x")),
            BranchUpstream::None
        );
        assert_eq!(parse_upstream("", &branch("x")), BranchUpstream::None);
    }

    /// `refs/heads/a` matches `refs/heads/a/b` as a pattern; only the exact
    /// ref's line counts.
    #[test]
    fn only_the_named_branch_is_read() {
        let out =
            "refs/heads/a/b\0origin\0refs/heads/a/b\0refs/remotes/origin/a/b\nrefs/heads/a\0\0\0\n";
        assert_eq!(parse_upstream(out, &branch("a")), BranchUpstream::None);
    }

    #[test]
    fn a_local_or_unusual_upstream_is_not_a_remote_ref() {
        // `git branch --track x main`: the remote is `.`.
        assert!(matches!(
            parse_upstream(
                "refs/heads/x\0.\0refs/heads/main\0refs/heads/main\n",
                &branch("x")
            ),
            BranchUpstream::Unusual(_)
        ));
        // A merge ref outside refs/heads.
        assert!(matches!(
            parse_upstream(
                "refs/heads/x\0origin\0refs/pull/1/head\0refs/remotes/origin/pr/1\n",
                &branch("x")
            ),
            BranchUpstream::Unusual(_)
        ));
        // A custom fetch refspec mapping the branch somewhere else.
        assert!(matches!(
            parse_upstream(
                "refs/heads/x\0origin\0refs/heads/x\0refs/remotes/mirror/x\n",
                &branch("x")
            ),
            BranchUpstream::Unusual(_)
        ));
        // A remote name that would be read as an option.
        assert!(matches!(
            parse_upstream(
                "refs/heads/x\0-evil\0refs/heads/x\0refs/remotes/-evil/x\n",
                &branch("x")
            ),
            BranchUpstream::Unusual(_)
        ));
    }
}
