//! The Push button on a pull request's local row, and its confirmation.
//!
//! The button is decided before it is drawn, from the [`PushPlan`] loaded
//! beside the local state: a fast-forward pushes on click; a diverged branch
//! asks first, in a confirmation that names the branch, the remote, and the
//! exact remote oid that will be replaced; anything else is a disabled
//! button whose tooltip says why. The push itself is
//! [`rostrum_local::run_push`], shared with the feed's Push all.

use gpui::{AnyElement, Context, div, prelude::*, rems};
use gpui_tokio::Tokio;
use rostrum_git::BranchName;
use rostrum_local::{ConfirmedRewrite, PushKind, PushPlan, PushResult, PushTarget, run_push};
use rostrum_ui::components::{Button, ButtonStyle, Dot, h_flex};

use super::{Loadable, PrDetail};

impl PrDetail {
    /// The branch to push, as the store and the feed row describe it now.
    fn push_target(&self, cx: &Context<Self>) -> Option<PushTarget> {
        let pull = self.pull(cx)?;
        let clone = self.store.read(cx).local_path(&self.repo)?;
        let branch = BranchName::new(pull.head_ref.clone()).ok()?;
        Some(PushTarget {
            clone,
            branch,
            head_repo: pull.head_repository_in(&self.repo),
        })
    }

    pub(super) fn render_push_button(&self, busy: bool, cx: &Context<Self>) -> AnyElement {
        let (enabled, label, tooltip, style) = match &self.push_plan {
            Loadable::Idle => (
                false,
                "Push",
                "Not checked out in any worktree of the clone".to_string(),
                ButtonStyle::Subtle,
            ),
            Loadable::Loading => (
                false,
                "Push",
                "Checking what a push would do…".to_string(),
                ButtonStyle::Subtle,
            ),
            Loadable::Failed(reason) => (
                false,
                "Push",
                format!("Could not tell what a push would do: {reason}"),
                ButtonStyle::Subtle,
            ),
            Loadable::Loaded(PushPlan::Skip(skip)) => (
                false,
                "Push",
                capitalise(&skip.reason()),
                ButtonStyle::Subtle,
            ),
            Loadable::Loaded(PushPlan::Ready(planned)) => match &planned.kind {
                PushKind::FastForward { ahead } => (
                    true,
                    "Push",
                    format!("Push {ahead} commit(s) to {}", planned.upstream),
                    ButtonStyle::Subtle,
                ),
                PushKind::Rewrite { ahead, behind, .. } => (
                    true,
                    "Push (rewrite)…",
                    format!(
                        "The branch has diverged from {} ({ahead} ahead, {behind} behind): pushing rewrites it, with a lease. Asks first",
                        planned.upstream
                    ),
                    ButtonStyle::Danger,
                ),
            },
        };
        Button::new("local-push", label)
            .style(style)
            .disabled(busy || !enabled)
            .tooltip(tooltip)
            .on_click(Self::on_click(cx, |this, cx| this.press_push(cx)))
            .into_any_element()
    }

    /// A fast-forward pushes now; a rewrite opens the confirmation.
    fn press_push(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.push_plan.loaded() else {
            return;
        };
        match plan {
            PushPlan::Skip(_) => {}
            PushPlan::Ready(planned) => match planned.confirmation() {
                None => self.run_push_branch(None, cx),
                Some(rewrite) => {
                    self.push_confirm = Some(rewrite);
                    cx.notify();
                }
            },
        }
    }

    /// The confirmation for a rewrite, while one is open.
    pub(super) fn render_push_confirm(
        &self,
        busy: bool,
        theme: &rostrum_ui::Theme,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let confirm = self.push_confirm.as_ref()?;
        let planned = self.push_plan.loaded()?.ready()?;
        let PushKind::Rewrite { behind, .. } = &planned.kind else {
            return None;
        };
        let text = format!(
            "Force-push (with lease) {} to {}? This replaces {} — dropping {behind} commit(s) there that are not here — with {}. If anyone has pushed since it was fetched, the remote refuses and nothing is lost.",
            confirm.branch,
            planned.upstream,
            confirm.expected.short(),
            planned.local.short(),
        );
        let confirmed = confirm.clone();
        Some(
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(Dot::new(theme.danger))
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.text_muted)
                        .child(text),
                )
                .child(
                    Button::new("local-push-confirm", "Force-push with lease")
                        .style(ButtonStyle::Danger)
                        .disabled(busy)
                        .on_click(Self::on_click(cx, move |this, cx| {
                            this.push_confirm = None;
                            this.run_push_branch(Some(confirmed.clone()), cx)
                        })),
                )
                .child(
                    Button::new("local-push-cancel", "Cancel").on_click(Self::on_click(
                        cx,
                        |this, cx| {
                            this.push_confirm = None;
                            cx.notify();
                        },
                    )),
                )
                .into_any_element(),
        )
    }

    /// Push this pull request's branch, then re-read the clone (the ahead
    /// count is what changed) and ask GitHub for the pull request again.
    fn run_push_branch(&mut self, confirmation: Option<ConfirmedRewrite>, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(target) = self.push_target(cx) else {
            return;
        };
        self.busy = Some("Pushing");
        self.error = None;
        self.notice = None;
        cx.notify();

        let repo = self.repo.clone();
        let number = self.number;
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, async move {
                run_push(&target, confirmation.as_ref()).await
            })
            .await;

            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(result) => {
                        tracing::info!(repo = %repo, number = number.0, outcome = %result.detail(), "push finished");
                        match &result {
                            PushResult::Pushed { .. } => {
                                this.notice = Some(capitalise(&result.detail()));
                                let repo = repo.clone();
                                this.store
                                    .update(cx, |store, cx| store.refresh_repo(repo, cx));
                            }
                            PushResult::Skipped(_) => {
                                this.notice = Some(capitalise(&result.detail()));
                            }
                            _ => this.error = Some(capitalise(&result.detail())),
                        }
                    }
                    Err(err) => this.error = Some(err.to_string()),
                }
                this.load_local(cx);
                cx.notify();
            })
            .ok();
        }));
    }
}

fn capitalise(text: &str) -> String {
    super::capitalise(text)
}
