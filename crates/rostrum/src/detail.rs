//! Detail pane for the selected pull request.
//!
//! Phase 1 renders the PR header only. The conversation timeline, files, and
//! review actions are phases 2–4; see `docs/features/pr_detail.md`.

use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px, rems};
use rostrum_core::{Mergeable, PullRequest, RepoState, ReviewDecision};
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, DiffStat, Initial, h_flex, hex_color, v_flex},
};

use crate::sync::Store;

pub struct DetailView {
    store: Entity<Store>,
    _subscriptions: Vec<Subscription>,
}

impl DetailView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self {
            store,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for DetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);

        let Some((repo, pull)) = store.state.selected_pr() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_subtle)
                .text_size(rems(0.85))
                .child(if store.state.total_open_prs() == 0 {
                    "No pull requests loaded".to_string()
                } else {
                    "Select a pull request".to_string()
                })
                .into_any_element();
        };

        render_pr(repo, pull, &theme).into_any_element()
    }
}

fn render_pr(
    repo: &RepoState,
    pull: &PullRequest,
    theme: &rostrum_ui::Theme,
) -> impl IntoElement + use<> {
    let author = pull.author.as_ref().map(|a| a.login.clone());

    v_flex()
        // `.id()` is required before `.overflow_y_scroll()`: scrolling state
        // lives on `StatefulInteractiveElement`, which needs a stable id.
        .id("pr-detail")
        .size_full()
        .p_5()
        .gap_4()
        .overflow_y_scroll()
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_color(theme.text)
                        .text_size(rems(1.25))
                        .child(pull.title.clone()),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_size(rems(0.8))
                        .text_color(theme.text_muted)
                        .child(format!("{} {}", repo.id, pull.number))
                        .when_some(author, |el, login| {
                            el.child(Initial::new(login.clone())).child(login)
                        })
                        .child(DiffStat::new(pull.additions, pull.deletions))
                        .child(format!("{} files", pull.changed_files)),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .when(pull.is_draft, |el| {
                    el.child(Chip::new("draft").color(theme.draft))
                })
                .child(
                    Chip::new(mergeable_text(pull.mergeable)).color(match pull.mergeable {
                        Mergeable::Mergeable => theme.success,
                        Mergeable::Conflicting => theme.danger,
                        Mergeable::Unknown => theme.text_subtle,
                    }),
                )
                .when_some(review_text(pull.review_decision), |el, (text, color)| {
                    el.child(Chip::new(text).color(color(theme)))
                })
                .when_some(pull.checks, |el, state| {
                    el.child(
                        Chip::new(format!("{state:?}").to_lowercase())
                            .color(theme.check_color(Some(state))),
                    )
                })
                .children(pull.labels.iter().map(|label| {
                    let chip = Chip::new(label.name.clone());
                    match hex_color(&label.color) {
                        Some(color) => chip.color(color),
                        None => chip,
                    }
                })),
        )
        .child(
            h_flex()
                .gap_1()
                .text_size(rems(0.78))
                .text_color(theme.text_subtle)
                .child(pull.base_ref.clone())
                .child("←")
                .child(pull.head_ref.clone()),
        )
        .child(
            div()
                .text_size(rems(0.78))
                .text_color(theme.text_subtle)
                .child(pull.url.clone()),
        )
        .child(
            div()
                .mt_2()
                .p_3()
                .rounded_tl(px(6.))
                .rounded_tr(px(6.))
                .rounded_bl(px(6.))
                .rounded_br(px(6.))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .text_size(rems(0.8))
                .text_color(theme.text_muted)
                .child(format!(
                    "{} comments. Conversation, diff, and review actions are not implemented yet.",
                    pull.comment_count
                )),
        )
}

fn mergeable_text(mergeable: Mergeable) -> &'static str {
    match mergeable {
        Mergeable::Mergeable => "mergeable",
        Mergeable::Conflicting => "conflicting",
        Mergeable::Unknown => "merge state unknown",
    }
}

type ThemeColor = fn(&rostrum_ui::Theme) -> gpui::Hsla;

fn review_text(decision: Option<ReviewDecision>) -> Option<(&'static str, ThemeColor)> {
    match decision? {
        ReviewDecision::Approved => Some(("approved", |t| t.success)),
        ReviewDecision::ChangesRequested => Some(("changes requested", |t| t.danger)),
        ReviewDecision::ReviewRequired => Some(("review required", |t| t.text_muted)),
    }
}
