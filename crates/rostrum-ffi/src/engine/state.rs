//! `CoreState`: everything the core knows, owned by the state actor.
//!
//! Each area of the API adds its own `impl CoreState` block in its module;
//! this file holds the fields and the helpers every area shares.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use rostrum_config::Config;
use rostrum_core::{Baseline, Conversation, Label, PrNumber, PullRequest, RepoId};
use rostrum_github::{GitHubClient, GitHubError};

use crate::{
    diff::LoadedFiles,
    engine::{actor::WeakActor, notifier::Notifier, recent::Recent, writer::Writer},
    error::RostrumError,
    feed::{FeedSnapshot, FeedState, ProbeSlot},
    remote::RemoteSession,
    review::DraftBook,
    session::{GitHubApi, Session},
};

/// Conversations kept in memory (SQLite keeps them all).
const RECENT_CONVERSATIONS: usize = 24;
/// Parsed diffs kept in memory. A diff can be large; SQLite keeps the raw
/// patches and re-parsing one is cheap next to fetching it.
const RECENT_DIFFS: usize = 6;

/// A pull request's identity: which repository, which number.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PullKey {
    pub repo: RepoId,
    pub number: PrNumber,
}

impl PullKey {
    /// Validate what Kotlin passed.
    pub(crate) fn parse(repo: &str, number: u32) -> Result<Self, RostrumError> {
        if number == 0 {
            return Err(RostrumError::invalid("pull request numbers start at 1"));
        }
        Ok(Self {
            repo: parse_repo(repo)?,
            number: PrNumber(number),
        })
    }
}

/// Parse an `owner/name` Kotlin passed back.
pub(crate) fn parse_repo(repo: &str) -> Result<RepoId, RostrumError> {
    repo.parse()
        .map_err(|error: rostrum_core::model::ParseRepoIdError| {
            RostrumError::invalid(error.to_string())
        })
}

pub(crate) struct CoreState {
    pub config_path: PathBuf,
    pub config: Config,
    /// Problems found loading the settings file, for `warnings()`.
    pub warnings: Vec<String>,
    pub session: Session,
    /// Where GitHub requests go: github.com, except in tests.
    pub github_api: GitHubApi,
    pub feed: FeedState,
    pub remote: Option<RemoteSession>,
    /// Pending reviews loaded so far, by pull request.
    pub drafts: HashMap<PullKey, DraftBook>,
    /// Draft ids are unique for the life of the process.
    pub next_draft_id: u64,
    pub conversations: Recent<PullKey, Arc<Conversation>>,
    pub files: Recent<PullKey, Arc<LoadedFiles>>,
    /// Repository label palettes, fetched once each.
    pub labels: HashMap<RepoId, Arc<Vec<Label>>>,
    /// Merge-state re-checks per repository this poll cycle.
    pub probes: HashMap<RepoId, ProbeSlot>,
    /// The notification seen set, once loaded from SQLite.
    pub baseline: Option<Baseline>,
    pub writer: Writer,
    pub notifier: Notifier,
    /// Whether a feed observer is registered, so snapshots are only built
    /// for delivery when someone is listening.
    pub observed: bool,
    /// This actor, for background work to report back to.
    pub me: WeakActor,
}

/// What [`CoreState::new`] needs.
pub(crate) struct Startup {
    pub github_api: GitHubApi,
    pub config_path: PathBuf,
    pub config: Config,
    pub warnings: Vec<String>,
    pub writer: Writer,
    pub notifier: Notifier,
}

impl CoreState {
    pub(crate) fn new(startup: Startup, me: WeakActor) -> Self {
        let (repo_ids, repo_warnings) = startup.config.repo_ids();
        let mut warnings = startup.warnings;
        warnings.extend(repo_warnings.into_iter().map(|warning| warning.0));
        let feed = FeedState::new(repo_ids, startup.config.feed_filter());
        Self {
            config_path: startup.config_path,
            config: startup.config,
            warnings,
            session: Session::SignedOut,
            github_api: startup.github_api,
            feed,
            remote: None,
            drafts: HashMap::new(),
            next_draft_id: 1,
            conversations: Recent::new(RECENT_CONVERSATIONS),
            files: Recent::new(RECENT_DIFFS),
            labels: HashMap::new(),
            probes: HashMap::new(),
            baseline: None,
            writer: startup.writer,
            notifier: startup.notifier,
            observed: false,
            me,
        }
    }

    /// Change the settings and write them, keeping memory and disk in step:
    /// if the write fails, the change is not applied.
    pub(crate) fn edit_config(
        &mut self,
        edit: impl FnOnce(&mut Config),
    ) -> Result<(), RostrumError> {
        let mut next = self.config.clone();
        edit(&mut next);
        next.save_to(&self.config_path)?;
        self.config = next;
        Ok(())
    }

    /// The pull request as last seen in the feed. Kept after it leaves the
    /// feed (merged, closed), so the detail view outlives the refresh.
    pub(crate) fn known(&self, key: &PullKey) -> Result<&PullRequest, RostrumError> {
        self.feed
            .known
            .get(key)
            .ok_or_else(|| RostrumError::UnknownPullRequest {
                repo: key.repo.to_string(),
                number: key.number.0,
            })
    }

    /// The client for GitHub calls, or `NotSignedIn`.
    pub(crate) fn github(&self) -> Result<GitHubClient, RostrumError> {
        self.session.client().ok_or(RostrumError::NotSignedIn)
    }

    /// Record what a GitHub call's failure says about the token.
    pub(crate) fn note_github_error(&mut self, error: &GitHubError) {
        if matches!(error, GitHubError::Unauthorized) {
            self.session.reject(error.to_string());
        }
    }

    /// The feed as it stands, without counting a change.
    pub(crate) fn snapshot(&self) -> FeedSnapshot {
        self.feed.snapshot(
            self.session.viewer(),
            self.probes.values().any(ProbeSlot::is_pending),
        )
    }

    /// Count a change to the feed, deliver the new snapshot to the observer,
    /// and return it.
    pub(crate) fn publish(&mut self) -> FeedSnapshot {
        self.feed.revision += 1;
        let snapshot = self.snapshot();
        if self.observed {
            self.notifier.publish(snapshot.clone());
        }
        snapshot
    }
}
