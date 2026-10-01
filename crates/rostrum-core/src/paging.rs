//! Paging a conversation backwards, past the newest page.
//!
//! A conversation is read through up to four GraphQL connections — comments,
//! reviews, review threads and timeline events — each fetched newest-first
//! (`last: N`). A connection with more behind its first page records where to
//! continue (`startCursor`) and how many it holds in all (`totalCount`). "Load
//! earlier" fetches the previous page of every connection that has one and
//! merges it in.
//!
//! Everything here is pure, so the rules — no duplicates, chronological
//! order, threads stored once by [`ThreadId`], how many remain — are tested
//! without a network, and the desktop and the phone page identically.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::timeline::{CommentId, Conversation, ReviewId, ReviewThread, ThreadId, TimelineItem};

/// An opaque GraphQL cursor: where the previous page of a connection ends.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageCursor(pub String);

impl PageCursor {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One of the connections a conversation is read through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Connection {
    Comments,
    Reviews,
    Threads,
    Events,
}

impl Connection {
    pub const ALL: [Self; 4] = [Self::Comments, Self::Reviews, Self::Threads, Self::Events];

    fn index(self) -> usize {
        match self {
            Self::Comments => 0,
            Self::Reviews => 1,
            Self::Threads => 2,
            Self::Events => 3,
        }
    }
}

/// Whether a connection has older entries than the ones held.
///
/// `total` is GitHub's `totalCount`. How many are still to load is derived —
/// total minus what the conversation holds — rather than stored, so the count
/// shown on the button cannot drift from the items actually merged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PageState {
    /// Every entry is held. Also what a conversation cached before paging
    /// existed decodes to.
    #[default]
    Complete,
    Earlier {
        before: PageCursor,
        total: u32,
    },
}

impl PageState {
    /// The state a fetched page describes: earlier entries exist when GitHub
    /// says so and gave a cursor to reach them.
    pub fn from_page_info(
        start_cursor: Option<String>,
        has_previous_page: bool,
        total: u32,
    ) -> Self {
        match (has_previous_page, start_cursor) {
            (true, Some(cursor)) => Self::Earlier {
                before: PageCursor(cursor),
                total,
            },
            _ => Self::Complete,
        }
    }
}

/// Paging state of each connection of one conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationPaging {
    #[serde(default)]
    pub comments: PageState,
    #[serde(default)]
    pub reviews: PageState,
    #[serde(default)]
    pub threads: PageState,
    #[serde(default)]
    pub events: PageState,
}

impl ConversationPaging {
    pub fn get(&self, connection: Connection) -> &PageState {
        match connection {
            Connection::Comments => &self.comments,
            Connection::Reviews => &self.reviews,
            Connection::Threads => &self.threads,
            Connection::Events => &self.events,
        }
    }

    fn set(&mut self, connection: Connection, state: PageState) {
        match connection {
            Connection::Comments => self.comments = state,
            Connection::Reviews => self.reviews = state,
            Connection::Threads => self.threads = state,
            Connection::Events => self.events = state,
        }
    }
}

/// What one fetched page says about each connection: `Some` for a connection
/// the page included, `None` for one it left out (`@include(if: false)`),
/// whose state the page must not touch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageUpdate {
    states: [Option<PageState>; 4],
}

impl PageUpdate {
    pub fn with(mut self, connection: Connection, state: PageState) -> Self {
        self.states[connection.index()] = Some(state);
        self
    }

    pub fn get(&self, connection: Connection) -> Option<&PageState> {
        self.states[connection.index()].as_ref()
    }
}

/// Which connections the next "load earlier" must fetch, and from where.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EarlierRequest {
    cursors: [Option<PageCursor>; 4],
}

impl EarlierRequest {
    /// The cursor to page `connection` back from, or `None` when that
    /// connection is complete and must be left out of the request.
    pub fn before(&self, connection: Connection) -> Option<&PageCursor> {
        self.cursors[connection.index()].as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.cursors.iter().all(Option::is_none)
    }
}

/// Identity of a timeline item for de-duplication across pages.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ItemKey {
    Body,
    Comment(CommentId),
    Review(ReviewId),
}

fn item_key(item: &TimelineItem) -> Option<ItemKey> {
    match item {
        TimelineItem::Body { .. } => Some(ItemKey::Body),
        TimelineItem::Comment { id, .. } => Some(ItemKey::Comment(id.clone())),
        TimelineItem::Review { id, .. } => Some(ItemKey::Review(id.clone())),
        // Events carry no id on the wire; two pages agree on an event's
        // kind, actor and time, so equality is the identity.
        TimelineItem::Event { .. } => None,
    }
}

fn connection_of(item: &TimelineItem) -> Option<Connection> {
    match item {
        TimelineItem::Body { .. } => None,
        TimelineItem::Comment { .. } => Some(Connection::Comments),
        TimelineItem::Review { .. } => Some(Connection::Reviews),
        TimelineItem::Event { .. } => Some(Connection::Events),
    }
}

/// When a thread started, for ordering it against another page's threads.
fn thread_started(thread: &ReviewThread) -> Option<DateTime<Utc>> {
    thread.comments.first().map(|comment| comment.created_at)
}

impl Conversation {
    /// How many entries of `connection` this conversation holds.
    pub fn held(&self, connection: Connection) -> usize {
        match connection {
            Connection::Threads => self.threads.len(),
            other => self
                .items
                .iter()
                .filter(|item| connection_of(item) == Some(other))
                .count(),
        }
    }

    /// How many older entries remain across every connection — the N in
    /// "Load earlier (N more)".
    pub fn earlier_remaining(&self) -> u32 {
        Connection::ALL
            .iter()
            .map(|connection| match self.paging.get(*connection) {
                PageState::Complete => 0,
                PageState::Earlier { total, .. } => {
                    let held = u32::try_from(self.held(*connection)).unwrap_or(u32::MAX);
                    total.saturating_sub(held)
                }
            })
            .sum()
    }

    pub fn has_earlier(&self) -> bool {
        Connection::ALL
            .iter()
            .any(|connection| matches!(self.paging.get(*connection), PageState::Earlier { .. }))
    }

    /// The cursors the next "load earlier" pages back from.
    pub fn earlier_request(&self) -> EarlierRequest {
        let mut request = EarlierRequest::default();
        for connection in Connection::ALL {
            if let PageState::Earlier { before, .. } = self.paging.get(connection) {
                request.cursors[connection.index()] = Some(before.clone());
            }
        }
        request
    }

    /// Record what a freshly decoded page says about its connections.
    pub fn apply_page(&mut self, update: &PageUpdate) {
        for connection in Connection::ALL {
            if let Some(state) = update.get(connection) {
                self.paging.set(connection, state.clone());
            }
        }
    }

    /// Merge an earlier page into this conversation.
    ///
    /// Entries already held are skipped, so a page that overlaps — a comment
    /// posted between the two requests shifts every cursor by one — adds
    /// nothing twice. Threads are stored once by [`ThreadId`]. Reviews are
    /// relinked to the threads they opened afterwards, since a review and its
    /// threads can arrive on different pages. The result is sorted, body
    /// first, and the paging of each connection the page included advances.
    pub fn merge_earlier(&mut self, page: Conversation, update: &PageUpdate) {
        self.absorb_items(page.items, |_| true);
        self.absorb_threads(page.threads, |_| true);
        self.apply_page(update);
        self.relink_threads();
        self.sort();
    }

    /// The conversation a reload should show: `fresh` (the newest page, just
    /// fetched), keeping whatever earlier pages `self` had already loaded.
    ///
    /// Per connection: if `fresh` holds everything, it is the truth, and an
    /// entry only `self` has was deleted. If `fresh` has older entries
    /// beyond its page, the entries `self` holds from before `fresh`'s oldest
    /// are carried over, and so is `self`'s cursor, which reaches further
    /// back. Anything within `fresh`'s window that it lacks was deleted and
    /// is dropped; a deletion further back cannot be seen without refetching
    /// every page, and shows until the pane is reopened.
    pub fn refreshed_by(&self, mut fresh: Conversation) -> Conversation {
        let mut carried = HashSet::new();
        for connection in [
            Connection::Comments,
            Connection::Reviews,
            Connection::Events,
        ] {
            if !matches!(fresh.paging.get(connection), PageState::Earlier { .. }) {
                continue;
            }
            let Some(oldest) = fresh
                .items
                .iter()
                .filter(|item| connection_of(item) == Some(connection))
                .map(TimelineItem::created_at)
                .min()
            else {
                continue;
            };
            let older: Vec<TimelineItem> = self
                .items
                .iter()
                .filter(|item| {
                    connection_of(item) == Some(connection) && item.created_at() < oldest
                })
                .cloned()
                .collect();
            if !older.is_empty() {
                fresh.absorb_items(older, |_| true);
                fresh
                    .paging
                    .set(connection, self.paging.get(connection).clone());
                carried.insert(connection);
            }
        }

        if matches!(fresh.paging.threads, PageState::Earlier { .. })
            && let Some(oldest) = fresh.threads.iter().filter_map(thread_started).min()
        {
            let older: Vec<ReviewThread> = self
                .threads
                .iter()
                .filter(|thread| thread_started(thread).is_some_and(|at| at < oldest))
                .cloned()
                .collect();
            if !older.is_empty() {
                fresh.absorb_threads(older, |_| true);
                fresh.paging.threads = self.paging.threads.clone();
                carried.insert(Connection::Threads);
            }
        }

        if !carried.is_empty() {
            fresh.relink_threads();
            fresh.sort();
        }
        fresh
    }

    fn absorb_items(&mut self, items: Vec<TimelineItem>, keep: impl Fn(&TimelineItem) -> bool) {
        let mut seen: HashSet<ItemKey> = self.items.iter().filter_map(item_key).collect();
        for item in items.into_iter().filter(|item| keep(item)) {
            match item_key(&item) {
                Some(key) => {
                    if seen.insert(key) {
                        self.items.push(item);
                    }
                }
                None => {
                    if !self.items.contains(&item) {
                        self.items.push(item);
                    }
                }
            }
        }
    }

    fn absorb_threads(&mut self, threads: Vec<ReviewThread>, keep: impl Fn(&ReviewThread) -> bool) {
        let mut seen: HashSet<ThreadId> = self.threads.iter().map(|t| t.id.clone()).collect();
        let mut added: Vec<ReviewThread> = threads
            .into_iter()
            .filter(|thread| keep(thread) && seen.insert(thread.id.clone()))
            .collect();
        if added.is_empty() {
            return;
        }
        // Earlier pages hold older threads; keep the stored list oldest
        // first, as GitHub orders the connection.
        added.append(&mut self.threads);
        self.threads = added;
        self.threads
            .sort_by_key(|thread| thread_started(thread).unwrap_or(DateTime::<Utc>::MIN_UTC));
    }

    /// Point every review at the threads it opened.
    ///
    /// A thread names the review that opened it ([`ReviewThread::opening_review`]);
    /// a review's `thread_ids` are rebuilt from that, in thread order, each
    /// id at most once. A thread with no recorded opener — cached before the
    /// field existed — keeps whatever reference it already had.
    pub fn relink_threads(&mut self) {
        let threads = &self.threads;
        for item in &mut self.items {
            let TimelineItem::Review { id, thread_ids, .. } = item else {
                continue;
            };
            let existing: HashSet<ThreadId> = thread_ids.iter().cloned().collect();
            *thread_ids = threads
                .iter()
                .filter(|thread| match &thread.opening_review {
                    Some(opener) => opener == id,
                    None => existing.contains(&thread.id),
                })
                .map(|thread| thread.id.clone())
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Side,
        timeline::{EventKind, ReviewState, ThreadComment},
    };

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid timestamp")
    }

    fn body() -> TimelineItem {
        TimelineItem::Body {
            author: None,
            body: "desc".into(),
            created_at: at(0),
        }
    }

    fn comment(id: &str, secs: i64) -> TimelineItem {
        TimelineItem::Comment {
            id: CommentId(id.into()),
            author: None,
            body: id.into(),
            created_at: at(secs),
        }
    }

    fn review(id: &str, secs: i64) -> TimelineItem {
        TimelineItem::Review {
            id: ReviewId(id.into()),
            author: None,
            state: ReviewState::Commented,
            body: String::new(),
            created_at: at(secs),
            thread_ids: Vec::new(),
        }
    }

    fn event(name: &str, secs: i64) -> TimelineItem {
        TimelineItem::Event {
            kind: EventKind::Labeled { name: name.into() },
            actor: None,
            created_at: at(secs),
        }
    }

    fn thread(id: &str, secs: i64, opener: &str) -> ReviewThread {
        ReviewThread {
            id: ThreadId(id.into()),
            path: "src/lib.rs".into(),
            line: Some(1),
            original_line: Some(1),
            side: Side::Right,
            is_resolved: false,
            is_outdated: false,
            comments: vec![ThreadComment {
                id: CommentId(format!("{id}-c")),
                database_id: None,
                author: None,
                body: String::new(),
                created_at: at(secs),
            }],
            opening_review: Some(ReviewId(opener.into())),
        }
    }

    fn earlier(cursor: &str, total: u32) -> PageState {
        PageState::Earlier {
            before: PageCursor(cursor.into()),
            total,
        }
    }

    fn ids(conversation: &Conversation) -> Vec<String> {
        conversation
            .items
            .iter()
            .map(|item| match item {
                TimelineItem::Body { .. } => "body".to_string(),
                TimelineItem::Comment { id, .. } => id.0.clone(),
                TimelineItem::Review { id, .. } => id.0.clone(),
                TimelineItem::Event { kind, .. } => kind.describe(),
            })
            .collect()
    }

    /// The newest page: comments c4, c5 of five, with c1..c3 still to load.
    fn newest() -> Conversation {
        let mut conversation = Conversation {
            items: vec![body(), comment("c4", 40), comment("c5", 50)],
            ..Default::default()
        };
        conversation
            .apply_page(&PageUpdate::default().with(Connection::Comments, earlier("k4", 5)));
        conversation.sort();
        conversation
    }

    #[test]
    fn page_info_becomes_a_state() {
        assert_eq!(
            PageState::from_page_info(Some("k".into()), true, 9),
            earlier("k", 9)
        );
        assert_eq!(
            PageState::from_page_info(Some("k".into()), false, 9),
            PageState::Complete
        );
        // A previous page with no cursor cannot be reached; treat as complete
        // rather than loop on a request that cannot advance.
        assert_eq!(
            PageState::from_page_info(None, true, 9),
            PageState::Complete
        );
    }

    #[test]
    fn remaining_is_total_minus_held_across_connections() {
        let mut conversation = newest();
        assert_eq!(conversation.earlier_remaining(), 3);
        assert!(conversation.has_earlier());

        conversation.items.push(event("bug", 45));
        conversation.apply_page(&PageUpdate::default().with(Connection::Events, earlier("e", 4)));
        assert_eq!(conversation.earlier_remaining(), 3 + 3);

        let request = conversation.earlier_request();
        assert_eq!(
            request.before(Connection::Comments),
            Some(&PageCursor("k4".into()))
        );
        assert_eq!(
            request.before(Connection::Events),
            Some(&PageCursor("e".into()))
        );
        assert_eq!(request.before(Connection::Reviews), None);
        assert!(!request.is_empty());
    }

    #[test]
    fn a_complete_conversation_asks_for_nothing() {
        let conversation = Conversation {
            items: vec![body(), comment("c1", 1)],
            ..Default::default()
        };
        assert_eq!(conversation.earlier_remaining(), 0);
        assert!(!conversation.has_earlier());
        assert!(conversation.earlier_request().is_empty());
    }

    #[test]
    fn an_earlier_page_merges_in_chronological_order() {
        let mut conversation = newest();
        let page = Conversation {
            items: vec![body(), comment("c2", 20), comment("c3", 30)],
            ..Default::default()
        };
        conversation.merge_earlier(
            page,
            &PageUpdate::default().with(Connection::Comments, earlier("k2", 5)),
        );

        assert_eq!(ids(&conversation), ["body", "c2", "c3", "c4", "c5"]);
        assert_eq!(conversation.earlier_remaining(), 1);

        let last = Conversation {
            items: vec![comment("c1", 10)],
            ..Default::default()
        };
        conversation.merge_earlier(
            last,
            &PageUpdate::default().with(Connection::Comments, PageState::Complete),
        );
        assert_eq!(ids(&conversation), ["body", "c1", "c2", "c3", "c4", "c5"]);
        assert!(!conversation.has_earlier());
    }

    /// A comment posted between two requests shifts the cursor, so the
    /// earlier page repeats an entry already held. It must not appear twice;
    /// nor may an event or the body.
    #[test]
    fn overlapping_pages_add_nothing_twice() {
        let mut conversation = newest();
        conversation.items.push(event("bug", 45));
        let page = Conversation {
            items: vec![
                body(),
                comment("c3", 30),
                comment("c4", 40),
                event("bug", 45),
            ],
            ..Default::default()
        };
        conversation.merge_earlier(page, &PageUpdate::default());
        assert_eq!(
            ids(&conversation),
            ["body", "c3", "c4", "added the bug label", "c5"]
        );
    }

    /// A page that left a connection out must not reset its paging.
    #[test]
    fn connections_absent_from_a_page_keep_their_state() {
        let mut conversation = newest();
        conversation.apply_page(&PageUpdate::default().with(Connection::Events, earlier("e", 9)));
        conversation.merge_earlier(
            Conversation::default(),
            &PageUpdate::default().with(Connection::Comments, PageState::Complete),
        );
        assert_eq!(conversation.paging.comments, PageState::Complete);
        assert_eq!(conversation.paging.events, earlier("e", 9));
    }

    /// The real shape of the live fixture: the newest threads were opened by
    /// reviews that only arrive with the earlier review page. After the merge
    /// each thread is stored once and referenced by exactly its opener.
    #[test]
    fn threads_link_to_reviews_from_another_page_without_duplicates() {
        let mut conversation = Conversation {
            items: vec![body(), review("r9", 90)],
            threads: vec![thread("t5", 50, "r2"), thread("t6", 60, "r3")],
            ..Default::default()
        };
        conversation.relink_threads();
        let page = Conversation {
            items: vec![review("r2", 20), review("r3", 30)],
            threads: vec![thread("t1", 10, "r1"), thread("t5", 50, "r2")],
            ..Default::default()
        };
        conversation.merge_earlier(page, &PageUpdate::default());

        let thread_ids: Vec<&str> = conversation
            .threads
            .iter()
            .map(|t| t.id.0.as_str())
            .collect();
        assert_eq!(thread_ids, ["t1", "t5", "t6"], "stored once, oldest first");

        let links: Vec<(String, Vec<String>)> = conversation
            .items
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Review { id, thread_ids, .. } => Some((
                    id.0.clone(),
                    thread_ids.iter().map(|t| t.0.clone()).collect(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            [
                ("r2".to_string(), vec!["t5".to_string()]),
                ("r3".to_string(), vec!["t6".to_string()]),
                ("r9".to_string(), vec![]),
            ]
        );
        // t1's opener has not loaded yet; it is held, and linked by nobody.
        let referenced: usize = links.iter().map(|(_, threads)| threads.len()).sum();
        assert_eq!(referenced, 2);
    }

    /// A thread cached before openers were recorded keeps its old link.
    #[test]
    fn threads_without_a_recorded_opener_keep_their_reference() {
        let mut legacy = thread("t1", 10, "r1");
        legacy.opening_review = None;
        let mut first = review("r1", 5);
        if let TimelineItem::Review { thread_ids, .. } = &mut first {
            thread_ids.push(ThreadId("t1".into()));
        }
        let mut conversation = Conversation {
            items: vec![first],
            threads: vec![legacy],
            ..Default::default()
        };
        conversation.relink_threads();
        let TimelineItem::Review { thread_ids, .. } = &conversation.items[0] else {
            panic!("review expected");
        };
        assert_eq!(thread_ids, &[ThreadId("t1".into())]);
    }

    /// A reload brings back the newest page only. Earlier pages already
    /// loaded stay, with the cursor that reaches furthest back; a newer entry
    /// the reload no longer has was deleted and goes.
    #[test]
    fn a_reload_keeps_loaded_earlier_pages_and_drops_deleted_entries() {
        let mut loaded = newest();
        loaded.merge_earlier(
            Conversation {
                items: vec![comment("c2", 20), comment("c3", 30)],
                ..Default::default()
            },
            &PageUpdate::default().with(Connection::Comments, earlier("k2", 5)),
        );

        // c5 was deleted and c6 posted; the newest page of two is c4, c6.
        let mut fresh = Conversation {
            items: vec![body(), comment("c4", 40), comment("c6", 60)],
            ..Default::default()
        };
        fresh.apply_page(&PageUpdate::default().with(Connection::Comments, earlier("k4", 5)));

        let merged = loaded.refreshed_by(fresh);
        assert_eq!(ids(&merged), ["body", "c2", "c3", "c4", "c6"]);
        assert_eq!(merged.paging.comments, earlier("k2", 5));
        assert_eq!(merged.earlier_remaining(), 1);
    }

    /// A reload that holds everything is the whole truth.
    #[test]
    fn a_complete_reload_replaces_everything() {
        let loaded = newest();
        let fresh = Conversation {
            items: vec![body(), comment("c5", 50)],
            ..Default::default()
        };
        let merged = loaded.refreshed_by(fresh.clone());
        assert_eq!(merged, fresh);
    }

    /// Nothing loaded beyond the newest page: the reload simply wins.
    #[test]
    fn a_reload_of_an_unpaged_conversation_is_the_reload() {
        let loaded = newest();
        let mut fresh = newest();
        fresh.items.push(comment("c6", 60));
        assert_eq!(loaded.refreshed_by(fresh.clone()), fresh);
    }

    #[test]
    fn a_reload_keeps_earlier_threads_by_id() {
        let mut loaded = Conversation {
            items: vec![review("r1", 10), review("r2", 20)],
            threads: vec![thread("t1", 10, "r1"), thread("t2", 20, "r2")],
            ..Default::default()
        };
        loaded.apply_page(&PageUpdate::default().with(Connection::Threads, earlier("old", 3)));
        loaded.relink_threads();

        let mut fresh = Conversation {
            items: vec![review("r2", 20)],
            threads: vec![thread("t2", 20, "r2")],
            ..Default::default()
        };
        fresh.apply_page(
            &PageUpdate::default()
                .with(Connection::Threads, earlier("new", 3))
                .with(Connection::Reviews, earlier("rnew", 2)),
        );
        let merged = loaded.refreshed_by(fresh);
        let threads: Vec<&str> = merged.threads.iter().map(|t| t.id.0.as_str()).collect();
        assert_eq!(threads, ["t1", "t2"]);
        assert_eq!(merged.paging.threads, earlier("old", 3));
        // r1 came back with the reviews carried over, and links t1 again.
        assert!(merged.items.iter().any(|item| matches!(item,
            TimelineItem::Review { id, thread_ids, .. }
                if id.0 == "r1" && thread_ids == &[ThreadId("t1".into())])));
    }

    /// Paging is cached with the conversation; a conversation cached before
    /// paging existed decodes as complete.
    #[test]
    fn paging_round_trips_and_defaults_to_complete() {
        let conversation = newest();
        let json = serde_json::to_string(&conversation).expect("encodes");
        let back: Conversation = serde_json::from_str(&json).expect("decodes");
        assert_eq!(back, conversation);

        let legacy: Conversation =
            serde_json::from_str(r#"{"items":[],"threads":[],"checks":[]}"#).expect("decodes");
        assert_eq!(legacy.paging, ConversationPaging::default());
        assert!(!legacy.has_earlier());
    }
}
