//! Drawing the issue pane: header, timeline, and the actions bar.

use gpui::{AnyElement, App, Context, Hsla, Window, div, prelude::*, rems};
use rostrum_core::{CloseReason, Issue, IssueState, TimelineItem};
use rostrum_github::{CloseAs, IssueStateChange};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, ButtonStyle, Chip, Initial, h_flex, v_flex},
};

use crate::{
    detail::conversation::{centered, load_earlier_button, render_plain_item},
    loadable::Loadable,
    pickers::{self, OpenPicker},
};

use super::{IssuePane, edit::EditPhase};

/// GitHub's colour language for an issue's state: green open, purple done,
/// grey not planned.
fn state_color(state: IssueState, theme: &Theme) -> Hsla {
    match state {
        IssueState::Open => theme.success,
        IssueState::Closed(Some(CloseReason::NotPlanned | CloseReason::Duplicate)) => {
            theme.text_muted
        }
        IssueState::Closed(_) => theme.accent,
    }
}

impl IssuePane {
    fn on_click(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        let entity = cx.entity();
        move |_event, _window, cx| entity.update(cx, |this, cx| f(this, cx))
    }

    fn render_header(&self, issue: &Issue, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let busy = self.busy.is_some();
        let picker_button = |id: &'static str, which: OpenPicker, closed: &'static str| {
            let open = self.picker == Some(which);
            Button::new(id, if open { "Close" } else { closed })
                .on_click(Self::on_click(cx, move |this, cx| {
                    this.toggle_picker(which, cx)
                }))
        };

        v_flex()
            .gap_2()
            .flex_none()
            .p_4()
            .border_b_1()
            .border_color(theme.border)
            .child(match &self.editing {
                // The title input replaces the title while editing; Save and
                // Cancel sit with the description editor below.
                Some(editing) => div().child(editing.title.clone()).into_any_element(),
                None => h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .text_size(rems(1.1))
                            .text_color(theme.text)
                            .child(issue.title.clone()),
                    )
                    .child(
                        Button::new("issue-edit-title", "Edit")
                            .disabled(busy || self.detail.loaded().is_none())
                            .tooltip("Edit the title and description")
                            .on_click(Self::on_click(cx, |this, cx| this.open_editor(cx))),
                    )
                    .into_any_element(),
            })
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .text_size(rems(0.76))
                    .text_color(theme.text_muted)
                    .child(Chip::new(issue.state.label()).color(state_color(issue.state, &theme)))
                    .child(format!("{} {}", self.repo, issue.number))
                    .when_some(issue.author.as_ref(), |el, author| {
                        el.child(Initial::new(author.login.clone()))
                            .child(format!("opened by {}", author.login))
                    })
                    .when_some(issue.milestone.as_ref(), |el, milestone| {
                        el.child(
                            Chip::new(format!("◷ {}", milestone.title)).color(theme.text_muted),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(issue.labels.iter().enumerate().map(|(ix, label)| {
                        pickers::label_chip(ix, label, busy, &theme, Self::label_toggle(cx))
                    }))
                    .child(picker_button(
                        "toggle-issue-labels",
                        OpenPicker::Labels,
                        "Labels…",
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        div()
                            .text_size(rems(0.74))
                            .text_color(theme.text_subtle)
                            .child(if issue.assignees.is_empty() {
                                "No one assigned"
                            } else {
                                "Assigned"
                            }),
                    )
                    .children(issue.assignees.iter().enumerate().map(|(ix, user)| {
                        pickers::assignee_chip(ix, user, busy, &theme, Self::assignee_toggle(cx))
                    }))
                    .child(picker_button(
                        "toggle-issue-assignees",
                        OpenPicker::Assignees,
                        "Assignees…",
                    )),
            )
            .map(|el| match self.picker {
                Some(OpenPicker::Labels) => el.child(pickers::label_picker(
                    &self.repo_labels,
                    &issue.labels,
                    busy,
                    &theme,
                    Self::label_toggle(cx),
                )),
                Some(OpenPicker::Assignees) => el.child(pickers::assignee_picker(
                    &self.assignable,
                    &issue.assignees,
                    busy,
                    &theme,
                    Self::assignee_toggle(cx),
                )),
                None => el,
            })
    }

    fn render_timeline(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let detail = match &self.detail {
            Loadable::Idle | Loadable::Loading => {
                return centered("Loading issue…", theme.text_subtle).into_any_element();
            }
            Loadable::Failed(message) => {
                return centered(message.clone(), theme.danger).into_any_element();
            }
            Loadable::Loaded(detail) => detail,
        };
        let (owner, repo) = (self.repo.owner(), self.repo.name());
        let busy = self.busy.is_some();

        div()
            .id("issue-timeline")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .child(
                v_flex()
                    .gap_3()
                    .when_some(
                        load_earlier_button(&detail.conversation, self.earlier_loading),
                        |el, button| {
                            el.child(
                                button
                                    .on_click(Self::on_click(cx, |this, cx| this.load_earlier(cx))),
                            )
                        },
                    )
                    .children(
                        detail
                            .conversation
                            .items
                            .iter()
                            .enumerate()
                            .map(|(ix, item)| match (item, &self.editing) {
                                (TimelineItem::Body { .. }, Some(_)) => self.render_editor(cx),
                                (TimelineItem::Body { .. }, None) => v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex().justify_end().child(
                                            Button::new("issue-edit-body", "Edit description")
                                                .disabled(busy)
                                                .on_click(Self::on_click(cx, |this, cx| {
                                                    this.open_editor(cx)
                                                })),
                                        ),
                                    )
                                    .child(render_plain_item(item, ix, owner, repo, &theme, cx))
                                    .into_any_element(),
                                _ => render_plain_item(item, ix, owner, repo, &theme, cx),
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The description editor, with Save and Cancel, and the conflict panel
    /// when someone else edited the issue while it was open.
    fn render_editor(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(editing) = &self.editing else {
            return div().into_any_element();
        };
        let busy = self.busy.is_some();
        let checking = matches!(editing.phase, EditPhase::Checking);

        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(theme.accent)
            .child(editing.body.clone())
            .when_some(editing.error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
            .map(|el| match &editing.phase {
                EditPhase::Conflict(current) => el.child(
                    v_flex()
                        .gap_2()
                        .p_2()
                        .bg(theme.surface_raised)
                        .child(
                            div()
                                .text_size(rems(0.78))
                                .text_color(theme.warning)
                                .child(format!(
                                    "This issue was edited elsewhere {} — its title is now “{}”. \
                                     Reload to start from that version, or overwrite it with yours.",
                                    crate::feed::relative_time(current.issue.updated_at),
                                    current.issue.title
                                )),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("issue-edit-reload", "Reload")
                                        .style(ButtonStyle::Primary)
                                        .disabled(busy)
                                        .tooltip("Replace your text with the current version")
                                        .on_click(Self::on_click(cx, |this, cx| {
                                            this.reload_edit(cx)
                                        })),
                                )
                                .child(
                                    Button::new("issue-edit-overwrite", "Overwrite")
                                        .style(ButtonStyle::Danger)
                                        .disabled(busy)
                                        .tooltip("Save your version over the other edit")
                                        .on_click(Self::on_click(cx, |this, cx| {
                                            this.overwrite_edit(cx)
                                        })),
                                )
                                .child(Button::new("issue-edit-cancel", "Cancel").on_click(
                                    Self::on_click(cx, |this, cx| this.cancel_edit(cx)),
                                )),
                        ),
                ),
                EditPhase::Editing | EditPhase::Checking => el.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(
                                "issue-edit-save",
                                if checking { "Checking…" } else { "Save" },
                            )
                            .style(ButtonStyle::Primary)
                            .disabled(busy || checking)
                            .tooltip("Save the title and description (ctrl-enter)")
                            .on_click(Self::on_click(cx, |this, cx| this.save_edit(cx))),
                        )
                        .child(
                            Button::new("issue-edit-cancel", "Cancel")
                                .disabled(busy)
                                .on_click(Self::on_click(cx, |this, cx| this.cancel_edit(cx))),
                        ),
                ),
            })
            .into_any_element()
    }

    fn render_actions(&self, issue: &Issue, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let busy = self.busy.is_some();
        let open = issue.state.is_open();

        v_flex()
            .gap_2()
            .flex_none()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .when_some(self.error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
            .when_some(self.busy, |el, label| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.text_subtle)
                        .child(format!("{label}…")),
                )
            })
            .child(self.composer.clone())
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("issue-comment", "Comment")
                            .style(ButtonStyle::Primary)
                            .disabled(busy)
                            .tooltip("Post the comment (ctrl-enter)")
                            .on_click(Self::on_click(cx, |this, cx| this.post_comment(cx))),
                    )
                    // Buttons carry the end state they move to, fixed at
                    // render: a poll landing before the click can only make
                    // the request redundant, never invert it.
                    .when(open, |el| {
                        el.child(
                            Button::new("issue-close-completed", "Close as completed")
                                .disabled(busy)
                                .on_click(Self::on_click(cx, |this, cx| {
                                    this.set_state(IssueStateChange::Close(CloseAs::Completed), cx)
                                })),
                        )
                        .child(
                            Button::new("issue-close-not-planned", "Close as not planned")
                                .disabled(busy)
                                .tooltip("Close without fixing: won't do, can't reproduce, stale")
                                .on_click(Self::on_click(cx, |this, cx| {
                                    this.set_state(IssueStateChange::Close(CloseAs::NotPlanned), cx)
                                })),
                        )
                    })
                    .when(!open, |el| {
                        el.child(
                            Button::new("issue-reopen", "Reopen")
                                .disabled(busy)
                                .on_click(Self::on_click(cx, |this, cx| {
                                    this.set_state(IssueStateChange::Reopen, cx)
                                })),
                        )
                    }),
            )
    }
}

impl Render for IssuePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(issue) = self.issue(cx) else {
            // Neither the feed nor the detail query has it yet.
            return match &self.detail {
                Loadable::Failed(message) => centered(message.clone(), theme.danger),
                _ => centered("Loading issue…".to_string(), theme.text_subtle),
            }
            .into_any_element();
        };

        v_flex()
            .size_full()
            .child(self.render_header(&issue, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_timeline(cx)),
            )
            .child(self.render_actions(&issue, cx))
            .into_any_element()
    }
}
