//! The repository view's branch data: what the repository's default branch
//! is, which trunk names exist, and the repository's page and stars.
//!
//! The comparisons themselves go through the same aliased `Ref.compare`
//! batch the feed uses ([`GitHubClient::divergences`]), fed by
//! [`rostrum_core::branches::ComparePlan`]; this module covers only the
//! question that has to be answered first — which branches there are to
//! compare.

use std::collections::{BTreeSet, HashMap};

use rostrum_core::{
    RepoId,
    branches::{RepoMeta, TrunkName},
};
use serde::Deserialize;

use crate::{GitHubClient, error::GitHubError};

/// The document asking for a repository's page, stars, default branch, and
/// whether each of `count` branch names exists.
///
/// Each name is its own aliased `ref` lookup — `r0..rN`, bound to `$r0..$rN`
/// — so one request answers for every candidate. As in the divergence batch,
/// the names travel as variables and the document is a function of `count`
/// alone, so no branch name can change its shape.
pub fn build_branch_meta_query(count: usize) -> String {
    let mut declarations = String::from("$owner: String!, $name: String!");
    let mut selections = String::new();
    for index in 0..count {
        declarations.push_str(&format!(", ${}: String!", ref_alias(index)));
        selections.push_str(&format!(
            "    {alias}: ref(qualifiedName: ${alias}) {{ name }}\n",
            alias = ref_alias(index)
        ));
    }
    format!(
        "query({declarations}) {{\n  repository(owner: $owner, name: $name) {{\n    url\n    stargazerCount\n    defaultBranchRef {{ name }}\n{selections}  }}\n}}\n"
    )
}

/// The variables a [`build_branch_meta_query`] document of the same length
/// expects. Names are sent fully qualified (`refs/heads/…`) so a tag that
/// shares a branch's name cannot answer for it.
pub fn branch_meta_variables(owner: &str, name: &str, probe: &[TrunkName]) -> serde_json::Value {
    let mut variables = serde_json::Map::new();
    variables.insert("owner".into(), owner.into());
    variables.insert("name".into(), name.into());
    for (index, branch) in probe.iter().enumerate() {
        variables.insert(ref_alias(index), branch.qualified().into());
    }
    serde_json::Value::Object(variables)
}

fn ref_alias(index: usize) -> String {
    format!("r{index}")
}

/// Response to a [`build_branch_meta_query`] document.
#[derive(Debug, Deserialize)]
pub struct BranchMetaData {
    /// `null` when the repository does not exist or is not visible.
    pub repository: Option<BranchMetaRepository>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMetaRepository {
    pub url: String,
    pub stargazer_count: u32,
    /// `null` for a repository with no commits.
    pub default_branch_ref: Option<RefNameNode>,
    /// The `rN` aliases. `null` for a name with no such branch — GitHub
    /// answers a missing ref with `null` and no error.
    #[serde(flatten)]
    pub refs: HashMap<String, Option<RefNameNode>>,
}

#[derive(Debug, Deserialize)]
pub struct RefNameNode {
    pub name: String,
}

impl BranchMetaRepository {
    /// What the repository says, with `probe[i]` reported as existing when
    /// alias `ri` resolved.
    ///
    /// A default branch whose name does not parse is reported as absent and
    /// logged, rather than failing the view: GitHub would not hold such a
    /// branch, so it can only mean the name rules here are too strict.
    pub fn into_domain(mut self, probe: &[TrunkName]) -> RepoMeta {
        let existing: BTreeSet<TrunkName> = probe
            .iter()
            .enumerate()
            .filter(|(index, _)| self.refs.remove(&ref_alias(*index)).flatten().is_some())
            .map(|(_, name)| name.clone())
            .collect();
        let default_branch = self
            .default_branch_ref
            .and_then(|node| match TrunkName::parse(&node.name) {
                Ok(name) => Some(name),
                Err(error) => {
                    tracing::warn!(branch = %node.name, %error, "default branch name did not parse");
                    None
                }
            });
        RepoMeta {
            url: self.url,
            stars: self.stargazer_count,
            default_branch,
            existing,
        }
    }
}

impl GitHubClient {
    /// The repository's page, stars and default branch, and which of
    /// `probe` exist as branches, in one request.
    pub async fn repo_branch_meta(
        &self,
        repo: &RepoId,
        probe: &[TrunkName],
    ) -> Result<RepoMeta, GitHubError> {
        let document = build_branch_meta_query(probe.len());
        let variables = branch_meta_variables(repo.owner(), repo.name(), probe);
        let data: BranchMetaData = self
            .graphql(&document, variables, &format!("{repo} branches"))
            .await?;
        let repository = data.repository.ok_or_else(|| GitHubError::NotFound {
            resource: repo.to_string(),
        })?;
        Ok(repository.into_domain(probe))
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{
        Divergence, PrNumber, PullRequest,
        branches::{ComparePlan, TrunkChoice, Trunks},
    };

    use super::*;
    use crate::graphql::{
        DivergenceBatchData, GraphQlResponse, divergence_batch_variables, unexcused_batch_errors,
    };

    fn name(raw: &str) -> TrunkName {
        TrunkName::parse(raw).expect("valid test name")
    }

    // --- the meta query ------------------------------------------------------

    #[test]
    fn the_meta_document_declares_one_aliased_ref_per_probed_name() {
        let document = build_branch_meta_query(3);
        for index in 0..3 {
            assert!(
                document.contains(&format!("$r{index}: String!")),
                "{document}"
            );
            assert!(
                document.contains(&format!(
                    "r{index}: ref(qualifiedName: $r{index}) {{ name }}"
                )),
                "{document}"
            );
        }
        assert!(!document.contains("$r3"), "{document}");
        assert!(document.contains("defaultBranchRef { name }"));
        assert!(document.contains("stargazerCount"));
        assert!(document.contains("repository(owner: $owner, name: $name)"));
        assert_eq!(document.matches("String!").count(), 2 + 3);
    }

    #[test]
    fn a_meta_document_with_nothing_to_probe_still_asks_for_the_default() {
        let document = build_branch_meta_query(0);
        assert!(document.contains("defaultBranchRef { name }"));
        assert!(!document.contains("ref(qualifiedName"));
    }

    /// Every declared variable is supplied, names are qualified, and no name
    /// reaches the document text.
    #[test]
    fn the_meta_variables_match_the_declarations_and_carry_qualified_names() {
        let probe = [name("main"), name("release/2.0")];
        let document = build_branch_meta_query(probe.len());
        let variables = branch_meta_variables("o", "n", &probe);
        let object = variables.as_object().expect("an object");
        for key in ["owner", "name", "r0", "r1"] {
            assert!(object.contains_key(key), "missing {key}");
            assert!(document.contains(&format!("${key}: String!")), "{key}");
        }
        assert_eq!(object.len(), 4);
        assert_eq!(object["r0"], "refs/heads/main");
        assert_eq!(object["r1"], "refs/heads/release/2.0");
        assert!(!document.contains("main"));
        assert!(!document.contains("release"));
    }

    #[test]
    fn a_meta_response_decodes_with_existing_names_by_alias() {
        let body = r#"{
          "data": {
            "repository": {
              "url": "https://github.com/o/n",
              "stargazerCount": 1234,
              "defaultBranchRef": { "name": "main" },
              "r0": { "name": "main" },
              "r1": null,
              "r2": { "name": "staging" },
              "r3": null
            }
          }
        }"#;
        let probe = [
            name("main"),
            name("master"),
            name("staging"),
            name("develop"),
        ];
        let response: GraphQlResponse<BranchMetaData> =
            serde_json::from_str(body).expect("should decode");
        let meta = response
            .data
            .and_then(|data| data.repository)
            .expect("repository present")
            .into_domain(&probe);
        assert_eq!(meta.url, "https://github.com/o/n");
        assert_eq!(meta.stars, 1234);
        assert_eq!(meta.default_branch, Some(name("main")));
        assert_eq!(
            meta.existing,
            BTreeSet::from([name("main"), name("staging")])
        );
    }

    /// A repository with no commits has no default branch; an alias GitHub
    /// left out entirely reads as "no such branch", not a decode failure.
    #[test]
    fn an_empty_repository_decodes_without_a_default_branch() {
        let body = r#"{
          "data": {
            "repository": {
              "url": "https://github.com/o/empty",
              "stargazerCount": 0,
              "defaultBranchRef": null,
              "r0": null
            }
          }
        }"#;
        let response: GraphQlResponse<BranchMetaData> =
            serde_json::from_str(body).expect("should decode");
        let meta = response
            .data
            .and_then(|data| data.repository)
            .expect("repository present")
            .into_domain(&[name("main"), name("master")]);
        assert_eq!(meta.default_branch, None);
        assert!(meta.existing.is_empty());
    }

    #[test]
    fn an_invisible_repository_decodes_as_absent() {
        let body = r#"{ "data": { "repository": null } }"#;
        let response: GraphQlResponse<BranchMetaData> =
            serde_json::from_str(body).expect("should decode");
        assert!(response.data.expect("data").repository.is_none());
    }

    // --- the comparison batch, as the branch view builds it ---------------------

    fn pr(number: u32, base: &str, head: &str) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: rostrum_core::NodeId(format!("PR_{number}")),
            title: String::new(),
            url: String::new(),
            is_draft: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            author: None,
            head_ref: head.into(),
            head_sha: String::new(),
            base_ref: base.into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: rostrum_core::Mergeable::Unknown,
            merge_state: rostrum_core::MergeStateStatus::Unknown,
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

    fn plan() -> ComparePlan {
        let trunks = Trunks::resolve(
            name("main"),
            &TrunkChoice::Configured(vec![name("staging")]),
            &BTreeSet::from([name("main"), name("staging")]),
        );
        ComparePlan::new(
            &trunks,
            &[pr(12, "staging", "feat/x"), pr(13, "main", "fork-head")],
        )
    }

    /// Trunk comparisons and pull request comparisons share one document:
    /// the trunk against the default branch first, then each pull request.
    #[test]
    fn the_branch_plan_binds_trunks_then_pulls_to_the_batch_variables() {
        let plan = plan();
        let variables = divergence_batch_variables("o", "n", plan.pairs());
        let object = variables.as_object().expect("an object");
        assert_eq!(object["b0"], "main");
        assert_eq!(object["h0"], "staging");
        assert_eq!(object["b1"], "staging");
        assert_eq!(object["h1"], "feat/x");
        assert_eq!(object["b2"], "main");
        assert_eq!(object["h2"], "fork-head");
        assert_eq!(object.len(), 2 + 3 * 2);
    }

    /// A cross-fork head answers `null` with a per-alias `NOT_FOUND`: the
    /// batch tolerates it and the pull request files as unknown, while the
    /// trunk and the other pull request keep their counts.
    #[test]
    fn a_branch_batch_with_a_cross_fork_head_files_it_as_unknown() {
        let body = r#"{
          "data": {
            "repository": {
              "p0": { "compare": { "aheadBy": 5, "behindBy": 2 } },
              "p1": { "compare": { "aheadBy": 1, "behindBy": 3 } },
              "p2": { "compare": null }
            }
          },
          "errors": [{
            "type": "NOT_FOUND",
            "path": ["repository", "p2", "compare"],
            "message": "Could not resolve to a Ref with the name 'fork-head'."
          }]
        }"#;
        let plan = plan();
        let response: GraphQlResponse<DivergenceBatchData> =
            serde_json::from_str(body).expect("should decode");
        assert!(unexcused_batch_errors(response.errors, plan.len()).is_empty());
        let answers = response.data.expect("data").into_domain(plan.len());
        let counts = plan.answer(answers).expect("aligned");
        assert_eq!(counts.trunk(&name("staging")), Some(Divergence::new(5, 2)));
        assert_eq!(counts.pull(PrNumber(12)), Some(Divergence::new(1, 3)));
        assert_eq!(counts.pull(PrNumber(13)), None);
    }
}
