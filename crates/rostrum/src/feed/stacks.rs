//! Stack rows in the feed: the header above a stack's members and the chain
//! glyph on each member.

use gpui::{AnyElement, Context, div, prelude::*, px, rems};
use rostrum_core::{Chrome, RepoIx, StackIx, StackPlace};
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, h_flex},
};

use super::{FeedView, card};

/// Extra left padding on a member row, so a stack reads as one block under its
/// header.
pub(super) const STACK_INDENT: f32 = 22.;

/// The chain glyph for a member at `place`: a line joining the members, open
/// at the top of the stack.
pub(super) fn stack_glyph(place: StackPlace) -> &'static str {
    match place {
        StackPlace::Bottom => "┗",
        StackPlace::Middle => "┣",
        StackPlace::Top => "┏",
        StackPlace::Only => "━",
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
        let theme = cx.theme().clone();

        let title = match group.stack.number {
            Some(number) => format!("Stack {number} · {} PRs", group.stack.members.len()),
            None => format!("Stackable chain · {} PRs", group.stack.members.len()),
        };
        let absent = group.absent();

        card(chrome, cx)
            .id(("stack-header", stack.0))
            .px_3()
            .py_1p5()
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
                    .child(div().w(px(0.)).flex_1()),
            )
            .into_any_element()
    }
}
