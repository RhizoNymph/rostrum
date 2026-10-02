//! Editing an issue's title and description from its pane.
//!
//! The rules — a non-blank title, nothing sent when nothing changed, and when
//! saving would overwrite someone else's edit — are [`IssueEditor`]'s. This
//! module holds the inputs and runs the save:
//!
//! 1. Save re-reads the issue.
//! 2. If nobody else changed the title or description since the editor
//!    opened, the PATCH goes out through the pane's `mutate()`.
//! 3. If someone did, the editor shows the conflict and waits: **Reload**
//!    takes their version as the new starting point, **Overwrite** sends
//!    this one over it.

use gpui::{AppContext, Context, Entity, Subscription};
use gpui_tokio::Tokio;
use rostrum_core::IssueDetail;
use rostrum_github::{EditCheck, EditError, IssueEditor, IssueMutation};
use rostrum_ui::{InputEvent, TextInput};

use crate::{loadable::Loadable, markdown_editor::MarkdownEditor};

use super::IssuePane;

/// Where an open editor is in its save.
pub(crate) enum EditPhase {
    /// Being edited.
    Editing,
    /// Save pressed; re-reading the issue to look for someone else's edit.
    Checking,
    /// Someone else changed the title or description since the editor
    /// opened. Their version is held so Reload can adopt it.
    Conflict(Box<IssueDetail>),
}

pub(crate) struct Editing {
    pub(crate) editor: IssueEditor,
    pub(crate) title: Entity<TextInput>,
    pub(crate) body: Entity<MarkdownEditor>,
    pub(crate) phase: EditPhase,
    /// Why the last save attempt did not go out.
    pub(crate) error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl IssuePane {
    /// Open the editor on the title and description as last fetched.
    pub(crate) fn open_editor(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(detail) = self.detail.loaded() else {
            return;
        };
        let editor = IssueEditor::from_detail(detail);
        let repo = self.repo.clone();
        let title = cx.new(|cx| TextInput::new("Title", cx).lines(1, 1));
        let body = cx.new(|cx| {
            MarkdownEditor::new(
                "issue-edit-preview",
                "Description (markdown)…",
                Some(repo),
                cx,
            )
        });
        let start_title = editor.title().to_string();
        let start_body = editor.body().to_string();
        title.update(cx, |input, cx| input.set_text(start_title, cx));
        body.update(cx, |body, cx| body.set_text(&start_body, cx));

        let body_input = body.read(cx).input().clone();
        let subscriptions = vec![cx.subscribe(&body_input, |this, _, event, cx| {
            if matches!(event, InputEvent::Submit) {
                this.save_edit(cx);
            }
        })];
        tracing::debug!(repo = %self.repo, issue = %self.number, "editing issue");
        self.editing = Some(Editing {
            editor,
            title,
            body,
            phase: EditPhase::Editing,
            error: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    pub(crate) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editing = None;
        cx.notify();
    }

    fn edit_text(&self, cx: &Context<Self>) -> Option<(String, String)> {
        let editing = self.editing.as_ref()?;
        Some((
            editing.title.read(cx).text().to_string(),
            editing.body.read(cx).text(cx),
        ))
    }

    /// Validate, then re-read the issue before sending, so an edit made by
    /// someone else since the editor opened is not silently replaced.
    pub(crate) fn save_edit(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some((title, body)) = self.edit_text(cx) else {
            return;
        };
        let client = self.client(cx);
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        if !matches!(editing.phase, EditPhase::Editing) {
            return;
        }
        match editing.editor.request(&title, &body) {
            Ok(_) => {}
            // Nothing to send; closing is what Save means here.
            Err(EditError::Unchanged) => {
                self.editing = None;
                cx.notify();
                return;
            }
            Err(error) => {
                editing.error = Some(error.to_string());
                cx.notify();
                return;
            }
        }
        let Some(client) = client else {
            editing.error = Some("Not authenticated".into());
            cx.notify();
            return;
        };
        editing.phase = EditPhase::Checking;
        editing.error = None;
        cx.notify();

        let (repo, number) = (self.repo.clone(), self.number);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let fresh = Tokio::spawn(
                &*cx,
                async move { client.issue_detail(&repo, number).await },
            )
            .await;
            this.update(cx, |this, cx| {
                match fresh {
                    Ok(Ok(current)) => this.checked(current, cx),
                    Ok(Err(error)) => this.edit_failed(error.to_string(), cx),
                    Err(error) => this.edit_failed(error.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The re-read has landed: send the edit, or stop at the conflict.
    fn checked(&mut self, current: IssueDetail, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        match editing.editor.check(&current) {
            EditCheck::Clear => {
                editing.phase = EditPhase::Editing;
                let Some((title, body)) = self.edit_text(cx) else {
                    return;
                };
                let Some(editing) = self.editing.as_mut() else {
                    return;
                };
                match editing.editor.request(&title, &body) {
                    Ok(edit) => self.mutate(IssueMutation::Edit(edit), cx),
                    Err(error) => editing.error = Some(error.to_string()),
                }
            }
            EditCheck::Conflict { updated_at, .. } => {
                tracing::info!(
                    repo = %self.repo,
                    issue = %self.number,
                    %updated_at,
                    "issue edited elsewhere while the editor was open"
                );
                editing.phase = EditPhase::Conflict(Box::new(current));
            }
        }
    }

    fn edit_failed(&mut self, message: String, _cx: &mut Context<Self>) {
        if let Some(editing) = self.editing.as_mut() {
            editing.phase = EditPhase::Editing;
            editing.error = Some(message);
        }
    }

    /// Take the other person's version as the new starting point. The inputs
    /// are refilled with it, so nothing of theirs is lost; whatever was typed
    /// here is replaced.
    pub(crate) fn reload_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let EditPhase::Conflict(current) =
            std::mem::replace(&mut editing.phase, EditPhase::Editing)
        else {
            return;
        };
        editing.editor = IssueEditor::from_detail(&current);
        editing.error = None;
        let (title, body) = (current.issue.title.clone(), current.body().to_string());
        editing
            .title
            .update(cx, |input, cx| input.set_text(title, cx));
        editing
            .body
            .update(cx, |input, cx| input.set_text(&body, cx));
        // The pane shows their version too.
        self.detail = match std::mem::replace(&mut self.detail, Loadable::Idle) {
            Loadable::Loaded(held) => Loadable::Loaded(IssueDetail {
                issue: current.issue.clone(),
                conversation: held.conversation.refreshed_by(current.conversation.clone()),
            }),
            _ => Loadable::Loaded(*current),
        };
        cx.notify();
    }

    /// Send this version over the other person's, which the user has seen.
    pub(crate) fn overwrite_edit(&mut self, cx: &mut Context<Self>) {
        let Some((title, body)) = self.edit_text(cx) else {
            return;
        };
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        if !matches!(editing.phase, EditPhase::Conflict(_)) {
            return;
        }
        match editing.editor.overwrite(&title, &body) {
            Ok(edit) => {
                editing.phase = EditPhase::Editing;
                tracing::info!(repo = %self.repo, issue = %self.number, "overwriting a concurrent edit");
                self.mutate(IssueMutation::Edit(edit), cx);
            }
            Err(error) => editing.error = Some(error.to_string()),
        }
        cx.notify();
    }
}
