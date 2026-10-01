//! One repository's own view.
//!
//! The left pane becomes this repository's pull requests (top) and issues
//! (bottom), each its own virtualized list, under a header with the
//! repository's name, stars, a link to GitHub and the trunk editor. With
//! nothing selected the right pane shows the branch tree
//! ([`branches::BranchesPane`]); selecting a row opens the same detail the
//! feed opens. See `docs/features/repo_view.md`.

pub mod branches;
mod issues;
pub mod model;
mod nav;
mod trunk_editor;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ListAlignment, ListState, Subscription, Window, actions, div, list, prelude::*, px, rems,
};
use rostrum_core::{PrNumber, RepoId, Selection};
use rostrum_ui::{
    ActiveTheme, InputEvent, PopoverAnchor, TextInput,
    components::{Button, ButtonStyle, h_flex, v_flex},
};

use crate::{feed::pr_row_content, nav::Nav, sync::Store};

use self::{
    model::RepoBranches,
    nav::{Position, Section, step},
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
}

pub struct RepoView {
    store: Entity<Store>,
    pub repo: RepoId,
    branches: Entity<RepoBranches>,
    pulls: ListState,
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
        let count = pull_count(&store, &repo, cx);
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
            pulls: ListState::new(count, ListAlignment::Top, px(400.)),
            focus_handle: cx.focus_handle(),
            editing_trunks: false,
            trunks_anchor: PopoverAnchor::default(),
            trunk_input,
            trunk_error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Keep the list's item count equal to the repository's pull requests.
    /// Rows are addressed by index and re-read on every paint, so only a
    /// change in count needs the list told.
    fn store_changed(&mut self, cx: &mut Context<Self>) {
        let count = pull_count(&self.store, &self.repo, cx);
        if count != self.pulls.item_count() {
            self.pulls.reset(count);
        }
        cx.notify();
    }

    // --- selection ------------------------------------------------------------

    fn current_position(&self, cx: &App) -> Option<Position> {
        let store = self.store.read(cx);
        let selection = store.state.selection.as_ref()?;
        if selection.repo != self.repo {
            return None;
        }
        let repo = store.state.repo(&self.repo)?;
        let index = repo.prs.iter().position(|pr| pr.number == selection.pr)?;
        Some(Position::pull(index))
    }

    fn navigate(&mut self, nav: Nav, cx: &mut Context<Self>) {
        let pulls = self.pulls.item_count();
        let issues = issues::count();
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
                    .and_then(|repo| repo.prs.get(target.index))
                    .map(|pr| pr.number);
                if let Some(number) = number {
                    self.select_pull(number, cx);
                    self.pulls.scroll_to_reveal_item(target.index);
                }
            }
            // Nothing to land on until issues arrive.
            Section::Issues => {}
        }
    }

    fn select_pull(&mut self, number: PrNumber, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.store.update(cx, |store, cx| {
            store.state.selection = Some(Selection { repo, pr: number });
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
            .is_some_and(|selection| selection.repo == self.repo);

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

    fn render_section_header(&self, label: &str, count: Option<usize>, cx: &App) -> AnyElement {
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
            .into_any_element()
    }

    fn render_pull(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        let Some(repo) = store.state.repo(&self.repo) else {
            return div().into_any_element();
        };
        let Some(pull) = repo.prs.get(ix) else {
            return div().into_any_element();
        };
        let selected =
            store.state.selection.as_ref().is_some_and(|selection| {
                selection.repo == self.repo && selection.pr == pull.number
            });
        let sync = store.sync_result(&self.repo, pull.number);
        let theme = cx.theme().clone();
        let content = pr_row_content(pull, sync, ix, &theme);
        let number = pull.number;

        div()
            .id(("repo-pr", ix))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .when(selected, |el| el.bg(theme.surface_selected))
            .hover(|el| el.bg(theme.surface_hover))
            .cursor_pointer()
            .child(content)
            .on_click(cx.listener(move |this, _, _window, cx| this.select_pull(number, cx)))
            .into_any_element()
    }
}

fn pull_count(store: &Entity<Store>, repo: &RepoId, cx: &App) -> usize {
    store
        .read(cx)
        .state
        .repo(repo)
        .map_or(0, |repo| repo.prs.len())
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
        let pulls = self.pulls.item_count();
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
                            .child(self.render_section_header("Pull requests", Some(pulls), cx))
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
                            .child(self.render_section_header("Issues", None, cx))
                            .child(issues::placeholder(cx)),
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
