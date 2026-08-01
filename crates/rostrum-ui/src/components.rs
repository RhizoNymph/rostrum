//! Small reusable components built on `gpui`.

use gpui::{App, Div, Hsla, SharedString, Window, div, prelude::*, px, rems};

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
}

impl Chip {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            color: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let accent = self.color.unwrap_or(cx.theme().text_muted);
        div()
            .px_1p5()
            .py_0p5()
            .rounded_full()
            .border_1()
            .border_color(Hsla { a: 0.45, ..accent })
            .bg(Hsla { a: 0.14, ..accent })
            .text_color(accent)
            .text_size(rems(0.7))
            .child(self.label)
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
