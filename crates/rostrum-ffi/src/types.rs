//! Records and enums shared by more than one area of the API.
//!
//! Colours are never sent as palette values for chrome: a chip carries a
//! [`ColorRole`] and Kotlin maps roles onto its palette. The only literal
//! colours that cross the boundary are ones the data itself defines — label
//! colours chosen on GitHub, and syntax colours from the highlighting theme —
//! and those are ARGB `u32`s (`0xAARRGGBB`), ready for `Color(argb.toInt())`.

use rostrum_core as core_model;

/// A GitHub account as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UserRef {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// What a coloured element means, for Kotlin to map onto its palette.
///
/// The reference palette: success `#3fb950`, warning `#d29922`, danger
/// `#f85149`, draft `#8b949e`, accent `#5b9dff`, neutral = muted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ColorRole {
    Success,
    Warning,
    Danger,
    Draft,
    Accent,
    Neutral,
}

/// A short coloured tag: `conflict`, `↓3`, `approved`, `handed off`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Chip {
    pub text: String,
    pub role: ColorRole,
    /// A sentence explaining the chip, for a long-press or tooltip.
    pub tooltip: Option<String>,
}

/// A label as GitHub defines it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabelView {
    pub name: String,
    /// The label's colour as opaque ARGB, or `None` when GitHub's hex value
    /// did not parse (render it neutral).
    pub color: Option<u32>,
}

/// Which side of a diff a line or comment belongs to. `Right` is the new
/// file, `Left` the old one — GitHub's own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Side {
    Left,
    Right,
}

/// Rolled-up CI state of a commit, or of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CheckState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
}

/// GitHub's aggregate review verdict on a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// Whether a pull request can be merged, and if not, why — the single verdict
/// derived from GitHub's `mergeable` and `mergeStateStatus` together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MergeStatus {
    /// GitHub has not finished computing the merge state.
    Computing,
    Conflicts,
    Draft,
    /// Branch protection: a required review or check is missing.
    Blocked,
    /// The base has moved and protection requires the branch be current.
    Behind,
    /// Mergeable, but CI is not green. Does not block.
    Unstable,
    Ready,
}

/// The state of one submitted review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReviewState {
    Pending,
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
}

/// Where a pull request is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PullState {
    Open,
    Closed,
    Merged,
}

// --- conversions from the domain model -------------------------------------

impl From<&core_model::User> for UserRef {
    fn from(user: &core_model::User) -> Self {
        Self {
            login: user.login.clone(),
            avatar_url: user.avatar_url.clone(),
        }
    }
}

impl From<core_model::Side> for Side {
    fn from(side: core_model::Side) -> Self {
        match side {
            core_model::Side::Left => Self::Left,
            core_model::Side::Right => Self::Right,
        }
    }
}

impl From<Side> for core_model::Side {
    fn from(side: Side) -> Self {
        match side {
            Side::Left => Self::Left,
            Side::Right => Self::Right,
        }
    }
}

impl From<core_model::CheckState> for CheckState {
    fn from(state: core_model::CheckState) -> Self {
        match state {
            core_model::CheckState::Expected => Self::Expected,
            core_model::CheckState::Error => Self::Error,
            core_model::CheckState::Failure => Self::Failure,
            core_model::CheckState::Pending => Self::Pending,
            core_model::CheckState::Success => Self::Success,
        }
    }
}

impl From<core_model::ReviewDecision> for ReviewDecision {
    fn from(decision: core_model::ReviewDecision) -> Self {
        match decision {
            core_model::ReviewDecision::Approved => Self::Approved,
            core_model::ReviewDecision::ChangesRequested => Self::ChangesRequested,
            core_model::ReviewDecision::ReviewRequired => Self::ReviewRequired,
        }
    }
}

impl From<core_model::MergeStatus> for MergeStatus {
    fn from(status: core_model::MergeStatus) -> Self {
        match status {
            core_model::MergeStatus::Computing => Self::Computing,
            core_model::MergeStatus::Conflicts => Self::Conflicts,
            core_model::MergeStatus::Draft => Self::Draft,
            core_model::MergeStatus::Blocked => Self::Blocked,
            core_model::MergeStatus::Behind => Self::Behind,
            core_model::MergeStatus::Unstable => Self::Unstable,
            core_model::MergeStatus::Ready => Self::Ready,
        }
    }
}

impl From<core_model::ReviewState> for ReviewState {
    fn from(state: core_model::ReviewState) -> Self {
        match state {
            core_model::ReviewState::Pending => Self::Pending,
            core_model::ReviewState::Commented => Self::Commented,
            core_model::ReviewState::Approved => Self::Approved,
            core_model::ReviewState::ChangesRequested => Self::ChangesRequested,
            core_model::ReviewState::Dismissed => Self::Dismissed,
        }
    }
}

impl From<&core_model::Label> for LabelView {
    fn from(label: &core_model::Label) -> Self {
        Self {
            name: label.name.clone(),
            color: hex_to_argb(&label.color),
        }
    }
}

/// Parse GitHub's six-digit hex (no `#`, though one is tolerated) into opaque
/// ARGB.
pub(crate) fn hex_to_argb(hex: &str) -> Option<u32> {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16)
        .ok()
        .map(|rgb| 0xFF00_0000 | rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_hex_becomes_opaque_argb() {
        assert_eq!(hex_to_argb("d73a4a"), Some(0xFFD7_3A4A));
        assert_eq!(hex_to_argb("#0E8A16"), Some(0xFF0E_8A16));
        assert_eq!(hex_to_argb("000000"), Some(0xFF00_0000));
    }

    #[test]
    fn malformed_label_hex_is_none() {
        for bad in ["", "fff", "gggggg", "1234567", "12345", "#12 456"] {
            assert_eq!(hex_to_argb(bad), None, "{bad}");
        }
    }

    #[test]
    fn sides_round_trip_through_the_domain_type() {
        for side in [Side::Left, Side::Right] {
            assert_eq!(Side::from(core_model::Side::from(side)), side);
        }
    }
}
