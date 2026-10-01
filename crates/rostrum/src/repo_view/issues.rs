//! The issues half of the repository view's sidebar.
//!
//! A placeholder until issues land (`feat/issues`): the section, its header
//! and its place in keyboard navigation are all in position, and only this
//! module changes when there is something to list.

use gpui::{App, IntoElement, div, prelude::*, rems};
use rostrum_ui::ActiveTheme;

/// How many issue rows the sidebar holds, for keyboard navigation.
pub fn count() -> usize {
    0
}

pub fn placeholder(cx: &App) -> impl IntoElement {
    div()
        .p_3()
        .text_size(rems(0.78))
        .text_color(cx.theme().text_subtle)
        .child("Issues are not loaded yet")
}
