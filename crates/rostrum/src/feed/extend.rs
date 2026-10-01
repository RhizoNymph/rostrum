//! "Add to stack": putting open pull requests on top of a stack GitHub
//! already has.
//!
//! Reached two ways from a stack's header: **Add to stack**, which starts
//! picking pull requests of the stack's repository, and **Extend with …**,
//! offered when a line of pull requests already builds on the stack's top.
//! Either opens one panel that orders the additions and says what will
//! happen: a plain `gh stack link` when they already chain off the top, or —
//! naming every branch it will rewrite — a rebase onto the top with a leased
//! push, which must be confirmed.

use gpui::{AnyElement, App, Context, SharedString, div, prelude::*, rems};
use rostrum_core::{ExtendPlan, PrNumber, RepoId, StackNumber, plan_extend};
use rostrum_ui::{
    ActiveTheme,
    components::{Button, Checkbox, h_flex, v_flex},
};

use super::{
    FeedView,
    arrange::{Picking, shift},
    stacks::{StackPanel, chain_text, paragraph, store_of},
};

/// What the panel says about a valid extension, and whether it needs the
/// rewrite confirmed.
pub(super) fn describe(plan: &ExtendPlan, has_handler: bool) -> (String, bool) {
    let additions = chain_text(plan.addition_numbers().as_slice());
    if !plan.needs_rewrite() {
        return (
            format!(
                "These already build on the top of stack {} ({}, `{}`): nothing is rebased or pushed; `gh stack link {} {}` adds them.",
                plan.stack,
                plan.top.number,
                plan.top.head,
                plan.stack,
                plan.addition_numbers()
                    .as_slice()
                    .iter()
                    .map(|n| n.0.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            false,
        );
    }
    let rewritten = plan
        .rewrites()
        .iter()
        .map(|m| format!("`{}` ({})", m.head, m.number))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!(
            "This rewrites history for exactly these branches: {rewritten}. Rostrum rebases them onto the top of stack {} ({}, `{}`) in scratch worktrees, force-pushes each with --force-with-lease against what was just fetched, then links {additions} onto the stack. The stack's own pull requests are not touched. A conflict stops before anything is pushed{}.",
            plan.stack,
            plan.top.number,
            plan.top.head,
            if has_handler {
                " and is handed to your conflict handler"
            } else {
                " and is aborted"
            }
        ),
        true,
    )
}

impl FeedView {
    /// Start picking additions for `stack`.
    pub(super) fn start_extending(
        &mut self,
        repo: RepoId,
        stack: StackNumber,
        cx: &mut Context<Self>,
    ) {
        self.stack_ui.picking = Some(Picking::extending(repo, stack));
        self.stack_ui.panel = None;
        self.stack_ui.error = None;
        cx.notify();
    }

    /// Open the panel for adding `order` to `stack`.
    pub(super) fn open_extend(
        &mut self,
        repo: RepoId,
        stack: StackNumber,
        order: Vec<PrNumber>,
        cx: &mut Context<Self>,
    ) {
        self.open_panel(
            StackPanel::Extend {
                repo,
                stack,
                order,
                confirmed: false,
            },
            cx,
        );
    }

    /// The validated plan the open Extend panel describes.
    pub(super) fn extension(&self, cx: &App) -> Result<ExtendPlan, String> {
        let Some(StackPanel::Extend {
            repo,
            stack,
            order,
            confirmed,
        }) = &self.stack_ui.panel
        else {
            return Err("no extension is open".into());
        };
        let state = store_of(self, cx)
            .state
            .repo(repo)
            .ok_or_else(|| format!("{repo} is no longer watched"))?;
        let plan = plan_extend(state, *stack, order).map_err(|err| err.to_string())?;
        if plan.needs_rewrite() && !confirmed {
            return Err("confirm the rewrite first".into());
        }
        Ok(plan)
    }

    /// The Extend panel's body, and whether its confirm button may be
    /// pressed.
    pub(super) fn render_extend_body(&mut self, cx: &mut Context<Self>) -> (AnyElement, bool) {
        let theme = cx.theme().clone();
        let Some(StackPanel::Extend {
            repo,
            stack,
            order,
            confirmed,
        }) = self.stack_ui.panel.clone()
        else {
            return (div().into_any_element(), false);
        };

        let store = store_of(self, cx);
        let titles: Vec<(PrNumber, String)> = order
            .iter()
            .map(|n| {
                let title = store
                    .state
                    .repo(&repo)
                    .and_then(|s| s.prs.iter().find(|pr| pr.number == *n))
                    .map(|pr| format!("{}  ({})", pr.title, pr.head_ref))
                    .unwrap_or_default();
                (*n, title)
            })
            .collect();
        let has_clone = store.local_path(&repo).is_some();
        let has_handler = store.conflict_handler().is_some();
        let plan = store
            .state
            .repo(&repo)
            .ok_or_else(|| format!("{repo} is no longer watched"))
            .and_then(|state| plan_extend(state, stack, &order).map_err(|e| e.to_string()));

        let (summary, needs_rewrite, valid) = match (&plan, has_clone) {
            (_, false) => (
                format!("{repo} has no local clone configured; stacks are extended from one."),
                false,
                false,
            ),
            (Err(err), _) => (err.clone(), false, false),
            (Ok(plan), true) => {
                let (text, rewrite) = describe(plan, has_handler);
                (text, rewrite, true)
            }
        };
        let ready = valid && (!needs_rewrite || confirmed);
        let len = titles.len();

        let body = v_flex()
            .gap_1p5()
            .child(paragraph(
                format!("Add {len} pull request(s) to the top of stack {stack} in {repo}, lowest first:"),
                &theme,
            ))
            .children(titles.into_iter().enumerate().map(|(at, (n, title))| {
                h_flex()
                    .gap_1()
                    .pl_2()
                    .child(
                        div()
                            .text_size(rems(0.72))
                            .text_color(theme.text_subtle)
                            .child(format!("{}. {n}", at + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.74))
                            .text_color(theme.text)
                            .child(title),
                    )
                    .child(
                        Button::new(SharedString::from(format!("extend-up-{at}")), "↑")
                            .disabled(at == 0)
                            .tooltip("Move toward the stack's top member")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.reorder_extension(at, true, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("extend-down-{at}")), "↓")
                            .disabled(at + 1 == len)
                            .tooltip("Move further up the stack")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.reorder_extension(at, false, cx)
                            })),
                    )
            }))
            .child(
                div()
                    .text_size(rems(0.72))
                    .text_color(if valid { theme.text } else { theme.danger })
                    .child(summary),
            )
            .when(needs_rewrite, |el| {
                el.child(
                    Checkbox::new(
                        "extend-confirm",
                        "I understand: the branches named above will be rebased and force-pushed (with lease)",
                        confirmed,
                    )
                    .on_toggle(cx.listener(|this, _, _window, cx| {
                        if let Some(StackPanel::Extend { confirmed, .. }) = &mut this.stack_ui.panel {
                            *confirmed = !*confirmed;
                            cx.notify();
                        }
                    })),
                )
            })
            .into_any_element();
        (body, ready)
    }

    fn reorder_extension(&mut self, at: usize, up: bool, cx: &mut Context<Self>) {
        if let Some(StackPanel::Extend {
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
    use chrono::Utc;
    use rostrum_core::{
        LoadState, MergeStateStatus, Mergeable, NodeId, PullRequest, RefName, RepoState, Stack,
        StackMembers,
    };

    use super::*;

    fn link(number: u32, head: &str, base: &str) -> PullRequest {
        PullRequest {
            number: PrNumber(number),
            node_id: NodeId(format!("PR_{number}")),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: head.into(),
            head_sha: String::new(),
            base_ref: base.into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            merge_state: MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
            base_divergence: None,
            is_cross_repository: false,
            pushed_at: None,
        }
    }

    fn plan(extra: Vec<PullRequest>, order: &[u32]) -> ExtendPlan {
        let mut prs = vec![link(1, "a", "main"), link(2, "b", "a")];
        prs.extend(extra);
        let id = RepoId::new("o", "r");
        let state = RepoState {
            prs,
            stacks: vec![Stack {
                repo: id.clone(),
                number: StackNumber::new(7),
                trunk: RefName::new("main").expect("valid"),
                members: StackMembers::new(vec![PrNumber(1), PrNumber(2)]).expect("valid"),
            }],
            load: LoadState::Idle,
            ..RepoState::new(id)
        };
        let order: Vec<PrNumber> = order.iter().copied().map(PrNumber).collect();
        plan_extend(&state, StackNumber::new(7).expect("non-zero"), &order).expect("valid")
    }

    #[test]
    fn a_chained_extension_names_the_link_and_needs_no_confirmation() {
        let (text, confirm) = describe(&plan(vec![link(3, "c", "b")], &[3]), false);
        assert!(!confirm);
        assert!(text.contains("`gh stack link 7 3`"), "{text}");
        assert!(text.contains("nothing is rebased or pushed"), "{text}");
    }

    #[test]
    fn a_rewrite_names_exactly_the_branches_it_rewrites() {
        let (text, confirm) = describe(
            &plan(vec![link(3, "c", "b"), link(4, "d", "main")], &[3, 4]),
            true,
        );
        assert!(confirm);
        assert!(text.contains("`d` (#4)"), "{text}");
        assert!(!text.contains("`c` (#3)"), "{text}");
        assert!(text.contains("--force-with-lease"), "{text}");
        assert!(text.contains("conflict handler"), "{text}");
    }
}
