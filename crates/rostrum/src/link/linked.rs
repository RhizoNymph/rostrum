//! Pull requests opened from a link that the feed does not hold.
//!
//! The detail pane reads a pull request's header from the feed. One a link
//! names may be in a repository that is not watched, or past the feed's
//! per-repository limit, or merged since; those are looked up on GitHub by
//! themselves and held here, by identity, for as long as the app runs. The
//! feed's own copy always wins: it is refreshed, this one is not.

use std::collections::HashMap;

use rostrum_core::{PrNumber, PullRequest, RepoId};

/// Where one lookup stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    Loading,
    Found(Box<PullRequest>),
    /// Already phrased for the pane.
    Failed(String),
}

/// Every lookup this session, by pull request.
#[derive(Debug, Default)]
pub struct LinkedPulls {
    lookups: HashMap<(RepoId, PrNumber), Lookup>,
}

impl LinkedPulls {
    /// Mark a lookup as started. `false` when one is already in flight or
    /// has found the pull request, so no second request goes out; a failed
    /// one is retried.
    pub fn begin(&mut self, repo: &RepoId, number: PrNumber) -> bool {
        let key = (repo.clone(), number);
        match self.lookups.get(&key) {
            Some(Lookup::Loading | Lookup::Found(_)) => false,
            Some(Lookup::Failed(_)) | None => {
                self.lookups.insert(key, Lookup::Loading);
                true
            }
        }
    }

    /// Record how a lookup ended.
    pub fn finish(&mut self, repo: &RepoId, number: PrNumber, outcome: Lookup) {
        self.lookups.insert((repo.clone(), number), outcome);
    }

    pub fn get(&self, repo: &RepoId, number: PrNumber) -> Option<&Lookup> {
        self.lookups.get(&(repo.clone(), number))
    }

    /// The pull request, once found.
    pub fn found(&self, repo: &RepoId, number: PrNumber) -> Option<&PullRequest> {
        match self.get(repo, number)? {
            Lookup::Found(pull) => Some(pull.as_ref()),
            Lookup::Loading | Lookup::Failed(_) => None,
        }
    }
}

/// What the pane says while it has no pull request to show.
pub fn placeholder(lookup: Option<&Lookup>, repo: &RepoId, number: PrNumber) -> String {
    match lookup {
        Some(Lookup::Loading) => format!("Looking up {repo}{number} on GitHub…"),
        Some(Lookup::Failed(message)) => message.clone(),
        // Found is drawn, not described; no lookup means the feed dropped it.
        Some(Lookup::Found(_)) | None => "This pull request is no longer open".to_string(),
    }
}

/// The sentence for a lookup GitHub refused.
pub fn failure_message(repo: &RepoId, number: PrNumber, error: &str) -> String {
    format!("Couldn't open {repo}{number}: {error}")
}

#[cfg(test)]
mod tests {
    use rostrum_core::{MergeStateStatus, Mergeable, NodeId};

    use super::*;

    fn repo() -> RepoId {
        "octo/elsewhere".parse().expect("repo")
    }

    fn pull(number: u32) -> PullRequest {
        let at = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp");
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{number}")),
            title: "Linked".into(),
            url: String::new(),
            is_draft: false,
            created_at: at,
            updated_at: at,
            author: None,
            head_ref: "topic".into(),
            head_sha: "abc".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
            base_divergence: None,
            is_cross_repository: false,
            pushed_at: None,
        }
    }

    #[test]
    fn a_lookup_starts_once_and_holds_what_it_found() {
        let mut linked = LinkedPulls::default();
        assert!(linked.begin(&repo(), PrNumber(7)));
        assert!(!linked.begin(&repo(), PrNumber(7)), "already in flight");
        assert_eq!(linked.found(&repo(), PrNumber(7)), None);

        linked.finish(&repo(), PrNumber(7), Lookup::Found(Box::new(pull(7))));
        assert_eq!(
            linked.found(&repo(), PrNumber(7)).map(|pull| pull.number),
            Some(PrNumber(7))
        );
        assert!(!linked.begin(&repo(), PrNumber(7)), "already found");
        // Another number is its own lookup.
        assert!(linked.begin(&repo(), PrNumber(8)));
    }

    #[test]
    fn a_failed_lookup_is_retried() {
        let mut linked = LinkedPulls::default();
        assert!(linked.begin(&repo(), PrNumber(7)));
        linked.finish(&repo(), PrNumber(7), Lookup::Failed("gone".into()));
        assert_eq!(linked.found(&repo(), PrNumber(7)), None);
        assert!(linked.begin(&repo(), PrNumber(7)));
        assert_eq!(linked.get(&repo(), PrNumber(7)), Some(&Lookup::Loading));
    }

    #[test]
    fn the_pane_says_what_is_happening() {
        assert_eq!(
            placeholder(Some(&Lookup::Loading), &repo(), PrNumber(7)),
            "Looking up octo/elsewhere#7 on GitHub…"
        );
        let failed = failure_message(&repo(), PrNumber(7), "not found");
        assert_eq!(failed, "Couldn't open octo/elsewhere#7: not found");
        assert_eq!(
            placeholder(Some(&Lookup::Failed(failed.clone())), &repo(), PrNumber(7)),
            failed
        );
        assert_eq!(
            placeholder(None, &repo(), PrNumber(7)),
            "This pull request is no longer open"
        );
    }
}
