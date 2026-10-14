//! The single pull request lookup: its document selects what the feed
//! selects, and its answer decodes into the same `PullRequest`.

use serde_json::{Value, json};

use crate::graphql::{GraphQlResponse, OPEN_PULL_REQUESTS, PULL_REQUEST, PrNode, PullQueryData};

/// `cli/cli`, as captured for the feed's own tests.
const ORG_REPO: &str = include_str!("../fixtures/feed_org_repo.json");

/// The fixture's first pull request node, as raw JSON.
fn first_node() -> Value {
    let body: Value = serde_json::from_str(ORG_REPO).expect("fixture is JSON");
    body["data"]["repository"]["pullRequests"]["nodes"][0].clone()
}

fn decode(body: Value) -> Option<rostrum_core::PullRequest> {
    let response: GraphQlResponse<PullQueryData> =
        serde_json::from_value(body).expect("answer should decode");
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    response.data.expect("data present").into_domain()
}

#[test]
fn the_lookup_selects_every_field_the_feed_selects() {
    for field in [
        "id",
        "headRefOid",
        "isCrossRepository",
        "mergeStateStatus",
        "reviewRequests(first: 10)",
        "labels(first: 10)",
        "statusCheckRollup { state }",
        "HEAD_REF_FORCE_PUSHED_EVENT",
    ] {
        assert!(OPEN_PULL_REQUESTS.contains(field), "feed lacks {field}");
        assert!(PULL_REQUEST.contains(field), "lookup lacks {field}");
    }
    assert!(PULL_REQUEST.contains("pullRequest(number: $number)"));
    // Any state: a link may name a pull request merged since.
    assert!(!PULL_REQUEST.contains("states:"));
}

#[test]
fn a_found_pull_request_decodes_as_the_feed_decodes_it() {
    let node = first_node();
    let looked_up = decode(json!({"data": {"repository": {"pullRequest": node.clone()}}}))
        .expect("pull request present");
    let from_feed = serde_json::from_value::<PrNode>(node)
        .expect("feed node decodes")
        .into_domain();
    assert_eq!(looked_up, from_feed);
    assert_eq!(looked_up.number.0, 13894);
    assert_eq!(
        looked_up.head_sha,
        "8f44336ccb889465115aa85319c275c8dec1c3b7"
    );
}

#[test]
fn a_missing_repository_or_number_is_none() {
    assert_eq!(decode(json!({"data": {"repository": null}})), None);
    assert_eq!(
        decode(json!({"data": {"repository": {"pullRequest": null}}})),
        None
    );
}
