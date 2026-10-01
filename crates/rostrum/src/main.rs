//! rostrum — open pull requests across many repositories, in one feed.

mod detail;
mod feed;
mod nav;
mod notify;
mod repo_view;
mod sync;

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, prelude::*, px, rems, size,
};
use gpui_platform::application;
use rostrum_core::{RepoId, Screen, Selection};
use rostrum_diff::Highlighter;
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, Dot, h_flex, v_flex},
};

use crate::{
    detail::PrDetail,
    feed::{FeedEvent, FeedView},
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
    detail: Option<Entity<PrDetail>>,
    /// Focus target for the detail side of the split. Lives on the workspace
    /// rather than on `PrDetail` so it survives the entity being rebuilt.
    detail_focus: FocusHandle,
    /// Loading syntect's defaults is slow, so one highlighter is shared by
    /// every detail view.
    highlighter: Rc<Highlighter>,
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

        let subscriptions = vec![
            cx.observe(&store, |this, _, cx| this.sync_detail(cx)),
            cx.subscribe_in(&feed, window, |this, _, event, window, cx| match event {
                FeedEvent::FocusDetail => {
                    window.focus(&this.detail_focus, cx);
                }
                FeedEvent::OpenRepo(repo) => this.open_repo(repo.clone(), window, cx),
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
            _notifier: notifier,
            _subscriptions: subscriptions,
        }
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
            store.state.selection = selection;
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
        let current = self.detail.as_ref().map(|detail| {
            let detail = detail.read(cx);
            (detail.repo.clone(), detail.number)
        });

        // The desktop has no issue detail pane yet, so an issue selection
        // shows the same placeholder as no selection at all.
        let selected_pr = match selection {
            Some(Selection::PullRequest { repo, number }) => Some((repo, number)),
            Some(Selection::Issue { .. }) | None => None,
        };
        match (selected_pr, current) {
            // Same pull request as before: leave the entity alone so its
            // loaded conversation and scroll position survive a refresh.
            (Some(selected), Some(current)) if selected == current => {}
            (Some((repo, number)), _) => {
                let store = self.store.clone();
                let highlighter = self.highlighter.clone();
                self.detail =
                    Some(cx.new(|cx| PrDetail::new(store, repo, number, highlighter, cx)));
            }
            (None, Some(_)) => self.detail = None,
            (None, None) => {}
        }
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.refresh_all(cx));
        if let Some(detail) = self.detail.clone() {
            detail.update(cx, |detail, cx| detail.refresh(cx));
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
        let repo_count = store.state.repos.len();
        let refreshing = store.is_refreshing();
        let warnings: Vec<String> = store.warnings.iter().map(|w| w.0.clone()).collect();
        let issue_selected = matches!(store.state.selection, Some(Selection::Issue { .. }));

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
                            .child(format!("{total} open across {repo_count} repos")),
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
            .child(
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
                            .map(|el| match (self.detail.clone(), &self.repo) {
                                (Some(detail), _) => el.child(detail),
                                (None, _) if issue_selected => el.child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(rems(0.85))
                                        .text_color(theme.text_subtle)
                                        .child(
                                            "Issue details are not available on the desktop yet",
                                        ),
                                ),
                                (None, Some(panes)) => el.child(panes.branches.clone()),
                                (None, None) => el.child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(rems(0.85))
                                        .text_color(theme.text_subtle)
                                        .child(if total == 0 {
                                            "No pull requests loaded"
                                        } else {
                                            "Select a pull request"
                                        }),
                                ),
                            }),
                    ),
            )
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
