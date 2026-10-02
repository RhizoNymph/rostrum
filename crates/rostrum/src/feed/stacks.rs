//! Stacks in the feed: the header above a stack's members, the chain glyph on
//! each member, the confirmations behind every stack action, and the status
//! line of the running one.
//!
//! Every action that changes GitHub is held behind a panel that says exactly
//! what will happen, the way the detail pane holds merge and close.
//! Arranging arbitrary pull requests (selection and ordering) lives in
//! [`super::arrange`].

use gpui::{AnyElement, App, Context, Entity, SharedString, div, prelude::*, rems};
use rostrum_core::{
    Chrome, MergeStatus, PrNumber, RepoId, RepoIx, Stack, StackIx, StackNumber, StackPlace,
    plan_stack,
};
use rostrum_stack::MergeMethod;
use rostrum_ui::{
    ActiveTheme, TextInput,
    components::{Button, ButtonStyle, Chip, h_flex, v_flex},
};

use super::{FeedView, arrange::Picking, card};
use crate::sync::{StackOpResult, Store};

/// Extra left padding on a member row, so a stack reads as one block under its
/// header.
pub(crate) const STACK_INDENT: f32 = 22.;

/// The chain glyph for a member at `place`: a line joining the members, bottom
/// first.
pub(crate) fn stack_glyph(place: StackPlace) -> &'static str {
    match place {
        StackPlace::Bottom => "┏",
        StackPlace::Middle => "┣",
        StackPlace::Top => "┗",
        StackPlace::Only => "━",
    }
}

/// The confirmation panel open above the feed, if any.
#[derive(Clone, Debug)]
pub(super) enum StackPanel {
    /// Link a detected chain on GitHub and track it in the clone.
    Make {
        stack: Stack,
    },
    /// Order the picked pull requests and choose a trunk.
    Arrange {
        repo: RepoId,
        order: Vec<PrNumber>,
        confirmed: bool,
    },
    /// GitHub's all-or-nothing merge of a stack.
    Merge {
        stack: Stack,
        number: StackNumber,
        method: MergeMethod,
    },
    Unstack {
        repo: RepoId,
        number: StackNumber,
    },
}

/// Everything the feed holds for stacks that is not the store's.
pub(super) struct StackUi {
    pub(super) picking: Option<Picking>,
    pub(super) panel: Option<StackPanel>,
    /// The trunk for an arrangement.
    pub(super) trunk_input: Entity<TextInput>,
    /// Why the last action could not start.
    pub(super) error: Option<String>,
}

impl StackUi {
    pub(super) fn new(cx: &mut Context<FeedView>) -> Self {
        Self {
            picking: None,
            panel: None,
            trunk_input: cx.new(|cx| TextInput::new("trunk branch, e.g. main", cx).lines(1, 1)),
            error: None,
        }
    }
}

impl FeedView {
    pub(super) fn render_stack_header(
        &mut self,
        repo: RepoIx,
        stack: StackIx,
        chrome: Chrome,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(placed) = self.feed.stack(stack).cloned() else {
            return div().into_any_element();
        };
        let Some(state) = self.repo_state(repo, cx) else {
            return div().into_any_element();
        };
        let group = placed.group;
        let rollup = group.rollup(&state.prs);
        let has_clone = self.store.read(cx).local_path(&state.id).is_some();
        let busy = self.store.read(cx).is_stack_busy();
        let theme = cx.theme().clone();

        let count = group.stack.members.len();
        let title = match group.stack.number {
            Some(number) => format!("Stack {number} · {count} PRs"),
            None => format!("Stackable chain · {count} PRs"),
        };
        let absent = group.absent();
        let number = group.stack.number;
        let stack_value = group.stack.clone();

        let actions = match number {
            Some(number) => h_flex()
                .gap_1()
                .child(
                    Button::new(("stack-merge", stack.0), "Merge stack")
                        .style(ButtonStyle::Primary)
                        .disabled(busy)
                        .tooltip("Merge every pull request in the stack at once, or none")
                        .on_click(cx.listener({
                            let stack_value = stack_value.clone();
                            move |this, _, _window, cx| {
                                this.open_panel(
                                    StackPanel::Merge {
                                        stack: stack_value.clone(),
                                        number,
                                        method: MergeMethod::Squash,
                                    },
                                    cx,
                                )
                            }
                        })),
                )
                .child(
                    Button::new(("stack-unstack", stack.0), "Unstack")
                        .disabled(busy || !has_clone)
                        .tooltip(if has_clone {
                            "Dissolve the stack; the pull requests stay open"
                        } else {
                            "Unstacking runs `gh stack unstack` in the repository's clone; configure one first"
                        })
                        .on_click(cx.listener({
                            let repo_id = state.id.clone();
                            move |this, _, _window, cx| {
                                this.open_panel(
                                    StackPanel::Unstack {
                                        repo: repo_id.clone(),
                                        number,
                                    },
                                    cx,
                                )
                            }
                        })),
                ),
            None => h_flex().child(
                Button::new(("stack-make", stack.0), "Make stack")
                    .style(ButtonStyle::Primary)
                    .disabled(busy || !has_clone)
                    .tooltip(if has_clone {
                        "Link these pull requests into a stack on GitHub; nothing is pushed"
                    } else {
                        "Stacks are made from the repository's clone; configure one first"
                    })
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.open_panel(
                            StackPanel::Make {
                                stack: stack_value.clone(),
                            },
                            cx,
                        )
                    })),
            ),
        };

        card(chrome, cx)
            .id(("stack-header", stack.0))
            .px_3()
            .py_1p5()
            .bg(theme.surface_raised)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_color(theme.accent)
                            .text_size(rems(0.8))
                            .child("⛓"),
                    )
                    .child(
                        div()
                            .text_color(theme.text)
                            .text_size(rems(0.76))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_color(theme.text_subtle)
                            .text_size(rems(0.72))
                            .child(format!("onto {}", group.stack.trunk)),
                    )
                    .when(absent > 0, |el| {
                        el.child(
                            div()
                                .text_color(theme.text_subtle)
                                .text_size(rems(0.7))
                                .child(format!("{absent} not open")),
                        )
                    })
                    .when_some(rollup, |el, rollup| {
                        let color = if rollup.all_mergeable() {
                            theme.success
                        } else {
                            theme.merge_color(rollup.worst)
                        };
                        el.child(
                            Chip::new(rollup.label())
                                .color(color)
                                .tooltip(("stack-rollup", stack.0), rollup.worst.explanation()),
                        )
                    })
                    .child(div().flex_1())
                    .child(actions),
            )
            .into_any_element()
    }

    pub(super) fn open_panel(&mut self, panel: StackPanel, cx: &mut Context<Self>) {
        self.stack_ui.error = None;
        self.stack_ui.panel = Some(panel);
        cx.notify();
    }

    pub(super) fn close_panel(&mut self, cx: &mut Context<Self>) {
        self.stack_ui.panel = None;
        self.stack_ui.error = None;
        cx.notify();
    }

    /// Run the open panel's action, or show why it cannot start.
    fn confirm_panel(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.stack_ui.panel.clone() else {
            return;
        };
        let result = match panel {
            StackPanel::Make { stack } => {
                let store = self.store.read(cx);
                match store.state.repo(&stack.repo) {
                    None => Err(format!("{} is no longer watched", stack.repo)),
                    Some(repo) => plan_stack(repo, stack.members.as_slice(), stack.trunk.clone())
                        .map_err(|err| err.to_string()),
                }
                .and_then(|plan| {
                    self.store
                        .update(cx, |store, cx| store.make_stack(plan, cx))
                })
            }
            StackPanel::Arrange { .. } => match self.arrangement(cx) {
                Ok((plan, _)) => {
                    let started = self
                        .store
                        .update(cx, |store, cx| store.make_stack(plan, cx));
                    if started.is_ok() {
                        self.stack_ui.picking = None;
                    }
                    started
                }
                Err(err) => Err(err),
            },
            StackPanel::Merge {
                stack,
                number,
                method,
            } => self.store.update(cx, |store, cx| {
                store.merge_stack(stack.repo.clone(), number, method, cx)
            }),
            StackPanel::Unstack { repo, number } => self
                .store
                .update(cx, |store, cx| store.unstack(repo, number, cx)),
        };
        match result {
            Ok(()) => self.close_panel(cx),
            Err(error) => {
                self.stack_ui.error = Some(error);
                cx.notify();
            }
        }
    }

    /// The selection toolbar, the open confirmation, and the status line.
    pub(super) fn render_stack_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let status = self.store.read(cx).stack_op().map(|op| {
            let color = match &op.finished {
                None => theme.text_subtle,
                Some(StackOpResult::Succeeded(_)) => theme.success,
                Some(StackOpResult::Stopped(_)) => theme.warning,
                Some(StackOpResult::Failed(_)) => theme.danger,
            };
            (op.line(), color, op.finished.is_some())
        });
        let picking = self
            .render_picking_bar(cx)
            .map(IntoElement::into_any_element);
        let panel = self.render_panel(cx);

        v_flex()
            .flex_none()
            .px_3()
            .gap_1p5()
            .when_some(picking, |el, bar| el.pb_1().child(bar))
            .when_some(panel, |el, panel| el.pb_2().child(panel))
            .when_some(status, |el, (line, color, finished)| {
                el.pb_2().child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(rems(0.72))
                                .text_color(color)
                                .child(line),
                        )
                        .when(finished, |el| {
                            el.child(
                                Button::new("stack-status-dismiss", "×")
                                    .tooltip("Dismiss")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.store
                                            .update(cx, |store, cx| store.dismiss_stack_op(cx))
                                    })),
                            )
                        }),
                )
            })
    }

    fn render_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.stack_ui.panel.clone()?;
        let theme = cx.theme().clone();
        let busy = self.store.read(cx).is_stack_busy();

        let (body, confirm_label, confirm_style, can_confirm): (
            AnyElement,
            &str,
            ButtonStyle,
            bool,
        ) = match &panel {
            StackPanel::Make { stack } => {
                let clone = self
                    .store
                    .read(cx)
                    .local_path(&stack.repo)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let chain = chain_text(stack.members.as_slice());
                let text = format!(
                    "Make {chain} onto `{}` in {} a stack. `gh stack link` creates it on GitHub from the existing pull requests — no branch is pushed — and `gh stack init` tracks it in {clone}, which checks the top branch out there.",
                    stack.trunk, stack.repo
                );
                (
                    paragraph(text, &theme),
                    "Make stack",
                    ButtonStyle::Primary,
                    true,
                )
            }
            StackPanel::Arrange { .. } => {
                let (body, ready) = self.render_arrange_body(cx);
                (body, "Arrange", ButtonStyle::Danger, ready)
            }
            StackPanel::Merge {
                stack,
                number,
                method,
            } => (
                self.render_merge_body(stack, *number, *method, cx),
                "Merge stack",
                ButtonStyle::Danger,
                true,
            ),
            StackPanel::Unstack { repo, number } => (
                paragraph(
                    format!(
                        "Unstack stack {number} in {repo}? GitHub dissolves the stack, and the clone stops tracking it if it did. The pull requests stay open with the bases they have now."
                    ),
                    &theme,
                ),
                "Unstack",
                ButtonStyle::Danger,
                true,
            ),
        };

        Some(
            v_flex()
                .gap_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_raised)
                .child(body)
                .when_some(self.stack_ui.error.clone(), |el, error| {
                    el.child(
                        div()
                            .text_size(rems(0.72))
                            .text_color(theme.danger)
                            .child(error),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1())
                        .child(
                            Button::new("stack-panel-cancel", "Cancel")
                                .on_click(cx.listener(|this, _, _window, cx| this.close_panel(cx))),
                        )
                        .child(
                            Button::new("stack-panel-confirm", confirm_label)
                                .style(confirm_style)
                                .disabled(busy || !can_confirm)
                                .tooltip(if busy {
                                    "Another stack operation is still running"
                                } else {
                                    confirm_label
                                })
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.confirm_panel(cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Every member with its merge state, the method, and the all-or-nothing
    /// note.
    fn render_merge_body(
        &mut self,
        stack: &Stack,
        number: StackNumber,
        method: MergeMethod,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);
        let members: Vec<(PrNumber, Option<(String, MergeStatus)>)> = stack
            .members
            .as_slice()
            .iter()
            .map(|n| {
                let pr = store
                    .state
                    .repo(&stack.repo)
                    .and_then(|repo| repo.prs.iter().find(|pr| pr.number == *n))
                    .map(|pr| (pr.title.clone(), pr.merge_status()));
                (*n, pr)
            })
            .collect();
        let blocked = members
            .iter()
            .filter(|(_, pr)| pr.as_ref().is_some_and(|(_, s)| s.blocks_merge()))
            .count();

        v_flex()
            .gap_1()
            .child(paragraph(
                format!(
                    "Merge stack {number} in {} into `{}`. All or nothing: GitHub merges every open pull request in the stack, bottom first, or none of them.",
                    stack.repo, stack.trunk
                ),
                &theme,
            ))
            .children(members.into_iter().enumerate().map(|(ix, (n, pr))| {
                let (title, chip) = match pr {
                    Some((title, status)) => (
                        title,
                        Chip::new(merge_word(status))
                            .color(theme.merge_color(status))
                            .tooltip(("stack-merge-member", ix), status.explanation()),
                    ),
                    None => (
                        "not open (merged or closed)".to_string(),
                        Chip::new("skipped"),
                    ),
                };
                h_flex()
                    .gap_2()
                    .pl_2()
                    .child(div().text_size(rems(0.72)).text_color(theme.text_subtle).child(n.to_string()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.74))
                            .text_color(theme.text)
                            .child(title),
                    )
                    .child(chip)
            }))
            .when(blocked > 0, |el| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.warning)
                        .child(format!(
                            "{blocked} member(s) look blocked; GitHub decides when the merge runs, and if any cannot merge, none will."
                        )),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_size(rems(0.72)).text_color(theme.text_subtle).child("Method"))
                    .children(MergeMethod::ALL.into_iter().map(|choice| {
                        Button::new(
                            SharedString::from(format!("stack-method-{}", choice.as_flag_value())),
                            choice.label(),
                        )
                        .style(if choice == method {
                            ButtonStyle::Primary
                        } else {
                            ButtonStyle::Subtle
                        })
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            if let Some(StackPanel::Merge { method, .. }) = &mut this.stack_ui.panel {
                                *method = choice;
                                cx.notify();
                            }
                        }))
                    })),
            )
            .into_any_element()
    }
}

/// `#1 ← #2 ← #3`, bottom first.
pub(super) fn chain_text(members: &[PrNumber]) -> String {
    members
        .iter()
        .map(PrNumber::to_string)
        .collect::<Vec<_>>()
        .join(" ← ")
}

fn merge_word(status: MergeStatus) -> &'static str {
    match status {
        MergeStatus::Ready => "ready",
        MergeStatus::Unstable => "checks failing",
        MergeStatus::Computing => "computing",
        MergeStatus::Draft => "draft",
        MergeStatus::Behind => "behind",
        MergeStatus::Blocked => "blocked",
        MergeStatus::Conflicts => "conflict",
    }
}

pub(super) fn paragraph(text: String, theme: &rostrum_ui::Theme) -> AnyElement {
    div()
        .text_size(rems(0.74))
        .text_color(theme.text)
        .child(text)
        .into_any_element()
}

/// The store, for the arrange module's validation.
pub(super) fn store_of<'a>(view: &FeedView, cx: &'a App) -> &'a Store {
    view.store.read(cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glyphs_join_members_bottom_first() {
        assert_eq!(stack_glyph(StackPlace::Bottom), "┏");
        assert_eq!(stack_glyph(StackPlace::Middle), "┣");
        assert_eq!(stack_glyph(StackPlace::Top), "┗");
    }

    #[test]
    fn a_chain_reads_bottom_first() {
        assert_eq!(chain_text(&[PrNumber(1), PrNumber(2)]), "#1 ← #2");
    }

    #[test]
    fn every_merge_state_has_a_word() {
        for status in [
            MergeStatus::Ready,
            MergeStatus::Unstable,
            MergeStatus::Computing,
            MergeStatus::Draft,
            MergeStatus::Behind,
            MergeStatus::Blocked,
            MergeStatus::Conflicts,
        ] {
            assert!(!merge_word(status).is_empty());
        }
    }
}
