//! The multi-repo pull request feed.
//!
//! Every repo and PR is flattened into one row stream rendered by a single
//! virtualized `list`; per-repo "containers" are reconstructed by having each
//! row draw the portion of the border that belongs to it. See
//! `docs/features/repo_feed.md` for why nesting lists does not work.

use std::rc::Rc;

use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ListAlignment, ListState, Subscription, Window, actions, div, list, prelude::*, px, rems,
};
use rostrum_core::{
    Chrome, Feed, FeedFilter, FeedRow, Mergeable, PrIx, RepoId, RepoIx, RepoState, ReviewDecision,
    Selection, flatten,
};
use rostrum_ui::{
    ActiveTheme, InputEvent, TextInput,
    components::{
        Button, ButtonStyle, Checkbox, Chip, DiffStat, Dot, Initial, h_flex, hex_color, v_flex,
    },
};

use crate::{
    nav::{self, Nav},
    sync::Store,
};

actions!(
    feed,
    [
        SelectNext,
        SelectPrevious,
        SelectFirst,
        SelectLast,
        OpenDetail,
        FocusFilter,
        DismissFilter,
        ToggleCollapse,
    ]
);

/// Key context of the scrolling row area.
///
/// Deliberately *not* on the feed's root: the filter box is a sibling of the
/// row area rather than a descendant, so it never sits in a dispatch path
/// where `j` would mean "next pull request" instead of the letter j.
const FEED_CONTEXT: &str = "Feed";

/// Key context wrapping the filter box. `TextInput` nests inside it, so
/// `escape` resolves here while typing without taking `escape` away from every
/// other `TextInput` in the app — notably the detail pane's composers.
const FILTER_CONTEXT: &str = "FilterBar";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("j", SelectNext, Some(FEED_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(FEED_CONTEXT)),
        KeyBinding::new("k", SelectPrevious, Some(FEED_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(FEED_CONTEXT)),
        KeyBinding::new("g g", SelectFirst, Some(FEED_CONTEXT)),
        KeyBinding::new("shift-g", SelectLast, Some(FEED_CONTEXT)),
        KeyBinding::new("enter", OpenDetail, Some(FEED_CONTEXT)),
        KeyBinding::new("/", FocusFilter, Some(FEED_CONTEXT)),
        KeyBinding::new("escape", DismissFilter, Some(FEED_CONTEXT)),
        KeyBinding::new("escape", DismissFilter, Some(FILTER_CONTEXT)),
        KeyBinding::new("c", ToggleCollapse, Some(FEED_CONTEXT)),
    ]);
}

/// Raised so the workspace, which owns the detail pane, can move focus into it.
#[derive(Clone, Copy, Debug)]
pub enum FeedEvent {
    FocusDetail,
}

/// Corner radius of a repo container, in pixels.
const ROW_RADIUS: f32 = 8.;

pub struct FeedView {
    store: Entity<Store>,
    filter: Entity<TextInput>,
    /// Input for adding a repository, shown while the manage panel is open.
    repo_input: Entity<TextInput>,
    /// Whether the repository management panel is open.
    managing_repos: bool,
    /// Why the last add attempt failed, shown under the input.
    repo_error: Option<String>,
    focus_handle: FocusHandle,
    feed: Rc<Feed>,
    list: ListState,
    _subscriptions: Vec<Subscription>,
}

impl FeedView {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let feed = Rc::new(build(&store, cx));
        let list = ListState::new(feed.len(), ListAlignment::Top, px(400.));
        let filter = cx.new(|cx| TextInput::new("Filter pull requests…", cx).lines(1, 1));
        let repo_input = cx.new(|cx| TextInput::new("owner/name or a GitHub URL", cx).lines(1, 1));

        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| this.store_changed(cx)),
            cx.subscribe(&filter, |this, filter, event, cx| {
                if matches!(event, InputEvent::Changed) {
                    let query = filter.read(cx).text().to_string();
                    this.set_query(query, cx);
                }
            }),
            // Enter in the repo box adds it, so the mouse is optional.
            cx.subscribe(&repo_input, |this, _, event, cx| {
                if matches!(event, InputEvent::Submit) {
                    this.add_repo(cx);
                }
            }),
        ];

        Self {
            store,
            filter,
            repo_input,
            managing_repos: false,
            repo_error: None,
            focus_handle: cx.focus_handle(),
            feed,
            list,
            _subscriptions: subscriptions,
        }
    }

    // --- repositories ------------------------------------------------------

    fn toggle_repo_panel(&mut self, cx: &mut Context<Self>) {
        self.managing_repos = !self.managing_repos;
        self.repo_error = None;
        cx.notify();
    }

    fn add_repo(&mut self, cx: &mut Context<Self>) {
        let input = self.repo_input.read(cx).text().trim().to_string();
        if input.is_empty() {
            return;
        }

        let result = self
            .store
            .update(cx, |store, cx| store.add_repo(&input, cx));

        match result {
            Ok(()) => {
                self.repo_error = None;
                self.repo_input.update(cx, |input, cx| input.clear(cx));
            }
            Err(message) => self.repo_error = Some(message),
        }
        cx.notify();
    }

    fn remove_repo(&mut self, id: RepoId, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.remove_repo(&id, cx));
        cx.notify();
    }

    fn toggle_hide_empty(&mut self, cx: &mut Context<Self>) {
        let hide = !self.store.read(cx).state.filter.hide_empty_repos;
        self.store
            .update(cx, |store, cx| store.set_hide_empty_repos(hide, cx));
    }

    /// The repository list, with a remove control each and an input to add one.
    ///
    /// This is the only place every configured repository is listed: hidden and
    /// collapsed repositories contribute no feed rows, so without it a repo
    /// with no open pull requests could never be removed.
    fn render_repo_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let repos: Vec<(RepoId, usize, bool)> = self
            .store
            .read(cx)
            .state
            .repos
            .iter()
            .map(|repo| (repo.id.clone(), repo.prs.len(), repo.load.is_failed()))
            .collect();

        v_flex()
            .gap_1p5()
            .p_2()
            .rounded_tl(px(6.))
            .rounded_tr(px(6.))
            .rounded_bl(px(6.))
            .rounded_br(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .children(
                repos
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (id, count, failed))| {
                        let name = id.to_string();
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(rems(0.76))
                                    .text_color(if failed { theme.danger } else { theme.text })
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.7))
                                    .text_color(theme.text_subtle)
                                    .child(count.to_string()),
                            )
                            .child(
                                div()
                                    .id(("remove-repo", ix))
                                    .px_1()
                                    .cursor_pointer()
                                    .text_size(rems(0.8))
                                    .text_color(theme.text_subtle)
                                    .hover(|el| el.text_color(theme.danger))
                                    .child("×")
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.remove_repo(id.clone(), cx)
                                    })),
                            )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.repo_input.clone()))
                    .child(
                        Button::new("add-repo", "Add")
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|this, _, _window, cx| this.add_repo(cx))),
                    ),
            )
            .when_some(self.repo_error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
    }

    // --- filtering ---------------------------------------------------------

    /// Mutate the live filter and let the store's notification rebuild the rows,
    /// so filter state has exactly one home.
    fn update_filter(&mut self, edit: impl FnOnce(&mut FeedFilter), cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            edit(&mut store.state.filter);
            cx.notify();
        });
    }

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.store.read(cx).state.filter.query == query {
            return;
        }
        self.update_filter(|filter| filter.query = query, cx);
    }

    fn toggle_drafts(&mut self, cx: &mut Context<Self>) {
        self.update_filter(|filter| filter.hide_drafts = !filter.hide_drafts, cx);
    }

    /// Reset the whole filter, including the text box that drives it.
    fn clear_filter(&mut self, cx: &mut Context<Self>) {
        self.filter.update(cx, |input, cx| input.clear(cx));
        self.update_filter(|filter| *filter = FeedFilter::default(), cx);
    }

    // --- keyboard navigation -----------------------------------------------

    /// Resolve the selection to a row *now*: every refresh can renumber rows,
    /// so a cached index would be stale by the time a key is pressed.
    fn current_row(&self, cx: &App) -> Option<usize> {
        let store = self.store.read(cx);
        nav::selected_row(
            &self.feed,
            &store.state.repos,
            store.state.selection.as_ref(),
        )
    }

    fn navigate(&mut self, nav: Nav, cx: &mut Context<Self>) {
        let current = self.current_row(cx);
        let Some(target) = nav::navigate(&self.feed, current, nav) else {
            return;
        };
        let Some(FeedRow::PrRow { repo, pr }) = self.feed.row(target) else {
            return;
        };
        self.select(repo, pr, cx);
        self.list.scroll_to_reveal_item(target);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::Next, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(Nav::Previous, cx);
    }

    fn select_first(&mut self, _: &SelectFirst, _window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::First, cx);
    }

    fn select_last(&mut self, _: &SelectLast, _window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Nav::Last, cx);
    }

    fn open_detail(&mut self, _: &OpenDetail, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FeedEvent::FocusDetail);
    }

    fn focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.filter.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// `escape`: clear an active filter first, and only give focus back to the
    /// rows once there is nothing left to clear.
    fn dismiss_filter(&mut self, _: &DismissFilter, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.read(cx).state.filter.is_active() {
            self.clear_filter(cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    fn toggle_collapse(
        &mut self,
        _: &ToggleCollapse,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self
            .store
            .read(cx)
            .state
            .selection
            .as_ref()
            .map(|selection| selection.repo.clone())
        else {
            return;
        };
        self.store
            .update(cx, |store, cx| store.toggle_collapsed(&id, cx));
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

    fn render_filter_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);
        let hide_drafts = store.state.filter.hide_drafts;
        let hide_empty = store.state.filter.hide_empty_repos;
        let hidden_repos = self.feed.hidden_repos();
        let repo_count = store.state.repos.len();
        let active = store.state.filter.is_active();
        let counts = visible_counts(&store.state.repos, &store.state.filter);

        v_flex()
            .flex_none()
            .px_3()
            .pb_2()
            .gap_1p5()
            .key_context(FILTER_CONTEXT)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.filter.clone()))
                    .child(
                        Button::new("hide-drafts", "drafts")
                            .style(if hide_drafts {
                                ButtonStyle::Primary
                            } else {
                                ButtonStyle::Subtle
                            })
                            .tooltip(if hide_drafts {
                                "Show draft pull requests"
                            } else {
                                "Hide draft pull requests"
                            })
                            .on_click(cx.listener(|this, _, _window, cx| this.toggle_drafts(cx))),
                    )
                    .child(
                        Button::new("manage-repos", format!("repos ({repo_count})"))
                            .style(if self.managing_repos {
                                ButtonStyle::Primary
                            } else {
                                ButtonStyle::Subtle
                            })
                            .tooltip("Add or remove repositories")
                            .on_click(
                                cx.listener(|this, _, _window, cx| this.toggle_repo_panel(cx)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("hide-empty", "hide empty repos", hide_empty).on_toggle(
                            cx.listener(|this, _, _window, cx| this.toggle_hide_empty(cx)),
                        ),
                    )
                    .when(hide_empty && hidden_repos > 0, |el| {
                        el.child(
                            div()
                                .text_size(rems(0.7))
                                .text_color(theme.text_subtle)
                                .child(format!("{hidden_repos} hidden")),
                        )
                    }),
            )
            .when(self.managing_repos, |el| {
                el.child(self.render_repo_panel(cx))
            })
            .when(active, |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(rems(0.72))
                                .text_color(theme.text_subtle)
                                .child(format!("{} of {} shown", counts.visible, counts.total)),
                        )
                        .child(
                            Button::new("clear-filter", "clear")
                                .tooltip("Clear the filter (escape)")
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.clear_filter(cx)),
                                ),
                        ),
                )
            })
    }
}

/// How much of the feed a filter is letting through.
///
/// Counted over every repository's pull requests rather than over feed rows:
/// a collapsed repo hides rows without the filter having rejected anything, and
/// reporting that as "filtered out" would be a lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisibleCounts {
    visible: usize,
    total: usize,
}

fn visible_counts(repos: &[RepoState], filter: &FeedFilter) -> VisibleCounts {
    let mut counts = VisibleCounts {
        visible: 0,
        total: 0,
    };
    for pr in repos.iter().flat_map(|repo| &repo.prs) {
        counts.total += 1;
        if filter.accepts(pr) {
            counts.visible += 1;
        }
    }
    counts
}

impl EventEmitter<FeedEvent> for FeedView {}

impl Focusable for FeedView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FeedView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            // Handlers sit on the root so they are in the dispatch path from
            // both the rows and the filter box; the key *contexts* below decide
            // which keystrokes ever reach them.
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::open_detail))
            .on_action(cx.listener(Self::focus_filter))
            .on_action(cx.listener(Self::dismiss_filter))
            .on_action(cx.listener(Self::toggle_collapse))
            .child(self.render_filter_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_3()
                    .key_context(FEED_CONTEXT)
                    .track_focus(&self.focus_handle)
                    .child(
                        list(
                            self.list.clone(),
                            cx.processor(|this, ix: usize, _window, cx| this.render_row(ix, cx)),
                        )
                        .size_full(),
                    ),
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

    fn pr(number: u32, draft: bool) -> rostrum_core::PullRequest {
        rostrum_core::PullRequest {
            number: rostrum_core::PrNumber(number),
            title: format!("PR {number}"),
            url: String::new(),
            is_draft: draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            head_ref: "feature".into(),
            head_sha: "deadbeef".into(),
            base_ref: "main".into(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            mergeable: Mergeable::Unknown,
            review_decision: None,
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
        }
    }

    fn repo(name: &str, prs: Vec<rostrum_core::PullRequest>, collapsed: bool) -> RepoState {
        RepoState {
            id: name.parse().expect("valid repo id"),
            prs,
            load: rostrum_core::LoadState::Loaded { at: Utc::now() },
            collapsed,
        }
    }

    #[test]
    fn counts_report_what_the_filter_lets_through() {
        let repos = vec![
            repo("a/b", vec![pr(1, false), pr(2, true)], false),
            repo("c/d", vec![pr(3, false)], false),
        ];

        let counts = visible_counts(&repos, &FeedFilter::default());
        assert_eq!(
            counts,
            VisibleCounts {
                visible: 3,
                total: 3
            }
        );

        let counts = visible_counts(
            &repos,
            &FeedFilter {
                query: String::new(),
                hide_drafts: true,
                hide_empty_repos: false,
            },
        );
        assert_eq!(
            counts,
            VisibleCounts {
                visible: 2,
                total: 3
            }
        );
    }

    /// Collapsing a repo hides rows without the filter rejecting anything, so
    /// the count must not shrink.
    #[test]
    fn counts_ignore_collapsed_repos() {
        let repos = vec![repo("a/b", vec![pr(1, false), pr(2, false)], true)];
        assert_eq!(
            visible_counts(&repos, &FeedFilter::default()),
            VisibleCounts {
                visible: 2,
                total: 2
            }
        );
    }
}
