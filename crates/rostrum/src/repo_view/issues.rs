//! The issues half of the repository view's sidebar.
//!
//! Reads `RepoState::issues` and `RepoState::issues_load`. The desktop does
//! not fetch issues itself yet; until it does the list rests at "not loaded",
//! and it fills in without changes here once something populates the state.

use gpui::{Div, div, prelude::*, px, rems};
use rostrum_core::{Issue, LoadState};
use rostrum_ui::{
    Theme,
    components::{Chip, Initial, h_flex, hex_color, v_flex},
};

use crate::feed::relative_time;

/// What to say in place of the list when it has no rows.
pub fn empty_message(load: &LoadState) -> String {
    match load {
        LoadState::Idle => "Issues are not loaded yet".to_string(),
        LoadState::Loading => "Loading…".to_string(),
        LoadState::Failed { message, .. } => message.clone(),
        LoadState::Loaded { .. } => "No open issues".to_string(),
    }
}

/// The header count: known once the list has loaded at least once.
pub fn header_count(load: &LoadState, len: usize) -> Option<usize> {
    match load {
        LoadState::Idle | LoadState::Loading if len == 0 => None,
        _ => Some(len),
    }
}

/// Two lines, like a pull request row: number, title and labels; then
/// author, age and comment count.
pub fn issue_row_content(issue: &Issue, theme: &Theme) -> Div {
    let labels: Vec<_> = issue
        .labels
        .iter()
        .take(3)
        .map(|l| (l.name.clone(), hex_color(&l.color)))
        .collect();
    let author = issue.author.as_ref().map(|a| a.login.clone());

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.72))
                        .child(issue.number.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(theme.text)
                        .text_size(rems(0.82))
                        .child(issue.title.clone()),
                )
                .children(labels.into_iter().map(|(name, color)| {
                    let chip = Chip::new(name);
                    match color {
                        Some(color) => chip.color(color),
                        None => chip,
                    }
                })),
        )
        .child(
            h_flex()
                .gap_2()
                .pl(px(4.))
                .when_some(author, |el, login| {
                    el.child(Initial::new(login.clone())).child(
                        div()
                            .text_color(theme.text_muted)
                            .text_size(rems(0.72))
                            .child(login),
                    )
                })
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.72))
                        .child(relative_time(issue.updated_at)),
                )
                .when(issue.comment_count > 0, |el| {
                    el.child(
                        div()
                            .text_color(theme.text_subtle)
                            .text_size(rems(0.72))
                            .child(format!("{} comments", issue.comment_count)),
                    )
                }),
        )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn the_count_is_unknown_until_the_list_has_loaded() {
        assert_eq!(header_count(&LoadState::Idle, 0), None);
        assert_eq!(header_count(&LoadState::Loading, 0), None);
        assert_eq!(
            header_count(&LoadState::Loaded { at: Utc::now() }, 0),
            Some(0)
        );
        // A background refresh of a loaded list keeps showing its count.
        assert_eq!(header_count(&LoadState::Loading, 4), Some(4));
    }

    #[test]
    fn an_empty_list_explains_itself_by_load_state() {
        assert_eq!(empty_message(&LoadState::Idle), "Issues are not loaded yet");
        assert_eq!(
            empty_message(&LoadState::Loaded { at: Utc::now() }),
            "No open issues"
        );
        assert_eq!(
            empty_message(&LoadState::Failed {
                message: "boom".into(),
                at: Utc::now()
            }),
            "boom"
        );
    }
}
