//! Drawing the CI view: the toolbar, the re-run banner, repositories whose
//! checks failed to load, the matrix (see [`super::matrix`]) and the log
//! pane.
//!
//! The matrix is one header row above one virtualized `list` of
//! [`rostrum_core::ci::GridLine`]s. Vertical scrolling is the list's; the
//! horizontal scroll is the view's own offset, applied to the header and to
//! every row alike, so the row-header column stays put and column headers
//! stay over their cells.

use gpui::{AnyElement, Context, ScrollWheelEvent, Window, div, list, prelude::*, px, rems};
use rostrum_core::LoadState;
use rostrum_ui::{
    ActiveTheme,
    components::{Button, ButtonStyle, Checkbox, h_flex, v_flex},
};

use super::{CI_CONTEXT, CiEvent, CiView, LOG_PANE_WIDTH, ROW_HEADER_WIDTH};

/// Room left for the list's own edge, so the last column is reachable.
const GUTTER: f32 = 16.;

impl CiView {
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let attention = self.filter.needs_attention;
        let running = self.store.read(cx).ci.any_running();
        h_flex()
            .flex_none()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("ci-back", "← Feed")
                    .tooltip("Back to the feed (escape or shift-c)")
                    .on_click(cx.listener(|_, _, _window, cx| cx.emit(CiEvent::Leave))),
            )
            .child(div().text_size(rems(0.9)).child("CI"))
            .child(
                Checkbox::new("ci-attention", "failing or running only (f)", attention)
                    .on_toggle(cx.listener(|this, _, _window, cx| this.toggle_attention(cx))),
            )
            .child(div().flex_1())
            .when(running, |el| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.text_subtle)
                        .child("checks running — refreshing every 15s"),
                )
            })
            .child(
                div()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child("hjkl move · enter log · r re-run"),
            )
    }

    /// The confirmation strip and the last re-run's outcome.
    fn render_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        if let Some(pending) = &self.confirm {
            let entry = self.grid.entry(&pending.cell)?.clone();
            let prompt = pending
                .targets
                .first()
                .map(|target| target.confirm_prompt(&entry))
                .unwrap_or_default();
            return Some(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(theme.surface_raised)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1().text_size(rems(0.8)).child(prompt))
                    .children(pending.targets.iter().enumerate().map(|(ix, target)| {
                        let target = *target;
                        Button::new(("ci-rerun", ix), target.label())
                            .style(if ix == 0 {
                                ButtonStyle::Primary
                            } else {
                                ButtonStyle::Subtle
                            })
                            .tooltip(target.confirm_prompt(&entry))
                            .on_click(
                                cx.listener(move |this, _, _window, cx| this.rerun(target, cx)),
                            )
                    }))
                    .child(
                        Button::new("ci-rerun-cancel", "Cancel").on_click(cx.listener(
                            |this, _, _window, cx| {
                                this.confirm = None;
                                cx.notify();
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        let (ok, message) = self.notice.clone()?;
        Some(
            div()
                .flex_none()
                .px_3()
                .py_1()
                .text_size(rems(0.76))
                .text_color(if ok { theme.text_muted } else { theme.danger })
                .child(message)
                .into_any_element(),
        )
    }
}

impl CiView {
    /// Repositories whose checks failed to load, one line each: with no
    /// per-repository section, this is where the failure shows.
    fn render_failures(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let failed: Vec<(String, String)> = self
            .grid
            .repos()
            .iter()
            .filter_map(|repo| {
                repo.load
                    .error_message()
                    .map(|message| (repo.repo.to_string(), message.to_string()))
            })
            .collect();
        if failed.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .flex_none()
                .px_3()
                .py_1()
                .gap_0p5()
                .border_b_1()
                .border_color(theme.border)
                .text_size(rems(0.74))
                .text_color(theme.danger)
                .children(
                    failed
                        .into_iter()
                        .map(|(repo, message)| div().child(format!("{repo}: {message}"))),
                )
                .into_any_element(),
        )
    }

    /// What the grid area says when it has no rows.
    fn empty_message(&self) -> &'static str {
        let loading = self
            .grid
            .repos()
            .iter()
            .any(|repo| matches!(repo.load, LoadState::Idle | LoadState::Loading));
        if self.filter.needs_attention && self.grid.hidden() > 0 {
            "Nothing failing or running"
        } else if loading {
            "Loading checks…"
        } else {
            "No open pull requests outside drafts"
        }
    }

    /// Horizontal wheel or trackpad movement scrolls the check columns;
    /// shift turns a vertical wheel horizontal.
    fn on_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        let dx = if delta.x != px(0.) {
            f32::from(delta.x)
        } else if event.modifiers.shift {
            f32::from(delta.y)
        } else {
            return;
        };
        self.hscroll
            .scroll_by(dx, self.columns_width(), self.columns_viewport);
        cx.notify();
    }
}

impl Render for CiView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let log = self.log.as_ref().map(|(view, _)| view.clone());
        // The view spans the window; the check columns get what the row
        // headers and the log pane leave.
        let log_width = if log.is_some() { LOG_PANE_WIDTH } else { 0. };
        self.columns_viewport =
            (f32::from(window.viewport_size().width) - ROW_HEADER_WIDTH - log_width - GUTTER)
                .max(0.);
        self.hscroll
            .clamp(self.columns_width(), self.columns_viewport);

        let banner = self.render_banner(cx);
        let failures = self.render_failures(cx);
        let header = self.render_header(&theme);
        let body = if self.grid.rows().is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .p_4()
                .text_size(rems(0.8))
                .text_color(theme.text_subtle)
                .child(self.empty_message())
                .into_any_element()
        } else {
            list(
                self.list.clone(),
                cx.processor(|this, ix: usize, _window, cx| this.render_line(ix, cx)),
            )
            .flex_1()
            .min_h_0()
            .w_full()
            .into_any_element()
        };

        v_flex()
            .size_full()
            .key_context(CI_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_up))
            .on_action(cx.listener(Self::on_down))
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_retry))
            .on_action(cx.listener(Self::on_toggle_attention))
            .on_action(cx.listener(Self::on_dismiss))
            .on_action(cx.listener(Self::on_leave))
            .child(self.render_toolbar(cx))
            .children(banner)
            .children(failures)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .id("ci-grid")
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .overflow_hidden()
                            .on_scroll_wheel(
                                cx.listener(|this, event, _window, cx| this.on_wheel(event, cx)),
                            )
                            .child(header)
                            .child(body),
                    )
                    .when_some(log, |el, log| {
                        el.child(div().w(px(1.)).h_full().bg(theme.border)).child(
                            div()
                                .w(px(LOG_PANE_WIDTH - 1.))
                                .h_full()
                                .flex_none()
                                .child(log),
                        )
                    }),
            )
    }
}
