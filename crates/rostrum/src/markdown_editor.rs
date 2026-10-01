//! A multi-line markdown input with Write and Preview tabs.
//!
//! Used wherever a description is written: the new-issue form and the issue
//! editor. The preview goes through the same GitHub-flavoured renderer the
//! timeline uses, so what is previewed is what will be shown.

use gpui::{App, Context, Entity, Window, div, prelude::*, rems};
use rostrum_core::RepoId;
use rostrum_ui::{
    ActiveTheme, TextInput,
    components::{Tab, tab_bar, v_flex},
    markdown,
};

/// Whether the text is being edited or previewed as rendered markdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Write,
    Preview,
}

pub struct MarkdownEditor {
    input: Entity<TextInput>,
    mode: Mode,
    /// The repository `#123` and `@user` shorthand resolve against in the
    /// preview; plain markdown when there is none yet.
    repo: Option<RepoId>,
    /// Distinguishes this editor's preview from any other in the window.
    id: &'static str,
}

impl MarkdownEditor {
    pub fn new(
        id: &'static str,
        placeholder: &'static str,
        repo: Option<RepoId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new(placeholder, cx).lines(6, 16));
        Self {
            input,
            mode: Mode::default(),
            repo,
            id,
        }
    }

    /// The text input itself, for subscribing to its submit event.
    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.input.update(cx, |input, cx| input.set_text(text, cx));
        cx.notify();
    }

    pub fn set_repo(&mut self, repo: Option<RepoId>, cx: &mut Context<Self>) {
        self.repo = repo;
        cx.notify();
    }
}

impl Render for MarkdownEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let entity = cx.entity();
        let body = match self.mode {
            Mode::Write => self.input.clone().into_any_element(),
            Mode::Preview => {
                let text = self.input.read(cx).text().to_string();
                let rendered = match (&self.repo, text.trim().is_empty()) {
                    (_, true) => div()
                        .text_size(rems(0.78))
                        .text_color(theme.text_subtle)
                        .child("Nothing to preview")
                        .into_any_element(),
                    (Some(repo), false) => {
                        markdown::render_github(&text, repo.owner(), repo.name(), 0, &theme, cx)
                    }
                    (None, false) => markdown::render_source(&text, 0, &theme, cx),
                };
                div()
                    .id(self.id)
                    .min_h(rems(8.))
                    .max_h(rems(24.))
                    .overflow_y_scroll()
                    .p_2()
                    .border_1()
                    .border_color(theme.border)
                    .child(rendered)
                    .into_any_element()
            }
        };

        v_flex()
            .gap_2()
            .child(div().flex_none().child(tab_bar(
                vec![Tab::new("Write"), Tab::new("Preview")],
                match self.mode {
                    Mode::Write => 0,
                    Mode::Preview => 1,
                },
                cx,
                move |ix, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.mode = if ix == 1 { Mode::Preview } else { Mode::Write };
                        cx.notify();
                    });
                },
            )))
            .child(body)
    }
}
