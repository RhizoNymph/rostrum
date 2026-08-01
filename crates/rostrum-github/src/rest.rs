//! REST v3 types for the mutating half of the review flow.
//!
//! GraphQL is used for reads, but GitHub's review mutations are far simpler over
//! REST (and `addPullRequestReview` in GraphQL cannot attach multi-line comments
//! without a preview header), so writes go through `api.github.com/repos/...`.

use rostrum_core::Side;
use serde::{Deserialize, Serialize};

/// One entry of `GET /repos/{owner}/{repo}/pulls/{number}/files`.
///
/// Deserialised directly from the REST payload: every field name already
/// matches, so there is no separate wire type to convert from.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PullRequestFile {
    pub filename: String,
    /// Present only when `status` is `renamed` or `copied`.
    pub previous_filename: Option<String>,
    /// GitHub's lowercase status: `added`, `removed`, `modified`, `renamed`,
    /// `copied`, `changed`, `unchanged`. Passed through untouched rather than
    /// parsed into an enum, so a status GitHub adds later cannot fail a decode.
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    /// Absent for binary files and for files whose diff GitHub omitted (very
    /// large diffs, or a response already at the 3000-file cap).
    pub patch: Option<String>,
}

impl PullRequestFile {
    /// The path this file had before the change, which differs from `filename`
    /// only for renames and copies.
    pub fn old_path(&self) -> &str {
        self.previous_filename.as_deref().unwrap_or(&self.filename)
    }

    /// Whether GitHub gave us a diff to render.
    pub fn has_patch(&self) -> bool {
        self.patch.is_some()
    }
}

/// The verdict attached to a submitted review.
///
/// `Pending` is deliberately absent: a pending review is one that was never
/// submitted, so it is not reachable through this API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewEvent {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
            Self::Comment => "COMMENT",
        }
    }
}

/// An inline comment queued locally, not yet sent to GitHub.
///
/// `line`/`side` anchor a single-line comment. Supplying `start_line` and
/// `start_side` as well turns it into a multi-line comment spanning
/// `start_line..=line`; GitHub rejects the request if only one of the pair is
/// present, which is why both are serialised together or not at all.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftComment {
    pub path: String,
    pub line: u32,
    pub side: Side,
    // GitHub rejects an explicit `null` here, so the keys must vanish entirely
    // rather than serialise as null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_side: Option<Side>,
    pub body: String,
}

impl DraftComment {
    /// A single-line comment on `path`.
    pub fn single(path: impl Into<String>, line: u32, side: Side, body: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line,
            side,
            start_line: None,
            start_side: None,
            body: body.into(),
        }
    }

    pub fn is_multi_line(&self) -> bool {
        self.start_line.is_some()
    }
}

/// Body of `POST /repos/{owner}/{repo}/pulls/{number}/reviews`.
///
/// Serialises to exactly the payload GitHub expects, so the client can hand it
/// straight to `.json()`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SubmitReview {
    pub event: ReviewEvent,
    pub body: String,
    /// May be empty. GitHub only rejects the review when the body is empty too.
    pub comments: Vec<DraftComment>,
}

impl SubmitReview {
    pub fn new(event: ReviewEvent, body: impl Into<String>) -> Self {
        Self {
            event,
            body: body.into(),
            comments: Vec::new(),
        }
    }

    pub fn with_comments(mut self, comments: Vec<DraftComment>) -> Self {
        self.comments = comments;
        self
    }

    /// Whether GitHub will reject this review as empty.
    pub fn is_empty(&self) -> bool {
        self.body.trim().is_empty() && self.comments.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// The open/closed state of a pull request, as `PATCH .../pulls/{n}` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
}

impl IssueState {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_a_files_page() {
        const PAGE: &str = r#"[
          {
            "sha": "abc",
            "filename": "src/main.rs",
            "status": "modified",
            "additions": 3,
            "deletions": 1,
            "changes": 4,
            "patch": "@@ -1 +1 @@\n-old\n+new"
          },
          {
            "sha": "def",
            "filename": "docs/new.md",
            "previous_filename": "docs/old.md",
            "status": "renamed",
            "additions": 0,
            "deletions": 0,
            "changes": 0
          },
          {
            "sha": "ghi",
            "filename": "assets/logo.png",
            "status": "added",
            "additions": 0,
            "deletions": 0,
            "changes": 0
          }
        ]"#;

        let files: Vec<PullRequestFile> = serde_json::from_str(PAGE).expect("page should decode");
        assert_eq!(files.len(), 3);

        assert_eq!(files[0].filename, "src/main.rs");
        assert_eq!(files[0].additions, 3);
        assert_eq!(files[0].deletions, 1);
        assert!(files[0].has_patch());
        assert_eq!(files[0].old_path(), "src/main.rs");

        assert_eq!(files[1].status, "renamed");
        assert_eq!(files[1].old_path(), "docs/old.md");

        // Binary files arrive with no `patch` key at all, not `"patch": null`.
        assert!(!files[2].has_patch());
        assert_eq!(files[2].previous_filename, None);
    }

    /// Statuses are stored verbatim so an unfamiliar one cannot fail a decode.
    #[test]
    fn unknown_status_strings_pass_through_untouched() {
        for status in [
            "added",
            "removed",
            "modified",
            "renamed",
            "copied",
            "changed",
            "unchanged",
            "something_github_added_later",
        ] {
            let body = json!({
                "filename": "f",
                "status": status,
                "additions": 0,
                "deletions": 0,
            });
            let file: PullRequestFile =
                serde_json::from_value(body).unwrap_or_else(|e| panic!("{status}: {e}"));
            assert_eq!(file.status, status);
        }
    }

    /// The exact payload matters: a stray `null` start_line is a 422 from GitHub.
    #[test]
    fn multi_line_comment_serialises_with_both_start_fields() {
        let review =
            SubmitReview::new(ReviewEvent::RequestChanges, "needs work").with_comments(vec![
                DraftComment {
                    path: "src/main.rs".into(),
                    line: 12,
                    side: Side::Right,
                    start_line: Some(9),
                    start_side: Some(Side::Left),
                    body: "span".into(),
                },
            ]);

        assert_eq!(
            serde_json::to_string(&review).expect("serialises"),
            r#"{"event":"REQUEST_CHANGES","body":"needs work","comments":[{"path":"src/main.rs","line":12,"side":"RIGHT","start_line":9,"start_side":"LEFT","body":"span"}]}"#
        );
    }

    #[test]
    fn single_line_comment_omits_start_fields_entirely() {
        let review =
            SubmitReview::new(ReviewEvent::Comment, "").with_comments(vec![DraftComment::single(
                "src/lib.rs",
                4,
                Side::Left,
                "here",
            )]);

        let text = serde_json::to_string(&review).expect("serialises");
        assert_eq!(
            text,
            r#"{"event":"COMMENT","body":"","comments":[{"path":"src/lib.rs","line":4,"side":"LEFT","body":"here"}]}"#
        );
        assert!(!text.contains("start_line"), "start_line leaked: {text}");
        assert!(!text.contains("start_side"), "start_side leaked: {text}");
        assert!(!text.contains("null"), "null leaked: {text}");
    }

    /// An approval with no inline comments is legitimate and must still send an
    /// (empty) array rather than omitting the key.
    #[test]
    fn approval_without_comments_still_sends_an_array() {
        let review = SubmitReview::new(ReviewEvent::Approve, "lgtm");
        assert!(!review.is_empty());
        assert_eq!(
            serde_json::to_value(&review).expect("serialises"),
            json!({ "event": "APPROVE", "body": "lgtm", "comments": [] })
        );
    }

    #[test]
    fn a_review_with_no_body_and_no_comments_is_empty() {
        assert!(SubmitReview::new(ReviewEvent::Comment, "   ").is_empty());
        assert!(
            !SubmitReview::new(ReviewEvent::Comment, "")
                .with_comments(vec![DraftComment::single("f", 1, Side::Right, "x")])
                .is_empty()
        );
    }

    #[test]
    fn review_events_map_to_their_wire_strings() {
        for (event, wire) in [
            (ReviewEvent::Approve, "APPROVE"),
            (ReviewEvent::RequestChanges, "REQUEST_CHANGES"),
            (ReviewEvent::Comment, "COMMENT"),
        ] {
            assert_eq!(event.as_api_str(), wire);
            assert_eq!(
                serde_json::to_value(event).expect("serialises"),
                json!(wire),
                "{event:?}"
            );
        }
    }

    #[test]
    fn merge_methods_map_to_their_wire_strings() {
        for (method, wire) in [
            (MergeMethod::Merge, "merge"),
            (MergeMethod::Squash, "squash"),
            (MergeMethod::Rebase, "rebase"),
        ] {
            assert_eq!(method.as_api_str(), wire);
            assert_eq!(
                serde_json::to_value(method).expect("serialises"),
                json!(wire),
                "{method:?}"
            );
        }
    }

    #[test]
    fn issue_states_map_to_their_wire_strings() {
        for (state, wire) in [(IssueState::Open, "open"), (IssueState::Closed, "closed")] {
            assert_eq!(state.as_api_str(), wire);
            assert_eq!(
                serde_json::to_value(state).expect("serialises"),
                json!(wire),
                "{state:?}"
            );
        }
    }

    #[test]
    fn sides_serialise_as_github_spells_them() {
        let comment = DraftComment::single("f", 1, Side::Right, "x");
        assert!(!comment.is_multi_line());
        assert_eq!(
            serde_json::to_value(&comment).expect("serialises")["side"],
            json!("RIGHT")
        );
        assert_eq!(Side::Left.as_api_str(), "LEFT");
    }
}
