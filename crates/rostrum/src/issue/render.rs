//! Drawing the issue pane: header, timeline, and the actions bar.

use gpui::{AnyElement, App, Context, Hsla, Window, div, prelude::*, rems};
use rostrum_core::{CloseReason, Issue, IssueState};
use rostrum_github::{CloseAs, IssueStateChange};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, ButtonStyle, Chip, Initial, h_flex, v_flex},
};

use crate::{
    detail::conversation::{centered, render_plain_item},
    loadable::Loadable,
    pickers::{self, OpenPicker},
};

use super::IssuePane;

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
            .child(
                div()
                    .text_size(rems(1.1))
                    .text_color(theme.text)
                    .child(issue.title.clone()),
            )
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

        div()
            .id("issue-timeline")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .child(
                v_flex().gap_3().children(
                    detail
                        .conversation
                        .items
                        .iter()
                        .enumerate()
                        .map(|(ix, item)| render_plain_item(item, ix, owner, repo, &theme, cx)),
                ),
            )
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
