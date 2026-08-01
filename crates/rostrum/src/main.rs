//! rostrum — open pull requests across many repositories, in one feed.

mod config;
mod detail;
mod feed;
mod sync;

use gpui::{
    App, Bounds, Context, Entity, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rems, size,
};
use gpui_platform::application;
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, Dot, h_flex, v_flex},
};

use crate::{detail::DetailView, feed::FeedView, sync::AuthStatus, sync::Store};

actions!(rostrum, [Quit, Refresh]);

/// Width of the feed pane, in pixels.
const FEED_WIDTH: f32 = 440.;

struct Workspace {
    store: Entity<Store>,
    feed: Entity<FeedView>,
    detail: Entity<DetailView>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    fn new(cx: &mut Context<Self>) -> Self {
        let store = cx.new(Store::new);
        let feed = cx.new(|cx| FeedView::new(store.clone(), cx));
        let detail = cx.new(|cx| DetailView::new(store.clone(), cx));
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];

        Self {
            store,
            feed,
            detail,
            _subscriptions: subscriptions,
        }
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.refresh_all(cx));
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
                            .child(self.feed.clone()),
                    )
                    .child(div().w(px(1.)).h_full().flex_none().bg(theme.border))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.detail.clone()),
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
            |_window, cx| cx.new(Workspace::new),
        )
        .expect("failed to open window");

        cx.activate(true);
    });
}
