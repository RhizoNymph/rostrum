//! Which repository a remote URL names.
//!
//! A push from a pull request's worktree goes to the remote the branch's
//! upstream names, and only when that remote is the pull request's head
//! repository. GitHub identifies a repository by `owner/name`; a remote URL
//! carries it as the last two path components in every form git accepts:
//!
//! - `git@github.com:owner/name.git` (scp-like)
//! - `ssh://git@github.com[:22]/owner/name.git`
//! - `https://[user@]github.com/owner/name[.git][/]`
//! - `git://…`, `file:///…/owner/name.git`, `/…/owner/name.git`
//!
//! The host is deliberately not compared: an SSH alias (`git@gh-work:owner/
//! name`) or a GitHub Enterprise host names the same `owner/name`, and the
//! user's own configuration chose it. Owner and name compare without regard
//! to case, as GitHub does.

use rostrum_core::RepoId;

/// The `owner/name` a remote URL points at, or `None` when the URL does not
/// end in two path components.
pub fn repo_of_url(url: &str) -> Option<RepoId> {
    let url = url.trim();
    let path = match url.split_once("://") {
        // `scheme://authority/path`: everything after the authority.
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path)?,
        // scp-like `[user@]host:path`, unless it is a plain local path (which
        // has no colon before its first slash).
        None => match (url.find(':'), url.find('/')) {
            (Some(colon), Some(slash)) if colon < slash => &url[colon + 1..],
            (Some(colon), None) => &url[colon + 1..],
            _ => url,
        },
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut components = path.rsplit('/');
    let name = components.next().filter(|name| !name.is_empty())?;
    let owner = components.next().filter(|owner| !owner.is_empty())?;
    Some(RepoId::new(owner, name))
}

/// Whether `url` names `repo`, compared as GitHub compares names.
pub fn url_names(url: &str, repo: &RepoId) -> bool {
    repo_of_url(url).is_some_and(|found| {
        found.owner().eq_ignore_ascii_case(repo.owner())
            && found.name().eq_ignore_ascii_case(repo.name())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> Option<RepoId> {
        Some(RepoId::new(owner, name))
    }

    #[test]
    fn every_url_form_git_accepts_names_its_repository() {
        for url in [
            "git@github.com:octo/repo.git",
            "git@github.com:octo/repo",
            "github.com:octo/repo.git",
            "ssh://git@github.com/octo/repo.git",
            "ssh://git@github.com:22/octo/repo.git",
            "https://github.com/octo/repo",
            "https://github.com/octo/repo.git",
            "https://github.com/octo/repo/",
            "https://user:token@github.com/octo/repo.git",
            "git://github.com/octo/repo.git",
            "file:///srv/git/octo/repo.git",
            "/srv/git/octo/repo.git",
            "../octo/repo.git",
            "git@gh-work:octo/repo.git",
        ] {
            assert_eq!(repo_of_url(url), repo("octo", "repo"), "{url}");
        }
    }

    #[test]
    fn a_url_without_an_owner_and_name_names_nothing() {
        for url in [
            "",
            "repo.git",
            "https://github.com/",
            "https://github.com/octo",
            "git@github.com:repo.git",
        ] {
            assert_eq!(repo_of_url(url), None, "{url}");
        }
    }

    #[test]
    fn names_compare_without_regard_to_case_but_otherwise_exactly() {
        let octo = RepoId::new("Octo", "Repo");
        assert!(url_names("git@github.com:octo/repo.git", &octo));
        assert!(!url_names("git@github.com:octo/repo-fork.git", &octo));
        assert!(!url_names("git@github.com:forker/repo.git", &octo));
        assert!(!url_names("not a url", &octo));
    }
}
