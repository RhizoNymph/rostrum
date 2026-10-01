//! Editing an existing issue's title and description, without the editor.
//!
//! The rules live here, free of any UI toolkit: what a valid edit is, and
//! whether saving would overwrite a change someone else made while the editor
//! was open.

use chrono::{DateTime, Utc};
use rostrum_core::{EmptyTitle, Issue, IssueDetail, IssueTitle};

use crate::issues::rest::IssueEdit;

/// Why an edit cannot be saved as it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(transparent)]
    EmptyTitle(#[from] EmptyTitle),
    /// Neither the title nor the description differs from what was opened.
    /// Not sent: a no-op edit would still bump the issue's `updatedAt`.
    #[error("nothing has changed")]
    Unchanged,
}

/// What a fresh read of the issue says about saving over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditCheck {
    /// Nobody has touched the title or description since the editor opened.
    Clear,
    /// Someone has. Saving now would discard their change, so the user picks:
    /// reload their version, or overwrite it.
    Conflict {
        /// When GitHub last saw the issue change.
        updated_at: DateTime<Utc>,
        /// The title and description as they are now.
        title: String,
        body: String,
    },
}

/// An editor's baseline: the issue as it was when editing began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueEditor {
    opened_at: DateTime<Utc>,
    title: String,
    body: String,
}

impl IssueEditor {
    /// Start editing `issue`, whose description is `body`.
    pub fn open(issue: &Issue, body: &str) -> Self {
        Self {
            opened_at: issue.updated_at,
            title: issue.title.clone(),
            body: body.to_string(),
        }
    }

    pub fn from_detail(detail: &IssueDetail) -> Self {
        Self::open(&detail.issue, detail.body())
    }

    /// The title the editor started from, to fill its input.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The description the editor started from, to fill its input.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The request that saves `title` and `body`, or why it cannot be sent.
    pub fn request(&self, title: &str, body: &str) -> Result<IssueEdit, EditError> {
        let title = IssueTitle::new(title)?;
        if title.as_str() == self.title.trim() && body == self.body {
            return Err(EditError::Unchanged);
        }
        Ok(IssueEdit {
            title,
            body: body.to_string(),
        })
    }

    /// Compare a fresh read of the issue against the baseline.
    ///
    /// A conflict needs both signs: GitHub saw the issue change after the
    /// editor opened, *and* the title or description differs from what the
    /// editor started with. `updatedAt` alone moves for every comment, label
    /// and assignment, none of which an edit would overwrite.
    pub fn check(&self, current: &IssueDetail) -> EditCheck {
        let moved = current.issue.updated_at > self.opened_at;
        let differs = current.issue.title != self.title || current.body() != self.body;
        if moved && differs {
            EditCheck::Conflict {
                updated_at: current.issue.updated_at,
                title: current.issue.title.clone(),
                body: current.body().to_string(),
            }
        } else {
            EditCheck::Clear
        }
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{
        Conversation, IssueNumber, IssueState, NodeId, TimelineItem,
    };
    use serde_json::json;

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid timestamp")
    }

    fn detail(title: &str, body: &str, updated: i64) -> IssueDetail {
        IssueDetail {
            issue: Issue {
                number: IssueNumber(7),
                node_id: NodeId("I_7".into()),
                title: title.into(),
                url: String::new(),
                state: IssueState::Open,
                created_at: at(0),
                updated_at: at(updated),
                author: None,
                assignees: Vec::new(),
                labels: Vec::new(),
                comment_count: 0,
                milestone: None,
            },
            conversation: Conversation {
                items: vec![TimelineItem::Body {
                    author: None,
                    body: body.into(),
                    created_at: at(0),
                }],
                ..Default::default()
            },
        }
    }

    fn editor() -> IssueEditor {
        IssueEditor::from_detail(&detail("Crash", "It crashes.", 100))
    }

    #[test]
    fn the_editor_starts_from_the_issue_as_it_was() {
        let editor = editor();
        assert_eq!(editor.title(), "Crash");
        assert_eq!(editor.body(), "It crashes.");
    }

    #[test]
    fn a_valid_edit_becomes_the_patch_body() {
        let edit = editor()
            .request("  Crash on start ", "It crashes on start.")
            .expect("valid");
        assert_eq!(
            serde_json::to_value(&edit).expect("serialises"),
            json!({ "title": "Crash on start", "body": "It crashes on start." })
        );
    }

    #[test]
    fn a_blank_title_is_refused() {
        assert_eq!(
            editor().request("   ", "anything"),
            Err(EditError::EmptyTitle(EmptyTitle))
        );
    }

    #[test]
    fn an_unchanged_edit_is_not_sent() {
        assert_eq!(
            editor().request("Crash", "It crashes."),
            Err(EditError::Unchanged)
        );
        // Surrounding whitespace on the title is not a change.
        assert_eq!(
            editor().request(" Crash ", "It crashes."),
            Err(EditError::Unchanged)
        );
        // Changing only the body is a change, and so is clearing it.
        assert!(editor().request("Crash", "").is_ok());
    }

    #[test]
    fn an_untouched_issue_is_clear_to_save() {
        assert_eq!(editor().check(&detail("Crash", "It crashes.", 100)), EditCheck::Clear);
    }

    /// Comments, labels and assignments move `updatedAt` without touching
    /// what the editor would overwrite; they must not raise a conflict.
    #[test]
    fn activity_that_does_not_touch_the_text_is_not_a_conflict() {
        assert_eq!(editor().check(&detail("Crash", "It crashes.", 500)), EditCheck::Clear);
    }

    #[test]
    fn someone_elses_edit_since_opening_is_a_conflict() {
        assert_eq!(
            editor().check(&detail("Crash (renamed)", "It crashes.", 500)),
            EditCheck::Conflict {
                updated_at: at(500),
                title: "Crash (renamed)".into(),
                body: "It crashes.".into(),
            }
        );
        assert!(matches!(
            editor().check(&detail("Crash", "Edited body", 500)),
            EditCheck::Conflict { .. }
        ));
    }

    /// Reloading resets the baseline to the other person's version, after
    /// which saving over it is no longer a conflict.
    #[test]
    fn reloading_after_a_conflict_clears_it() {
        let theirs = detail("Crash (renamed)", "Theirs", 500);
        assert!(matches!(editor().check(&theirs), EditCheck::Conflict { .. }));
        let reloaded = IssueEditor::from_detail(&theirs);
        assert_eq!(reloaded.check(&theirs), EditCheck::Clear);
        assert_eq!(reloaded.title(), "Crash (renamed)");
        assert!(reloaded.request("Crash (renamed)", "Mine").is_ok());
    }
}
