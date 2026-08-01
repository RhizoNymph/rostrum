//! Theme definition and access.
//!
//! A plain struct held as a GPUI global. Deliberately not Zed's `theme` crate,
//! which is GPL-3.0-or-later.

use gpui::{App, Global, Hsla, SharedString, rgb};
use rostrum_core::{CheckState, MergeStatus};

/// Fonts preferred in order; the first one actually installed wins. GPUI will
/// happily accept a family name that does not exist and then render nothing,
/// so the choice is resolved against the system list at startup.
const UI_FONT_CANDIDATES: &[&str] = &[
    "Inter",
    "SF Pro Text",
    "Segoe UI",
    "Ubuntu",
    "Cantarell",
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
];

const MONO_FONT_CANDIDATES: &[&str] = &[
    "JetBrains Mono",
    "Fira Code",
    "Cascadia Code",
    "SF Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "monospace",
];

#[derive(Clone, Debug)]
pub struct Theme {
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub surface_hover: Hsla,
    pub surface_selected: Hsla,

    pub border: Hsla,
    pub border_strong: Hsla,

    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    pub text_inverse: Hsla,

    pub accent: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub draft: Hsla,

    pub added: Hsla,
    pub removed: Hsla,

    pub ui_font: SharedString,
    pub mono_font: SharedString,
}

impl Theme {
    pub fn dark(ui_font: SharedString, mono_font: SharedString) -> Self {
        Self {
            background: rgb(0x0f1115).into(),
            surface: rgb(0x161920).into(),
            surface_raised: rgb(0x1c2029).into(),
            surface_hover: rgb(0x222735).into(),
            surface_selected: rgb(0x25304a).into(),

            border: rgb(0x272b36).into(),
            border_strong: rgb(0x39404f).into(),

            text: rgb(0xe4e7ee).into(),
            text_muted: rgb(0x9aa2b4).into(),
            text_subtle: rgb(0x6c7486).into(),
            text_inverse: rgb(0x0f1115).into(),

            accent: rgb(0x5b9dff).into(),
            success: rgb(0x3fb950).into(),
            warning: rgb(0xd29922).into(),
            danger: rgb(0xf85149).into(),
            draft: rgb(0x8b949e).into(),

            added: rgb(0x3fb950).into(),
            removed: rgb(0xf85149).into(),

            ui_font,
            mono_font,
        }
    }

    /// Colour for a CI rollup state.
    pub fn check_color(&self, state: Option<CheckState>) -> Hsla {
        match state {
            Some(CheckState::Success) => self.success,
            Some(CheckState::Failure | CheckState::Error) => self.danger,
            Some(CheckState::Pending | CheckState::Expected) => self.warning,
            None => self.text_subtle,
        }
    }

    /// Colour for a merge verdict.
    ///
    /// Only conflicts are red: they are the one state the branch cannot leave
    /// without someone editing code. Protection rules and a stale base are
    /// ordinary waypoints in a review, so they are amber rather than alarming.
    pub fn merge_color(&self, status: MergeStatus) -> Hsla {
        match status {
            MergeStatus::Conflicts => self.danger,
            MergeStatus::Blocked | MergeStatus::Behind | MergeStatus::Unstable => self.warning,
            MergeStatus::Ready => self.success,
            MergeStatus::Draft => self.draft,
            MergeStatus::Computing => self.text_subtle,
        }
    }
}

struct GlobalTheme(Theme);

impl Global for GlobalTheme {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        &self.global::<GlobalTheme>().0
    }
}

/// Install the theme, resolving font families against what is actually
/// installed on this machine.
pub fn init(cx: &mut App) {
    let available = cx.text_system().all_font_names();
    let ui_font = pick_font(&available, UI_FONT_CANDIDATES);
    let mono_font = pick_font(&available, MONO_FONT_CANDIDATES);
    tracing::info!(ui = %ui_font, mono = %mono_font, "resolved fonts");
    cx.set_global(GlobalTheme(Theme::dark(ui_font, mono_font)));
}

fn pick_font(available: &[String], candidates: &[&str]) -> SharedString {
    for candidate in candidates {
        if available.iter().any(|name| name == candidate) {
            return SharedString::from(candidate.to_string());
        }
    }
    available
        .first()
        .map(|name| SharedString::from(name.clone()))
        .unwrap_or_else(|| SharedString::from("sans-serif"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_first_installed_candidate() {
        let available = vec!["DejaVu Sans".to_string(), "Ubuntu".to_string()];
        assert_eq!(pick_font(&available, UI_FONT_CANDIDATES), "Ubuntu");
    }

    #[test]
    fn falls_back_to_any_available_font() {
        let available = vec!["Weird Font".to_string()];
        assert_eq!(pick_font(&available, UI_FONT_CANDIDATES), "Weird Font");
    }

    #[test]
    fn falls_back_to_generic_when_nothing_is_installed() {
        assert_eq!(pick_font(&[], UI_FONT_CANDIDATES), "sans-serif");
    }
}
