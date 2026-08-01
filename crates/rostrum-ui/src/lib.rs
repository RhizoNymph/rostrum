//! Local component layer built directly on `gpui`.
//!
//! Deliberately does not depend on Zed's `ui`/`theme` crates, which are
//! GPL-3.0-or-later; `gpui` itself is Apache-2.0.

pub mod components;
pub mod input;
pub mod markdown;
pub mod selection;
pub mod theme;

pub use input::{InputEvent, TextInput};
pub use selection::SelectableText;
pub use theme::{ActiveTheme, Theme};
