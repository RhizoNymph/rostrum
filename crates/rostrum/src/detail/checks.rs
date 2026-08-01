//! The Checks tab: CI results for the head commit.

use gpui::{AnyElement, Context, div, prelude::*, rems};
use rostrum_ui::{
    ActiveTheme,
    components::{Dot, h_flex, v_flex},
};

use crate::detail::{Loadable, PrDetail};

pub fn render(detail: &PrDetail, cx: &Context<PrDetail>) -> AnyElement {
    let theme = cx.theme().clone();

    let checks = match &detail.conversation {
        Loadable::Idle | Loadable::Loading => {
            return notice("Loading…", theme.text_subtle).into_any_element();
        }
        Loadable::Failed(message) => {
            return notice(message.clone(), theme.danger).into_any_element();
        }
        Loadable::Loaded(conversation) => &conversation.checks,
    };

    if checks.is_empty() {
        return notice("No checks reported for this commit", theme.text_subtle).into_any_element();
    }

    div()
        .id("checks")
        .size_full()
        .overflow_y_scroll()
        .p_4()
        .child(v_flex().gap_1().children(checks.iter().map(|check| {
            h_flex()
                .gap_2()
                .py_1p5()
                .px_2()
                .border_b_1()
                .border_color(theme.border)
                .child(Dot::new(theme.check_color(check.state)))
                .child(
                    div()
                        .flex_1()
                        .text_size(rems(0.8))
                        .text_color(theme.text)
                        .child(check.name.clone()),
                )
                .child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.text_subtle)
                        .child(match check.state {
                            Some(state) => format!("{state:?}").to_lowercase(),
                            None => "no status".to_string(),
                        }),
                )
        })))
        .into_any_element()
}

fn notice(message: impl Into<String>, color: gpui::Hsla) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(rems(0.82))
        .text_color(color)
        .child(message.into())
}
