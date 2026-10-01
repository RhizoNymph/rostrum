//! The label and assignee pickers, shared by the pull request pane, the issue
//! pane and the new-issue form.
//!
//! Each is a pure function of what is applied, what can be applied, and
//! whether a mutation is in flight; what a click *does* is the caller's
//! [`Toggle`]. That keeps one rendering of "a chip with an ×" and "a list with
//! ticks" no matter whether the click becomes a REST call or an edit to a
//! form that has not been sent yet.

use std::{collections::HashSet, rc::Rc};

use gpui::{AnyElement, App, Hsla, div, prelude::*, px, rems};
use rostrum_core::{Label, User};
use rostrum_ui::{
    Theme,
    components::{Chip, Initial, h_flex, hex_color, v_flex},
};

use crate::loadable::Loadable;

/// Which picker panel is open. One at a time: two stacked lists under a
/// header would push the timeline out of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenPicker {
    Labels,
    Assignees,
}

/// What a click on a chip or a picker row asks for: the name or login, and
/// whether it is currently applied (so the click means "remove").
pub(crate) type Toggle = Rc<dyn Fn(String, bool, &mut App)>;

/// One applied label, with the affordance that takes it off again.
///
/// While `busy`, the × drops its click handler entirely rather than merely
/// looking disabled, so nothing can be double-submitted.
pub(crate) fn label_chip(
    ix: usize,
    label: &Label,
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    let color = hex_color(&label.color).unwrap_or(theme.text_muted);
    removable_chip(
        ("remove-label", ix),
        Chip::new(label.name.clone())
            .color(color)
            .into_any_element(),
        label.name.clone(),
        busy,
        theme,
        on_toggle,
    )
}

/// One assignee, with the affordance that unassigns them.
pub(crate) fn assignee_chip(
    ix: usize,
    user: &User,
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    removable_chip(
        ("remove-assignee", ix),
        h_flex()
            .gap_1()
            .text_size(rems(0.74))
            .text_color(theme.text_muted)
            .child(Initial::new(user.login.clone()))
            .child(user.login.clone())
            .into_any_element(),
        user.login.clone(),
        busy,
        theme,
        on_toggle,
    )
}

fn removable_chip(
    id: (&'static str, usize),
    body: AnyElement,
    key: String,
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    let danger = theme.danger;
    h_flex()
        .gap_0p5()
        .child(body)
        .child(
            div()
                .id(id)
                .px_1()
                .text_size(rems(0.7))
                .text_color(theme.text_subtle)
                .child("×")
                .when(!busy, |el| {
                    el.cursor_pointer()
                        .hover(move |el| el.text_color(danger))
                        .on_click(move |_, _window, cx| on_toggle(key.clone(), true, cx))
                }),
        )
        .into_any_element()
}

/// The label picker: every label the repository defines, applied ones ticked.
pub(crate) fn label_picker(
    palette: &Loadable<Vec<Label>>,
    applied: &[Label],
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    let applied: HashSet<&str> = applied.iter().map(|label| label.name.as_str()).collect();
    picker(
        "label-picker",
        palette,
        "Loading labels…",
        "This repository defines no labels",
        theme,
        |labels| {
            labels
                .iter()
                .enumerate()
                .map(|(ix, label)| {
                    let color = hex_color(&label.color).unwrap_or(theme.text_muted);
                    picker_row(
                        ("repo-label", ix),
                        applied.contains(label.name.as_str()),
                        Chip::new(label.name.clone())
                            .color(color)
                            .into_any_element(),
                        label.name.clone(),
                        busy,
                        theme,
                        on_toggle.clone(),
                    )
                })
                .collect()
        },
    )
}

/// The assignee picker: everyone the repository lets an issue be assigned
/// to, assigned ones ticked.
pub(crate) fn assignee_picker(
    candidates: &Loadable<Vec<User>>,
    assigned: &[User],
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    let assigned: HashSet<_> = assigned.iter().map(User::key).collect();
    picker(
        "assignee-picker",
        candidates,
        "Loading people…",
        "Nobody can be assigned in this repository",
        theme,
        |users| {
            users
                .iter()
                .enumerate()
                .map(|(ix, user)| {
                    picker_row(
                        ("assignable", ix),
                        assigned.contains(&user.key()),
                        h_flex()
                            .gap_1p5()
                            .text_size(rems(0.75))
                            .text_color(theme.text)
                            .child(Initial::new(user.login.clone()))
                            .child(user.login.clone())
                            .into_any_element(),
                        user.login.clone(),
                        busy,
                        theme,
                        on_toggle.clone(),
                    )
                })
                .collect()
        },
    )
}

/// The panel every picker shares: a loading line, an error, an empty line, or
/// a scrolling list.
fn picker<T>(
    id: &'static str,
    source: &Loadable<Vec<T>>,
    loading: &'static str,
    empty: &'static str,
    theme: &Theme,
    rows: impl FnOnce(&[T]) -> Vec<AnyElement>,
) -> AnyElement {
    let panel = v_flex()
        .gap_1()
        .p_2()
        .bg(theme.surface_raised)
        .border_1()
        .border_color(theme.border);
    let note =
        |text: String, color: Hsla| div().text_size(rems(0.75)).text_color(color).child(text);

    match source {
        Loadable::Idle | Loadable::Loading => panel.child(note(loading.into(), theme.text_subtle)),
        Loadable::Failed(message) => panel.child(note(message.clone(), theme.danger)),
        Loadable::Loaded(items) if items.is_empty() => {
            panel.child(note(empty.into(), theme.text_subtle))
        }
        Loadable::Loaded(items) => panel.child(
            div()
                .id(id)
                .max_h(px(200.))
                .overflow_y_scroll()
                .child(v_flex().gap_0p5().children(rows(items))),
        ),
    }
    .into_any_element()
}

fn picker_row(
    id: (&'static str, usize),
    is_applied: bool,
    body: AnyElement,
    key: String,
    busy: bool,
    theme: &Theme,
    on_toggle: Toggle,
) -> AnyElement {
    let hover_bg = theme.surface;
    h_flex()
        .id(id)
        .gap_2()
        .px_1()
        .py_0p5()
        .child(
            div()
                .w(px(12.))
                .flex_none()
                .text_size(rems(0.7))
                .text_color(theme.text)
                .child(if is_applied { "✓" } else { "" }),
        )
        .child(body)
        // The mutation is refused while another is in flight, so the row
        // must not look live.
        .when(busy, |el| el.opacity(0.45))
        .when(!busy, |el| {
            el.cursor_pointer()
                .hover(move |el| el.bg(hover_bg))
                .on_click(move |_, _window, cx| on_toggle(key.clone(), is_applied, cx))
        })
        .into_any_element()
}
