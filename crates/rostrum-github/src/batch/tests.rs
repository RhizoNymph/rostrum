//! Batched documents against captured answers.
//!
//! The three fixtures were captured live with [`build_repo_batch`]'s
//! documents at `first: 3` over zed-industries/zed, rust-lang/rust and a
//! repository that does not exist — so each carries two answered aliases and
//! a `NOT_FOUND` scoped to `["r2"]` — unedited apart from pretty-printing.
//! Each cost one point.

use std::collections::BTreeSet;

use rostrum_core::{PrNumber, RepoId};
use serde_json::{Value, json};

use super::{
    FetchedPulls, RepoBatch,
    decode::{BatchData, split_batch},
    divergence::{
        RepoCompareNode, RepoPairs, build_divergence_document, divergence_document_variables,
        excused_pair_error, repo_divergences,
    },
    document::{alias_index, build_repo_batch, repo_alias, repo_batch_variables},
};
use crate::{
    ci::wire::{CI_CHECKS, CI_CHECKS_SELECTION, CiChecksData, CiRepositoryNode},
    error::GitHubError,
    graphql::{
        GraphQlResponse, OPEN_PULL_REQUESTS, OPEN_PULLS_SELECTION, PrNode, RepositoryNode,
        is_rate_limited, response_cost,
    },
    issues::wire::{IssuesRepositoryNode, OPEN_ISSUES, OPEN_ISSUES_SELECTION, OpenIssuesData},
};

const PULLS: &str = include_str!("../../fixtures/batch/open_pulls_batch.json");
const ISSUES: &str = include_str!("../../fixtures/batch/open_issues_batch.json");
const CHECKS: &str = include_str!("../../fixtures/batch/ci_checks_batch.json");

fn repos() -> Vec<RepoId> {
    [
        "zed-industries/zed",
        "rust-lang/rust",
        "RhizoNymph/no-such-repository-rostrum-fixture",
    ]
    .iter()
    .map(|name| name.parse().expect("valid repo id"))
    .collect()
}

fn numbers<T>(items: &[T], number: impl Fn(&T) -> u32) -> Vec<u32> {
    items.iter().map(number).collect()
}

fn decode_pulls(body: &str) -> Result<RepoBatch<FetchedPulls>, GitHubError> {
    let response: GraphQlResponse<BatchData<RepositoryNode>> =
        serde_json::from_str(body).expect("pulls batch decodes");
    split_batch(
        &repos(),
        response,
        |_, _| false,
        |_, node| {
            Some(FetchedPulls {
                meta: node.meta.into_domain(),
                pull_requests: node
                    .pull_requests
                    .into_vec()
                    .into_iter()
                    .map(PrNode::into_domain)
                    .collect(),
            })
        },
    )
}

/// The pulls fixture with `edit` applied to its JSON.
fn edited(body: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut value: Value = serde_json::from_str(body).expect("fixture is JSON");
    edit(&mut value);
    value.to_string()
}

// --- documents ------------------------------------------------------------

/// Every `$name` the document uses is declared, and the variables bind
/// exactly the declared names — the contract the document and its variables
/// are built to keep.
fn declared(document: &str) -> BTreeSet<String> {
    let header = &document[document.find('(').expect("has variables") + 1
        ..document.find(") {").expect("header ends")];
    header
        .split(',')
        .map(|declaration| {
            declaration
                .trim()
                .trim_start_matches('$')
                .split(':')
                .next()
                .expect("named")
                .to_string()
        })
        .collect()
}

fn bound(variables: &Value) -> BTreeSet<String> {
    variables
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn a_batch_aliases_each_repository_once() {
    let document = build_repo_batch(OPEN_ISSUES_SELECTION, 3, "");
    for index in 0..3 {
        assert!(document.contains(&format!(
            "r{index}: repository(owner: $o{index}, name: $n{index})"
        )));
    }
    assert!(!document.contains("r3:"));
    assert_eq!(document.matches("issues(states: OPEN").count(), 3);
    assert_eq!(document.matches("rateLimit").count(), 1);
    assert!(!document.contains("viewer"));
}

#[test]
fn the_feed_batch_asks_for_the_viewer_once() {
    let document = build_repo_batch(OPEN_PULLS_SELECTION, 2, "viewer { login avatarUrl }");
    assert_eq!(document.matches("viewer { login avatarUrl }").count(), 1);
}

#[test]
fn variables_bind_exactly_what_the_document_declares() {
    let document = build_repo_batch(CI_CHECKS_SELECTION, 3, "");
    let variables = repo_batch_variables(&repos(), 25);
    assert_eq!(declared(&document), bound(&variables));
    assert_eq!(variables["first"], 25);
    assert_eq!(variables["o1"], "rust-lang");
    assert_eq!(variables["n1"], "rust");
}

/// The batched selections are the one-repository documents' selections,
/// not copies that could drift.
#[test]
fn batches_select_what_the_single_documents_select() {
    assert!(OPEN_PULL_REQUESTS.contains(OPEN_PULLS_SELECTION));
    assert!(OPEN_ISSUES.contains(OPEN_ISSUES_SELECTION));
    assert!(CI_CHECKS.contains(CI_CHECKS_SELECTION));
}

#[test]
fn aliases_round_trip() {
    assert_eq!(alias_index(&repo_alias(7), 8), Some(7));
    assert_eq!(alias_index("r7", 7), None, "out of range");
    assert_eq!(
        alias_index("r01", 8),
        None,
        "not an alias this module writes"
    );
    assert_eq!(alias_index("rateLimit", 8), None);
    assert_eq!(alias_index("viewer", 8), None);
}

// --- decoding the captured answers ----------------------------------------

#[test]
fn the_rate_limit_and_viewer_are_not_mistaken_for_repositories() {
    let response: GraphQlResponse<BatchData<RepositoryNode>> =
        serde_json::from_str(PULLS).expect("decodes");
    let data = response.data.expect("data");
    assert_eq!(data.repos.len(), 3);
    assert_eq!(data.rate_limit.map(|limit| limit.cost), Some(1));
    assert!(data.viewer.is_some());
}

#[test]
fn a_pulls_batch_answers_per_repository() {
    let batch = decode_pulls(PULLS).expect("batch applies");
    assert_eq!(
        batch.viewer.map(|viewer| viewer.login),
        Some("RhizoNymph".into())
    );
    let [(zed_id, zed), (rust_id, rust), (gone_id, gone)] =
        <[_; 3]>::try_from(batch.results).expect("three results");
    assert_eq!((zed_id, rust_id, gone_id.clone()), {
        let r = repos();
        (r[0].clone(), r[1].clone(), r[2].clone())
    });
    let zed = zed.expect("zed answered");
    assert_eq!(
        numbers(&zed.pull_requests, |pr| pr.number.0),
        [65721, 65720, 59842]
    );
    assert!(zed.meta.is_some());
    assert!(zed.pull_requests.iter().all(|pr| !pr.head_sha.is_empty()));
    let rust = rust.expect("rust answered");
    assert_eq!(
        numbers(&rust.pull_requests, |pr| pr.number.0),
        [164438, 162916, 163948]
    );
    assert!(
        matches!(gone, Err(GitHubError::NotFound { resource }) if resource == gone_id.to_string())
    );
}

#[test]
fn an_issues_batch_answers_per_repository() {
    let response: GraphQlResponse<BatchData<IssuesRepositoryNode>> =
        serde_json::from_str(ISSUES).expect("decodes");
    let batch = split_batch(
        &repos(),
        response,
        |_, _| false,
        |_, node| {
            OpenIssuesData {
                rate_limit: None,
                repository: Some(node),
            }
            .into_domain()
        },
    )
    .expect("batch applies");
    assert_eq!(batch.rate_limit.map(|limit| limit.cost), Some(1));
    let results: Vec<_> = batch
        .results
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    let zed = results[0].as_ref().expect("zed answered");
    assert_eq!(numbers(zed, |issue| issue.number.0), [65710, 15138, 13104]);
    let rust = results[1].as_ref().expect("rust answered");
    assert_eq!(
        numbers(rust, |issue| issue.number.0),
        [164415, 164379, 164433]
    );
    assert!(matches!(results[2], Err(GitHubError::NotFound { .. })));
}

#[test]
fn a_checks_batch_answers_per_repository() {
    let response: GraphQlResponse<BatchData<CiRepositoryNode>> =
        serde_json::from_str(CHECKS).expect("decodes");
    let batch = split_batch(
        &repos(),
        response,
        |_, _| false,
        |_, node| {
            CiChecksData {
                rate_limit: None,
                repository: Some(node),
            }
            .into_domain()
        },
    )
    .expect("batch applies");
    let results: Vec<_> = batch
        .results
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    let zed = results[0].as_ref().expect("zed answered");
    assert_eq!(numbers(zed, |pr| pr.number.0), [65721, 65720, 59842]);
    assert!(zed.iter().all(|pr| !pr.head_sha.is_empty()));
    assert!(!zed[0].entries.is_empty(), "contexts decoded");
    let rust = results[1].as_ref().expect("rust answered");
    assert_eq!(rust.len(), 3);
    assert!(rust.iter().any(|pr| !pr.entries.is_empty()));
    assert!(matches!(results[2], Err(GitHubError::NotFound { .. })));
}

#[test]
fn a_forbidden_alias_fails_that_repository_alone() {
    let body = edited(PULLS, |value| {
        value["data"]["r1"] = Value::Null;
        value["errors"] = json!([{
            "type": "FORBIDDEN",
            "path": ["r1"],
            "message": "Resource protected by organization SAML enforcement."
        }]);
    });
    let batch = decode_pulls(&body).expect("batch applies");
    let results: Vec<_> = batch
        .results
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    assert!(results[0].is_ok());
    assert!(
        matches!(&results[1], Err(GitHubError::Forbidden { reason }) if reason.contains("SAML"))
    );
    assert!(
        matches!(results[2], Err(GitHubError::NotFound { .. })),
        "a null alias with no error of its own is NotFound"
    );
}

#[test]
fn an_error_inside_an_alias_fails_that_repository_with_githubs_words() {
    let body = edited(PULLS, |value| {
        value["errors"] = json!([{
            "type": "SOMETHING_NEW",
            "path": ["r0", "pullRequests", "nodes", 0, "author"],
            "message": "something new went wrong"
        }]);
    });
    let batch = decode_pulls(&body).expect("batch applies");
    let results: Vec<_> = batch
        .results
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    assert!(matches!(&results[0], Err(GitHubError::GraphQl { errors }) if errors.len() == 1));
    assert!(results[1].is_ok());
}

#[test]
fn an_error_outside_every_alias_fails_the_batch() {
    for path in [Value::Null, json!(["rateLimit"]), json!(["r9"])] {
        let body = edited(PULLS, |value| {
            value["errors"] = json!([{ "type": "INTERNAL", "path": path, "message": "boom" }]);
        });
        assert!(
            matches!(decode_pulls(&body), Err(GitHubError::GraphQl { .. })),
            "{path}"
        );
    }
}

#[test]
fn a_batch_without_data_is_empty() {
    assert!(matches!(
        decode_pulls(r#"{"data": null}"#),
        Err(GitHubError::EmptyData)
    ));
}

#[test]
fn the_cost_is_read_from_the_answer() {
    assert_eq!(response_cost(PULLS), 1);
    assert_eq!(
        response_cost(
            r#"{"data":{"rateLimit":{"cost":7,"remaining":1,"resetAt":"2026-01-01T00:00:00Z"}}}"#
        ),
        7
    );
    assert_eq!(
        response_cost(r#"{"data":{"viewer":null}}"#),
        1,
        "unselected: the minimum"
    );
    assert_eq!(response_cost("not json"), 1);
}

#[test]
fn a_rate_limited_answer_is_recognised() {
    let response: GraphQlResponse<Value> = serde_json::from_str(
        r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#,
    )
    .expect("decodes");
    assert!(is_rate_limited(&response.errors));
    let other: GraphQlResponse<Value> = serde_json::from_str(PULLS).expect("decodes");
    assert!(!is_rate_limited(&other.errors));
}

// --- divergence across repositories ---------------------------------------

fn pairs() -> Vec<RepoPairs> {
    let r = repos();
    vec![
        RepoPairs {
            repo: r[0].clone(),
            pairs: vec![
                ("refs/heads/main".into(), "feature".into()),
                ("main".into(), "fork-branch".into()),
            ],
        },
        RepoPairs {
            repo: r[1].clone(),
            pairs: vec![("master".into(), "x\"y{z".into())],
        },
    ]
}

#[test]
fn the_divergence_document_nests_pairs_under_repositories() {
    let document = build_divergence_document(&pairs());
    assert!(document.contains("r0: repository(owner: $o0, name: $n0)"));
    assert!(document.contains("p1: ref(qualifiedName: $r0b1) { compare(headRef: $r0h1)"));
    assert!(document.contains("r1: repository(owner: $o1, name: $n1)"));
    assert!(!document.contains("r1b1"));
    assert!(
        !document.contains("x\"y"),
        "branch names travel as variables"
    );
    let variables = divergence_document_variables(&pairs());
    assert_eq!(declared(&document), bound(&variables));
    assert_eq!(variables["r1h0"], "x\"y{z");
}

#[test]
fn a_cross_fork_pair_is_excused_and_a_missing_repository_is_not() {
    let error = |path: Value, kind: &str| -> crate::error::GraphQlError {
        serde_json::from_value(json!({ "type": kind, "path": path, "message": "m" }))
            .expect("error decodes")
    };
    assert!(excused_pair_error(
        &error(json!(["r0", "p1", "compare"]), "NOT_FOUND"),
        2
    ));
    assert!(!excused_pair_error(
        &error(json!(["r0", "p2", "compare"]), "NOT_FOUND"),
        2
    ));
    assert!(!excused_pair_error(&error(json!(["r0"]), "NOT_FOUND"), 2));
    assert!(!excused_pair_error(
        &error(json!(["r0", "p0"]), "FORBIDDEN"),
        2
    ));
}

#[test]
fn a_divergence_batch_answers_per_repository() {
    let body = json!({
        "data": {
            "rateLimit": { "cost": 1, "remaining": 4000, "resetAt": "2026-01-01T00:00:00Z" },
            "r0": {
                "p0": { "compare": { "aheadBy": 2, "behindBy": 5 } },
                "p1": { "compare": null }
            },
            "r1": null
        },
        "errors": [
            { "type": "NOT_FOUND", "path": ["r0", "p1", "compare"], "message": "No commit found" },
            { "type": "NOT_FOUND", "path": ["r1"], "message": "Could not resolve" }
        ]
    });
    let response: GraphQlResponse<BatchData<RepoCompareNode>> =
        serde_json::from_value(body).expect("decodes");
    let asked = pairs();
    let ids: Vec<RepoId> = asked.iter().map(|entry| entry.repo.clone()).collect();
    let batch = split_batch(
        &ids,
        response,
        |index, error| excused_pair_error(error, asked[index].pairs.len()),
        |index, node| Some(repo_divergences(node, asked[index].pairs.len())),
    )
    .expect("batch applies");
    let results: Vec<_> = batch
        .results
        .into_iter()
        .map(|(_, result)| result)
        .collect();
    let first = results[0].as_ref().expect("first repository answered");
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].map(|d| (d.ahead, d.behind)), Some((2, 5)));
    assert_eq!(first[1], None, "the fork pair is unknown, not an error");
    assert!(matches!(results[1], Err(GitHubError::NotFound { .. })));
}

#[test]
fn pr_numbers_survive_the_batch() {
    let batch = decode_pulls(PULLS).expect("batch applies");
    let (_, zed) = &batch.results[0];
    assert_eq!(
        zed.as_ref().map(|z| z.pull_requests[0].number).ok(),
        Some(PrNumber(65721))
    );
}
