use chrono::{DateTime, Utc};
use rostrum_core::{EndingError, PrNumber, PullEnding};

use super::wire::{ClosedNodeError, ClosedPullsData, DecodedClosed};
use super::*;
use crate::graphql::GraphQlResponse;

/// Captured from the live API (`zed-industries/zed`, `first: 10`) with
/// [`CLOSED_PULL_REQUESTS`], unedited apart from pretty-printing. It holds
/// merges and closes, a closed draft, forks, and pull requests updated long
/// after they ended (#65455, #55292, #60582) — which is what makes update
/// order and ending order differ.
const ZED: &str = include_str!("../../fixtures/closed/closed_zed.json");

fn decode(body: &str) -> DecodedClosed {
    let response: GraphQlResponse<ClosedPullsData> =
        serde_json::from_str(body).expect("response should decode");
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    response
        .data
        .expect("data present")
        .into_domain()
        .expect("repository present")
}

fn at(text: &str) -> DateTime<Utc> {
    text.parse().expect("valid timestamp")
}

#[test]
fn the_document_asks_for_both_endings_in_update_order() {
    assert!(CLOSED_PULL_REQUESTS.contains("states: [MERGED, CLOSED]"));
    assert!(CLOSED_PULL_REQUESTS.contains("orderBy: {field: UPDATED_AT, direction: DESC}"));
    for field in ["state", "mergedAt", "closedAt", "headRefOid", "id"] {
        assert!(
            CLOSED_PULL_REQUESTS.contains(&format!("\n        {field}\n")),
            "{field} is not selected"
        );
    }
}

#[test]
fn the_fixture_decodes_every_node() {
    let decoded = decode(ZED);
    assert_eq!(decoded.closed.len(), 10);
    assert!(decoded.rejected.is_empty(), "{:?}", decoded.rejected);
}

#[test]
fn merges_end_at_merged_at_and_closes_at_closed_at() {
    let decoded = decode(ZED);
    let find = |number: u32| {
        decoded
            .closed
            .iter()
            .find(|closed| closed.number() == PrNumber(number))
            .unwrap_or_else(|| panic!("#{number} is in the fixture"))
    };
    assert_eq!(
        find(65607).ending,
        PullEnding::Merged {
            at: at("2026-10-14T19:58:12Z")
        }
    );
    assert_eq!(
        find(65606).ending,
        PullEnding::Closed {
            at: at("2026-10-14T18:58:04Z")
        }
    );
    // Closed on the 12th, updated on the 14th: ends when it closed.
    assert_eq!(find(65455).ended_at(), at("2026-10-12T11:42:41Z"));
    assert_eq!(find(65455).pull.updated_at, at("2026-10-14T18:43:26Z"));
}

#[test]
fn the_shared_fields_decode_as_they_do_for_open_pull_requests() {
    let decoded = decode(ZED);
    let draft = decoded
        .closed
        .iter()
        .find(|closed| closed.number() == PrNumber(65606))
        .expect("present");
    assert!(draft.pull.is_draft);
    assert!(!draft.pull.node_id.as_str().is_empty());
    assert!(!draft.pull.head_sha.is_empty());
    assert!(draft.pull.author.is_some());
    assert!(
        decoded
            .closed
            .iter()
            .any(|closed| closed.pull.is_cross_repository)
    );
    // Never fetched for a closed pull request.
    assert!(
        decoded
            .closed
            .iter()
            .all(|closed| closed.pull.checks.is_none())
    );
}

/// Fetch order is update order: #55292 merged at 09:00 but was updated at
/// 18:25, so it arrives ahead of #65177, which merged at 17:50.
#[test]
fn the_list_arrives_in_update_order_not_ending_order() {
    let decoded = decode(ZED);
    let updated: Vec<_> = decoded
        .closed
        .iter()
        .map(|closed| closed.pull.updated_at)
        .collect();
    assert!(updated.windows(2).all(|pair| pair[0] >= pair[1]));
    let ended: Vec<_> = decoded
        .closed
        .iter()
        .map(|closed| closed.ended_at())
        .collect();
    assert!(!ended.windows(2).all(|pair| pair[0] >= pair[1]));
}

fn node(state: &str, merged_at: Option<&str>, closed_at: Option<&str>) -> String {
    let time = |value: Option<&str>| value.map_or("null".to_string(), |v| format!("\"{v}\""));
    format!(
        r#"{{
          "id": "PR_x", "number": 7, "title": "t", "url": "u", "state": "{state}",
          "isDraft": false, "createdAt": "2026-01-01T00:00:00Z",
          "updatedAt": "2026-01-02T00:00:00Z",
          "closedAt": {closed}, "mergedAt": {merged},
          "author": null, "headRefName": "h", "headRefOid": "abc", "baseRefName": "main",
          "isCrossRepository": false, "additions": 1, "deletions": 2, "changedFiles": 3,
          "reviewDecision": null,
          "assignees": {{ "nodes": [] }}, "labels": {{ "nodes": [] }},
          "comments": {{ "totalCount": 0 }}
        }}"#,
        closed = time(closed_at),
        merged = time(merged_at),
    )
}

fn list(nodes: &[String]) -> String {
    format!(
        r#"{{ "data": {{ "rateLimit": null, "repository": {{ "pullRequests": {{ "nodes": [{}] }} }} }} }}"#,
        nodes.join(",")
    )
}

/// One malformed node is dropped with its reason; its neighbours still list.
#[test]
fn an_inconsistent_node_is_rejected_alone() {
    let decoded = decode(&list(&[
        node("MERGED", None, Some("2026-01-02T00:00:00Z")),
        node("CLOSED", None, Some("2026-01-02T00:00:00Z")),
        node("OPEN", None, None),
        node("SOMETHING_NEW", None, None),
    ]));
    assert_eq!(decoded.closed.len(), 1);
    assert_eq!(
        decoded.rejected,
        vec![
            ClosedNodeError::Ending {
                number: 7,
                source: EndingError::MergedWithoutTime
            },
            ClosedNodeError::Ending {
                number: 7,
                source: EndingError::StillOpen
            },
            ClosedNodeError::UnknownState { number: 7 },
        ]
    );
}

#[test]
fn a_withheld_repository_is_none() {
    let response: GraphQlResponse<ClosedPullsData> =
        serde_json::from_str(r#"{ "data": { "rateLimit": null, "repository": null } }"#)
            .expect("decodes");
    assert!(response.data.expect("data").into_domain().is_none());
}

#[test]
fn a_null_connection_is_an_empty_list() {
    let response: GraphQlResponse<ClosedPullsData> = serde_json::from_str(
        r#"{ "data": { "repository": { "pullRequests": { "nodes": null } } } }"#,
    )
    .expect("decodes");
    let decoded = response.data.expect("data").into_domain().expect("repo");
    assert!(decoded.closed.is_empty());
    assert!(decoded.rejected.is_empty());
}
