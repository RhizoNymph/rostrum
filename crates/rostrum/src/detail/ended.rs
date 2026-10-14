//! A pull request that has ended — merged, or closed without merging — in
//! the detail pane.
//!
//! An ended pull request cannot be approved, merged, closed again or moved
//! in or out of draft, so the pane offers none of that: its action bar
//! becomes a statement of how it ended plus the comment composer (GitHub
//! accepts comments on a closed pull request). The conversation, files and
//! checks still read as they do for an open one.

use chrono::{DateTime, Utc};
use gpui::{AnyElement, Context, Hsla, div, prelude::*, rems};
use rostrum_core::{PullEnding, PullState};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, Chip, h_flex, v_flex},
};

use super::PrDetail;
use crate::feed::relative_time;

/// How the pane knows a pull request has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// From the Closed tab's list: the kind and the time.
    Known(PullEnding),
    /// From the conversation's `state` alone — a pull request merged while
    /// the pane was open, or one a link opened — so no time is known.
    Merged,
    Closed,
}

impl Ended {
    fn is_merged(self) -> bool {
        match self {
            Self::Known(ending) => ending.is_merged(),
            Self::Merged => true,
            Self::Closed => false,
        }
    }

    fn at(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Known(ending) => Some(ending.at()),
            Self::Merged | Self::Closed => None,
        }
    }

    /// "merged 3h ago", or "merged" when the time is not known.
    pub(crate) fn describe(self) -> String {
        let verb = if self.is_merged() { "merged" } else { "closed" };
        match self.at() {
            Some(at) => format!("{verb} {}", relative_time(at)),
            None => verb.to_string(),
        }
    }

    pub(crate) fn color(self, theme: &Theme) -> Hsla {
        if self.is_merged() {
            theme.accent
        } else {
            theme.danger
        }
    }
}

/// Whether, and how, the pull request the pane shows has ended.
///
/// `listed` is its entry on the Closed tab, if the feed holds one; `state` is
/// what the conversation fetch reported, if it has landed. The conversation
/// is fetched when the pane opens, so it is the fresher of the two:
///
/// - `Open` means not ended, whatever the list says — it was reopened since
///   the list was fetched.
/// - `Merged` or `Closed` agrees with the list when the kinds match, and
///   then the list supplies the time; otherwise the kind stands alone.
/// - No state yet (still loading, or an older cache) defers to the list.
pub(crate) fn ended(listed: Option<PullEnding>, state: Option<PullState>) -> Option<Ended> {
    match (state, listed) {
        (Some(PullState::Open), _) => None,
        (Some(state), Some(ending)) if ending.state() == state => Some(Ended::Known(ending)),
        (Some(PullState::Merged), _) => Some(Ended::Merged),
        (Some(PullState::Closed), _) => Some(Ended::Closed),
        (None, listed) => listed.map(Ended::Known),
    }
}

impl PrDetail {
    /// The action bar of an ended pull request: how it ended, the comment
    /// composer and its button, and nothing that would change its state.
    pub(super) fn render_ended_actions(
        &self,
        ended: Ended,
        busy: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        v_flex()
            .gap_2()
            .flex_none()
            .p_3()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .children(self.status_lines(&theme))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Chip::new(ended.describe()).color(ended.color(&theme)))
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_muted)
                            .child(if ended.is_merged() {
                                "This pull request has been merged; review and merge controls are not offered."
                            } else {
                                "This pull request was closed without merging; review and merge controls are not offered."
                            }),
                    ),
            )
            .child(self.composer.clone())
            .child(
                h_flex().gap_2().child(
                    Button::new("comment", "Comment")
                        .disabled(busy)
                        .on_click(Self::on_click(cx, |this, cx| this.post_comment(cx))),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).expect("valid")
    }

    #[test]
    fn an_open_pull_request_has_not_ended() {
        assert_eq!(ended(None, None), None);
        assert_eq!(ended(None, Some(PullState::Open)), None);
    }

    /// Reopened since the Closed tab fetched it: the conversation wins.
    #[test]
    fn a_fresh_open_state_overrides_a_stale_closed_entry() {
        let listed = PullEnding::Closed { at: at(5) };
        assert_eq!(ended(Some(listed), Some(PullState::Open)), None);
    }

    #[test]
    fn a_listed_ending_supplies_the_time_when_the_kinds_agree() {
        let listed = PullEnding::Merged { at: at(5) };
        assert_eq!(
            ended(Some(listed), Some(PullState::Merged)),
            Some(Ended::Known(listed))
        );
        assert_eq!(ended(Some(listed), None), Some(Ended::Known(listed)));
    }

    #[test]
    fn a_disagreeing_list_entry_yields_to_the_conversation() {
        let listed = PullEnding::Closed { at: at(5) };
        assert_eq!(
            ended(Some(listed), Some(PullState::Merged)),
            Some(Ended::Merged)
        );
    }

    /// Merged while the pane was open: no list entry, the state alone.
    #[test]
    fn the_conversation_state_alone_ends_a_pull_request() {
        assert_eq!(ended(None, Some(PullState::Merged)), Some(Ended::Merged));
        assert_eq!(ended(None, Some(PullState::Closed)), Some(Ended::Closed));
    }

    #[test]
    fn the_description_carries_the_time_only_when_known() {
        assert_eq!(Ended::Merged.describe(), "merged");
        assert_eq!(Ended::Closed.describe(), "closed");
        let known = Ended::Known(PullEnding::Closed {
            at: Utc::now() - chrono::Duration::hours(3),
        });
        assert_eq!(known.describe(), "closed 3h ago");
    }
}
