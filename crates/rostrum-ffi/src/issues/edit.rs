//! Editing an issue's title and description from the phone.
//!
//! The rules are `rostrum_github::IssueEditor`'s, shared with the desktop: a
//! blank title is refused, an unchanged edit is not sent (it would still bump
//! `updatedAt`), and before saving the issue is re-read — if someone changed
//! the title or description after the editor opened, the edit stops with
//! [`RostrumError::EditConflict`] carrying their version, and the caller
//! either reloads it or saves again with `overwrite`.
//!
//! The editor's baseline is the held detail Kotlin opened the editor from,
//! recognised by its `updatedAt` matching `base_updated_at`. When that copy
//! is gone (the cache was cleared, or a newer copy replaced it), the text
//! the user started from is unknown, so any change after `base_updated_at`
//! counts as a conflict: asking once too often beats overwriting silently.

use std::time::SystemTime;

use chrono::{DateTime, Utc};
use rostrum_core::{IssueDetail as CoreIssueDetail, IssueTitle};
use rostrum_github::{EditCheck, EditError, IssueEdit, IssueEditor, IssueMutation};

use super::{IssueDetail, render};
use crate::{
    engine::{RostrumCore, state::IssueKey},
    error::RostrumError,
};

/// What the edit is checked against.
#[derive(Debug)]
pub(crate) enum Baseline {
    /// The detail the editor opened from.
    Held(IssueEditor),
    /// Only when the editor opened.
    Unknown { opened_at: DateTime<Utc> },
}

impl Baseline {
    pub(crate) fn new(held: Option<&CoreIssueDetail>, opened_at: DateTime<Utc>) -> Self {
        match held {
            Some(held) if held.issue.updated_at == opened_at => {
                Self::Held(IssueEditor::from_detail(held))
            }
            _ => Self::Unknown { opened_at },
        }
    }

    /// The request to send, or why there is none. `overwrite` skips the
    /// unchanged check: putting the original back is a valid overwrite.
    pub(crate) fn request(
        &self,
        title: &str,
        body: &str,
        overwrite: bool,
    ) -> Result<IssueEdit, EditError> {
        match self {
            Self::Held(editor) if overwrite => editor.overwrite(title, body),
            Self::Held(editor) => editor.request(title, body),
            Self::Unknown { .. } => Ok(IssueEdit {
                title: IssueTitle::new(title)?,
                body: body.to_string(),
            }),
        }
    }

    /// What a fresh read says about saving over it.
    pub(crate) fn check(&self, current: &CoreIssueDetail) -> EditCheck {
        match self {
            Self::Held(editor) => editor.check(current),
            Self::Unknown { opened_at } if current.issue.updated_at > *opened_at => {
                EditCheck::Conflict {
                    updated_at: current.issue.updated_at,
                    title: current.issue.title.clone(),
                    body: current.body().to_string(),
                }
            }
            Self::Unknown { .. } => EditCheck::Clear,
        }
    }
}

fn conflict(check: EditCheck) -> Result<(), RostrumError> {
    match check {
        EditCheck::Clear => Ok(()),
        EditCheck::Conflict {
            updated_at,
            title,
            body,
        } => Err(RostrumError::EditConflict {
            title,
            body,
            updated_at: updated_at.into(),
        }),
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl RostrumCore {
    /// Save a new title and description. `base_updated_at` is the
    /// `issue.updatedAt` of the detail the editor opened from.
    ///
    /// Without `overwrite`, the issue is re-read first: if its title or
    /// description changed since, nothing is sent and `EditConflict` carries
    /// the current version (their change is also kept, so the screen can
    /// show it). An edit identical to the baseline sends nothing and returns
    /// the detail as held. With `overwrite`, the edit is sent as it is.
    /// Returns the issue as GitHub now has it.
    pub async fn edit_issue(
        &self,
        repo: String,
        number: u32,
        title: String,
        body: String,
        base_updated_at: SystemTime,
        overwrite: bool,
    ) -> Result<IssueDetail, RostrumError> {
        let key = IssueKey::parse(&repo, number)?;
        let held = self.held_issue(&key).await?;
        let baseline = Baseline::new(held.as_deref(), base_updated_at.into());
        let edit = match baseline.request(&title, &body, overwrite) {
            Ok(edit) => edit,
            Err(EditError::Unchanged) => {
                tracing::debug!(repo = %key.repo, number, "issue edit unchanged; nothing sent");
                let detail = match held {
                    Some(held) => held,
                    None => self.fetch_issue(&key).await?,
                };
                return render(&key, detail, self.viewer_now().await?).await;
            }
            Err(error @ EditError::EmptyTitle(_)) => {
                return Err(RostrumError::invalid(error.to_string()));
            }
        };
        if !overwrite {
            let current = self.fetch_issue(&key).await?;
            if let Err(error) = conflict(baseline.check(&current)) {
                tracing::info!(repo = %key.repo, number, "issue edited elsewhere since the editor opened");
                return Err(error);
            }
        } else {
            tracing::info!(repo = %key.repo, number, "overwriting the issue's title and description");
        }
        self.mutate_issue(key.clone(), IssueMutation::Edit(edit))
            .await?;
        let detail = self.fetch_issue(&key).await?;
        render(&key, detail, self.viewer_now().await?).await
    }
}

#[cfg(test)]
mod tests {
    use rostrum_core::{Conversation, TimelineItem};

    use super::*;
    use crate::test_support::{at, issue};

    fn detail(title: &str, body: &str, updated: u32) -> CoreIssueDetail {
        let mut opened = issue(7, "alice");
        opened.title = title.into();
        opened.updated_at = at(updated);
        CoreIssueDetail {
            issue: opened,
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

    #[test]
    fn the_held_copy_is_the_baseline_only_when_it_is_the_one_opened() {
        let held = detail("Crash", "It crashes.", 5);
        assert!(matches!(
            Baseline::new(Some(&held), at(5)),
            Baseline::Held(_)
        ));
        assert!(matches!(
            Baseline::new(Some(&held), at(4)),
            Baseline::Unknown { .. }
        ));
        assert!(matches!(
            Baseline::new(None, at(5)),
            Baseline::Unknown { .. }
        ));
    }

    #[test]
    fn a_held_baseline_refuses_unchanged_edits_unless_overwriting() {
        let baseline = Baseline::new(Some(&detail("Crash", "It crashes.", 5)), at(5));
        assert_eq!(
            baseline.request("Crash", "It crashes.", false),
            Err(EditError::Unchanged)
        );
        assert!(baseline.request("Crash", "It crashes.", true).is_ok());
        assert!(matches!(
            baseline.request("  ", "x", true),
            Err(EditError::EmptyTitle(_))
        ));
    }

    #[test]
    fn a_held_baseline_conflicts_only_on_a_text_change() {
        let baseline = Baseline::new(Some(&detail("Crash", "It crashes.", 5)), at(5));
        assert_eq!(
            baseline.check(&detail("Crash", "It crashes.", 9)),
            EditCheck::Clear
        );
        assert!(matches!(
            baseline.check(&detail("Theirs", "It crashes.", 9)),
            EditCheck::Conflict { .. }
        ));
    }

    #[test]
    fn an_unknown_baseline_conflicts_on_any_later_change() {
        let baseline = Baseline::new(None, at(5));
        assert_eq!(baseline.check(&detail("Crash", "x", 5)), EditCheck::Clear);
        let EditCheck::Conflict { title, body, .. } = baseline.check(&detail("Crash", "x", 6))
        else {
            panic!("conflict");
        };
        assert_eq!((title.as_str(), body.as_str()), ("Crash", "x"));
        assert!(baseline.request("Same", "x", false).is_ok());
    }

    #[test]
    fn a_conflict_becomes_the_typed_error() {
        let error = conflict(EditCheck::Conflict {
            updated_at: at(9),
            title: "T".into(),
            body: "B".into(),
        })
        .expect_err("conflict");
        assert_eq!(
            error,
            RostrumError::EditConflict {
                title: "T".into(),
                body: "B".into(),
                updated_at: at(9).into(),
            }
        );
    }
}
