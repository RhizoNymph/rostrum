//! The multi-repo pull request feed.
//!
//! Every repo and PR is flattened into one row stream rendered by a single
//! virtualized `list`; per-repo "containers" are reconstructed by having each
//! row draw the portion of the border that belongs to it. See
//! `docs/features/repo_feed.md` for why nesting lists does not work.

use std::rc::Rc;

use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, App, Context, Div, Entity, ListAlignment, ListState, Subscription, Window, div,
    list, prelude::*, px, rems,
};
use rostrum_core::{
    Chrome, Feed, FeedRow, Mergeable, PrIx, RepoIx, RepoState, ReviewDecision, Selection, flatten,
};
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, DiffStat, Dot, Initial, h_flex, hex_color, v_flex},
};

use crate::sync::Store;

/// Corner radius of a repo container, in pixels.
const ROW_RADIUS: f32 = 8.;

pub struct FeedView {
    store: Entity<Store>,
    feed: Rc<Feed>,
    list: ListState,
    _subscriptions: Vec<Subscription>,
}

impl FeedView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let feed = Rc::new(build(&store, cx));
        let list = ListState::new(feed.len(), ListAlignment::Top, px(400.));
        let subscriptions = vec![cx.observe(&store, |this, _, cx| this.store_changed(cx))];

        Self {
            store,
            feed,
            list,
            _subscriptions: subscriptions,
        }
    }

    /// Rebuild the row stream when the store changes.
    ///
    /// Rows address state by index, so a poll that returns identical structure
    /// leaves the stream equal even when PR contents changed. In that case a
    /// repaint suffices, and skipping the splice preserves scroll position and
    /// measured row heights.
    fn store_changed(&mut self, cx: &mut Context<Self>) {
        let rebuilt = build(&self.store, cx);
        if rebuilt != *self.feed {
            tracing::debug!(
                from = self.feed.len(),
                to = rebuilt.len(),
                "feed structure changed"
            );
            self.list.splice(0..self.feed.len(), rebuilt.len());
            self.feed = Rc::new(rebuilt);
        }
        cx.notify();
    }

    fn select(&mut self, repo: RepoIx, pr: PrIx, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            let Some(repo_state) = store.state.repos.get(repo.0) else {
                return;
            };
            let Some(pull) = repo_state.prs.get(pr.0) else {
                return;
            };
            store.state.selection = Some(Selection {
                repo: repo_state.id.clone(),
                pr: pull.number,
            });
            cx.notify();
        });
    }

    fn render_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.feed.row(ix) else {
            return div().into_any_element();
        };
        tracing::trace!(ix, ?row, "render row");
        let chrome = self.feed.chrome(ix);

        match row {
            FeedRow::Spacer { .. } => div().h(px(10.)).into_any_element(),
            FeedRow::RepoHeader { repo } => self.render_repo_header(repo, chrome, cx),
            FeedRow::PrRow { repo, pr } => self.render_pr_row(repo, pr, chrome, ix, cx),
            FeedRow::RepoEmpty { repo } => {
                self.render_notice(repo, chrome, "No open pull requests", cx)
            }
            FeedRow::RepoLoading { repo } => self.render_notice(repo, chrome, "Loading…", cx),
            FeedRow::RepoError { repo } => {
                let message = self
                    .repo_state(repo, cx)
                    .and_then(|r| r.load.error_message().map(str::to_string))
                    .unwrap_or_else(|| "Refresh failed".to_string());
                self.render_error(repo, chrome, message, cx)
            }
        }
    }

    fn repo_state<'a>(&self, repo: RepoIx, cx: &'a App) -> Option<&'a RepoState> {
        self.store.read(cx).state.repos.get(repo.0)
    }

    fn render_repo_header(
        &mut self,
        repo: RepoIx,
        chrome: Chrome,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.repo_state(repo, cx) else {
            return div().into_any_element();
        };

        let name = state.id.to_string();
        let count = state.prs.len();
        let collapsed = state.collapsed;
        let failed = state.load.is_failed();
        let id = state.id.clone();
        let store = self.store.clone();

        card(chrome, cx)
            .id(("repo-header", repo.0))
            .h(px(38.))
            .px_3()
            .bg(cx.theme().surface_raised)
            .child(
                h_flex()
                    .size_full()
                    .gap_2()
                    .child(
                        div()
                            .text_color(cx.theme().text_subtle)
                            .text_size(rems(0.7))
                            .child(if collapsed { "▸" } else { "▾" }),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().text)
                            .text_size(rems(0.82))
                            .child(name),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().text_subtle)
                            .text_size(rems(0.75))
                            .child(format!("{count}")),
                    )
                    .when(failed, |el| {
                        el.child(Chip::new("error").color(cx.theme().danger))
                    }),
            )
            .on_click(move |_, _window, cx| {
                store.update(cx, |store, cx| store.toggle_collapsed(&id, cx));
            })
            .into_any_element()
    }

    fn render_pr_row(
        &mut self,
        repo: RepoIx,
        pr: PrIx,
        chrome: Chrome,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.repo_state(repo, cx) else {
            return div().into_any_element();
        };
        let Some(pull) = state.prs.get(pr.0) else {
            return div().into_any_element();
        };

        let selected = self
            .store
            .read(cx)
            .state
            .selection
            .as_ref()
            .is_some_and(|s| s.repo == state.id && s.pr == pull.number);

        let number = pull.number.to_string();
        let title = pull.title.clone();
        let author = pull.author.as_ref().map(|a| a.login.clone());
        let updated = relative_time(pull.updated_at);
        let is_draft = pull.is_draft;
        let additions = pull.additions;
        let deletions = pull.deletions;
        let checks = pull.checks;
        let decision = pull.review_decision;
        let conflicting = pull.mergeable == Mergeable::Conflicting;
        let labels: Vec<_> = pull
            .labels
            .iter()
            .take(3)
            .map(|l| (l.name.clone(), hex_color(&l.color)))
            .collect();

        let theme = cx.theme().clone();

        card(chrome, cx)
            .id(("pr", ix))
            .px_3()
            .py_2()
            .when(selected, |el| el.bg(theme.surface_selected))
            .hover(|el| el.bg(theme.surface_hover))
            .cursor_pointer()
            .child(
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
                            .when(conflicting, |el| {
                                el.child(Chip::new("conflict").color(theme.danger))
                            })
                            .when_some(review_label(decision), |el, (text, color)| {
                                el.child(Chip::new(text).color(color(&theme)))
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
                            .child(DiffStat::new(additions, deletions))
                            .children(labels.into_iter().map(|(name, color)| {
                                let chip = Chip::new(name);
                                match color {
                                    Some(color) => chip.color(color),
                                    None => chip,
                                }
                            })),
                    ),
            )
            .on_click(cx.listener(move |this, _, _window, cx| this.select(repo, pr, cx)))
            .into_any_element()
    }

    fn render_notice(
        &mut self,
        repo: RepoIx,
        chrome: Chrome,
        message: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        card(chrome, cx)
            .id(("notice", repo.0))
            .px_3()
            .py_3()
            .text_color(cx.theme().text_subtle)
            .text_size(rems(0.78))
            .child(message.to_string())
            .into_any_element()
    }

    fn render_error(
        &mut self,
        repo: RepoIx,
        chrome: Chrome,
        message: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        card(chrome, cx)
            .id(("error", repo.0))
            .px_3()
            .py_3()
            .text_color(cx.theme().danger)
            .text_size(rems(0.78))
            .child(message)
            .into_any_element()
    }
}

impl Render for FeedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().px_3().child(
            list(
                self.list.clone(),
                cx.processor(|this, ix: usize, _window, cx| this.render_row(ix, cx)),
            )
            .size_full(),
        )
    }
}

fn build(store: &Entity<Store>, cx: &App) -> Feed {
    let store = store.read(cx);
    flatten(&store.state.repos, &store.state.filter)
}

/// Draw the portion of the container border this row owns.
fn card(chrome: Chrome, cx: &App) -> Div {
    let theme = cx.theme();

    if chrome == Chrome::None {
        return div();
    }

    let base = div()
        .bg(theme.surface)
        .border_l_1()
        .border_r_1()
        .border_color(theme.border);

    match chrome {
        Chrome::Top => base
            .border_t_1()
            .rounded_tl(px(ROW_RADIUS))
            .rounded_tr(px(ROW_RADIUS)),
        Chrome::Bottom => base
            .border_b_1()
            .rounded_bl(px(ROW_RADIUS))
            .rounded_br(px(ROW_RADIUS)),
        Chrome::Solo => base
            .border_t_1()
            .border_b_1()
            .rounded_tl(px(ROW_RADIUS))
            .rounded_tr(px(ROW_RADIUS))
            .rounded_bl(px(ROW_RADIUS))
            .rounded_br(px(ROW_RADIUS)),
        Chrome::Middle | Chrome::None => base,
    }
}

type ThemeColor = fn(&rostrum_ui::Theme) -> gpui::Hsla;

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
