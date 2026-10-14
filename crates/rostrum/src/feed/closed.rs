//! The Closed tab's rows: one list of merged and closed pull requests across
//! every repository, newest ending first. The order and the row stream come
//! from `rostrum_core::closed`; this draws them.
//!
//! Each row says how the pull request ended — a "merged" or "closed" chip
//! and, on the second line, "merged 3h ago" — and, because the list mixes
//! repositories without headers, which repository it is from.

use gpui::{AnyElement, Context, Div, div, prelude::*, px, rems};
use rostrum_core::{Chrome, ClosedIx, ClosedPull, FeedRow, PullEnding, RepoId, RepoIx, Selection};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Chip, DiffStat, Initial, h_flex, hex_color, v_flex},
};

use super::{FeedView, relative_time, rows::card};

impl FeedView {
    pub(super) fn render_closed_row(
        &mut self,
        repo: RepoIx,
        closed_ix: ClosedIx,
        chrome: Chrome,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.repo_state(repo, cx) else {
            return div().into_any_element();
        };
        let Some(closed) = state.closed.get(closed_ix.0) else {
            return div().into_any_element();
        };

        let selected = self.store.read(cx).state.selection
            == Some(Selection::PullRequest {
                repo: state.id.clone(),
                number: closed.number(),
            });
        let theme = cx.theme().clone();
        let content = closed_row_content(&state.id, closed, &theme);

        card(chrome, cx)
            .id(("closed", ix))
            .px_3()
            .py_2()
            .when(selected, |el| el.bg(theme.surface_selected))
            .hover(|el| el.bg(theme.surface_hover))
            .cursor_pointer()
            .child(content)
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.select(
                    FeedRow::ClosedRow {
                        repo,
                        closed: closed_ix,
                    },
                    cx,
                )
            }))
            .into_any_element()
    }
}

/// Colour of an ending's chip: the accent for a merge, red for a close.
fn ending_color(ending: PullEnding, theme: &Theme) -> gpui::Hsla {
    if ending.is_merged() {
        theme.accent
    } else {
        theme.danger
    }
}

/// The two lines of a closed row: ending chip, number, title; then
/// repository, author, when it ended, size and labels.
pub(crate) fn closed_row_content(repo: &RepoId, closed: &ClosedPull, theme: &Theme) -> Div {
    let pull = &closed.pull;
    let ending = closed.ending;
    let author = pull.author.as_ref().map(|a| a.login.clone());
    let labels: Vec<_> = pull
        .labels
        .iter()
        .take(3)
        .map(|l| (l.name.clone(), hex_color(&l.color)))
        .collect();

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .child(Chip::new(ending.verb()).color(ending_color(ending, theme)))
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.72))
                        .child(pull.number.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(theme.text)
                        .text_size(rems(0.82))
                        .child(pull.title.clone()),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .pl(px(15.))
                .flex_wrap()
                .child(
                    div()
                        .text_color(theme.text_muted)
                        .text_size(rems(0.72))
                        .child(repo.to_string()),
                )
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
                        .text_color(ending_color(ending, theme))
                        .text_size(rems(0.72))
                        .child(format!("{} {}", ending.verb(), relative_time(ending.at()))),
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
