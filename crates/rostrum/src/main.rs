//! rostrum — open pull requests across many repositories, in one feed.

mod ci;
mod detail;
mod feed;
mod issue;
mod loadable;
mod markdown_editor;
mod nav;
mod notify;
mod pickers;
mod repo_view;
mod sync;

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, prelude::*, px, rems, size,
};
use gpui_platform::application;
use rostrum_core::{FeedTab, RepoId, Screen, Selection};
use rostrum_diff::Highlighter;
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, Dot, h_flex, v_flex},
};

use crate::{
    detail::PrDetail,
    feed::{FeedEvent, FeedView},
    issue::{IssuePane, NewIssueEvent, NewIssueForm},
    notify::Notifier,
    repo_view::{RepoView, RepoViewEvent, branches::BranchesPane, model::RepoBranches},
    sync::AuthStatus,
    sync::Store,
};

actions!(rostrum, [Quit, Refresh]);

/// Width of the feed pane, in pixels.
const FEED_WIDTH: f32 = 440.;

/// Key context of the detail pane. `enter` in the feed moves focus here, which
/// takes the pane's descendants out of the feed's navigation bindings.
const DETAIL_CONTEXT: &str = "Detail";

/// The entities behind one repository's view, created on entering it and
/// dropped on leaving, which cancels their requests.
struct RepoPanes {
    /// The left pane: pull requests and issues.
    view: Entity<RepoView>,
    /// The right pane when nothing is selected: the branch tree.
    branches: Entity<BranchesPane>,
    /// The branch data both panes read. Held so it lives as long as they do.
    _model: Entity<RepoBranches>,
    _subscription: Subscription,
}

/// What the right-hand side of the split is showing.
enum DetailPane {
    PullRequest(Entity<PrDetail>),
    Issue(Entity<IssuePane>),
    /// The new-issue form. Not a selection: opening it clears the selection,
    /// and creating the issue selects it, which replaces the form.
    NewIssue {
        form: Entity<NewIssueForm>,
        /// Held so the form's cancel event keeps reaching the workspace.
        _cancelled: Subscription,
    },
}

impl DetailPane {
    /// The selection this pane shows, if it shows one.
    fn selection(&self, cx: &App) -> Option<Selection> {
        match self {
            Self::PullRequest(detail) => {
                let detail = detail.read(cx);
                Some(Selection::PullRequest {
                    repo: detail.repo.clone(),
                    number: detail.number,
                })
            }
            Self::Issue(pane) => {
                let pane = pane.read(cx);
                Some(Selection::Issue {
                    repo: pane.repo.clone(),
                    number: pane.number,
                })
            }
            Self::NewIssue { .. } => None,
        }
    }
}

struct Workspace {
    store: Entity<Store>,
    feed: Entity<FeedView>,
    /// Which screen the left pane shows. The feed entity is kept while a
    /// repository is open, so its scroll position survives the round trip.
    screen: Screen,
    /// Present exactly when `screen` is a repository.
    repo: Option<RepoPanes>,
    /// Rebuilt whenever the selection changes; dropping the previous entity
    /// cancels its in-flight requests.
    detail: Option<DetailPane>,
    /// Focus target for the detail side of the split. Lives on the workspace
    /// rather than on `PrDetail` so it survives the entity being rebuilt.
    detail_focus: FocusHandle,
    /// Loading syntect's defaults is slow, so one highlighter is shared by
    /// every detail view.
    highlighter: Rc<Highlighter>,
    /// The CI grid. Built once and kept, so its scroll position and
    /// selection survive switching away and back.
    ci: Entity<ci::CiView>,
    /// Whether the window shows the CI grid instead of the feed and detail.
    showing_ci: bool,
    /// Watches the store for newly arrived pull requests. Held so it is not
    /// dropped; it has no rendered form.
    _notifier: Entity<Notifier>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.new(Store::new);
        let feed = cx.new(|cx| FeedView::new(store.clone(), cx));
        let notifier = cx.new(|cx| Notifier::new(store.clone(), cx));
        let ci = cx.new(|cx| ci::CiView::new(store.clone(), cx));

        let subscriptions = vec![
            cx.subscribe_in(&ci, window, |this, _, event, window, cx| match event {
                ci::CiEvent::Leave => this.show_ci(false, window, cx),
            }),
            cx.observe(&store, |this, _, cx| this.sync_detail(cx)),
            cx.subscribe_in(&feed, window, |this, _, event, window, cx| match event {
                FeedEvent::FocusDetail => {
                    window.focus(&this.detail_focus, cx);
                }
                FeedEvent::OpenRepo(repo) => this.open_repo(repo.clone(), window, cx),
                FeedEvent::NewIssue { repo } => this.open_new_issue(repo.clone(), cx),
                FeedEvent::OpenCi => this.show_ci(true, window, cx),
            }),
        ];

        // Start with the feed focused so the keyboard works without a click.
        let feed_focus = feed.focus_handle(cx);
        window.focus(&feed_focus, cx);

        Self {
            store,
            feed,
            screen: Screen::Feed,
            repo: None,
            detail: None,
            detail_focus: cx.focus_handle(),
            highlighter: Rc::new(Highlighter::new()),
            ci,
            showing_ci: false,
            _notifier: notifier,
            _subscriptions: subscriptions,
        }
    }

    /// Switch the whole window to the CI grid, or back to the feed. Focus
    /// follows, so the grid's keys work at once and the feed's afterwards.
    fn show_ci(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.showing_ci == show {
            return;
        }
        tracing::info!(show, "CI view");
        self.showing_ci = show;
        self.ci.update(cx, |ci, cx| ci.set_visible(show, cx));
        let focus = if show {
            self.ci.focus_handle(cx)
        } else {
            self.feed.focus_handle(cx)
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Switch the left pane to `repo`'s own view.
    fn open_repo(&mut self, repo: RepoId, window: &mut Window, cx: &mut Context<Self>) {
        let screen = &mut self.screen;
        self.store.update(cx, |store, cx| {
            let mut selection = store.state.selection.take();
            screen.enter_repo(repo.clone(), &mut selection);
            store.state.selection = selection;
            cx.notify();
        });

        let current = self
            .repo
            .as_ref()
            .map(|panes| panes.view.read(cx).repo.clone());
        if current.as_ref() != Some(&repo) {
            tracing::info!(repo = %repo, "opened repository view");
            let store = self.store.clone();
            let model = cx.new(|cx| RepoBranches::new(store.clone(), repo.clone(), cx));
            let view = cx.new(|cx| RepoView::new(store.clone(), repo.clone(), model.clone(), cx));
            let branches = cx.new(|cx| BranchesPane::new(store, repo, model.clone(), cx));
            let subscription =
                cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
                    RepoViewEvent::Back => this.close_repo(window, cx),
                    RepoViewEvent::FocusDetail => window.focus(&this.detail_focus, cx),
                    RepoViewEvent::ShowBranches => {
                        // The selection is already cleared; a new-issue form
                        // is not a selection, so it has to be closed here.
                        if matches!(this.detail, Some(DetailPane::NewIssue { .. })) {
                            this.detail = None;
                            cx.notify();
                        }
                    }
                    RepoViewEvent::NewIssue => {
                        let repo = this.screen.repo().cloned();
                        this.open_new_issue(repo, cx);
                    }
                });
            self.repo = Some(RepoPanes {
                view,
                branches,
                _model: model,
                _subscription: subscription,
            });
        }
        if let Some(panes) = &self.repo {
            let focus = panes.view.focus_handle(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Back to the multi-repository feed.
    fn close_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let screen = &mut self.screen;
        self.store.update(cx, |store, cx| {
            let mut selection = store.state.selection.take();
            screen.back(&mut selection);
            // The view lists both kinds; the feed shows one tab at a time.
            // Land on the tab that holds what is selected, so it is visible.
            let tab = match &selection {
                Some(Selection::PullRequest { .. }) => Some(FeedTab::PullRequests),
                Some(Selection::Issue { .. }) => Some(FeedTab::Issues),
                None => None,
            };
            store.state.selection = selection;
            if let Some(tab) = tab {
                store.set_tab(tab, cx);
            }
            cx.notify();
        });
        if self.repo.take().is_some() {
            tracing::info!("closed repository view");
        }
        let focus = self.feed.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Keep the detail pane in step with the store's selection.
    fn sync_detail(&mut self, cx: &mut Context<Self>) {
        // A repository removed while its view is open leaves nothing to show.
        if let Some(repo) = self.screen.repo()
            && self.store.read(cx).state.repo(repo).is_none()
        {
            self.screen = Screen::Feed;
            self.repo = None;
        }

        let selection = self.store.read(cx).state.selection.clone();
        let current = self.detail.as_ref().map(|pane| pane.selection(cx));

        match (selection, current) {
            // Same item as before: leave the entity alone so its loaded
            // conversation and scroll position survive a refresh.
            (Some(selection), Some(Some(shown))) if selection == shown => {}
            (Some(selection), _) => self.detail = Some(self.open(selection, cx)),
            // The form is not a selection, so a store change with nothing
            // selected must not close it mid-typing.
            (None, Some(None)) | (None, None) => {}
            (None, Some(Some(_))) => self.detail = None,
        }
        cx.notify();
    }

    fn open(&self, selection: Selection, cx: &mut Context<Self>) -> DetailPane {
        let store = self.store.clone();
        match selection {
            Selection::PullRequest { repo, number } => {
                let highlighter = self.highlighter.clone();
                DetailPane::PullRequest(
                    cx.new(|cx| PrDetail::new(store, repo, number, highlighter, cx)),
                )
            }
            Selection::Issue { repo, number } => {
                DetailPane::Issue(cx.new(|cx| IssuePane::new(store, repo, number, cx)))
            }
        }
    }

    /// Show the new-issue form, preset to `repo` when it came from a
    /// repository's header. The selection is cleared so the form is the one
    /// thing the pane shows; picking a row, or creating the issue, replaces it.
    fn open_new_issue(&mut self, repo: Option<RepoId>, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let form = cx.new(|cx| NewIssueForm::new(store, repo, cx));
        let subscription = cx.subscribe(&form, |this, _, event, cx| match event {
            NewIssueEvent::Cancelled => {
                this.detail = None;
                cx.notify();
            }
        });
        self.detail = Some(DetailPane::NewIssue {
            form,
            _cancelled: subscription,
        });
        self.store.update(cx, |store, cx| {
            store.state.selection = None;
            store.set_tab(FeedTab::Issues, cx);
            cx.notify();
        });
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.refresh_all(cx));
        match &self.detail {
            Some(DetailPane::PullRequest(detail)) => {
                detail.update(cx, |detail, cx| detail.refresh(cx));
            }
            Some(DetailPane::Issue(pane)) => pane.update(cx, |pane, cx| pane.refresh(cx)),
            Some(DetailPane::NewIssue { .. }) | None => {}
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let store = self.store.read(cx);

        let (status_text, status_color) = match &store.auth {
            AuthStatus::Resolving => ("authenticating…".to_string(), theme.text_subtle),
            AuthStatus::Ready { source } => (source.clone(), theme.success),
            AuthStatus::Failed { message } => (message.clone(), theme.danger),
        };
        let total = store.state.total_open_prs();
        let issues = store.state.total_open_issues();
        let repo_count = store.state.repos.len();
        let refreshing = store.is_refreshing();
        let warnings: Vec<String> = store.warnings.iter().map(|w| w.0.clone()).collect();

        v_flex()
            .size_full()
            .bg(theme.background)
            .font_family(theme.ui_font.clone())
            .text_color(theme.text)
            .key_context("Workspace")
            .on_action(cx.listener(Self::refresh))
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_none()
                    .px_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .bg(theme.surface_raised)
                    .child(
                        div()
                            .text_size(rems(0.9))
                            .text_color(theme.text)
                            .child("rostrum"),
                    )
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_subtle)
                            .child(format!(
                                "{total} pull requests and {issues} issues open across {repo_count} repos"
                            )),
                    )
                    .child(div().flex_1())
                    .when(refreshing, |el| {
                        el.child(
                            div()
                                .text_size(rems(0.72))
                                .text_color(theme.text_subtle)
                                .child("refreshing…"),
                        )
                    })
                    .child(Dot::new(status_color))
                    .child(
                        div()
                            .text_size(rems(0.72))
                            .text_color(theme.text_muted)
                            .child(status_text),
                    ),
            )
            .when(!warnings.is_empty(), |el| {
                el.child(
                    v_flex()
                        .flex_none()
                        .px_4()
                        .py_2()
                        .gap_1()
                        .bg(theme.surface)
                        .border_b_1()
                        .border_color(theme.border)
                        .children(warnings.into_iter().map(|warning| {
                            h_flex()
                                .gap_2()
                                .child(Chip::new("config").color(theme.warning))
                                .child(
                                    div()
                                        .text_size(rems(0.75))
                                        .text_color(theme.text_muted)
                                        .child(warning),
                                )
                        })),
                )
            })
            .when(self.showing_ci, |el| {
                el.child(div().flex_1().min_h_0().child(self.ci.clone()))
            })
            .when(!self.showing_ci, |el| el.child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(FEED_WIDTH))
                            .h_full()
                            .flex_none()
                            .py_3()
                            .overflow_hidden()
                            .map(|el| match &self.repo {
                                Some(panes) => el.child(panes.view.clone()),
                                None => el.child(self.feed.clone()),
                            }),
                    )
                    .child(div().w(px(1.)).h_full().flex_none().bg(theme.border))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .key_context(DETAIL_CONTEXT)
                            .track_focus(&self.detail_focus)
                            .map(|el| match (&self.detail, &self.repo) {
                                (Some(DetailPane::PullRequest(detail)), _) => {
                                    el.child(detail.clone())
                                }
                                (Some(DetailPane::Issue(pane)), _) => el.child(pane.clone()),
                                (Some(DetailPane::NewIssue { form, .. }), _) => {
                                    el.child(form.clone())
                                }
                                // Nothing selected inside a repository's view:
                                // its branch tree.
                                (None, Some(panes)) => el.child(panes.branches.clone()),
                                (None, None) => el.child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(rems(0.85))
                                        .text_color(theme.text_subtle)
                                        .child(if total + issues == 0 {
                                            "Nothing loaded yet"
                                        } else {
                                            "Select a pull request or an issue"
                                        }),
                                ),
                            }),
                    ),
            ))
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rostrum=info,rostrum_github=info".into()),
        )
        .init();

    application().run(|cx: &mut App| {
        gpui_tokio::init(cx);
        rostrum_ui::theme::init(cx);
        rostrum_ui::input::bind_keys(cx);
        feed::bind_keys(cx);
        repo_view::bind_keys(cx);
        detail::bind_keys(cx);
        ci::bind_keys(cx);

        cx.bind_keys([
            gpui::KeyBinding::new("ctrl-q", Quit, None),
            gpui::KeyBinding::new("cmd-q", Quit, None),
            gpui::KeyBinding::new("ctrl-r", Refresh, None),
            gpui::KeyBinding::new("cmd-r", Refresh, None),
        ]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("rostrum".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .expect("failed to open window");

        cx.activate(true);
    });
}
