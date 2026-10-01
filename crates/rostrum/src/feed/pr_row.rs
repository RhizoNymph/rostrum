//! The body of a pull request row, shared by the feed and the repository
//! view so the two lists cannot drift apart in what a row says.

use chrono::{DateTime, Utc};
use gpui::{Div, div, prelude::*, px, rems};
use rostrum_core::{MergeStatus, PullRequest, ReviewDecision};
use rostrum_local::LocalResult;
use rostrum_ui::{
    Theme,
    components::{Chip, DiffStat, Dot, Initial, h_flex, hex_color, v_flex},
};

/// The two lines of a pull request row: status dot, number, title and chips;
/// then author, age, size and labels.
///
/// `sync` is the latest "sync all" verdict for this pull request, if any.
/// `ix` makes the row's tooltip ids unique within its list.
pub(crate) fn pr_row_content(
    pull: &PullRequest,
    sync: Option<&LocalResult>,
    ix: usize,
    theme: &Theme,
) -> Div {
    let number = pull.number.to_string();
    let title = pull.title.clone();
    let author = pull.author.as_ref().map(|a| a.login.clone());
    let updated = relative_time(pull.updated_at);
    let is_draft = pull.is_draft;
    let checks = pull.checks;
    let merge = pull.merge_status();
    let base_ref = pull.base_ref.clone();
    // The exact count from the divergence batch, once it has answered.
    // Only "behind" earns a chip: ahead is the normal state of a pull
    // request and says nothing the reviewer must act on.
    let behind = pull
        .base_divergence
        .filter(|d| d.is_behind())
        .map(|d| d.behind);
    // The merge chip's own "behind" carries less than the count, so it
    // yields to the count when both are known; every other merge chip
    // says something the count does not.
    let merge_chip = merge
        .chip()
        .filter(|_| behind.is_none() || merge != MergeStatus::Behind);
    let labels: Vec<_> = pull
        .labels
        .iter()
        .take(3)
        .map(|l| (l.name.clone(), hex_color(&l.color)))
        .collect();
    // The latest "sync all" verdict, when it is one worth a chip.
    let sync_chip = sync.and_then(|result| {
        result
            .chip()
            .map(|text| (text, result.detail(), sync_chip_color(result)))
    });

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .child(Dot::new(theme.check_color(checks)))
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.72))
                        .child(number),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(if is_draft {
                            theme.text_muted
                        } else {
                            theme.text
                        })
                        .text_size(rems(0.82))
                        .child(title),
                )
                .when(is_draft, |el| {
                    el.child(Chip::new("draft").color(theme.draft))
                })
                .when_some(behind, |el, behind| {
                    el.child(
                        Chip::new(format!("↓{behind}"))
                            .color(theme.warning)
                            // A distinct tag from the merge chip's: GPUI
                            // element ids must be unique within the row, and
                            // both can render.
                            .tooltip(
                                ("behind-count", ix),
                                format!("{behind} commit(s) behind {base_ref}"),
                            ),
                    )
                })
                // `chip` returns nothing for draft and unstable: the draft
                // chip beside this one and the check dot at the head of the
                // row already say both.
                .when_some(merge_chip, |el, text| {
                    el.child(
                        Chip::new(text)
                            .color(theme.merge_color(merge))
                            .tooltip(("merge-status", ix), merge.explanation()),
                    )
                })
                .when_some(review_label(pull.review_decision), |el, (text, color)| {
                    el.child(Chip::new(text).color(color(theme)))
                })
                .when_some(sync_chip, |el, (text, detail, color)| {
                    el.child(
                        Chip::new(text)
                            .color(color(theme))
                            // Its own tag: the merge and behind chips can
                            // share the row, and GPUI element ids must not
                            // collide.
                            .tooltip(("sync-result", ix), detail),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_2()
                .pl(px(15.))
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
                        .child(updated),
                )
                .child(DiffStat::new(pull.additions, pull.deletions))
                .children(labels.into_iter().map(|(name, color)| {
                    let chip = Chip::new(name);
                    match color {
                        Some(color) => chip.color(color),
                        None => chip,
                    }
                })),
        )
}

pub(crate) type ThemeColor = fn(&Theme) -> gpui::Hsla;

/// Severity colour for a sync verdict chip: red for what stopped, accent for
/// what was handed on, amber for what git would not start.
fn sync_chip_color(result: &LocalResult) -> ThemeColor {
    match result {
        LocalResult::Conflicted(_) | LocalResult::Failed(_) => |t| t.danger,
        LocalResult::HandedOff { .. } => |t| t.accent,
        LocalResult::Refused(_)
        | LocalResult::NotCheckedOut
        | LocalResult::UpToDate
        | LocalResult::Completed => |t| t.warning,
    }
}

fn review_label(decision: Option<ReviewDecision>) -> Option<(&'static str, ThemeColor)> {
    match decision? {
        ReviewDecision::Approved => Some(("approved", |t| t.success)),
        ReviewDecision::ChangesRequested => Some(("changes", |t| t.danger)),
        ReviewDecision::ReviewRequired => None,
    }
}

pub(crate) fn relative_time(then: DateTime<Utc>) -> String {
    let seconds = (Utc::now() - then).num_seconds().max(0);
    match seconds {
        s if s < 60 => "just now".to_string(),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s if s < 2_592_000 => format!("{}d ago", s / 86_400),
        s => format!("{}mo ago", s / 2_592_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_buckets() {
        let now = Utc::now();
        assert_eq!(relative_time(now), "just now");
        assert_eq!(relative_time(now - chrono::Duration::minutes(5)), "5m ago");
        assert_eq!(relative_time(now - chrono::Duration::hours(3)), "3h ago");
        assert_eq!(relative_time(now - chrono::Duration::days(2)), "2d ago");
        assert_eq!(relative_time(now - chrono::Duration::days(70)), "2mo ago");
    }

    /// Clock skew between GitHub and the local machine must not produce
    /// nonsense like "-3m ago".
    #[test]
    fn future_timestamps_clamp_to_just_now() {
        assert_eq!(
            relative_time(Utc::now() + chrono::Duration::hours(1)),
            "just now"
        );
    }
}
