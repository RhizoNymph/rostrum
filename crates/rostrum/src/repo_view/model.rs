//! One repository's branch data: its page and stars, its trunks, and the
//! counts that measure them and its pull requests.
//!
//! Fetched lazily — when the repository view opens — and again whenever the
//! feed's poll lands a refresh for the repository, for as long as the view
//! is open. Dropping the entity (leaving the view) cancels whatever is in
//! flight.

use chrono::{DateTime, Utc};
use gpui::{Context, Entity, Subscription, Task};
use gpui_tokio::Tokio;
use rostrum_config::Warning;
use rostrum_core::{
    LoadState, PullRequest, RepoId,
    branches::{
        BranchCounts, BranchTree, ComparePlan, PlanError, RepoMeta, TrunkChoice, TrunkName, Trunks,
        build_tree,
    },
};
use rostrum_github::{GitHubClient, GitHubError};

use crate::sync::Store;

/// Why the branch data could not be fetched.
#[derive(Debug, thiserror::Error)]
pub enum BranchFetchError {
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error("the comparison batch came back misaligned: {0}")]
    Plan(#[from] PlanError),
    #[error("the fetch did not complete: {0}")]
    Join(String),
}

/// The repository's branches, once known.
#[derive(Clone, Debug)]
pub enum Branches {
    /// No commits yet, so no default branch to measure anything from.
    Empty,
    Ready {
        trunks: Trunks,
        counts: BranchCounts,
    },
}

/// Everything one fetch learns.
#[derive(Clone, Debug)]
pub struct BranchSnapshot {
    pub meta: RepoMeta,
    pub branches: Branches,
}

/// Where the latest fetch stands. A failure keeps the previous snapshot on
/// screen, so it is reported beside the data rather than instead of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchStatus {
    Idle,
    Fetching,
    Failed(String),
}

pub struct RepoBranches {
    store: Entity<Store>,
    pub repo: RepoId,
    snapshot: Option<BranchSnapshot>,
    status: FetchStatus,
    /// The repository refresh the current data was fetched after. A newer
    /// one landing is what triggers the next fetch.
    seen_refresh: Option<DateTime<Utc>>,
    /// The in-flight fetch; replacing it cancels the previous one.
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl RepoBranches {
    pub fn new(store: Entity<Store>, repo: RepoId, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |this, _, cx| this.store_changed(cx))];
        let mut this = Self {
            store,
            seen_refresh: None,
            repo,
            snapshot: None,
            status: FetchStatus::Idle,
            task: None,
            _subscriptions: subscriptions,
        };
        this.seen_refresh = this.repo_refreshed_at(cx);
        this.fetch(cx);
        this
    }

    pub fn snapshot(&self) -> Option<&BranchSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn status(&self) -> &FetchStatus {
        &self.status
    }

    /// The tree over the repository's current pull requests.
    ///
    /// Built per call rather than stored: the pull requests come from the
    /// store and change with every poll, while the counts are keyed by
    /// identity, so pairing the latest of each is always right.
    pub fn tree(&self, prs: &[PullRequest]) -> Option<BranchTree> {
        match &self.snapshot.as_ref()?.branches {
            Branches::Empty => None,
            Branches::Ready { trunks, counts } => Some(build_tree(trunks, prs, counts)),
        }
    }

    /// The trunks besides the default branch, as last resolved.
    pub fn other_trunks(&self) -> Vec<TrunkName> {
        match self.snapshot.as_ref().map(|snapshot| &snapshot.branches) {
            Some(Branches::Ready { trunks, .. }) => trunks
                .others()
                .iter()
                .map(|other| other.name.clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn repo_refreshed_at(&self, cx: &Context<Self>) -> Option<DateTime<Utc>> {
        match self.store.read(cx).state.repo(&self.repo)?.load {
            LoadState::Loaded { at } => Some(at),
            _ => None,
        }
    }

    /// Follow the feed's poll: a newer refresh of this repository means its
    /// pull requests may have moved, so the counts are fetched again.
    fn store_changed(&mut self, cx: &mut Context<Self>) {
        let refreshed = self.repo_refreshed_at(cx);
        if refreshed.is_some() && refreshed != self.seen_refresh {
            self.seen_refresh = refreshed;
            self.fetch(cx);
        }
        cx.notify();
    }

    /// Fetch everything again, replacing any fetch in flight.
    pub fn fetch(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let Some(client) = store.client() else {
            // Auth has not resolved; the refresh that follows it will land
            // here through `store_changed`.
            return;
        };
        let (choice, warnings) = store.config.trunk_choice(&self.repo);
        for Warning(warning) in warnings {
            tracing::debug!(repo = %self.repo, warning, "trunk configuration");
        }
        let prs = store
            .state
            .repo(&self.repo)
            .map(|repo| repo.prs.clone())
            .unwrap_or_default();
        let repo = self.repo.clone();

        self.status = FetchStatus::Fetching;
        cx.notify();
        tracing::debug!(repo = %repo, prs = prs.len(), "fetching branches");

        self.task = Some(cx.spawn(async move |this, cx| {
            let outcome = Tokio::spawn(&*cx, fetch_branches(client, repo, choice, prs)).await;
            let result = match outcome {
                Ok(result) => result,
                Err(error) => Err(BranchFetchError::Join(error.to_string())),
            };
            this.update(cx, |this, cx| this.apply(result, cx)).ok();
        }));
    }

    fn apply(&mut self, result: Result<BranchSnapshot, BranchFetchError>, cx: &mut Context<Self>) {
        match result {
            Ok(snapshot) => {
                tracing::debug!(repo = %self.repo, "branches updated");
                self.snapshot = Some(snapshot);
                self.status = FetchStatus::Idle;
            }
            Err(error) => {
                tracing::warn!(repo = %self.repo, %error, "branch fetch failed");
                self.status = FetchStatus::Failed(error.to_string());
            }
        }
        self.task = None;
        cx.notify();
    }

    /// Change the repository's trunks, save the choice, and re-fetch so the
    /// new names are probed and compared.
    pub fn set_choice(&mut self, choice: TrunkChoice, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.store
            .update(cx, |store, cx| store.set_trunk_choice(&repo, &choice, cx));
        self.fetch(cx);
    }

    pub fn choice(&self, cx: &gpui::App) -> TrunkChoice {
        self.store.read(cx).config.trunk_choice(&self.repo).0
    }
}

/// The two requests behind the view: which branches exist, then every
/// comparison in one aliased batch.
async fn fetch_branches(
    client: GitHubClient,
    repo: RepoId,
    choice: TrunkChoice,
    prs: Vec<PullRequest>,
) -> Result<BranchSnapshot, BranchFetchError> {
    let probe = choice.names_to_probe();
    let meta = client.repo_branch_meta(&repo, &probe).await?;
    let Some(default) = meta.default_branch.clone() else {
        return Ok(BranchSnapshot {
            meta,
            branches: Branches::Empty,
        });
    };
    let trunks = Trunks::resolve(default, &choice, &meta.existing);
    let plan = ComparePlan::new(&trunks, &prs);
    let answers = client.divergences(&repo, plan.pairs()).await?;
    let counts = plan.answer(answers)?;
    Ok(BranchSnapshot {
        meta,
        branches: Branches::Ready { trunks, counts },
    })
}

impl Store {
    /// Record a repository's trunk choice in the config file.
    pub fn set_trunk_choice(
        &mut self,
        repo: &RepoId,
        choice: &TrunkChoice,
        cx: &mut Context<Self>,
    ) {
        self.config.set_trunk_choice(repo, choice);
        self.persist_config();
        cx.notify();
    }
}
