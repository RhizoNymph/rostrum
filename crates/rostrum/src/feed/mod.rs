//! The multi-repo pull request feed.
//!
//! Every repo and PR is flattened into one row stream rendered by a single
//! virtualized `list`; per-repo "containers" are reconstructed by having each
//! row draw the portion of the border that belongs to it. See
//! `docs/features/repo_feed.md` for why nesting lists does not work.

mod rows;

use std::rc::Rc;

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, ListAlignment,
    ListState, SharedString, Subscription, Window, actions, div, list, prelude::*, px, rems,
};
use rostrum_core::{
    AuthorEntry, Feed, FeedFilter, FeedRow, FeedTab, LoginKey, RepoId, RepoState, Selection,
    VisibleAuthors, authors::visible as visible_authors, flatten_tab, tab_counts,
};
use rostrum_ui::{
    ActiveTheme, InputEvent, Popover, PopoverAnchor, TextInput,
    components::{Button, ButtonStyle, Checkbox, Tab, h_flex, tab_bar, v_flex},
};

pub(crate) use rows::relative_time;

use crate::{
    nav::{self, Nav},
    sync::{Store, SyncKind},
};

mod sort_menu;

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
        NextTab,
        PreviousTab,
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
        // Brackets, not a modifier chord: they sit beside `j`/`k` on the
        // home row and read as "left"/"right", and like them they are inert
        // while typing in the filter box.
        KeyBinding::new("]", NextTab, Some(FEED_CONTEXT)),
        KeyBinding::new("[", PreviousTab, Some(FEED_CONTEXT)),
    ]);
}

/// Raised so the workspace, which owns the detail pane, can act on it.
#[derive(Clone, Debug)]
pub enum FeedEvent {
    FocusDetail,
    /// Open the new-issue form, with this repository chosen when the request
    /// came from its header, or the form's own default when it came from the
    /// tab bar.
    NewIssue {
        repo: Option<RepoId>,
    },
}

/// Corner radius of a repo container, in pixels.
const ROW_RADIUS: f32 = 8.;

/// Which header popover is open. At most one: opening any closes the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderPopover {
    Authors,
    Repos,
    Sort,
}

pub struct FeedView {
    store: Entity<Store>,
    filter: Entity<TextInput>,
    /// Input for adding a repository, shown while the repos popover is open.
    repo_input: Entity<TextInput>,
    /// The open header popover, if any. Deliberately *not* persisted.
    popover: Option<HeaderPopover>,
    authors_anchor: PopoverAnchor,
    repos_anchor: PopoverAnchor,
    sort_anchor: PopoverAnchor,
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
        let filter = cx.new(|cx| TextInput::new("Filter…", cx).lines(1, 1));
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
            popover: None,
            authors_anchor: PopoverAnchor::default(),
            repos_anchor: PopoverAnchor::default(),
            sort_anchor: PopoverAnchor::default(),
            repo_error: None,
            focus_handle: cx.focus_handle(),
            feed,
            list,
            _subscriptions: subscriptions,
        }
    }

    // --- repositories ------------------------------------------------------

    /// Open `which`, or close it if it is already open. Opening one header
    /// popover closes the other.
    fn toggle_popover(&mut self, which: HeaderPopover, cx: &mut Context<Self>) {
        self.popover = if self.popover == Some(which) {
            None
        } else {
            Some(which)
        };
        self.repo_error = None;
        cx.notify();
    }

    fn close_popover(&mut self, cx: &mut Context<Self>) {
        if self.popover.take().is_some() {
            self.repo_error = None;
            cx.notify();
        }
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

    /// The repos popover: the repository list, with a remove control each and
    /// an input to add one.
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
    /// Edit the parts of the filter that live and die with the session.
    ///
    /// Only the search query qualifies. Everything else is a preference and
    /// goes through a `Store` setter, which writes it to the config as well —
    /// see [`Store::set_hide_drafts`] and friends.
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
        let hide = !self.store.read(cx).state.filter.hide_drafts;
        self.store
            .update(cx, |store, cx| store.set_hide_drafts(hide, cx));
    }

    fn toggle_author(&mut self, login: LoginKey, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.toggle_author(login, cx));
    }

    fn toggle_include_involved(&mut self, cx: &mut Context<Self>) {
        let include = !self.store.read(cx).state.filter.include_involved;
        self.store
            .update(cx, |store, cx| store.set_include_involved(include, cx));
    }

    /// Show every author again without disturbing the rest of the filter.
    fn clear_authors(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.clear_authors(cx));
    }

    /// Reset the whole filter, including the text box that drives it.
    fn clear_filter(&mut self, cx: &mut Context<Self>) {
        self.filter.update(cx, |input, cx| input.clear(cx));
        self.store.update(cx, |store, cx| store.clear_filter(cx));
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
        let Some(row) = self.feed.row(target) else {
            return;
        };
        self.select(row, cx);
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

    fn next_tab(&mut self, _: &NextTab, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.store.read(cx).state.tab.next();
        self.set_tab(tab, cx);
    }

    fn previous_tab(&mut self, _: &PreviousTab, _window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.store.read(cx).state.tab.previous();
        self.set_tab(tab, cx);
    }

    /// Switch lists. The selection is left alone, so the detail pane keeps
    /// showing whatever was open; `j`/`k` then enter the new tab's list from
    /// its end, since the old selection has no row there.
    fn set_tab(&mut self, tab: FeedTab, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.set_tab(tab, cx));
        self.list.scroll_to(gpui::ListOffset::default());
    }

    fn open_detail(&mut self, _: &OpenDetail, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FeedEvent::FocusDetail);
    }

    fn focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.filter.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// `escape`: close an open header popover first, then clear an active
    /// filter, and only give focus back to the
    /// rows once there is nothing left to clear.
    fn dismiss_filter(&mut self, _: &DismissFilter, window: &mut Window, cx: &mut Context<Self>) {
        if self.popover.is_some() {
            self.close_popover(cx);
        } else if self.store.read(cx).state.filter.is_active() {
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
            .map(|selection| selection.repo().clone())
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

    /// Select the item a row shows, by identity. Rows other than items, and
    /// rows whose indices a refresh has since invalidated, select nothing.
    fn select(&mut self, row: FeedRow, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            let Some(selection) = selection_for(&store.state.repos, row) else {
                return;
            };
            store.state.selection = Some(selection);
            cx.notify();
        });
    }

    /// The authors popover: every author with open work, selected ones and the
    /// viewer first, as a toggleable list, plus the "include involved"
    /// widening and a way back to everyone.
    ///
    /// The list is not capped the way the old inline row was: it scrolls
    /// inside the popover instead, so nobody is ever hidden behind "+N more".
    /// Ordering still comes from [`rostrum_core::authors::visible`].
    fn render_author_popover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);
        let selected = store.state.filter.authors.clone();
        let include_involved = store.state.filter.include_involved;
        let entries = store.authors();
        let everyone = entries.len();
        let VisibleAuthors { shown, .. } = visible_authors(entries, &selected, everyone);

        v_flex()
            .gap_1()
            .when(shown.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.text_subtle)
                        .child("No authors yet — the feed has not loaded"),
                )
            })
            .children(shown.into_iter().map(|entry| {
                let checked = selected.contains(&entry.key);
                let login = entry.key.clone();
                Checkbox::new(
                    SharedString::from(format!("author-{}", entry.key)),
                    author_label(&entry),
                    checked,
                )
                .on_toggle(
                    cx.listener(move |this, _, _window, cx| this.toggle_author(login.clone(), cx)),
                )
            }))
            .child(div().h(px(1.)).my_1().bg(theme.border))
            .child(
                Checkbox::new(
                    "authors-include-involved",
                    "include involved in",
                    include_involved,
                )
                .on_toggle(cx.listener(|this, _, _window, cx| this.toggle_include_involved(cx))),
            )
            .child(
                div()
                    .text_size(rems(0.7))
                    .text_color(theme.text_subtle)
                    .child(if include_involved {
                        "opened by, assigned to, or awaiting review from"
                    } else {
                        "opened by"
                    }),
            )
            .when(!selected.is_empty(), |el| {
                el.child(
                    Button::new("authors-clear", "all authors")
                        .tooltip("Stop filtering by author")
                        .on_click(cx.listener(|this, _, _window, cx| this.clear_authors(cx))),
                )
            })
    }

    /// The authors button and its popover. The label carries the selection so
    /// an active author filter is visible with the popover closed.
    fn authors_button(&mut self, selected: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.popover == Some(HeaderPopover::Authors);
        let label = if selected == 0 {
            "authors".to_string()
        } else {
            format!("authors \u{00b7} {selected}")
        };
        let content = if open {
            Some(self.render_author_popover(cx).into_any_element())
        } else {
            None
        };
        Popover::new(
            "authors-popover",
            self.authors_anchor.clone(),
            Button::new("authors-button", label)
                .style(if open || selected > 0 {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Subtle
                })
                .tooltip("Filter by author")
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.toggle_popover(HeaderPopover::Authors, cx)
                })),
        )
        .open(content)
        .on_dismiss(cx.listener(|this, _, _window, cx| this.close_popover(cx)))
    }

    /// The repos button and its popover.
    fn repos_button(&mut self, repo_count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.popover == Some(HeaderPopover::Repos);
        let content = if open {
            Some(self.render_repo_panel(cx).into_any_element())
        } else {
            None
        };
        Popover::new(
            "repos-popover",
            self.repos_anchor.clone(),
            Button::new("manage-repos", format!("repos ({repo_count})"))
                .style(if open {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Subtle
                })
                .tooltip("Add or remove repositories")
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.toggle_popover(HeaderPopover::Repos, cx)
                })),
        )
        .open(content)
        .on_dismiss(cx.listener(|this, _, _window, cx| this.close_popover(cx)))
    }

    /// "Pull requests | Issues" across the top of the pane, each with the
    /// number of open items the filter lets through, and — on the Issues
    /// tab — the way to open a new one.
    fn render_tabs(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let active = store.state.tab;
        let counts = tab_counts(&store.state.repos, &store.state.filter);
        let tabs = FeedTab::ALL
            .iter()
            .map(|tab| Tab::new(tab.label()).badge(counts.get(*tab)))
            .collect();
        let entity = cx.entity();

        h_flex()
            .flex_none()
            .px_3()
            .pb_2()
            .items_end()
            .child(div().flex_1().child(tab_bar(
                tabs,
                active.index(),
                cx,
                move |ix, _window, cx| {
                    entity.update(cx, |this, cx| this.set_tab(FeedTab::from_index(ix), cx));
                },
            )))
            .when(active == FeedTab::Issues, |el| {
                el.child(
                    div().pl_2().pb_1().child(
                        Button::new("new-issue", "New issue")
                            .style(ButtonStyle::Primary)
                            .tooltip("Open an issue in one of the watched repositories")
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(FeedEvent::NewIssue { repo: None })
                            })),
                    ),
                )
            })
    }

    fn render_filter_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);
        let hide_drafts = store.state.filter.hide_drafts;
        let hide_empty = store.state.filter.hide_empty_repos;
        let hidden_repos = self.feed.hidden_repos();
        let repo_count = store.state.repos.len();
        let author_count = store.state.filter.authors.len();
        let active = store.state.filter.is_active();
        let tab = store.state.tab;
        let counts = visible_counts(&store.state.repos, &store.state.filter, tab);
        let has_clone = store.has_any_clone();
        let syncing = store.is_syncing();
        let autostash = store.autostash();
        let sync_status = store.sync().map(|sync| {
            if sync.is_finished() {
                format!("{}: {}", sync.kind.label(), sync.summary().describe())
            } else {
                format!("{}: {}/{}…", sync.kind.label(), sync.done, sync.total)
            }
        });

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
                    // Drafts are a pull request notion; on the Issues tab the
                    // toggle would do nothing, so it is not offered there.
                    .when(tab == FeedTab::PullRequests, |el| {
                        el.child(
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
                                .on_click(
                                    cx.listener(|this, _, _window, cx| this.toggle_drafts(cx)),
                                ),
                        )
                    })
                    .child(self.authors_button(author_count, cx))
                    .child(self.repos_button(repo_count, cx))
                    .child(self.sort_button(cx)),
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
            // Only for users with a clone to sync: three greyed-out buttons
            // would be a puzzle for everyone else.
            .when(has_clone, |el| {
                let sync_button = |id: &'static str, kind: SyncKind, cx: &mut Context<Self>| {
                    Button::new(id, kind.label())
                        .disabled(syncing)
                        .tooltip(if syncing {
                            "A sync is already running"
                        } else {
                            "Run on every checked-out pull request of every repository with a clone"
                        })
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.store.update(cx, |store, cx| store.sync_all(kind, cx));
                        }))
                };
                el.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(sync_button("sync-pull", SyncKind::Pull, cx))
                        .child(sync_button("sync-merge-base", SyncKind::MergeBase, cx))
                        .child(sync_button("sync-rebase-base", SyncKind::RebaseBase, cx))
                        .child(
                            Checkbox::new("sync-autostash", "Stash local changes", autostash)
                                .on_toggle(cx.listener(move |this, _, _window, cx| {
                                    this.store.update(cx, |store, cx| {
                                        store.set_autostash(!autostash, cx)
                                    });
                                })),
                        )
                        .when_some(sync_status, |el, status| {
                            el.child(
                                div()
                                    .text_size(rems(0.72))
                                    .text_color(theme.text_subtle)
                                    .child(status),
                            )
                        }),
                )
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

/// One author's row in the popover: who, and how much open work they have.
fn author_label(entry: &AuthorEntry) -> String {
    let who = if entry.is_viewer {
        format!("{} (you)", entry.user.login)
    } else {
        entry.user.login.clone()
    };
    match entry.open_prs {
        0 => who,
        n => format!("{who} \u{00b7} {n}"),
    }
}

/// How much of the active tab's list a filter is letting through.
///
/// Counted over every repository's items rather than over feed rows: a
/// collapsed repo hides rows without the filter having rejected anything, and
/// reporting that as "filtered out" would be a lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisibleCounts {
    visible: usize,
    total: usize,
}

fn visible_counts(repos: &[RepoState], filter: &FeedFilter, tab: FeedTab) -> VisibleCounts {
    let total = repos
        .iter()
        .map(|repo| match tab {
            FeedTab::PullRequests => repo.prs.len(),
            FeedTab::Issues => repo.issues.len(),
        })
        .sum();
    VisibleCounts {
        visible: tab_counts(repos, filter).get(tab),
        total,
    }
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
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .child(self.render_tabs(cx))
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
    flatten_tab(&store.state.repos, &store.state.filter, store.state.tab)
}

/// The identity a feed row stands for, resolved against the state it was
/// built from.
fn selection_for(repos: &[RepoState], row: FeedRow) -> Option<Selection> {
    match row {
        FeedRow::PrRow { repo, pr } => {
            let state = repos.get(repo.0)?;
            Some(Selection::PullRequest {
                repo: state.id.clone(),
                number: state.prs.get(pr.0)?.number,
            })
        }
        FeedRow::IssueRow { repo, issue } => {
            let state = repos.get(repo.0)?;
            Some(Selection::Issue {
                repo: state.id.clone(),
                number: state.issues.get(issue.0)?.number,
            })
        }
        FeedRow::RepoHeader { .. }
        | FeedRow::RepoEmpty { .. }
        | FeedRow::RepoError { .. }
        | FeedRow::RepoLoading { .. }
        | FeedRow::Spacer { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rostrum_core::{IssueIx, IssueNumber, PrIx, PrNumber, RepoIx};

    use super::*;

    fn issue(number: u32) -> rostrum_core::Issue {
        rostrum_core::Issue {
            number: IssueNumber(number),
            node_id: rostrum_core::NodeId(format!("I_{number}")),
            title: format!("Issue {number}"),
            url: String::new(),
            state: rostrum_core::IssueState::Open,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author: None,
            assignees: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            milestone: None,
        }
    }

    fn pr(number: u32, draft: bool) -> rostrum_core::PullRequest {
        rostrum_core::PullRequest {
            number: rostrum_core::PrNumber(number),
            node_id: rostrum_core::NodeId(format!("PR_{number}")),
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
            mergeable: rostrum_core::Mergeable::Unknown,
            merge_state: rostrum_core::MergeStateStatus::Unknown,
            review_decision: None,
            assignees: Vec::new(),
            review_requests: Vec::new(),
            labels: Vec::new(),
            comment_count: 0,
            checks: None,
            base_divergence: None,
            pushed_at: None,
        }
    }

    fn repo(name: &str, prs: Vec<rostrum_core::PullRequest>, collapsed: bool) -> RepoState {
        RepoState {
            prs,
            load: rostrum_core::LoadState::Loaded { at: Utc::now() },
            collapsed,
            ..RepoState::new(name.parse().expect("valid repo id"))
        }
    }

    #[test]
    fn issue_counts_report_what_the_filter_lets_through() {
        let mut first = repo("a/b", vec![pr(1, false)], false);
        first.issues = vec![issue(1), issue(2)];
        first.issues[1].title = "needle".into();
        let repos = vec![first];

        assert_eq!(
            visible_counts(&repos, &FeedFilter::default(), FeedTab::Issues),
            VisibleCounts {
                visible: 2,
                total: 2
            }
        );
        let narrowed = FeedFilter {
            query: "needle".into(),
            ..Default::default()
        };
        assert_eq!(
            visible_counts(&repos, &narrowed, FeedTab::Issues),
            VisibleCounts {
                visible: 1,
                total: 2
            }
        );
        // The pull request tab counts its own list.
        assert_eq!(
            visible_counts(&repos, &FeedFilter::default(), FeedTab::PullRequests),
            VisibleCounts {
                visible: 1,
                total: 1
            }
        );
    }

    /// A row resolves to the identity of what it shows, of the right kind,
    /// and chrome rows select nothing.
    #[test]
    fn rows_select_by_identity_and_kind() {
        let mut state = repo("a/b", vec![pr(4, false)], false);
        state.issues = vec![issue(9)];
        let repos = vec![state];
        let id: RepoId = "a/b".parse().expect("valid");

        assert_eq!(
            selection_for(
                &repos,
                FeedRow::PrRow {
                    repo: RepoIx(0),
                    pr: PrIx(0)
                }
            ),
            Some(Selection::PullRequest {
                repo: id.clone(),
                number: PrNumber(4)
            })
        );
        assert_eq!(
            selection_for(
                &repos,
                FeedRow::IssueRow {
                    repo: RepoIx(0),
                    issue: IssueIx(0)
                }
            ),
            Some(Selection::Issue {
                repo: id,
                number: IssueNumber(9)
            })
        );
        assert_eq!(
            selection_for(&repos, FeedRow::RepoHeader { repo: RepoIx(0) }),
            None
        );
        // An index a refresh has invalidated selects nothing.
        assert_eq!(
            selection_for(
                &repos,
                FeedRow::IssueRow {
                    repo: RepoIx(0),
                    issue: IssueIx(5)
                }
            ),
            None
        );
    }

    #[test]
    fn counts_report_what_the_filter_lets_through() {
        let repos = vec![
            repo("a/b", vec![pr(1, false), pr(2, true)], false),
            repo("c/d", vec![pr(3, false)], false),
        ];

        let counts = visible_counts(&repos, &FeedFilter::default(), FeedTab::PullRequests);
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
                ..Default::default()
            },
            FeedTab::PullRequests,
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
            visible_counts(&repos, &FeedFilter::default(), FeedTab::PullRequests),
            VisibleCounts {
                visible: 2,
                total: 2
            }
        );
    }
}
