//! The right-hand pane of the CI view: one check's log or output.
//!
//! What it shows depends on where the check came from:
//!
//! - an Actions job: its log, parsed by `rostrum_core::ci::parse_log` —
//!   timestamps and ANSI removed, `##[group]` sections collapsible (all but
//!   the failing one start collapsed), the failing step tinted, searchable,
//!   and cut to its last [`DEFAULT_LOG_LINES`] lines with a "load full"
//!   button when longer;
//! - another app's check run: its output title, summary, text and
//!   annotations, and a link to the app's own page;
//! - a legacy commit status: the link alone.

use std::{collections::BTreeSet, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, AppContext, Context, Entity, EventEmitter, Subscription, Task,
    UniformListScrollHandle, Window, div, prelude::*, px, rems, uniform_list,
};
use gpui_tokio::Tokio;
use rostrum_core::{
    RepoId,
    ci::{
        CheckEntry, CheckOutput, CheckSource, DEFAULT_LOG_LINES, LineKind, LineLimit, ParsedLog,
        parse_log,
    },
};
use rostrum_github::GitHubClient;
use rostrum_ui::{
    ActiveTheme, InputEvent, TextInput, Theme,
    components::{Button, ButtonStyle, Chip, h_flex, v_flex},
    markdown,
};

use crate::loadable::Loadable;

/// Raised so the CI view can close the pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogViewEvent {
    Close,
}

/// What the Tokio side hands back: `Send`, unlike [`Content`]'s `Rc`.
enum Fetched {
    Log(Arc<String>, ParsedLog),
    Output(CheckOutput),
    LinkOnly,
}

impl From<Fetched> for Content {
    fn from(fetched: Fetched) -> Self {
        match fetched {
            Fetched::Log(raw, parsed) => Content::Log {
                raw,
                parsed: Rc::new(parsed),
            },
            Fetched::Output(output) => Content::Output(output),
            Fetched::LinkOnly => Content::LinkOnly,
        }
    }
}

/// What the pane holds once loaded.
enum Content {
    /// An Actions job's log. The raw text is kept so "load full" can re-parse
    /// without fetching again.
    Log {
        raw: Arc<String>,
        parsed: Rc<ParsedLog>,
    },
    Output(CheckOutput),
    /// A legacy status: there is nothing to fetch.
    LinkOnly,
}

pub struct LogView {
    pub(crate) repo: RepoId,
    pub(crate) entry: CheckEntry,
    content: Loadable<Content>,
    collapsed: BTreeSet<usize>,
    /// Indices into the parsed lines, with collapsed groups folded.
    visible: Rc<Vec<usize>>,
    search: Entity<TextInput>,
    matches: Vec<usize>,
    current_match: Option<usize>,
    scroll: UniformListScrollHandle,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LogViewEvent> for LogView {}

impl LogView {
    pub fn new(
        repo: RepoId,
        entry: CheckEntry,
        client: Option<GitHubClient>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search the log…", cx).lines(1, 1));
        let subscriptions = vec![cx.subscribe(&search, |this, input, event, cx| match event {
            InputEvent::Changed => {
                let query = input.read(cx).text().to_string();
                this.run_search(&query, cx);
            }
            InputEvent::Submit => this.next_match(cx),
        })];
        let mut view = Self {
            repo,
            entry,
            content: Loadable::Idle,
            collapsed: BTreeSet::new(),
            visible: Rc::new(Vec::new()),
            search,
            matches: Vec::new(),
            current_match: None,
            scroll: UniformListScrollHandle::new(),
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        view.load(client, cx);
        view
    }

    fn load(&mut self, client: Option<GitHubClient>, cx: &mut Context<Self>) {
        let source = self.entry.source.clone();
        if matches!(source, CheckSource::Status) {
            self.content = Loadable::Loaded(Content::LinkOnly);
            return;
        }
        let Some(client) = client else {
            self.content = Loadable::Failed("Not authenticated".into());
            return;
        };
        self.content = Loadable::Loading;
        let repo = self.repo.clone();
        self.tasks.push(cx.spawn(async move |this, cx| {
            let loaded = Tokio::spawn(&*cx, async move {
                match source {
                    CheckSource::Actions { job_id, .. } => {
                        let raw = Arc::new(client.job_log(&repo, job_id).await?);
                        // Parsing is CPU work on a log that can run to
                        // megabytes; it stays off the UI thread.
                        let parsed = parse_log(&raw, LineLimit::Last(DEFAULT_LOG_LINES));
                        Ok(Fetched::Log(raw, parsed))
                    }
                    CheckSource::App { check_run_id, .. } => client
                        .check_output(&repo, check_run_id)
                        .await
                        .map(Fetched::Output),
                    CheckSource::Status => Ok(Fetched::LinkOnly),
                }
            })
            .await;
            this.update(cx, |this, cx| {
                this.content = match loaded {
                    Ok(Ok(fetched)) => Loadable::Loaded(fetched.into()),
                    Ok(Err(error)) => Loadable::Failed(error.to_string()),
                    Err(error) => Loadable::Failed(error.to_string()),
                };
                this.reset_view(cx);
            })
            .ok();
        }));
    }

    fn parsed(&self) -> Option<&Rc<ParsedLog>> {
        match self.content.loaded() {
            Some(Content::Log { parsed, .. }) => Some(parsed),
            _ => None,
        }
    }

    /// After a (re)parse: groups collapsed except the failing one, the view
    /// on the first error.
    fn reset_view(&mut self, cx: &mut Context<Self>) {
        if let Some(parsed) = self.parsed().cloned() {
            self.collapsed = parsed.default_collapsed();
            self.rebuild_visible();
            if let Some(error) = parsed.first_error {
                self.reveal(error);
            }
            tracing::debug!(
                lines = parsed.lines.len(),
                dropped = parsed.dropped,
                groups = parsed.groups.len(),
                failing_step = ?parsed.failing_step,
                "log parsed"
            );
        }
        cx.notify();
    }

    fn rebuild_visible(&mut self) {
        self.visible = Rc::new(
            self.parsed()
                .map(|parsed| parsed.visible(&self.collapsed))
                .unwrap_or_default(),
        );
    }

    /// Expand whatever hides `line` and scroll to it.
    fn reveal(&mut self, line: usize) {
        if let Some(parsed) = self.parsed().cloned() {
            for group in parsed.groups_hiding(line) {
                self.collapsed.remove(&group);
            }
            self.rebuild_visible();
            if let Some(row) = self.visible.iter().position(|ix| *ix == line) {
                self.scroll
                    .scroll_to_item(row, gpui::ScrollStrategy::Center);
            }
        }
    }

    fn toggle_group(&mut self, group: usize, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
        self.rebuild_visible();
        cx.notify();
    }

    fn run_search(&mut self, query: &str, cx: &mut Context<Self>) {
        self.matches = self
            .parsed()
            .map(|parsed| parsed.search(query))
            .unwrap_or_default();
        self.current_match = None;
        self.next_match(cx);
    }

    fn next_match(&mut self, cx: &mut Context<Self>) {
        if self.matches.is_empty() {
            cx.notify();
            return;
        }
        let next = self
            .current_match
            .map_or(0, |current| (current + 1) % self.matches.len());
        self.current_match = Some(next);
        self.reveal(self.matches[next]);
        cx.notify();
    }

    /// Re-parse the whole log, past the default cut.
    fn load_full(&mut self, cx: &mut Context<Self>) {
        let Some(Content::Log { raw, .. }) = self.content.loaded() else {
            return;
        };
        let raw = raw.clone();
        self.tasks.push(cx.spawn(async move |this, cx| {
            let text = raw.clone();
            let parsed = Tokio::spawn(&*cx, async move { parse_log(&text, LineLimit::Full) }).await;
            this.update(cx, |this, cx| {
                if let Ok(parsed) = parsed {
                    this.content = Loadable::Loaded(Content::Log {
                        raw,
                        parsed: Rc::new(parsed),
                    });
                    this.reset_view(cx);
                }
            })
            .ok();
        }));
    }

    fn render_line(&self, row: usize, theme: &Theme, cx: &Context<Self>) -> AnyElement {
        let Some(parsed) = self.parsed() else {
            return div().into_any_element();
        };
        let Some(&ix) = self.visible.get(row) else {
            return div().into_any_element();
        };
        let line = &parsed.lines[ix];
        let group = parsed.group_at(ix);
        let is_match = self.current_match.map(|m| self.matches[m]) == Some(ix);
        let failing = parsed.in_failing_step(ix);
        let color = match line.kind {
            LineKind::Error => theme.danger,
            LineKind::Warning => theme.warning,
            LineKind::Notice => theme.accent,
            LineKind::Debug => theme.text_subtle,
            LineKind::Command => theme.text_muted,
            LineKind::GroupHeader => theme.text,
            LineKind::Plain => theme.text,
        };
        let bg = if is_match {
            Some(gpui::Hsla {
                a: 0.25,
                ..theme.accent
            })
        } else if line.kind == LineKind::Error {
            Some(gpui::Hsla {
                a: 0.16,
                ..theme.danger
            })
        } else if failing {
            Some(gpui::Hsla {
                a: 0.06,
                ..theme.danger
            })
        } else {
            None
        };

        h_flex()
            .id(("log-line", ix))
            .w_full()
            .h(px(18.))
            .px_2()
            .gap_2()
            .font_family(theme.mono_font.clone())
            .text_size(rems(0.72))
            .when_some(bg, |el, bg| el.bg(bg))
            .child(
                div()
                    .w(px(48.))
                    .flex_none()
                    .text_color(theme.text_subtle)
                    .child(line.number.to_string()),
            )
            .when_some(group, |el, group| {
                let open = !self.collapsed.contains(&group);
                el.cursor_pointer()
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .text_color(theme.text_subtle)
                            .child(if open { "▾" } else { "▸" }),
                    )
                    .on_click(cx.listener(move |this, _, _window, cx| this.toggle_group(group, cx)))
            })
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(color)
                    .when(line.kind == LineKind::GroupHeader, |el| {
                        el.font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .child(line.text.clone()),
            )
            .into_any_element()
    }

    fn render_log(&self, parsed: &Rc<ParsedLog>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let count = self.visible.len();
        let failing = parsed
            .failing_step
            .and_then(|ix| parsed.steps.get(ix))
            .map(|step| step.title.clone());
        let matches = self.matches.len();
        let position = self.current_match.map(|m| m + 1);

        v_flex()
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .child(div().flex_1().child(self.search.clone()))
                    .child(
                        div()
                            .text_size(rems(0.72))
                            .text_color(theme.text_subtle)
                            .child(match (position, matches) {
                                (_, 0) => "no matches".to_string(),
                                (Some(at), n) => format!("{at}/{n}"),
                                (None, n) => format!("{n} matches"),
                            }),
                    )
                    .child(
                        Button::new("log-next-match", "Next")
                            .disabled(matches == 0)
                            .on_click(cx.listener(|this, _, _window, cx| this.next_match(cx))),
                    ),
            )
            .when_some(failing, |el, step| {
                el.child(
                    h_flex()
                        .px_2()
                        .gap_2()
                        .child(Chip::new("failed step").color(theme.danger))
                        .child(
                            div()
                                .text_size(rems(0.75))
                                .text_color(theme.text)
                                .child(step),
                        ),
                )
            })
            .when(parsed.dropped > 0, |el| {
                el.child(
                    h_flex()
                        .px_2()
                        .gap_2()
                        .child(div().text_size(rems(0.72)).text_color(theme.warning).child(
                            format!(
                                "Showing the last {} lines; {} earlier lines are hidden",
                                parsed.lines.len(),
                                parsed.dropped
                            ),
                        ))
                        .child(
                            Button::new("log-load-full", "Load full log")
                                .on_click(cx.listener(|this, _, _window, cx| this.load_full(cx))),
                        ),
                )
            })
            .child(
                uniform_list(
                    "log-lines",
                    count,
                    cx.processor(move |this, range: Range<usize>, _window, cx| {
                        let theme = cx.theme().clone();
                        range
                            .map(|row| this.render_line(row, &theme, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1(),
            )
            .into_any_element()
    }

    fn render_output(&self, output: &CheckOutput, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (owner, name) = (self.repo.owner().to_string(), self.repo.name().to_string());
        v_flex()
            .id("check-output")
            .size_full()
            .overflow_y_scroll()
            .p_3()
            .gap_3()
            .when_some(output.title.clone(), |el, title| {
                el.child(div().text_size(rems(0.95)).child(title))
            })
            .when_some(output.summary.clone(), |el, summary| {
                el.child(markdown::render_github(
                    &summary, &owner, &name, 900_001, &theme, cx,
                ))
            })
            .when_some(output.text.clone(), |el, text| {
                el.child(markdown::render_github(
                    &text, &owner, &name, 900_002, &theme, cx,
                ))
            })
            .when(
                output.annotations.is_empty() && output.summary.is_none() && output.text.is_none(),
                |el| {
                    el.child(
                        div()
                            .text_size(rems(0.78))
                            .text_color(theme.text_subtle)
                            .child("This check reported no output."),
                    )
                },
            )
            .children(output.annotations.iter().map(|note| {
                let color = match note.level {
                    rostrum_core::ci::AnnotationLevel::Failure => theme.danger,
                    rostrum_core::ci::AnnotationLevel::Warning => theme.warning,
                    rostrum_core::ci::AnnotationLevel::Notice => theme.accent,
                };
                v_flex()
                    .gap_0p5()
                    .p_2()
                    .border_l_2()
                    .border_color(color)
                    .child(
                        div()
                            .font_family(theme.mono_font.clone())
                            .text_size(rems(0.72))
                            .text_color(theme.text_subtle)
                            .child(note.location()),
                    )
                    .when_some(note.title.clone(), |el, title| {
                        el.child(div().text_size(rems(0.78)).child(title))
                    })
                    .child(
                        div()
                            .text_size(rems(0.76))
                            .text_color(theme.text)
                            .child(note.message.clone()),
                    )
            }))
            .into_any_element()
    }
}

impl Render for LogView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let url = self.entry.details_url.clone();
        let producer = self.entry.producer().to_string();
        let title = self.entry.key.to_string();

        let body = match &self.content {
            Loadable::Idle | Loadable::Loading => div()
                .p_3()
                .text_color(theme.text_subtle)
                .child("Loading…")
                .into_any_element(),
            Loadable::Failed(message) => div()
                .p_3()
                .text_color(theme.danger)
                .child(message.clone())
                .into_any_element(),
            Loadable::Loaded(Content::Log { parsed, .. }) => {
                let parsed = parsed.clone();
                self.render_log(&parsed, cx)
            }
            Loadable::Loaded(Content::Output(output)) => self.render_output(output, cx),
            Loadable::Loaded(Content::LinkOnly) => div()
                .p_3()
                .text_size(rems(0.78))
                .text_color(theme.text_subtle)
                .child("A commit status carries no log; its details live on the provider's page.")
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(theme.surface)
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().text_size(rems(0.85)).child(title))
                            .child(
                                div()
                                    .text_size(rems(0.72))
                                    .text_color(theme.text_subtle)
                                    .child(format!(
                                        "{} · {}",
                                        self.entry.status.label(),
                                        self.repo
                                    )),
                            ),
                    )
                    .when_some(url, |el, url| {
                        el.child(
                            Button::new("check-open-link", format!("Open on {producer}"))
                                .tooltip(url.clone())
                                .on_click(move |_, _window, cx| cx.open_url(&url)),
                        )
                    })
                    .child(
                        Button::new("log-close", "Close")
                            .style(ButtonStyle::Subtle)
                            .tooltip("Close the log (escape)")
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(LogViewEvent::Close)),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}
