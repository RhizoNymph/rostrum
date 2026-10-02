//! The feed's state, and the snapshot Kotlin renders from it.
//!
//! Row composition is `rostrum_core::flatten`'s, the same function the
//! desktop's list is built from: which repositories are hidden, when a body is
//! loading, failed or empty. This walks its rows into nested sections, which
//! suit a `LazyColumn` better than a flat stream does.

use std::collections::HashMap;

use chrono::Utc;
use rostrum_core::{
    FeedFilter, FeedOrder, FeedRow, LoadState, PullRequest, RepoId, RepoMeta, RepoState, User,
    carry_forward_divergence, flatten_in,
};
use rostrum_github::GitHubError;

use crate::{
    engine::state::PullKey,
    feed::{FeedPreferences, FeedSnapshot, RepoBody, RepoLoad, RepoSection, summary::summarize},
    types::UserRef,
};

/// What one repository fetch brought back.
pub(crate) struct Fetched {
    pub prs: Vec<PullRequest>,
    pub viewer: Option<User>,
    /// The repository's own facts, which `flatten` sorts repositories by.
    pub meta: Option<RepoMeta>,
}

/// What applying a fetch did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    /// A newer fetch of the same repository already landed; this one was
    /// dropped.
    Stale,
    /// The repository now holds the fetched list.
    Loaded,
    /// The fetch failed; the previous list stands.
    Failed,
}

pub(crate) struct FeedState {
    /// Watched repositories, in settings order.
    pub repos: Vec<RepoState>,
    pub filter: FeedFilter,
    /// Bumped on every change; see `FeedSnapshot::revision`.
    pub revision: u64,
    /// Whether the SQLite cache has been read into `repos`.
    pub hydrated: bool,
    /// Every pull request seen, by identity — including ones since merged or
    /// closed, so their detail view keeps working after the feed drops them.
    pub known: HashMap<PullKey, PullRequest>,
    /// Sequence numbers, so an older fetch landing after a newer one is
    /// discarded rather than overwriting it.
    next_seq: u64,
    applied: HashMap<RepoId, u64>,
}

impl FeedState {
    pub(crate) fn new(repos: Vec<RepoId>, filter: FeedFilter) -> Self {
        Self {
            repos: repos.into_iter().map(RepoState::new).collect(),
            filter,
            revision: 0,
            hydrated: false,
            known: HashMap::new(),
            next_seq: 1,
            applied: HashMap::new(),
        }
    }

    pub(crate) fn repo_ids(&self) -> Vec<RepoId> {
        self.repos.iter().map(|repo| repo.id.clone()).collect()
    }

    /// Start a fetch of `repo`: its sequence number, and a spinner if it has
    /// nothing to show yet. `None` if the repository is not watched.
    pub(crate) fn begin_fetch(&mut self, repo: &RepoId) -> Option<u64> {
        let state = self.repos.iter_mut().find(|state| &state.id == repo)?;
        // A background refresh of a repository that already has data must
        // not blank its card.
        if state.prs.is_empty() {
            state.load = LoadState::Loading;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        Some(seq)
    }

    /// Apply a fetch that was started with `seq`.
    pub(crate) fn apply_fetch(
        &mut self,
        repo: &RepoId,
        seq: u64,
        outcome: Result<Fetched, &GitHubError>,
    ) -> Applied {
        if self.applied.get(repo).is_some_and(|&newest| newest >= seq) {
            return Applied::Stale;
        }
        let Some(state) = self.repos.iter_mut().find(|state| &state.id == repo) else {
            return Applied::Stale;
        };
        self.applied.insert(repo.clone(), seq);
        match outcome {
            Ok(mut fetched) => {
                carry_forward_divergence(&state.prs, &mut fetched.prs);
                for pr in &fetched.prs {
                    self.known.insert(
                        PullKey {
                            repo: repo.clone(),
                            number: pr.number,
                        },
                        pr.clone(),
                    );
                }
                state.prs = fetched.prs;
                // Keep the last known metadata through an answer without it.
                if fetched.meta.is_some() {
                    state.meta = fetched.meta;
                }
                state.load = LoadState::Loaded { at: Utc::now() };
                Applied::Loaded
            }
            Err(error) => {
                state.load = LoadState::Failed {
                    message: error.to_string(),
                    at: Utc::now(),
                };
                Applied::Failed
            }
        }
    }

    /// Fill repositories that have nothing yet from the cache. Never
    /// overwrites data that already arrived from the network.
    pub(crate) fn hydrate(&mut self, cached: Vec<(RepoId, Vec<PullRequest>)>) {
        self.hydrated = true;
        for (id, prs) in cached {
            for pr in &prs {
                self.known
                    .entry(PullKey {
                        repo: id.clone(),
                        number: pr.number,
                    })
                    .or_insert_with(|| pr.clone());
            }
            if let Some(repo) = self.repos.iter_mut().find(|repo| repo.id == id)
                && repo.prs.is_empty()
            {
                repo.prs = prs;
            }
        }
    }

    /// Watch a repository, in the position `order` (the settings list) puts
    /// it.
    pub(crate) fn add_repo(&mut self, id: RepoId, order: &[String]) {
        if self.repos.iter().any(|repo| repo.id == id) {
            return;
        }
        self.repos.push(RepoState::new(id));
        let position = |repo: &RepoState| {
            let name = repo.id.to_string();
            order
                .iter()
                .position(|entry| entry == &name)
                .unwrap_or(usize::MAX)
        };
        self.repos.sort_by_key(position);
    }

    /// Stop watching a repository and forget what the feed held for it.
    pub(crate) fn remove_repo(&mut self, id: &RepoId) {
        self.repos.retain(|repo| &repo.id != id);
        self.known.retain(|key, _| &key.repo != id);
        self.applied.remove(id);
    }

    /// Watch exactly `ids`, in that order: repositories kept keep their
    /// state (pull requests, load state, collapse), new ones start idle, and
    /// the rest are forgotten as `remove_repo` forgets them. Returns the
    /// repositories that were dropped.
    pub(crate) fn set_repos(&mut self, ids: Vec<RepoId>) -> Vec<RepoId> {
        let mut previous: HashMap<RepoId, RepoState> = HashMap::new();
        let mut removed = Vec::new();
        for repo in std::mem::take(&mut self.repos) {
            if ids.contains(&repo.id) {
                previous.insert(repo.id.clone(), repo);
            } else {
                removed.push(repo.id);
            }
        }
        self.repos = ids
            .into_iter()
            .map(|id| previous.remove(&id).unwrap_or_else(|| RepoState::new(id)))
            .collect();
        for id in &removed {
            self.known.retain(|key, _| &key.repo != id);
            self.applied.remove(id);
        }
        removed
    }

    pub(crate) fn toggle_collapsed(&mut self, id: &RepoId) -> bool {
        match self.repos.iter_mut().find(|repo| &repo.id == id) {
            Some(repo) => {
                repo.collapsed = !repo.collapsed;
                true
            }
            None => false,
        }
    }

    pub(crate) fn preferences(&self) -> FeedPreferences {
        FeedPreferences {
            hide_drafts: self.filter.hide_drafts,
            hide_empty_repos: self.filter.hide_empty_repos,
            authors: self
                .filter
                .authors
                .iter()
                .map(|login| login.as_str().to_string())
                .collect(),
            include_involved: self.filter.include_involved,
        }
    }

    /// The feed as Kotlin renders it.
    pub(crate) fn snapshot(&self, viewer: Option<&User>, settling: bool) -> FeedSnapshot {
        // The phone keeps the user's own repository order (they arrange it
        // in settings) and the fetched item order until it has a sort
        // control; the desktop's `flatten` applies the persisted sort.
        let feed = flatten_in(&self.repos, &self.filter, FeedOrder::AsListed);
        let viewer_key = viewer.map(User::key);

        let mut sections: Vec<RepoSection> = Vec::new();
        let mut building: Option<Building> = None;
        for row in feed.rows() {
            match *row {
                FeedRow::RepoHeader { repo } => {
                    if let Some(done) = building.take() {
                        sections.push(done.finish(&self.repos, &self.filter));
                    }
                    building = Some(Building::new(repo.0));
                }
                FeedRow::PrRow { repo, pr } => {
                    if let (Some(current), Some(state)) =
                        (building.as_mut(), self.repos.get(repo.0))
                        && let Some(pull) = state.prs.get(pr.0)
                    {
                        current
                            .pulls
                            .push(summarize(&state.id, pull, viewer_key.as_ref()));
                    }
                }
                FeedRow::RepoEmpty { .. } => set_body(&mut building, RepoBody::Empty),
                FeedRow::RepoLoading { .. } => set_body(&mut building, RepoBody::Loading),
                FeedRow::RepoError { repo } => {
                    let reason = self
                        .repos
                        .get(repo.0)
                        .and_then(|state| state.load.error_message())
                        .unwrap_or("the last refresh failed")
                        .to_string();
                    set_body(&mut building, RepoBody::Failed { reason });
                }
                // `flatten` builds the pull request tab only; the phone has
                // no issue list yet.
                FeedRow::Spacer { .. } | FeedRow::IssueRow { .. } => {}
            }
        }
        if let Some(done) = building.take() {
            sections.push(done.finish(&self.repos, &self.filter));
        }

        let visible_open = self
            .repos
            .iter()
            .flat_map(|repo| &repo.prs)
            .filter(|pr| self.filter.accepts(pr))
            .count();
        FeedSnapshot {
            revision: self.revision,
            repos: sections,
            hidden_empty_repos: count(feed.hidden_repos()),
            total_open: count(self.repos.iter().map(|repo| repo.prs.len()).sum()),
            visible_open: count(visible_open),
            query: self.filter.query.clone(),
            preferences: self.preferences(),
            filter_active: self.filter.is_active(),
            merge_states_settling: settling,
            viewer: viewer.map(UserRef::from),
        }
    }
}

/// A section being assembled from `flatten`'s rows.
struct Building {
    repo: usize,
    body: Option<RepoBody>,
    pulls: Vec<crate::feed::PrSummary>,
}

impl Building {
    fn new(repo: usize) -> Self {
        Self {
            repo,
            body: None,
            pulls: Vec::new(),
        }
    }

    fn finish(self, repos: &[RepoState], filter: &FeedFilter) -> RepoSection {
        let state = &repos[self.repo];
        let body = if state.collapsed {
            RepoBody::Collapsed
        } else if let Some(body) = self.body {
            body
        } else if self.pulls.is_empty() {
            RepoBody::Empty
        } else {
            RepoBody::Pulls { pulls: self.pulls }
        };
        RepoSection {
            repo: state.id.to_string(),
            load: load_of(&state.load),
            open_count: count(state.prs.len()),
            visible_count: count(state.prs.iter().filter(|pr| filter.accepts(pr)).count()),
            collapsed: state.collapsed,
            body,
        }
    }
}

fn set_body(building: &mut Option<Building>, body: RepoBody) {
    if let Some(current) = building.as_mut() {
        current.body = Some(body);
    }
}

fn load_of(load: &LoadState) -> RepoLoad {
    match load {
        LoadState::Idle => RepoLoad::Idle,
        LoadState::Loading => RepoLoad::Loading,
        LoadState::Loaded { at } => RepoLoad::Loaded { at: (*at).into() },
        LoadState::Failed { message, at } => RepoLoad::Failed {
            reason: message.clone(),
            at: (*at).into(),
        },
    }
}

/// Counts cross as `u32`; a feed of four billion pull requests saturates.
pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rostrum_core::{Divergence, LoginKey, PrNumber};

    use super::*;
    use crate::test_support::{pull, pull_by};

    fn repo_id(name: &str) -> RepoId {
        name.parse().expect("repo")
    }

    fn feed(names: &[&str]) -> FeedState {
        FeedState::new(
            names.iter().map(|name| repo_id(name)).collect(),
            FeedFilter::default(),
        )
    }

    fn load(state: &mut FeedState, name: &str, prs: Vec<PullRequest>) {
        let id = repo_id(name);
        let seq = state.begin_fetch(&id).expect("watched");
        assert_eq!(
            state.apply_fetch(
                &id,
                seq,
                Ok(Fetched {
                    prs,
                    viewer: None,
                    meta: None,
                })
            ),
            Applied::Loaded
        );
    }

    fn pulls(section: &RepoSection) -> Vec<u32> {
        match &section.body {
            RepoBody::Pulls { pulls } => pulls.iter().map(|pull| pull.number).collect(),
            other => panic!("expected pulls, got {other:?}"),
        }
    }

    #[test]
    fn a_new_feed_shows_every_repository_idle() {
        let state = feed(&["a/b", "c/d"]);
        let snapshot = state.snapshot(None, false);
        assert_eq!(snapshot.repos.len(), 2);
        // Idle with nothing cached: a loading body, never hidden.
        assert!(
            snapshot
                .repos
                .iter()
                .all(|section| section.body == RepoBody::Loading && section.load == RepoLoad::Idle)
        );
        assert_eq!(snapshot.total_open, 0);
    }

    #[test]
    fn sections_keep_settings_order_and_carry_their_pulls() {
        let mut state = feed(&["a/b", "c/d"]);
        load(&mut state, "c/d", vec![pull(3)]);
        load(&mut state, "a/b", vec![pull(1), pull(2)]);
        let snapshot = state.snapshot(None, false);
        assert_eq!(snapshot.repos[0].repo, "a/b");
        assert_eq!(pulls(&snapshot.repos[0]), vec![1, 2]);
        assert_eq!(pulls(&snapshot.repos[1]), vec![3]);
        assert_eq!(snapshot.total_open, 3);
        assert_eq!(snapshot.visible_open, 3);
        assert!(matches!(snapshot.repos[0].load, RepoLoad::Loaded { .. }));
    }

    #[test]
    fn empty_loaded_repositories_are_hidden_and_counted() {
        let mut state = feed(&["a/b", "c/d"]);
        load(&mut state, "a/b", vec![]);
        load(&mut state, "c/d", vec![pull(1)]);
        let snapshot = state.snapshot(None, false);
        assert_eq!(snapshot.repos.len(), 1);
        assert_eq!(snapshot.hidden_empty_repos, 1);

        state.filter.hide_empty_repos = false;
        let shown = state.snapshot(None, false);
        assert_eq!(shown.repos.len(), 2);
        assert_eq!(shown.repos[0].body, RepoBody::Empty);
        assert_eq!(shown.hidden_empty_repos, 0);
    }

    /// A failed repository with nothing cached shows its error; with older
    /// data it keeps showing that data, and the header says it failed.
    #[test]
    fn a_failure_shows_the_error_or_keeps_the_stale_list() {
        let mut state = feed(&["a/b", "c/d"]);
        let error = GitHubError::NotFound {
            resource: "a/b".into(),
        };
        let a = repo_id("a/b");
        let seq = state.begin_fetch(&a).expect("watched");
        assert_eq!(state.apply_fetch(&a, seq, Err(&error)), Applied::Failed);

        load(&mut state, "c/d", vec![pull(9)]);
        let c = repo_id("c/d");
        let seq = state.begin_fetch(&c).expect("watched");
        state.apply_fetch(&c, seq, Err(&error));

        let snapshot = state.snapshot(None, false);
        let RepoBody::Failed { reason } = &snapshot.repos[0].body else {
            panic!("expected failure, got {:?}", snapshot.repos[0].body);
        };
        assert!(reason.contains("not found"), "{reason}");
        assert_eq!(pulls(&snapshot.repos[1]), vec![9]);
        assert!(matches!(snapshot.repos[1].load, RepoLoad::Failed { .. }));
    }

    #[test]
    fn an_older_fetch_landing_late_is_discarded() {
        let mut state = feed(&["a/b"]);
        let id = repo_id("a/b");
        let older = state.begin_fetch(&id).expect("watched");
        let newer = state.begin_fetch(&id).expect("watched");
        let fetched = |numbers: &[u32]| Fetched {
            prs: numbers.iter().copied().map(pull).collect(),
            viewer: None,
            meta: None,
        };
        assert_eq!(
            state.apply_fetch(&id, newer, Ok(fetched(&[2]))),
            Applied::Loaded
        );
        assert_eq!(
            state.apply_fetch(&id, older, Ok(fetched(&[1]))),
            Applied::Stale
        );
        assert_eq!(pulls(&state.snapshot(None, false).repos[0]), vec![2]);
    }

    #[test]
    fn a_refresh_carries_the_previous_divergence_forward() {
        let mut state = feed(&["a/b"]);
        let mut first = pull(1);
        first.base_divergence = Some(Divergence::new(0, 2));
        load(&mut state, "a/b", vec![first]);
        load(&mut state, "a/b", vec![pull(1)]);
        let snapshot = state.snapshot(None, false);
        let RepoBody::Pulls { pulls } = &snapshot.repos[0].body else {
            panic!("pulls");
        };
        assert_eq!(pulls[0].base_divergence.as_ref().map(|d| d.behind), Some(2));
    }

    #[test]
    fn the_filter_narrows_rows_but_not_open_counts() {
        let mut state = feed(&["a/b"]);
        let mut draft = pull(2);
        draft.is_draft = true;
        load(
            &mut state,
            "a/b",
            vec![pull_by(1, "alice"), draft, pull_by(3, "bob")],
        );

        state.filter.hide_drafts = true;
        state.filter.authors = BTreeSet::from([LoginKey::new("Alice")]);
        let snapshot = state.snapshot(None, false);
        assert_eq!(pulls(&snapshot.repos[0]), vec![1]);
        assert_eq!(snapshot.repos[0].open_count, 3);
        assert_eq!(snapshot.repos[0].visible_count, 1);
        assert_eq!(snapshot.visible_open, 1);
        assert!(snapshot.filter_active);
        assert_eq!(snapshot.preferences.authors, vec!["alice".to_string()]);

        state.filter.query = "no such title".into();
        let searched = state.snapshot(None, false);
        // Loaded with nothing matching: hidden, since hide_empty_repos is on.
        assert!(searched.repos.is_empty());
        assert_eq!(searched.hidden_empty_repos, 1);
    }

    #[test]
    fn a_collapsed_repository_keeps_its_counts_but_no_rows() {
        let mut state = feed(&["a/b"]);
        load(&mut state, "a/b", vec![pull(1), pull(2)]);
        assert!(state.toggle_collapsed(&repo_id("a/b")));
        let snapshot = state.snapshot(None, false);
        assert_eq!(snapshot.repos[0].body, RepoBody::Collapsed);
        assert!(snapshot.repos[0].collapsed);
        assert_eq!(snapshot.repos[0].visible_count, 2);
        assert!(!state.toggle_collapsed(&repo_id("x/y")));
    }

    #[test]
    fn hydration_fills_gaps_only_and_remembers_pull_requests() {
        let mut state = feed(&["a/b", "c/d"]);
        load(&mut state, "a/b", vec![pull(5)]);
        state.hydrate(vec![
            (repo_id("a/b"), vec![pull(1)]),
            (repo_id("c/d"), vec![pull(2)]),
        ]);
        let snapshot = state.snapshot(None, false);
        assert_eq!(pulls(&snapshot.repos[0]), vec![5]);
        assert_eq!(pulls(&snapshot.repos[1]), vec![2]);
        assert!(state.known.contains_key(&PullKey {
            repo: repo_id("c/d"),
            number: PrNumber(2)
        }));
        assert!(state.hydrated);
    }

    #[test]
    fn known_pull_requests_outlive_the_feed() {
        let mut state = feed(&["a/b"]);
        load(&mut state, "a/b", vec![pull(1)]);
        load(&mut state, "a/b", vec![]);
        assert!(state.known.contains_key(&PullKey {
            repo: repo_id("a/b"),
            number: PrNumber(1)
        }));
        state.remove_repo(&repo_id("a/b"));
        assert!(state.known.is_empty());
        assert!(state.repos.is_empty());
    }

    #[test]
    fn setting_the_repositories_keeps_reorders_adds_and_forgets() {
        let mut state = feed(&["a/b", "c/d", "e/f"]);
        load(&mut state, "a/b", vec![pull(1)]);
        load(&mut state, "c/d", vec![pull(2)]);
        assert!(state.toggle_collapsed(&repo_id("c/d")));

        let removed = state.set_repos(vec![repo_id("g/h"), repo_id("c/d"), repo_id("a/b")]);
        assert_eq!(removed, vec![repo_id("e/f")]);
        assert_eq!(
            state.repo_ids(),
            vec![repo_id("g/h"), repo_id("c/d"), repo_id("a/b")]
        );
        // Kept repositories keep what they had; the new one starts idle.
        let snapshot = state.snapshot(None, false);
        assert_eq!(snapshot.repos[0].load, RepoLoad::Idle);
        assert_eq!(snapshot.repos[1].body, RepoBody::Collapsed);
        assert_eq!(pulls(&snapshot.repos[2]), vec![1]);

        // Dropping a repository forgets its pull requests and its fetch order.
        let removed = state.set_repos(vec![repo_id("g/h")]);
        assert_eq!(removed, vec![repo_id("c/d"), repo_id("a/b")]);
        assert!(state.known.is_empty());
        let snapshot = state.snapshot(None, false);
        assert_eq!(
            snapshot
                .repos
                .iter()
                .map(|section| section.repo.as_str())
                .collect::<Vec<_>>(),
            vec!["g/h"]
        );
        // A re-added repository fetches afresh rather than being taken for
        // one whose newer fetch already landed.
        state.set_repos(vec![repo_id("g/h"), repo_id("a/b")]);
        load(&mut state, "a/b", vec![pull(5)]);
    }

    #[test]
    fn added_repositories_follow_settings_order() {
        let mut state = feed(&["c/d"]);
        state.add_repo(repo_id("a/b"), &["a/b".into(), "c/d".into()]);
        state.add_repo(repo_id("a/b"), &["a/b".into(), "c/d".into()]);
        assert_eq!(state.repo_ids(), vec![repo_id("a/b"), repo_id("c/d")]);
    }

    #[test]
    fn the_viewer_marks_their_rows() {
        let mut state = feed(&["a/b"]);
        load(
            &mut state,
            "a/b",
            vec![pull_by(1, "Me"), pull_by(2, "other")],
        );
        let me = User {
            login: "me".into(),
            avatar_url: None,
        };
        let snapshot = state.snapshot(Some(&me), true);
        let RepoBody::Pulls { pulls } = &snapshot.repos[0].body else {
            panic!("pulls");
        };
        assert!(pulls[0].is_yours);
        assert!(!pulls[1].is_yours);
        assert!(snapshot.merge_states_settling);
        assert_eq!(snapshot.viewer.map(|v| v.login), Some("me".to_string()));
    }
}
