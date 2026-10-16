//! Divergence for several repositories in one document.
//!
//! The one-repository batch ([`crate::graphql::build_divergence_batch`])
//! nested one level deeper: `rN: repository(…) { pM: ref(…) { compare(…) } }`.
//! `Ref.compare` is not a connection, so GitHub charges the document its
//! one-point minimum however many repositories and pull requests it spans
//! (checked by dry run at 40 repositories × 25 pull requests).

use std::collections::HashMap;

use rostrum_core::RepoId;

use crate::{
    batch::document::repo_alias,
    error::GraphQlError,
    graphql::{BaseRefNode, DivergenceBatchData},
};

/// One repository's comparisons, `(base_ref, head_ref)` per pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoPairs {
    pub repo: RepoId,
    pub pairs: Vec<(String, String)>,
}

/// The wire node one repository's alias decodes into: pair alias → base ref.
pub type RepoCompareNode = HashMap<String, Option<BaseRefNode>>;

fn pair_alias(index: usize) -> String {
    format!("p{index}")
}

/// The document for `repos`, each with however many pairs it has. Every
/// repository must have at least one pair (an empty selection set is a
/// syntax error); the client filters the empty ones out.
pub fn build_divergence_document(repos: &[RepoPairs]) -> String {
    let mut declarations = Vec::new();
    let mut body = String::new();
    for (r, entry) in repos.iter().enumerate() {
        declarations.push(format!("$o{r}: String!, $n{r}: String!"));
        body.push_str(&format!(
            "  {}: repository(owner: $o{r}, name: $n{r}) {{\n",
            repo_alias(r)
        ));
        for p in 0..entry.pairs.len() {
            declarations.push(format!("$r{r}b{p}: String!, $r{r}h{p}: String!"));
            body.push_str(&format!(
                "    {}: ref(qualifiedName: $r{r}b{p}) {{ compare(headRef: $r{r}h{p}) {{ aheadBy behindBy }} }}\n",
                pair_alias(p)
            ));
        }
        body.push_str("  }\n");
    }
    format!(
        "query({}) {{\n  rateLimit {{ cost remaining resetAt }}\n{body}}}\n",
        declarations.join(", ")
    )
}

/// The variables a [`build_divergence_document`] over `repos` expects.
pub fn divergence_document_variables(repos: &[RepoPairs]) -> serde_json::Value {
    let mut variables = serde_json::Map::new();
    for (r, entry) in repos.iter().enumerate() {
        variables.insert(format!("o{r}"), entry.repo.owner().into());
        variables.insert(format!("n{r}"), entry.repo.name().into());
        for (p, (base, head)) in entry.pairs.iter().enumerate() {
            variables.insert(format!("r{r}b{p}"), base.as_str().into());
            variables.insert(format!("r{r}h{p}"), head.as_str().into());
        }
    }
    serde_json::Value::Object(variables)
}

/// Whether `error` is the cross-fork answer for one pair of repository
/// `repo` (which has `pairs` pairs): a `NOT_FOUND` at `["rN", "pM", …]`.
/// That pair is `None`; the rest of the repository still answers.
pub fn excused_pair_error(error: &GraphQlError, pairs: usize) -> bool {
    if error.kind.as_deref() != Some("NOT_FOUND") {
        return false;
    }
    let Some([_, pair, ..]) = error.path.as_deref() else {
        return false;
    };
    pair.as_str()
        .is_some_and(|alias| (0..pairs).any(|index| alias == pair_alias(index)))
}

/// One repository's answers, index-aligned with the pairs it was sent.
pub fn repo_divergences(
    node: RepoCompareNode,
    pairs: usize,
) -> Vec<Option<rostrum_core::Divergence>> {
    DivergenceBatchData {
        repository: Some(node),
    }
    .into_domain(pairs)
}
