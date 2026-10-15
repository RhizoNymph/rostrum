//! The toolbar's **Push all** button, its progress line, and the one
//! confirmation a batch with rewrites needs.
//!
//! Fast-forwards never ask. When any branch has diverged from its upstream
//! (a local rebase, say), the panel lists every such branch — repository,
//! pull request, the remote branch, and the exact oid that will be replaced
//! — and offers three answers for the whole batch: push everything,
//! force-pushing the listed branches with their leases; push the
//! fast-forwards only; or cancel and push nothing.

use gpui::{AnyElement, Context, div, prelude::*, rems};
use rostrum_local::{PushKind, RewriteDecision};
use rostrum_ui::{
    ActiveTheme,
    components::{Button, ButtonStyle, v_flex},
};

use super::FeedView;
use crate::sync::PushAll;

impl FeedView {
    pub(super) fn push_all_button(&self, syncing: bool, cx: &mut Context<Self>) -> AnyElement {
        Button::new("sync-push", "Push all")
            .disabled(syncing)
            .tooltip(if syncing {
                "A sync is already running"
            } else {
                "Push every checked-out pull request branch that is ahead of its upstream. Diverged branches are listed for confirmation first"
            })
            .on_click(cx.listener(|this, _, _window, cx| {
                this.store.update(cx, |store, cx| store.push_all(cx));
            }))
            .into_any_element()
    }

    /// The progress line for a Push all, when one is the latest batch.
    pub(super) fn push_all_status(&self, cx: &Context<Self>) -> Option<String> {
        match self.store.read(cx).push_all_state()? {
            PushAll::Planning { total } => Some(format!("Push all: checking {total} branch(es)…")),
            PushAll::Confirming { .. } => Some("Push all: waiting for confirmation".into()),
            PushAll::Running(progress) if progress.is_finished() => {
                Some(format!("Push all: {}", progress.summary().describe()))
            }
            PushAll::Running(progress) => {
                Some(format!("Push all: {}/{}…", progress.done, progress.total))
            }
        }
    }

    /// The confirmation, while a planned batch has rewrites to ask about.
    pub(super) fn render_push_all_confirm(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);
        let PushAll::Confirming { plan, .. } = store.push_all_state()? else {
            return None;
        };
        let rewrites = plan.rewrites();
        let lines: Vec<String> = rewrites
            .iter()
            .map(|((repo, number), planned)| {
                let behind = match &planned.kind {
                    PushKind::Rewrite { behind, .. } => behind.get(),
                    PushKind::FastForward { .. } => 0,
                };
                let expected = planned
                    .confirmation()
                    .map(|confirm| confirm.expected.short().to_string())
                    .unwrap_or_default();
                format!(
                    "{repo}{number} {} → {}: replaces {expected} (dropping {behind} commit(s) there) with {}",
                    planned.branch,
                    planned.upstream,
                    planned.local.short(),
                )
            })
            .collect();
        let all = plan.pushes(RewriteDecision::RewriteListed);
        let fast_forwards = plan.pushes(RewriteDecision::FastForwardsOnly);
        let count = rewrites.len();

        Some(
            v_flex()
                .gap_1()
                .p_2()
                .border_1()
                .border_color(theme.danger)
                .rounded_md()
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.text)
                        .child(format!(
                            "{count} branch(es) have diverged from their upstream. Pushing rewrites them with --force-with-lease; if anyone has pushed since they were fetched, the remote refuses and nothing is lost:"
                        )),
                )
                .children(lines.into_iter().map(|line| {
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.text_muted)
                        .child(line)
                }))
                .child(
                    gpui::div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("push-all-rewrite", format!("Push {all}, force-pushing {count} with lease"))
                                .style(ButtonStyle::Danger)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.store.update(cx, |store, cx| {
                                        store.answer_push_all(Some(RewriteDecision::RewriteListed), cx)
                                    });
                                })),
                        )
                        .child(
                            Button::new("push-all-ff", format!("Fast-forwards only ({fast_forwards})"))
                                .tooltip("Push the branches that are only ahead; leave the diverged ones alone")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.store.update(cx, |store, cx| {
                                        store.answer_push_all(Some(RewriteDecision::FastForwardsOnly), cx)
                                    });
                                })),
                        )
                        .child(
                            Button::new("push-all-cancel", "Cancel")
                                .tooltip("Push nothing")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.store
                                        .update(cx, |store, cx| store.answer_push_all(None, cx));
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
