//! The conversation, render-ready: markdown flattened, events worded.

use rostrum_core::{CheckRun, Conversation, EventKind, RepoId, ReviewThread, TimelineItem, User};

use crate::{
    detail::{
        CheckRunView, ReviewThreadView, ThreadCommentView, TimelineEntry, TimelineEvent,
        TimelineKind,
    },
    feed::chips::{checks_role, checks_text, review_state_chip},
    markdown::render,
    types::UserRef,
};

/// Every timeline item, description first, oldest first.
pub(crate) fn timeline(conversation: &Conversation, repo: &RepoId) -> Vec<TimelineEntry> {
    conversation
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| entry(index, item, repo))
        .collect()
}

fn entry(index: usize, item: &TimelineItem, repo: &RepoId) -> TimelineEntry {
    let author = |user: &Option<User>| user.as_ref().map(UserRef::from);
    match item {
        TimelineItem::Body {
            author: by,
            body,
            created_at,
        } => TimelineEntry {
            id: "description".into(),
            author: author(by),
            created_at: (*created_at).into(),
            kind: TimelineKind::Description {
                body: render(body, repo),
                source: body.clone(),
            },
        },
        TimelineItem::Comment {
            id,
            author: by,
            body,
            created_at,
        } => TimelineEntry {
            id: id.0.clone(),
            author: author(by),
            created_at: (*created_at).into(),
            kind: TimelineKind::Comment {
                body: render(body, repo),
                source: body.clone(),
            },
        },
        TimelineItem::Review {
            id,
            author: by,
            state,
            body,
            created_at,
            thread_ids,
        } => TimelineEntry {
            id: id.0.clone(),
            author: author(by),
            created_at: (*created_at).into(),
            kind: TimelineKind::Review {
                state: (*state).into(),
                chip: review_state_chip(*state),
                body: render(body, repo),
                source: body.clone(),
                thread_ids: thread_ids.iter().map(|id| id.0.clone()).collect(),
            },
        },
        TimelineItem::Event {
            kind,
            actor,
            created_at,
        } => TimelineEntry {
            // Events carry no id of their own; their position is stable for
            // a given fetch, which is what list diffing needs.
            id: format!("event-{index}"),
            author: author(actor),
            created_at: (*created_at).into(),
            kind: TimelineKind::Event {
                event: event(kind),
                text: event_text(kind),
            },
        },
    }
}

fn event(kind: &EventKind) -> TimelineEvent {
    match kind {
        EventKind::Merged => TimelineEvent::Merged,
        EventKind::Closed => TimelineEvent::Closed,
        EventKind::Reopened => TimelineEvent::Reopened,
        EventKind::ReadyForReview => TimelineEvent::ReadyForReview,
        EventKind::ConvertedToDraft => TimelineEvent::ConvertedToDraft,
        EventKind::HeadRefForcePushed => TimelineEvent::ForcePushed,
        EventKind::ReviewRequested { reviewer } => TimelineEvent::ReviewRequested {
            reviewer: reviewer.clone(),
        },
        EventKind::Assigned { assignee } => TimelineEvent::Assigned {
            assignee: assignee.clone(),
        },
        EventKind::Labeled { name } => TimelineEvent::Labeled {
            label: name.clone(),
        },
        EventKind::Unlabeled { name } => TimelineEvent::Unlabeled {
            label: name.clone(),
        },
        EventKind::Renamed { from, to } => TimelineEvent::Renamed {
            from: from.clone(),
            to: to.clone(),
        },
        EventKind::Other(kind) => TimelineEvent::Other { kind: kind.clone() },
    }
}

/// The words after the actor's login — the desktop's phrasing.
fn event_text(kind: &EventKind) -> String {
    match kind {
        EventKind::Merged => "merged this".into(),
        EventKind::Closed => "closed this".into(),
        EventKind::Reopened => "reopened this".into(),
        EventKind::ReadyForReview => "marked ready for review".into(),
        EventKind::ConvertedToDraft => "converted to draft".into(),
        EventKind::HeadRefForcePushed => "force-pushed".into(),
        EventKind::ReviewRequested { reviewer } => format!("requested a review from {reviewer}"),
        EventKind::Assigned { assignee } => format!("assigned {assignee}"),
        EventKind::Labeled { name } => format!("added the {name} label"),
        EventKind::Unlabeled { name } => format!("removed the {name} label"),
        EventKind::Renamed { from, to } => format!("renamed this from “{from}” to “{to}”"),
        EventKind::Other(kind) => kind.clone(),
    }
}

/// A thread with its comments rendered.
pub(crate) fn thread_view(thread: &ReviewThread, repo: &RepoId) -> ReviewThreadView {
    ReviewThreadView {
        id: thread.id.0.clone(),
        path: thread.path.clone(),
        line: thread.line,
        original_line: thread.original_line,
        side: thread.side.into(),
        resolved: thread.is_resolved,
        outdated: thread.is_outdated,
        location: match thread.line {
            Some(line) => format!("{}:{line}", thread.path),
            None => format!("{} (outdated)", thread.path),
        },
        comments: thread
            .comments
            .iter()
            .map(|comment| ThreadCommentView {
                id: comment.id.0.clone(),
                author: comment.author.as_ref().map(UserRef::from),
                created_at: comment.created_at.into(),
                body: render(&comment.body, repo),
                source: comment.body.clone(),
            })
            .collect(),
        can_reply: thread.reply_target().is_some(),
    }
}

pub(crate) fn check_view(check: &CheckRun) -> CheckRunView {
    CheckRunView {
        name: check.name.clone(),
        state: check.state.map(Into::into),
        role: checks_role(check.state),
        status_text: checks_text(check.state).to_string(),
        url: check.url.clone(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use rostrum_core::{CheckState, CommentId, ReviewId, ReviewState, Side, ThreadComment, ThreadId};

    use super::*;
    use crate::types::ColorRole;

    fn at(secs: i64) -> chrono::DateTime<chrono::Utc> {
        DateTime::from_timestamp(secs, 0).expect("time")
    }

    fn repo() -> RepoId {
        "octo/repo".parse().expect("repo")
    }

    fn conversation() -> Conversation {
        Conversation {
            items: vec![
                TimelineItem::Body {
                    author: Some(User {
                        login: "alice".into(),
                        avatar_url: None,
                    }),
                    body: "Fixes #3".into(),
                    created_at: at(0),
                },
                TimelineItem::Review {
                    id: ReviewId("R1".into()),
                    author: None,
                    state: ReviewState::ChangesRequested,
                    body: String::new(),
                    created_at: at(10),
                    thread_ids: vec![ThreadId("T1".into())],
                },
                TimelineItem::Event {
                    kind: EventKind::Renamed {
                        from: "a".into(),
                        to: "b".into(),
                    },
                    actor: None,
                    created_at: at(20),
                },
            ],
            threads: vec![],
            checks: vec![],
            state: None,
        }
    }

    #[test]
    fn items_keep_their_order_ids_and_rendering() {
        let entries = timeline(&conversation(), &repo());
        assert_eq!(
            entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
            vec!["description", "R1", "event-2"]
        );
        let TimelineKind::Description { body, source } = &entries[0].kind else {
            panic!("description first");
        };
        assert_eq!(source, "Fixes #3");
        assert_eq!(
            body[0].spans[1].link.as_deref(),
            Some("https://github.com/octo/repo/issues/3")
        );
        let TimelineKind::Review { chip, thread_ids, .. } = &entries[1].kind else {
            panic!("review");
        };
        assert_eq!(chip.text, "requested changes");
        assert_eq!(chip.role, ColorRole::Danger);
        assert_eq!(thread_ids, &vec!["T1".to_string()]);
        let TimelineKind::Event { event, text } = &entries[2].kind else {
            panic!("event");
        };
        assert_eq!(
            event,
            &TimelineEvent::Renamed {
                from: "a".into(),
                to: "b".into()
            }
        );
        assert_eq!(text, "renamed this from “a” to “b”");
    }

    #[test]
    fn every_event_has_words() {
        for (kind, words) in [
            (EventKind::Merged, "merged this"),
            (EventKind::HeadRefForcePushed, "force-pushed"),
            (
                EventKind::ReviewRequested {
                    reviewer: "bob".into(),
                },
                "requested a review from bob",
            ),
            (
                EventKind::Labeled { name: "bug".into() },
                "added the bug label",
            ),
            (EventKind::Other("PinnedEvent".into()), "PinnedEvent"),
        ] {
            assert_eq!(event_text(&kind), words);
        }
    }

    #[test]
    fn threads_say_where_they_are_and_whether_they_take_replies() {
        let mut thread = ReviewThread {
            id: ThreadId("T1".into()),
            path: "src/lib.rs".into(),
            line: Some(12),
            original_line: Some(10),
            side: Side::Left,
            is_resolved: true,
            is_outdated: false,
            comments: vec![ThreadComment {
                id: CommentId("C1".into()),
                database_id: Some(77),
                author: None,
                body: "`nit`".into(),
                created_at: at(5),
            }],
        };
        let view = thread_view(&thread, &repo());
        assert_eq!(view.location, "src/lib.rs:12");
        assert!(view.can_reply);
        assert!(view.resolved);
        assert!(view.comments[0].body[0].spans[0].code);

        thread.line = None;
        thread.comments[0].database_id = None;
        let outdated = thread_view(&thread, &repo());
        assert_eq!(outdated.location, "src/lib.rs (outdated)");
        assert!(!outdated.can_reply);
    }

    #[test]
    fn checks_carry_a_role_and_a_word() {
        let view = check_view(&CheckRun {
            name: "ci".into(),
            state: Some(CheckState::Failure),
            url: Some("https://ci".into()),
        });
        assert_eq!(view.role, ColorRole::Danger);
        assert_eq!(view.status_text, "failure");
        let silent = check_view(&CheckRun {
            name: "lint".into(),
            state: None,
            url: None,
        });
        assert_eq!(silent.status_text, "no status");
    }
}
