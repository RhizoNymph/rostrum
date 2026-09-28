//! What the phone tells the desktop about a pull request.
//!
//! The desktop knows nothing about GitHub; every local job carries the pull
//! request it is for (`PrRef`), built here from what the feed loaded.

use std::collections::HashSet;

use rostrum_core::{Conversation, PullRequest, RepoId, RepoState, TimelineItem};
use rostrum_remote::api::{CloneInfo, PrKey, PrRef};

/// The pull request as a job's subject. `body` is the description when the
/// conversation has been loaded, so a conflict handed to a tmux session
/// carries it; otherwise the desktop's handler reads it from the URL.
pub(crate) fn pr_ref(repo: &RepoId, pr: &PullRequest, body: Option<String>) -> PrRef {
    PrRef {
        key: PrKey {
            repo: repo.clone(),
            number: pr.number,
        },
        title: pr.title.clone(),
        url: pr.url.clone(),
        body: body.unwrap_or_default(),
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
    }
}

/// The description of a loaded conversation.
pub(crate) fn description(conversation: &Conversation) -> Option<String> {
    conversation.items.iter().find_map(|item| match item {
        TimelineItem::Body { body, .. } => Some(body.clone()),
        _ => None,
    })
}

/// Every open pull request in the feed whose repository has a clone on the
/// desktop, in feed order — the set "sync all" runs over. The feed filter
/// does not apply: syncing is about the clones, not about what is on screen.
pub(crate) fn sync_refs(
    repos: &[RepoState],
    clones: &[CloneInfo],
    body: impl Fn(&RepoId, &PullRequest) -> Option<String>,
) -> Vec<PrRef> {
    let cloned: HashSet<&RepoId> = clones.iter().map(|clone| &clone.repo).collect();
    repos
        .iter()
        .filter(|repo| cloned.contains(&repo.id))
        .flat_map(|repo| {
            repo.prs
                .iter()
                .map(|pr| pr_ref(&repo.id, pr, body(&repo.id, pr)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rostrum_core::{LoadState, PrNumber, User};

    use super::*;
    use crate::test_support::pull;

    fn repo(name: &str, numbers: &[u32]) -> RepoState {
        let mut state = RepoState::new(name.parse().expect("repo"));
        state.prs = numbers.iter().copied().map(pull).collect();
        state.load = LoadState::Loaded { at: Utc::now() };
        state
    }

    fn clone_of(name: &str) -> CloneInfo {
        CloneInfo {
            repo: name.parse().expect("repo"),
            path: format!("/code/{name}"),
        }
    }

    #[test]
    fn only_repositories_with_a_clone_are_synced() {
        let repos = vec![repo("a/b", &[1, 2]), repo("c/d", &[3]), repo("e/f", &[4])];
        let refs = sync_refs(&repos, &[clone_of("e/f"), clone_of("a/b")], |_, _| None);
        assert_eq!(
            refs.iter()
                .map(|r| (r.key.repo.to_string(), r.key.number.0))
                .collect::<Vec<_>>(),
            vec![("a/b".into(), 1), ("a/b".into(), 2), ("e/f".into(), 4)]
        );
        assert_eq!(refs[0].head_ref, "branch-1");
        assert_eq!(refs[0].base_ref, "main");
        assert!(refs[0].body.is_empty());
    }

    #[test]
    fn no_clones_means_nothing_to_sync() {
        assert!(sync_refs(&[repo("a/b", &[1])], &[], |_, _| None).is_empty());
    }

    #[test]
    fn a_known_description_rides_along() {
        let refs = sync_refs(&[repo("a/b", &[1, 2])], &[clone_of("a/b")], |_, pr| {
            (pr.number == PrNumber(2)).then(|| "the description".to_string())
        });
        assert_eq!(refs[0].body, "");
        assert_eq!(refs[1].body, "the description");
    }

    #[test]
    fn the_description_is_the_body_item() {
        let conversation = Conversation {
            items: vec![TimelineItem::Body {
                author: Some(User {
                    login: "a".into(),
                    avatar_url: None,
                }),
                body: "why".into(),
                created_at: Utc::now(),
            }],
            ..Default::default()
        };
        assert_eq!(description(&conversation).as_deref(), Some("why"));
        assert_eq!(description(&Conversation::default()), None);
    }
}
