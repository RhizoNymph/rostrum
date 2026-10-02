//! Arranging arbitrary pull requests into a stack.
//!
//! A selection mode in the feed: "Arrange PRs" turns pull request rows into
//! toggles, limited to one repository at a time, in the order clicked. The
//! panel then lets the user reorder them, choose the trunk, and — because the
//! branches will be rebased and force-pushed with a lease — confirm that
//! explicitly before anything runs.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, rems};
use rostrum_core::{PrNumber, RefName, RepoId, StackPlan, plan_stack};
use rostrum_ui::{
    ActiveTheme,
    components::{Button, ButtonStyle, Checkbox, h_flex, v_flex},
};

use super::{
    FeedView,
    stacks::{StackPanel, chain_text, paragraph, store_of},
};

/// Pull requests picked for an arrangement, in the order they were clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Picking {
    /// `None` until the first pick fixes the repository.
    pub(super) repo: Option<RepoId>,
    pub(super) picked: Vec<PrNumber>,
}

impl Picking {
    fn new() -> Self {
        Self {
            repo: None,
            picked: Vec::new(),
        }
    }

    /// Toggle `number` in `repo`. Picking in another repository starts over
    /// there: a stack never spans repositories.
    pub(super) fn toggle(&mut self, repo: &RepoId, number: PrNumber) {
        if self.repo.as_ref() != Some(repo) {
            self.repo = Some(repo.clone());
            self.picked.clear();
        }
        if let Some(at) = self.picked.iter().position(|n| *n == number) {
            self.picked.remove(at);
        } else {
            self.picked.push(number);
        }
    }

    /// Where `number` sits in the pick order (1-based), if picked in `repo`.
    pub(super) fn position(&self, repo: &RepoId, number: PrNumber) -> Option<usize> {
        if self.repo.as_ref() != Some(repo) {
            return None;
        }
        self.picked
            .iter()
            .position(|n| *n == number)
            .map(|at| at + 1)
    }
}

/// Move the member at `at` one place toward the bottom (`up`) or the top.
pub(super) fn shift(order: &mut [PrNumber], at: usize, up: bool) {
    match up {
        true if at > 0 && at < order.len() => order.swap(at, at - 1),
        false if at + 1 < order.len() => order.swap(at, at + 1),
        _ => {}
    }
}

impl FeedView {
    pub(super) fn is_picking(&self) -> bool {
        self.stack_ui.picking.is_some()
    }

    pub(super) fn start_picking(&mut self, cx: &mut Context<Self>) {
        self.stack_ui.picking = Some(Picking::new());
        self.stack_ui.panel = None;
        cx.notify();
    }

    pub(super) fn stop_picking(&mut self, cx: &mut Context<Self>) {
        self.stack_ui.picking = None;
        if matches!(self.stack_ui.panel, Some(StackPanel::Arrange { .. })) {
            self.stack_ui.panel = None;
        }
        cx.notify();
    }

    /// A row click while picking.
    pub(super) fn toggle_pick(&mut self, repo: RepoId, number: PrNumber, cx: &mut Context<Self>) {
        if let Some(picking) = &mut self.stack_ui.picking {
            picking.toggle(&repo, number);
            cx.notify();
        }
    }

    /// The pick badge for a row: `Some(None)` unpicked, `Some(Some(n))` picked
    /// n-th, `None` when not picking.
    pub(super) fn pick_badge(&self, repo: &RepoId, number: PrNumber) -> Option<Option<usize>> {
        self.stack_ui
            .picking
            .as_ref()
            .map(|picking| picking.position(repo, number))
    }

    fn open_arrange(&mut self, cx: &mut Context<Self>) {
        let Some(Picking {
            repo: Some(repo),
            picked,
        }) = self.stack_ui.picking.clone()
        else {
            return;
        };
        // Suggest the base of the first pick that is not itself a picked head:
        // the branch the arrangement most plausibly sits on.
        let trunk = store_of(self, cx).state.repo(&repo).and_then(|state| {
            let heads: Vec<&str> = picked
                .iter()
                .filter_map(|n| state.prs.iter().find(|pr| pr.number == *n))
                .map(|pr| pr.head_ref.as_str())
                .collect();
            picked
                .iter()
                .filter_map(|n| state.prs.iter().find(|pr| pr.number == *n))
                .map(|pr| pr.base_ref.clone())
                .find(|base| !heads.contains(&base.as_str()))
        });
        let trunk = trunk.unwrap_or_else(|| "main".into());
        self.stack_ui
            .trunk_input
            .update(cx, |input, cx| input.set_text(trunk, cx));
        self.open_panel(
            StackPanel::Arrange {
                repo,
                order: picked,
                confirmed: false,
            },
            cx,
        );
    }

    /// The plan the open Arrange panel describes, and whether the user has
    /// confirmed the rewrite it needs.
    pub(super) fn arrangement(&self, cx: &gpui::App) -> Result<(StackPlan, bool), String> {
        let Some(StackPanel::Arrange {
            repo,
            order,
            confirmed,
        }) = &self.stack_ui.panel
        else {
            return Err("no arrangement is open".into());
        };
        let trunk = self.stack_ui.trunk_input.read(cx).text().trim().to_string();
        let trunk = RefName::new(trunk).map_err(|err| err.to_string())?;
        let state = store_of(self, cx)
            .state
            .repo(repo)
            .ok_or_else(|| format!("{repo} is no longer watched"))?;
        let plan = plan_stack(state, order, trunk).map_err(|err| err.to_string())?;
        if plan.needs_rewrite() && !confirmed {
            return Err("confirm the rewrite first".into());
        }
        Ok((plan, *confirmed))
    }

    /// The toolbar shown while picking.
    pub(super) fn render_picking_bar(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let picking = self.stack_ui.picking.clone()?;
        let theme = cx.theme().clone();
        let count = picking.picked.len();
        let label = match &picking.repo {
            Some(repo) if count > 0 => format!(
                "Arrange: {count} picked in {repo} — {}",
                chain_text(&picking.picked)
            ),
            _ => "Arrange: click pull requests of one repository, bottom of the stack first".into(),
        };
        Some(
            h_flex()
                .gap_2()
                .p_1p5()
                .rounded_md()
                .bg(theme.surface_raised)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(rems(0.72))
                        .text_color(theme.text)
                        .child(label),
                )
                .child(
                    Button::new("arrange-open", "Arrange…")
                        .style(ButtonStyle::Primary)
                        .disabled(count < 2)
                        .tooltip("Order them, choose the trunk, and confirm")
                        .on_click(cx.listener(|this, _, _window, cx| this.open_arrange(cx))),
                )
                .child(
                    Button::new("arrange-cancel", "Cancel")
                        .on_click(cx.listener(|this, _, _window, cx| this.stop_picking(cx))),
                ),
        )
    }

    /// The Arrange panel's body, and whether its confirm button may be
    /// pressed.
    pub(super) fn render_arrange_body(&mut self, cx: &mut Context<Self>) -> (AnyElement, bool) {
        let theme = cx.theme().clone();
        let Some(StackPanel::Arrange {
            repo,
            order,
            confirmed,
        }) = self.stack_ui.panel.clone()
        else {
            return (div().into_any_element(), false);
        };

        let titles: Vec<(PrNumber, String, String)> = {
            let state = store_of(self, cx).state.repo(&repo);
            order
                .iter()
                .map(|n| {
                    let pr = state.and_then(|s| s.prs.iter().find(|pr| pr.number == *n));
                    (
                        *n,
                        pr.map(|pr| pr.title.clone()).unwrap_or_default(),
                        pr.map(|pr| pr.head_ref.clone()).unwrap_or_default(),
                    )
                })
                .collect()
        };

        // Validate with the box treated as ticked, so the plan's own errors
        // show before the user is asked to confirm anything.
        let trunk = self.stack_ui.trunk_input.read(cx).text().trim().to_string();
        let plan = RefName::new(trunk)
            .map_err(|err| err.to_string())
            .and_then(|trunk| {
                store_of(self, cx)
                    .state
                    .repo(&repo)
                    .ok_or_else(|| format!("{repo} is no longer watched"))
                    .and_then(|state| plan_stack(state, &order, trunk).map_err(|e| e.to_string()))
            });
        let clone = store_of(self, cx)
            .local_path(&repo)
            .map(|p| p.display().to_string());

        let (summary, needs_rewrite, valid) = match (&plan, &clone) {
            (_, None) => (
                format!("{repo} has no local clone configured; arranging runs in one."),
                false,
                false,
            ),
            (Err(err), _) => (err.clone(), false, false),
            (Ok(plan), Some(clone)) if plan.needs_rewrite() => {
                let affected: Vec<PrNumber> = plan
                    .members()
                    .iter()
                    .enumerate()
                    .skip_while(|(ix, m)| &m.base == plan.parent_of(*ix))
                    .map(|(_, m)| m.number)
                    .collect();
                (
                    format!(
                        "This rewrites history. In {clone}, rostrum rebases {} onto the branch below each (in scratch worktrees; your checkouts are not touched), then force-pushes each rewritten branch with --force-with-lease against what was just fetched, then links the stack on GitHub (retargeting the bases) and tracks it locally. A conflict stops before anything is pushed{}.",
                        chain_text(&affected),
                        if store_of(self, cx).conflict_handler().is_some() {
                            " and is handed to your conflict handler"
                        } else {
                            " and is aborted"
                        }
                    ),
                    true,
                    true,
                )
            }
            (Ok(_), Some(_)) => (
                "Their bases already chain in this order: nothing is rebased or pushed; the stack is linked on GitHub and tracked locally.".into(),
                false,
                true,
            ),
        };
        let ready = valid && (!needs_rewrite || confirmed);
        let len = titles.len();

        let body = v_flex()
            .gap_1p5()
            .child(paragraph(
                format!("Arrange {len} pull requests of {repo} into a stack, bottom first:"),
                &theme,
            ))
            .children(titles.into_iter().enumerate().map(|(at, (n, title, head))| {
                h_flex()
                    .gap_1()
                    .pl_2()
                    .child(
                        div()
                            .w(gpui::px(18.))
                            .text_size(rems(0.7))
                            .text_color(theme.text_subtle)
                            .child(format!("{}.", at + 1)),
                    )
                    .child(div().text_size(rems(0.72)).text_color(theme.text_subtle).child(n.to_string()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.74))
                            .text_color(theme.text)
                            .child(format!("{title}  ({head})")),
                    )
                    .child(
                        Button::new(SharedString::from(format!("arrange-up-{at}")), "↑")
                            .disabled(at == 0)
                            .tooltip("Move toward the bottom of the stack")
                            .on_click(cx.listener(move |this, _, _window, cx| this.reorder(at, true, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("arrange-down-{at}")), "↓")
                            .disabled(at + 1 == len)
                            .tooltip("Move toward the top of the stack")
                            .on_click(cx.listener(move |this, _, _window, cx| this.reorder(at, false, cx))),
                    )
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_size(rems(0.72)).text_color(theme.text_subtle).child("Trunk"))
                    .child(div().flex_1().min_w_0().child(self.stack_ui.trunk_input.clone())),
            )
            .child(div().text_size(rems(0.72)).text_color(if valid { theme.text } else { theme.danger }).child(summary))
            .when(needs_rewrite, |el| {
                el.child(
                    Checkbox::new(
                        "arrange-confirm",
                        "I understand: these branches will be rebased and force-pushed (with lease)",
                        confirmed,
                    )
                    .on_toggle(cx.listener(|this, _, _window, cx| {
                        if let Some(StackPanel::Arrange { confirmed, .. }) = &mut this.stack_ui.panel {
                            *confirmed = !*confirmed;
                            cx.notify();
                        }
                    })),
                )
            })
            .into_any_element();
        (body, ready)
    }

    fn reorder(&mut self, at: usize, up: bool, cx: &mut Context<Self>) {
        if let Some(StackPanel::Arrange {
            order, confirmed, ..
        }) = &mut self.stack_ui.panel
        {
            shift(order, at, up);
            // A different order is a different rewrite; ask again.
            *confirmed = false;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> RepoId {
        RepoId::new("o", name)
    }

    #[test]
    fn picks_keep_click_order_and_toggle_off() {
        let mut picking = Picking::new();
        picking.toggle(&repo("r"), PrNumber(3));
        picking.toggle(&repo("r"), PrNumber(1));
        picking.toggle(&repo("r"), PrNumber(2));
        assert_eq!(picking.picked, vec![PrNumber(3), PrNumber(1), PrNumber(2)]);
        assert_eq!(picking.position(&repo("r"), PrNumber(1)), Some(2));
        picking.toggle(&repo("r"), PrNumber(1));
        assert_eq!(picking.picked, vec![PrNumber(3), PrNumber(2)]);
        assert_eq!(picking.position(&repo("r"), PrNumber(1)), None);
    }

    #[test]
    fn picking_in_another_repository_starts_over() {
        let mut picking = Picking::new();
        picking.toggle(&repo("a"), PrNumber(1));
        picking.toggle(&repo("a"), PrNumber(2));
        picking.toggle(&repo("b"), PrNumber(7));
        assert_eq!(picking.repo, Some(repo("b")));
        assert_eq!(picking.picked, vec![PrNumber(7)]);
        assert_eq!(picking.position(&repo("a"), PrNumber(1)), None);
    }

    #[test]
    fn shifting_moves_one_place_and_stops_at_the_ends() {
        let mut order = vec![PrNumber(1), PrNumber(2), PrNumber(3)];
        shift(&mut order, 2, true);
        assert_eq!(order, vec![PrNumber(1), PrNumber(3), PrNumber(2)]);
        shift(&mut order, 0, true);
        assert_eq!(order, vec![PrNumber(1), PrNumber(3), PrNumber(2)]);
        shift(&mut order, 2, false);
        assert_eq!(order, vec![PrNumber(1), PrNumber(3), PrNumber(2)]);
        shift(&mut order, 0, false);
        assert_eq!(order, vec![PrNumber(3), PrNumber(1), PrNumber(2)]);
        shift(&mut order, 9, true);
        assert_eq!(order, vec![PrNumber(3), PrNumber(1), PrNumber(2)]);
    }
}
