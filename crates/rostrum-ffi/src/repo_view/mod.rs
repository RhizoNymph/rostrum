//! One repository's screen: its pull requests and issues together, unfiltered,
//! and its branch tree.
//!
//! The lists are the feed's own data, so they cost no request: the feed
//! fetches every watched repository's pull requests, issues and stacks. Order
//! and stack grouping are `rostrum_core`'s (`repo_pull_rows`,
//! `order_issues`), the same as the desktop's repository view. The branch
//! tree is the one network call here, built exactly as the desktop builds it:
//! `rostrum_core::branches` resolves the trunks, plans the comparisons and
//! places every pull request; this module only fetches and converts.
//!
//! Stack actions from the phone (through the paired desktop) are a later
//! phase; they will hang off `PullItem::Stack` and this module without
//! changing the shapes here.

mod tree;
mod types;

use rostrum_core::{
    IssueIx, RepoId, RepoState,
    branches::{
        BranchCounts, ComparePlan, RepoMeta as BranchMeta, TrunkChoice, TrunkName, Trunks,
        build_tree,
    },
    repo_pull_rows,
    sort::order_issues,
};

pub use types::{
    BranchDrift, BranchNote, BranchRow, BranchTree, RepoOverview, TrunkDrift, TrunkSettings,
};

use crate::{
    engine::{
        RostrumCore,
        state::{CoreState, parse_repo},
    },
    error::RostrumError,
    feed::load_of,
    issues::summary::summarize_issue,
    stacks::PullItems,
};

/// The repository's page when nothing better is known.
fn github_url(repo: &RepoId) -> String {
    format!("https://github.com/{repo}")
}

fn watched<'a>(state: &'a CoreState, repo: &RepoId) -> Result<&'a RepoState, RostrumError> {
    state
        .feed
        .repos
        .iter()
        .find(|watched| &watched.id == repo)
        .ok_or_else(|| RostrumError::invalid(format!("{repo} is not in the repository list")))
}

/// The overview record from the feed's state for one repository.
fn overview(state: &CoreState, repo: &RepoState) -> RepoOverview {
    let viewer = state.session.viewer().map(rostrum_core::User::key);
    let viewer = viewer.as_ref();
    let sort = state.feed.filter.sort.items;
    let (rows, stacks) = repo_pull_rows(repo, sort);
    let mut pulls = PullItems::new(repo, &stacks, viewer);
    for row in rows {
        pulls.push(row);
    }
    let mut order: Vec<IssueIx> = (0..repo.issues.len()).map(IssueIx).collect();
    order_issues(&repo.issues, &mut order, sort);
    let branch = state.branch_meta.get(&repo.id);
    RepoOverview {
        repo: repo.id.to_string(),
        url: branch
            .map(|meta| meta.url.clone())
            .unwrap_or_else(|| github_url(&repo.id)),
        stars: branch
            .map(|meta| meta.stars)
            .or_else(|| repo.meta.as_ref().map(|meta| meta.stars)),
        default_branch: branch
            .and_then(|meta| meta.default_branch.as_ref())
            .map(|name| name.as_str().to_string()),
        pulls: pulls.finish(),
        issues: order
            .into_iter()
            .map(|ix| summarize_issue(&repo.id, &repo.issues[ix.0], viewer))
            .collect(),
        pulls_load: load_of(&repo.load),
        issues_load: load_of(&repo.issues_load),
    }
}

/// The trunk list Kotlin sent, checked: `None` is "detect", a list is taken
/// in order with repeats dropped. A name git would refuse is `InvalidInput`
/// naming it, and nothing is saved.
pub(crate) fn parse_trunks(names: Option<Vec<String>>) -> Result<TrunkChoice, RostrumError> {
    let Some(names) = names else {
        return Ok(TrunkChoice::Detected);
    };
    let mut parsed: Vec<TrunkName> = Vec::with_capacity(names.len());
    for raw in &names {
        let name = TrunkName::parse(raw).map_err(|error| {
            RostrumError::invalid(format!("`{raw}` is not a branch name: {error}"))
        })?;
        if !parsed.contains(&name) {
            parsed.push(name);
        }
    }
    Ok(TrunkChoice::Configured(parsed))
}

pub(crate) fn trunk_settings(choice: &TrunkChoice, meta: Option<&BranchMeta>) -> TrunkSettings {
    let names = |names: &mut dyn Iterator<Item = &TrunkName>| {
        names
            .map(|name| name.as_str().to_string())
            .collect::<Vec<_>>()
    };
    TrunkSettings {
        detected: matches!(choice, TrunkChoice::Detected),
        configured: match choice {
            TrunkChoice::Detected => Vec::new(),
            TrunkChoice::Configured(list) => names(&mut list.iter()),
        },
        existing: meta
            .map(|meta| names(&mut meta.existing.iter()))
            .unwrap_or_default(),
    }
}

fn choice_of(state: &CoreState, repo: &RepoId) -> TrunkChoice {
    let (choice, warnings) = state.config.trunk_choice(repo);
    for warning in warnings {
        tracing::warn!(%repo, warning = %warning.0, "trunk setting skipped");
    }
    choice
}

/// What the branch fetch brings back: the facts, and, for a repository with a
/// default branch, the trunks and counts.
struct Fetched {
    meta: BranchMeta,
    ready: Option<(Trunks, BranchCounts)>,
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// A watched repository's screen: every open pull request (stacks
    /// grouped) and issue, unfiltered, in the item sort. No network: it is
    /// what the feed last fetched. `InvalidInput` for a repository not in the
    /// list.
    pub async fn repo_overview(&self, repo: String) -> Result<RepoOverview, RostrumError> {
        let id = parse_repo(&repo)?;
        self.actor
            .try_call(move |state| Ok(overview(state, watched(state, &id)?)))
            .await
    }

    /// Fetch a watched repository's branch tree: trunks with their distance
    /// from the default branch, and every open pull request under the branch
    /// it targets with its ahead/behind. Two GitHub requests.
    pub async fn branch_tree(&self, repo: String) -> Result<BranchTree, RostrumError> {
        let id = parse_repo(&repo)?;
        let lookup = id.clone();
        let (client, choice, prs) = self
            .actor
            .try_call(move |state| {
                let prs = watched(state, &lookup)?.prs.clone();
                Ok((state.github()?, choice_of(state, &lookup), prs))
            })
            .await?;

        let probe = choice.names_to_probe();
        let meta = self
            .github(client.repo_branch_meta(&id, &probe).await)
            .await?;
        let fetched = match meta.default_branch.clone() {
            None => Fetched { meta, ready: None },
            Some(default) => {
                let trunks = Trunks::resolve(default, &choice, &meta.existing);
                let plan = ComparePlan::new(&trunks, &prs);
                let answers = self
                    .github(client.divergences(&id, plan.pairs()).await)
                    .await?;
                let counts = plan
                    .answer(answers)
                    .map_err(|error| RostrumError::internal(error.to_string()))?;
                Fetched {
                    meta,
                    ready: Some((trunks, counts)),
                }
            }
        };
        tracing::info!(repo = %id, trunks = fetched.ready.as_ref().map_or(0, |(trunks, _)| trunks.names().count()), "branch tree fetched");

        self.actor
            .try_call(move |state| {
                let Fetched { meta, ready } = fetched;
                state.branch_meta.insert(id.clone(), meta.clone());
                let repo = watched(state, &id)?;
                let viewer = state.session.viewer().map(rostrum_core::User::key);
                let rows = match &ready {
                    Some((trunks, counts)) => tree::rows(
                        &build_tree(trunks, &repo.prs, counts),
                        repo,
                        viewer.as_ref(),
                    ),
                    None => Vec::new(),
                };
                Ok(BranchTree {
                    repo: id.to_string(),
                    url: meta.url.clone(),
                    stars: meta.stars,
                    default_branch: meta
                        .default_branch
                        .as_ref()
                        .map(|name| name.as_str().to_string()),
                    trunks: trunk_settings(&choice, Some(&meta)),
                    rows,
                })
            })
            .await
    }

    /// A repository's trunk setting. `existing` is filled once `branch_tree`
    /// has run this session.
    pub async fn trunks(&self, repo: String) -> Result<TrunkSettings, RostrumError> {
        let id = parse_repo(&repo)?;
        self.actor
            .call(move |state| trunk_settings(&choice_of(state, &id), state.branch_meta.get(&id)))
            .await
    }

    /// Set a repository's trunks: `None` to detect them, or the names in
    /// order (empty means the default branch alone). Persisted beside the
    /// desktop's setting. Call `branch_tree` again to see the effect.
    pub async fn set_trunks(
        &self,
        repo: String,
        names: Option<Vec<String>>,
    ) -> Result<TrunkSettings, RostrumError> {
        let id = parse_repo(&repo)?;
        let choice = parse_trunks(names)?;
        self.change_config(
            move |_, config| {
                config.set_trunk_choice(&id, &choice);
                Ok((id, choice))
            },
            |state, (id, choice)| {
                tracing::info!(repo = %id, detected = matches!(choice, TrunkChoice::Detected), "trunks set");
                Ok(trunk_settings(&choice, state.branch_meta.get(&id)))
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn trunk(name: &str) -> TrunkName {
        TrunkName::parse(name).expect("trunk")
    }

    #[test]
    fn no_list_means_detect_and_a_list_is_kept_in_order_without_repeats() {
        assert_eq!(parse_trunks(None).expect("detect"), TrunkChoice::Detected);
        assert_eq!(
            parse_trunks(Some(vec![
                "staging".into(),
                "main".into(),
                "staging".into()
            ]))
            .expect("list"),
            TrunkChoice::Configured(vec![trunk("staging"), trunk("main")])
        );
        assert_eq!(
            parse_trunks(Some(Vec::new())).expect("empty"),
            TrunkChoice::Configured(Vec::new())
        );
    }

    #[test]
    fn a_name_git_would_refuse_is_invalid_input_naming_it() {
        let error =
            parse_trunks(Some(vec!["main".into(), "bad..name".into()])).expect_err("refused");
        assert!(
            matches!(&error, RostrumError::InvalidInput { reason } if reason.contains("bad..name")),
            "{error:?}"
        );
        assert!(parse_trunks(Some(vec!["  ".into()])).is_err());
    }

    #[test]
    fn settings_report_the_choice_and_what_exists() {
        let meta = BranchMeta {
            url: "https://github.com/a/b".into(),
            stars: 3,
            default_branch: Some(trunk("main")),
            existing: BTreeSet::from([trunk("main"), trunk("develop")]),
        };
        let detected = trunk_settings(&TrunkChoice::Detected, Some(&meta));
        assert!(detected.detected);
        assert!(detected.configured.is_empty());
        assert_eq!(detected.existing, vec!["develop", "main"]);
        let configured = trunk_settings(&TrunkChoice::Configured(vec![trunk("release")]), None);
        assert!(!configured.detected);
        assert_eq!(configured.configured, vec!["release"]);
        assert!(configured.existing.is_empty());
    }
}
