//! One repository's own view.
//!
//! The left pane becomes this repository's pull requests (top) and issues
//! (bottom), each its own virtualized list, under a header with the
//! repository's name, stars, a link to GitHub and the trunk editor. With
//! nothing selected the right pane shows the branch tree
//! ([`branches::BranchesPane`]); selecting a row opens the same detail the
//! feed opens — a `PrDetail` or an `IssuePane`. Both lists follow the feed's
//! item sort ([`order::ListOrder`]) and draw their rows with the feed's own
//! row bodies. See `docs/features/repo_view.md`.

pub mod branches;
mod issues;
pub mod model;
mod nav;
mod order;
mod trunk_editor;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ListAlignment, ListState, Subscription, Window, actions, div, list, prelude::*, px, rems,
};
use rostrum_core::{IssueNumber, LoadState, PrNumber, RepoId, Selection};
use rostrum_ui::{
    ActiveTheme, InputEvent, PopoverAnchor, TextInput,
    components::{Button, ButtonStyle, Chip, h_flex, v_flex},
};

use crate::{
    feed::{STACK_INDENT, issue_row_content, pr_row_content, stack_glyph},
    nav::Nav,
    sync::Store,
};

use self::{
    model::RepoBranches,
    nav::{Position, Section, step},
    order::ListOrder,
};

actions!(
    repo_view,
    [
        SelectNext,
        SelectPrevious,
        SelectFirst,
        SelectLast,
        OpenDetail,
        Back,
        ShowBranches,
    ]
);

/// Key context of the repository view's lists. Like the feed's, it sits on
/// the scrolling area only, so the trunk editor's input never sees `j`.
const REPO_CONTEXT: &str = "RepoView";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("j", SelectNext, Some(REPO_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(REPO_CONTEXT)),
        KeyBinding::new("k", SelectPrevious, Some(REPO_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(REPO_CONTEXT)),
        KeyBinding::new("g g", SelectFirst, Some(REPO_CONTEXT)),
        KeyBinding::new("shift-g", SelectLast, Some(REPO_CONTEXT)),
        KeyBinding::new("enter", OpenDetail, Some(REPO_CONTEXT)),
        KeyBinding::new("escape", Back, Some(REPO_CONTEXT)),
        KeyBinding::new("backspace", Back, Some(REPO_CONTEXT)),
        KeyBinding::new("b", ShowBranches, Some(REPO_CONTEXT)),
    ]);
}

/// Raised for the workspace, which owns navigation and the detail pane.
#[derive(Clone, Copy, Debug)]
pub enum RepoViewEvent {
    /// Return to the multi-repository feed.
    Back,
    FocusDetail,
    /// Put the branch tree in the right pane. The selection is already
    /// cleared; this also tells the workspace to close a new-issue form,
    /// which is not a selection.
    ShowBranches,
    /// Open the new-issue form for this view's repository.
    NewIssue,
}

pub struct RepoView {
    store: Entity<Store>,
    pub repo: RepoId,
    branches: Entity<RepoBranches>,
    /// Which pull request and issue each list row shows, in the feed's item
    /// sort. Rebuilt on every store change.
    order: ListOrder,
    pulls: ListState,
    issues: ListState,
    focus_handle: FocusHandle,
    /// Whether the trunk editor popover is open.
    editing_trunks: bool,
    trunks_anchor: PopoverAnchor,
    trunk_input: Entity<TextInput>,
    trunk_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl RepoView {
    pub fn new(
        store: Entity<Store>,
        repo: RepoId,
        branches: Entity<RepoBranches>,
        cx: &mut Context<Self>,
    ) -> Self {
        let order = list_order(&store, &repo, cx);
        let (pulls, issues) = (order.pulls(), order.issues());
        let trunk_input = cx.new(|cx| TextInput::new("branch name", cx).lines(1, 1));
        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| this.store_changed(cx)),
            cx.observe(&branches, |_, _, cx| cx.notify()),
            cx.subscribe(&trunk_input, |this, _, event, cx| {
                if matches!(event, InputEvent::Submit) {
                    this.add_trunk(cx);
                }
            }),
        ];
        Self {
            store,
            repo,
            branches,
            order,
            pulls: ListState::new(pulls, ListAlignment::Top, px(400.)),
            issues: ListState::new(issues, ListAlignment::Top, px(400.)),
            focus_handle: cx.focus_handle(),
            editing_trunks: false,
            trunks_anchor: PopoverAnchor::default(),
            trunk_input,
            trunk_error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Keep each list's item count equal to the repository's pull requests
    /// and issues, in sort order. Rows are re-read through `order` on every
    /// paint; a change in count resets a list, and a reorder of the same
    /// count re-measures it in place so the scroll position holds.
    fn store_changed(&mut self, cx: &mut Context<Self>) {
        let order = list_order(&self.store, &self.repo, cx);
        if order == self.order {
            cx.notify();
            return;
        }
        let (pulls, issues) = (order.pulls(), order.issues());
        if pulls != self.pulls.item_count() {
            self.pulls.reset(pulls);
        } else {
            self.pulls.splice(0..pulls, pulls);
        }
        if issues != self.issues.item_count() {
            self.issues.reset(issues);
        } else {
            self.issues.splice(0..issues, issues);
        }
        self.order = order;
        cx.notify();
    }

    // --- selection ------------------------------------------------------------

    /// The displayed row of the current selection, if it is in this view.
    fn current_position(&self, cx: &App) -> Option<Position> {
        let store = self.store.read(cx);
        let selection = store.state.selection.as_ref()?;
        let repo = store.state.repo(&self.repo)?;
        self.order.position_of(repo, selection)
    }

    fn navigate(&mut self, nav: Nav, cx: &mut Context<Self>) {
        let pulls = self.order.pulls();
        let issues = self.order.issues();
        let Some(target) = step(pulls, issues, self.current_position(cx), nav) else {
            return;
        };
        match target.section {
            Section::Pulls => {
                let number = self
                    .store
                    .read(cx)
                    .state
                    .repo(&self.repo)
                    .zip(self.order.pull_at(target.index))
                    .and_then(|(repo, ix)| repo.prs.get(ix.0))
                    .map(|pr| pr.number);
                if let Some(number) = number {
                    self.select_pull(number, cx);
                    self.pulls.scroll_to_reveal_item(target.index);
                }
            }
            Section::Issues => {
                let number = self
                    .store
                    .read(cx)
                    .state
                    .repo(&self.repo)
                    .zip(self.order.issue_at(target.index))
                    .and_then(|(repo, ix)| repo.issues.get(ix.0))
                    .map(|issue| issue.number);
                if let Some(number) = number {
                    self.select_issue(number, cx);
                    self.issues.scroll_to_reveal_item(target.index);
                }
            }
        }
    }

    fn select_pull(&mut self, number: PrNumber, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.store.update(cx, |store, cx| {
            store.state.selection = Some(Selection::PullRequest { repo, number });
            cx.notify();
        });
    }

    fn select_issue(&mut self, number: IssueNumber, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.store.update(cx, |store, cx| {
            store.state.selection = Some(Selection::Issue { repo, number });
            cx.notify();
        });
    }

    /// Clear the selection, which puts the branch tree back in the right pane.
    fn show_branches(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            if store.state.selection.take().is_some() {
                cx.notify();
            }
        });
        cx.emit(RepoViewEvent::ShowBranches);
    }

    fn on_select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::Next, cx);
    }

    fn on_select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::Previous, cx);
    }

    fn on_select_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::First, cx);
    }

    fn on_select_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::Last, cx);
    }

    fn on_open_detail(&mut self, _: &OpenDetail, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(RepoViewEvent::FocusDetail);
    }

    /// `escape`: close the trunk editor if it is open, else leave the view.
    fn on_back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing_trunks {
            self.close_trunk_editor(cx);
        } else {
            cx.emit(RepoViewEvent::Back);
        }
    }

    fn on_show_branches(&mut self, _: &ShowBranches, _: &mut Window, cx: &mut Context<Self>) {
        self.show_branches(cx);
    }

    // --- rendering --------------------------------------------------------------

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let meta = self
            .branches
            .read(cx)
            .snapshot()
            .map(|snapshot| snapshot.meta.clone());
        let url = meta
            .as_ref()
            .map(|meta| meta.url.clone())
            .unwrap_or_else(|| format!("https://github.com/{}", self.repo));
        let stars = meta.as_ref().map(|meta| meta.stars);
        let has_selection = self
            .store
            .read(cx)
            .state
            .selection
            .as_ref()
            .is_some_and(|selection| selection.repo() == &self.repo);

        v_flex()
            .flex_none()
            .px_3()
            .pb_2()
            .gap_1p5()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("repo-back", "‹ Feed")
                            .tooltip("Back to every repository (escape)")
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(RepoViewEvent::Back)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.9))
                            .text_color(theme.text)
                            .child(self.repo.to_string()),
                    )
                    .when_some(stars, |el, stars| {
                        el.child(
                            div()
                                .text_size(rems(0.75))
                                .text_color(theme.text_muted)
                                .child(format!("★ {}", format_stars(stars))),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("repo-open-github", "Open on GitHub")
                            .tooltip(url.clone())
                            .on_click(move |_, _window, cx| cx.open_url(&url)),
                    )
                    .child(
                        Button::new("repo-branches", "Branches")
                            .style(if has_selection {
                                ButtonStyle::Subtle
                            } else {
                                ButtonStyle::Primary
                            })
                            .tooltip("Show the branch tree (b)")
                            .on_click(cx.listener(|this, _, _window, cx| this.show_branches(cx))),
                    )
                    .child(self.trunks_button(cx)),
            )
    }

    fn render_section_header(
        &self,
        label: &str,
        count: Option<usize>,
        action: Option<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .flex_none()
            .px_3()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(rems(0.78))
                    .text_color(theme.text)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .text_size(rems(0.72))
                    .text_color(theme.text_subtle)
                    .child(count.map_or_else(|| "—".to_string(), |count| count.to_string())),
            )
            .when_some(action, |el, action| el.child(div().flex_1()).child(action))
            .into_any_element()
    }

    /// The `ix`th displayed pull request.
    fn render_pull(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        let Some(repo) = store.state.repo(&self.repo) else {
            return div().into_any_element();
        };
        let Some(pull) = self.order.pull_at(ix).and_then(|pr| repo.prs.get(pr.0)) else {
            return div().into_any_element();
        };
        let selected = store.state.selection.as_ref().is_some_and(|selection| {
            matches!(selection, Selection::PullRequest { repo, number }
                    if *repo == self.repo && *number == pull.number)
        });
        let sync = store.sync_result(&self.repo, pull.number);
        let theme = cx.theme().clone();
        let content = pr_row_content(pull, sync, ix, &theme);
        let number = pull.number;
        let glyph = self.order.slot_at(ix).map(|slot| stack_glyph(slot.place));
        // The stack's header rides on its bottom member's row, so the list
        // keeps one row per pull request. Its actions stay in the feed.
        let header = self.order.header_at(ix).map(|placed| {
            let group = &placed.group;
            let count = group.stack.members.len();
            let title = match group.stack.number {
                Some(number) => format!("Stack {number} · {count} PRs"),
                None => format!("Stackable chain · {count} PRs"),
            };
            let rollup = group.rollup(&repo.prs);
            h_flex()
                .gap_2()
                .pb_1()
                .child(
                    div()
                        .text_color(theme.accent)
                        .text_size(rems(0.78))
                        .child("⛓"),
                )
                .child(
                    div()
                        .text_color(theme.text)
                        .text_size(rems(0.74))
                        .child(title),
                )
                .child(
                    div()
                        .text_color(theme.text_subtle)
                        .text_size(rems(0.7))
                        .child(format!("onto {}", group.stack.trunk)),
                )
                .when_some(rollup, |el, rollup| {
                    let color = if rollup.all_mergeable() {
                        theme.success
                    } else {
                        theme.merge_color(rollup.worst)
                    };
                    el.child(Chip::new(rollup.label()).color(color))
                })
        });

        div()
            .id(("repo-pr", ix))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .when(selected, |el| el.bg(theme.surface_selected))
            .hover(|el| el.bg(theme.surface_hover))
            .cursor_pointer()
            .child(
                v_flex()
                    .when_some(header, |el, header| el.child(header))
                    .child(
                        h_flex()
                            .items_start()
                            .gap_2()
                            .when(glyph.is_some(), |el| el.pl(px(STACK_INDENT - 12.)))
                            .when_some(glyph, |el, glyph| {
                                el.child(
                                    div()
                                        .w(px(10.))
                                        .text_color(theme.accent)
                                        .text_size(rems(0.72))
                                        .child(glyph),
                                )
                            })
                            .child(content.flex_1().min_w_0()),
                    ),
            )
            .on_click(cx.listener(move |this, _, _window, cx| this.select_pull(number, cx)))
            .into_any_element()
    }

    /// The `ix`th displayed issue.
    fn render_issue(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        let Some(issue) = store
            .state
            .repo(&self.repo)
            .zip(self.order.issue_at(ix))
            .and_then(|(repo, issue)| repo.issues.get(issue.0))
        else {
            return div().into_any_element();
        };
        let selected = store.state.selection.as_ref().is_some_and(|selection| {
            matches!(selection, Selection::Issue { repo, number }
                if *repo == self.repo && *number == issue.number)
        });
        let theme = cx.theme().clone();
        let content = issue_row_content(issue, &theme);
        let number = issue.number;

        div()
            .id(("repo-issue", ix))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .when(selected, |el| el.bg(theme.surface_selected))
            .hover(|el| el.bg(theme.surface_hover))
            .cursor_pointer()
            .child(content)
            .on_click(cx.listener(move |this, _, _window, cx| this.select_issue(number, cx)))
            .into_any_element()
    }
}

/// The repository's pull requests and issues in the feed's item sort.
fn list_order(store: &Entity<Store>, repo: &RepoId, cx: &App) -> ListOrder {
    let state = &store.read(cx).state;
    ListOrder::new(state.repo(repo), state.filter.sort.items)
}

/// `1234` → `1.2k`, the way GitHub shows a star count.
fn format_stars(stars: u32) -> String {
    match stars {
        0..1_000 => stars.to_string(),
        1_000..1_000_000 => format!("{:.1}k", f64::from(stars) / 1_000.),
        _ => format!("{:.1}m", f64::from(stars) / 1_000_000.),
    }
}

impl EventEmitter<RepoViewEvent> for RepoView {}

impl Focusable for RepoView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RepoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let pulls = self.order.pulls();
        let issue_count = self.order.issues();
        let issues_load = self
            .store
            .read(cx)
            .state
            .repo(&self.repo)
            .map_or(LoadState::Idle, |repo| repo.issues_load.clone());
        let loading = self
            .store
            .read(cx)
            .state
            .repo(&self.repo)
            .is_some_and(|repo| repo.prs.is_empty() && !repo.load.is_failed())
            && pulls == 0;

        v_flex()
            .size_full()
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_select_previous))
            .on_action(cx.listener(Self::on_select_first))
            .on_action(cx.listener(Self::on_select_last))
            .on_action(cx.listener(Self::on_open_detail))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_show_branches))
            .child(self.render_header(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .key_context(REPO_CONTEXT)
                    .track_focus(&self.focus_handle)
                    .border_t_1()
                    .border_color(theme.border)
                    // Fixed halves: each list scrolls on its own, and neither
                    // can push the other off screen.
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .child(self.render_section_header(
                                "Pull requests",
                                Some(pulls),
                                None,
                                cx,
                            ))
                            .child(if pulls == 0 {
                                div()
                                    .p_3()
                                    .text_size(rems(0.78))
                                    .text_color(theme.text_subtle)
                                    .child(if loading {
                                        "Loading…"
                                    } else {
                                        "No open pull requests"
                                    })
                                    .into_any_element()
                            } else {
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(
                                        list(
                                            self.pulls.clone(),
                                            cx.processor(|this, ix: usize, _window, cx| {
                                                this.render_pull(ix, cx)
                                            }),
                                        )
                                        .size_full(),
                                    )
                                    .into_any_element()
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(
                                self.render_section_header(
                                    "Issues",
                                    issues::header_count(&issues_load, issue_count),
                                    Some(
                                        Button::new("repo-new-issue", "+ New issue")
                                            .tooltip("Open a new issue in this repository")
                                            .on_click(cx.listener(|_, _, _window, cx| {
                                                cx.emit(RepoViewEvent::NewIssue)
                                            }))
                                            .into_any_element(),
                                    ),
                                    cx,
                                ),
                            )
                            .child(if issue_count == 0 {
                                div()
                                    .p_3()
                                    .text_size(rems(0.78))
                                    .text_color(theme.text_subtle)
                                    .child(issues::empty_message(&issues_load))
                                    .into_any_element()
                            } else {
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(
                                        list(
                                            self.issues.clone(),
                                            cx.processor(|this, ix: usize, _window, cx| {
                                                this.render_issue(ix, cx)
                                            }),
                                        )
                                        .size_full(),
                                    )
                                    .into_any_element()
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_counts_abbreviate_like_github() {
        assert_eq!(format_stars(0), "0");
        assert_eq!(format_stars(999), "999");
        assert_eq!(format_stars(1_234), "1.2k");
        assert_eq!(format_stars(56_789), "56.8k");
        assert_eq!(format_stars(2_500_000), "2.5m");
    }
}
