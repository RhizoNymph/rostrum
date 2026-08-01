//! Small reusable components built on `gpui`.

use gpui::{App, Context, Div, ElementId, Hsla, SharedString, Window, div, prelude::*, px, rems};

use crate::theme::ActiveTheme;

/// Horizontal stack, vertically centred.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// Vertical stack.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// Parse GitHub's six-digit hex label colour (no leading `#`).
pub fn hex_color(hex: &str) -> Option<Hsla> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(gpui::rgb(value).into())
}

/// A small rounded label, used for PR labels and status text.
#[derive(IntoElement)]
pub struct Chip {
    label: SharedString,
    color: Option<Hsla>,
    /// Tooltip text paired with the element id that hovering needs. The two
    /// travel together so a chip cannot ask for a tooltip without supplying
    /// the identity GPUI requires to track the hover.
    tooltip: Option<(ElementId, SharedString)>,
}

impl Chip {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            color: None,
            tooltip: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn tooltip(mut self, id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some((id.into(), text.into()));
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let accent = self.color.unwrap_or(cx.theme().text_muted);
        let base = div()
            .px_1p5()
            .py_0p5()
            .rounded_full()
            .border_1()
            .border_color(Hsla { a: 0.45, ..accent })
            .bg(Hsla { a: 0.14, ..accent })
            .text_color(accent)
            .text_size(rems(0.7))
            .child(self.label);

        // `.id()` yields a `Stateful<Div>`, a different type, so the two cases
        // are erased rather than folded together with `when_some`.
        match self.tooltip {
            Some((id, text)) => base
                .id(id)
                .tooltip(move |_window, cx| cx.new(|_| TextTooltip { text: text.clone() }).into())
                .into_any_element(),
            None => base.into_any_element(),
        }
    }
}

/// A filled circle, used for CI status.
#[derive(IntoElement)]
pub struct Dot {
    color: Hsla,
    size: gpui::Pixels,
}

impl Dot {
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            size: px(7.),
        }
    }

    pub fn size(mut self, size: gpui::Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Dot {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .w(self.size)
            .h(self.size)
            .rounded_full()
            .bg(self.color)
            .flex_none()
    }
}

/// Avatar stand-in: a coloured circle bearing the first letter of a login.
///
/// Avoids fetching remote images, which the prototype does not need.
#[derive(IntoElement)]
pub struct Initial {
    login: SharedString,
}

impl Initial {
    pub fn new(login: impl Into<SharedString>) -> Self {
        Self {
            login: login.into(),
        }
    }
}

impl RenderOnce for Initial {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let letter = self
            .login
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());

        // Stable per-login hue so the same person keeps the same colour.
        let hue = self
            .login
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let color = Hsla {
            h: (hue % 360) as f32 / 360.,
            s: 0.45,
            l: 0.55,
            a: 1.0,
        };

        div()
            .w(px(18.))
            .h(px(18.))
            .flex_none()
            .rounded_full()
            .bg(Hsla { a: 0.25, ..color })
            .text_color(color)
            .text_size(rems(0.65))
            .flex()
            .items_center()
            .justify_center()
            .child(letter)
    }
}

/// `+123 −45`, coloured per side.
#[derive(IntoElement)]
pub struct DiffStat {
    additions: u32,
    deletions: u32,
}

impl DiffStat {
    pub fn new(additions: u32, deletions: u32) -> Self {
        Self {
            additions,
            deletions,
        }
    }
}

impl RenderOnce for DiffStat {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_1()
            .text_size(rems(0.7))
            .child(
                div()
                    .text_color(cx.theme().added)
                    .child(format!("+{}", self.additions)),
            )
            .child(
                div()
                    .text_color(cx.theme().removed)
                    .child(format!("−{}", self.deletions)),
            )
    }
}

/// Visual weight of a [`Button`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    #[default]
    Subtle,
    Primary,
    Danger,
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    style: ButtonStyle,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static>;

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Subtle,
            disabled: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// A disabled button keeps its tooltip, so the reason stays discoverable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = match self.style {
            ButtonStyle::Primary => (theme.accent, theme.text_inverse, theme.accent),
            ButtonStyle::Danger => (
                Hsla {
                    a: 0.16,
                    ..theme.danger
                },
                theme.danger,
                Hsla {
                    a: 0.5,
                    ..theme.danger
                },
            ),
            ButtonStyle::Subtle => (theme.surface_raised, theme.text, theme.border_strong),
        };
        let accent = theme.accent;
        let handler = self.on_click;
        let disabled = self.disabled;

        div()
            .id(self.id)
            .px_2p5()
            .py_1()
            .rounded_tl(px(5.))
            .rounded_tr(px(5.))
            .rounded_bl(px(5.))
            .rounded_br(px(5.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_size(rems(0.78))
            .text_color(fg)
            .when(disabled, |el| el.opacity(0.45))
            .when(!disabled, |el| {
                el.cursor_pointer().hover(move |el| el.border_color(accent))
            })
            .when_some(self.tooltip, |el, text| {
                el.tooltip(move |_window, cx| cx.new(|_| TextTooltip { text: text.clone() }).into())
            })
            .child(self.label)
            .on_click(move |event, window, cx| {
                if !disabled && let Some(handler) = handler.as_ref() {
                    handler(event, window, cx);
                }
            })
    }
}

/// Minimal tooltip body.
pub struct TextTooltip {
    pub text: SharedString,
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded_tl(px(5.))
            .rounded_tr(px(5.))
            .rounded_bl(px(5.))
            .rounded_br(px(5.))
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.border_strong)
            .text_size(rems(0.75))
            .text_color(theme.text)
            .child(self.text.clone())
    }
}

/// A labelled checkbox.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: SharedString,
    checked: bool,
    on_toggle: Option<ClickHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked,
            on_toggle: None,
        }
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let checked = self.checked;
        let handler = self.on_toggle;

        h_flex()
            .id(self.id)
            .gap_1p5()
            .cursor_pointer()
            .text_size(rems(0.75))
            .text_color(if checked {
                theme.text
            } else {
                theme.text_muted
            })
            .child(
                div()
                    .w(px(13.))
                    .h(px(13.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_tl(px(3.))
                    .rounded_tr(px(3.))
                    .rounded_bl(px(3.))
                    .rounded_br(px(3.))
                    .border_1()
                    .border_color(if checked {
                        theme.accent
                    } else {
                        theme.border_strong
                    })
                    .bg(if checked {
                        theme.accent
                    } else {
                        theme.background
                    })
                    .text_size(rems(0.6))
                    .text_color(theme.text_inverse)
                    .child(if checked { "✓" } else { "" }),
            )
            .child(self.label)
            .on_click(move |event, window, cx| {
                if let Some(handler) = handler.as_ref() {
                    handler(event, window, cx);
                }
            })
    }
}

/// A single tab in a [`tab_bar`].
pub struct Tab {
    pub label: SharedString,
    pub badge: Option<usize>,
}

impl Tab {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            badge: None,
        }
    }

    pub fn badge(mut self, count: usize) -> Self {
        self.badge = Some(count);
        self
    }
}

/// Render a row of tabs. `on_select` receives the index of the chosen tab.
pub fn tab_bar(
    tabs: Vec<Tab>,
    selected: usize,
    cx: &App,
    on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let theme = cx.theme();
    let (text, muted, accent, border) = (theme.text, theme.text_muted, theme.accent, theme.border);

    h_flex()
        .gap_1()
        .border_b_1()
        .border_color(border)
        .children(tabs.into_iter().enumerate().map(move |(ix, tab)| {
            let active = ix == selected;
            let on_select = on_select.clone();
            h_flex()
                .id(("tab", ix))
                .gap_1p5()
                .px_3()
                .py_2()
                .cursor_pointer()
                .text_size(rems(0.8))
                .border_b_2()
                .border_color(if active {
                    accent
                } else {
                    gpui::transparent_black()
                })
                .text_color(if active { text } else { muted })
                .hover(move |el| el.text_color(text))
                .child(tab.label)
                .when_some(tab.badge, |el, count| {
                    el.child(
                        div()
                            .text_size(rems(0.68))
                            .text_color(muted)
                            .child(count.to_string()),
                    )
                })
                .on_click(move |_event, window, cx| on_select(ix, window, cx))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_label_colors() {
        assert!(hex_color("d73a4a").is_some());
        assert!(hex_color("#d73a4a").is_some());
        assert!(hex_color("  ffffff  ").is_some());
    }

    #[test]
    fn rejects_malformed_colors() {
        assert!(hex_color("").is_none());
        assert!(hex_color("fff").is_none());
        assert!(hex_color("gggggg").is_none());
        assert!(hex_color("d73a4a11").is_none());
    }
}
