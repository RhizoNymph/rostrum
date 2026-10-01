//! Paging against captured responses.
//!
//! The fixtures under `fixtures/paging/` were captured from the live API with
//! the documents in this crate at `pageSize: 3`, so a small file holds both a
//! newest page with `hasPreviousPage: true` and the page before it:
//!
//! - `issue_newest.json` / `issue_earlier_comments.json`: rust-lang/rust#126600
//!   (156 comments, 28 events), then the earlier comments alone.
//! - `pr_newest.json` / `pr_earlier_reviews_threads.json`: rust-lang/rust#162858,
//!   then the earlier reviews and threads alone. The newest threads were
//!   opened by reviews that only arrive on the earlier review page.

use std::collections::HashSet;

use rostrum_core::{
    Connection, Conversation, EarlierRequest, IssueNumber, PageCursor, PageState, PrNumber, RepoId,
    ThreadId, TimelineItem,
};
use serde_json::{Value, json};

use crate::{
    client::conversation_variables,
    conversation::{ConversationQueryData, PULL_REQUEST_CONVERSATION},
    graphql::GraphQlResponse,
    issues::{
        issue_variables,
        wire::{ISSUE_DETAIL, IssueDetailData},
    },
};

const ISSUE_NEWEST: &str = include_str!("../fixtures/paging/issue_newest.json");
const ISSUE_EARLIER: &str = include_str!("../fixtures/paging/issue_earlier_comments.json");
const PR_NEWEST: &str = include_str!("../fixtures/paging/pr_newest.json");
const PR_EARLIER: &str = include_str!("../fixtures/paging/pr_earlier_reviews_threads.json");

fn issue_page(body: &str) -> (Conversation, rostrum_core::PageUpdate) {
    let response: GraphQlResponse<IssueDetailData> =
        serde_json::from_str(body).expect("fixture decodes");
    assert!(response.errors.is_empty());
    let (_, conversation, update) = response
        .data
        .and_then(|data| data.repository)
        .and_then(|repository| repository.issue)
        .expect("issue present")
        .into_page();
    (conversation, update)
}

fn pr_page(body: &str) -> (Conversation, rostrum_core::PageUpdate) {
    let response: GraphQlResponse<ConversationQueryData> =
        serde_json::from_str(body).expect("fixture decodes");
    assert!(response.errors.is_empty());
    response
        .data
        .and_then(|data| data.repository)
        .and_then(|repository| repository.pull_request)
        .expect("pull request present")
        .into_page()
}

fn newest(page: (Conversation, rostrum_core::PageUpdate)) -> Conversation {
    let (mut conversation, update) = page;
    conversation.apply_page(&update);
    conversation
}

fn comment_ids(conversation: &Conversation) -> Vec<String> {
    conversation
        .items
        .iter()
        .filter_map(|item| match item {
            TimelineItem::Comment { id, .. } => Some(id.0.clone()),
            _ => None,
        })
        .collect()
}

fn cursor(state: &PageState) -> &str {
    match state {
        PageState::Earlier { before, .. } => before.as_str(),
        PageState::Complete => panic!("expected earlier pages"),
    }
}

/// Every `$name` a document declares.
fn declared(document: &str) -> HashSet<String> {
    let header = &document[..document.find('{').expect("document has a body")];
    header
        .split('$')
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .collect()
}

fn keys(variables: &Value) -> HashSet<String> {
    variables
        .as_object()
        .expect("variables are an object")
        .keys()
        .cloned()
        .collect()
}

// --- cursor query construction --------------------------------------------

#[test]
fn the_newest_page_includes_every_connection_from_its_newest_end() {
    let variables = conversation_variables(&RepoId::new("a", "b"), PrNumber(7), None);
    assert_eq!(
        variables,
        json!({
            "owner": "a", "name": "b", "number": 7, "pageSize": 100,
            "withComments": true, "commentsBefore": null,
            "withReviews": true, "reviewsBefore": null,
            "withThreads": true, "threadsBefore": null,
            "withEvents": true, "eventsBefore": null,
        })
    );
}

/// An earlier page asks only for the connections with somewhere to go, each
/// from its own cursor; a complete connection is switched off rather than
/// asked for from its newest end, which would fetch its newest page again.
#[test]
fn an_earlier_page_includes_only_connections_with_a_cursor() {
    let (conversation, update) = issue_page(ISSUE_NEWEST);
    let mut conversation = conversation;
    conversation.apply_page(&update);
    // Pretend the events are all loaded.
    let mut request = conversation.earlier_request();
    assert!(request.before(Connection::Events).is_some());
    conversation.merge_earlier(
        Conversation::default(),
        &rostrum_core::PageUpdate::default().with(Connection::Events, PageState::Complete),
    );
    request = conversation.earlier_request();

    let variables = issue_variables(
        &RepoId::new("rust-lang", "rust"),
        IssueNumber(126600),
        Some(&request),
    );
    assert_eq!(variables["withComments"], json!(true));
    assert_eq!(
        variables["commentsBefore"],
        json!("Y3Vyc29yOnYyOpHOnFSHBg==")
    );
    assert_eq!(variables["withEvents"], json!(false));
    assert_eq!(variables["eventsBefore"], Value::Null);
}

/// The variables the builders produce are exactly the ones each document
/// declares — GitHub rejects an undeclared or unbound variable.
#[test]
fn the_variables_match_each_documents_declarations() {
    let request = EarlierRequest::default();
    let pr = conversation_variables(&RepoId::new("a", "b"), PrNumber(1), Some(&request));
    assert_eq!(keys(&pr), declared(PULL_REQUEST_CONVERSATION));
    let issue = issue_variables(&RepoId::new("a", "b"), IssueNumber(1), None);
    assert_eq!(keys(&issue), declared(ISSUE_DETAIL));
}

/// Each long connection is read newest-first from its cursor, switched by
/// its flag, and reports where it starts.
#[test]
fn every_paged_connection_reads_backwards_and_reports_its_page() {
    for (document, connections) in [
        (
            PULL_REQUEST_CONVERSATION,
            &[
                ("comments", "comments"),
                ("reviews", "reviews"),
                ("reviewThreads", "threads"),
                ("timelineItems", "events"),
            ][..],
        ),
        (
            ISSUE_DETAIL,
            &[("comments", "comments"), ("timelineItems", "events")][..],
        ),
    ] {
        for (field, stem) in connections {
            let start = document
                .find(&format!("{field}(last: $pageSize, before: ${stem}Before"))
                .unwrap_or_else(|| panic!("{field} is not paged backwards"));
            let rest = &document[start..];
            let switch = format!("@include(if: $with{}", capitalise(stem));
            assert!(rest.contains(&switch), "{field} lacks {switch}");
            assert!(
                rest.contains("pageInfo { startCursor hasPreviousPage }"),
                "{field} lacks pageInfo"
            );
        }
        // Top-level connections sit at six spaces; a review thread's own
        // comments, nested deeper, are bounded per thread and not paged.
        for (field, _) in connections {
            assert!(
                !document.contains(&format!("\n      {field}(first:")),
                "{field} is still read from the oldest end"
            );
        }
    }
}

fn capitalise(stem: &str) -> String {
    let mut chars = stem.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

// --- decoding with hasPreviousPage -----------------------------------------

#[test]
fn a_captured_newest_issue_page_records_its_cursors_and_totals() {
    let conversation = newest(issue_page(ISSUE_NEWEST));
    assert_eq!(
        conversation.paging.comments,
        PageState::Earlier {
            before: PageCursor("Y3Vyc29yOnYyOpHOnFSHBg==".into()),
            total: 156
        }
    );
    assert!(matches!(
        conversation.paging.events,
        PageState::Earlier { total: 28, .. }
    ));
    assert_eq!(comment_ids(&conversation).len(), 3);
    assert_eq!(conversation.earlier_remaining(), (156 - 3) + (28 - 3));
}

#[test]
fn a_captured_earlier_issue_page_carries_only_what_was_asked_for() {
    let (page, update) = issue_page(ISSUE_EARLIER);
    assert!(update.get(Connection::Comments).is_some());
    assert!(
        update.get(Connection::Events).is_none(),
        "events were left out"
    );
    assert_eq!(comment_ids(&page).len(), 3);
}

// --- merging across pages --------------------------------------------------

#[test]
fn captured_issue_pages_merge_in_order_without_duplicates() {
    let mut conversation = newest(issue_page(ISSUE_NEWEST));
    let events_before = conversation.paging.events.clone();
    let newest_ids = comment_ids(&conversation);

    let (page, update) = issue_page(ISSUE_EARLIER);
    conversation.merge_earlier(page.clone(), &update);
    // Merging the same page again — a double click — changes nothing.
    let once = conversation.clone();
    conversation.merge_earlier(page, &update);
    assert_eq!(conversation, once);

    let ids = comment_ids(&conversation);
    assert_eq!(ids.len(), 6);
    assert_eq!(
        &ids[3..],
        newest_ids.as_slice(),
        "the newest page stays last"
    );
    let unique: HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
    let stamps: Vec<_> = conversation.items[1..]
        .iter()
        .map(TimelineItem::created_at)
        .collect();
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]), "not chronological");

    assert_eq!(
        cursor(&conversation.paging.comments),
        "Y3Vyc29yOnYyOpHOkQDonw=="
    );
    assert_eq!(conversation.paging.events, events_before);
    assert_eq!(conversation.earlier_remaining(), (156 - 6) + (28 - 3));
}

/// The live shape that motivates relinking: on the newest page the threads'
/// opening reviews are absent, so no review lists them; once the earlier
/// review page lands, each thread is listed by exactly its opener, and no
/// thread is stored twice.
#[test]
fn captured_pr_pages_link_threads_to_reviews_from_the_earlier_page() {
    let mut conversation = newest(pr_page(PR_NEWEST));
    for connection in Connection::ALL {
        assert!(
            matches!(
                conversation.paging.get(connection),
                PageState::Earlier { .. }
            ),
            "{connection:?}"
        );
    }
    let linked = |conversation: &Conversation| -> Vec<ThreadId> {
        conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Review { thread_ids, .. } => Some(thread_ids.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    };
    assert!(linked(&conversation).is_empty(), "openers not loaded yet");

    let (page, update) = pr_page(PR_EARLIER);
    assert!(update.get(Connection::Comments).is_none());
    assert!(update.get(Connection::Events).is_none());
    conversation.merge_earlier(page, &update);

    assert_eq!(conversation.threads.len(), 6);
    let stored: HashSet<_> = conversation.threads.iter().map(|t| t.id.clone()).collect();
    assert_eq!(stored.len(), 6, "a thread stored twice");

    let links = linked(&conversation);
    let unique: HashSet<_> = links.iter().collect();
    assert_eq!(unique.len(), links.len(), "a thread listed by two reviews");
    for newest_thread in [
        "PRRT_kwDOAAsO6M6kLyAn",
        "PRRT_kwDOAAsO6M6kSyWc",
        "PRRT_kwDOAAsO6M6oAsPw",
    ] {
        assert!(
            links.contains(&ThreadId(newest_thread.into())),
            "{newest_thread} was not linked to its opener"
        );
    }
    assert_eq!(conversation.held(Connection::Reviews), 6);
    assert_eq!(
        conversation.earlier_remaining(),
        (40 - 3) + (42 - 6) + (22 - 6) + (124 - 3)
    );
}
