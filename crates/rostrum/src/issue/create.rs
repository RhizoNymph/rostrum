//! The form that opens a new issue: repository, title, markdown body with
//! Write/Preview, labels and assignees.
//!
//! The rules — which repository, a non-blank title, labels and people
//! belonging to the chosen repository — are [`IssueDraft`]'s; this view only
//! draws them and sends the request it builds.

use std::rc::Rc;

use gpui::{App, Context, Entity, EventEmitter, Subscription, Task, Window, div, prelude::*, rems};
use gpui_tokio::Tokio;
use rostrum_core::{Label, RepoId, Selection, User};
use rostrum_github::{GitHubClient, GitHubError, IssueDraft};
use rostrum_ui::{
    ActiveTheme, InputEvent, TextInput,
    components::{Button, ButtonStyle, Tab, h_flex, tab_bar, v_flex},
    markdown,
};

use crate::{
    loadable::Loadable,
    pickers::{self, OpenPicker, Toggle},
    sync::Store,
};

/// Raised so the workspace can close the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewIssueEvent {
    Cancelled,
}

/// Whether the body is being edited or previewed as rendered markdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BodyMode {
    #[default]
    Write,
    Preview,
}

pub struct NewIssueForm {
    store: Entity<Store>,
    draft: IssueDraft,
    title: Entity<TextInput>,
    body: Entity<TextInput>,
    mode: BodyMode,
    picker: Option<OpenPicker>,
    /// Both palettes belong to `draft.repo()`, and are reset when it changes.
    repo_labels: Loadable<Vec<Label>>,
    assignable: Loadable<Vec<User>>,
    /// Whether the create request is in flight.
    sending: bool,
    error: Option<String>,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewIssueEvent> for NewIssueForm {}

impl NewIssueForm {
    /// `repo` is the repository the form was opened from; `None` falls back
    /// to the first watched one, which the chooser lets the user change.
    pub fn new(store: Entity<Store>, repo: Option<RepoId>, cx: &mut Context<Self>) -> Self {
        let repo = repo.or_else(|| {
            store
                .read(cx)
                .state
                .repos
                .first()
                .map(|repo| repo.id.clone())
        });
        let title = cx.new(|cx| TextInput::new("Title", cx).lines(1, 1));
        let body = cx.new(|cx| TextInput::new("Describe the issue (markdown)…", cx).lines(6, 16));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            // Re-render as the title is typed, so the Create button's
            // enabled state follows it.
            cx.subscribe(&title, |_, _, event, cx| {
                if matches!(event, InputEvent::Changed) {
                    cx.notify();
                }
            }),
            cx.subscribe(&body, |this, _, event, cx| {
                if matches!(event, InputEvent::Submit) {
                    this.create(cx);
                }
            }),
        ];
        Self {
            store,
            draft: IssueDraft::new(repo),
            title,
            body,
            mode: BodyMode::default(),
            picker: None,
            repo_labels: Loadable::Idle,
            assignable: Loadable::Idle,
            sending: false,
            error: None,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    fn client(&self, cx: &App) -> Option<GitHubClient> {
        self.store.read(cx).client()
    }

    fn choose_repo(&mut self, repo: RepoId, cx: &mut Context<Self>) {
        if self.draft.repo() == Some(&repo) {
            return;
        }
        self.draft.set_repo(repo);
        // The palettes were the old repository's.
        self.repo_labels = Loadable::Idle;
        self.assignable = Loadable::Idle;
        self.picker = None;
        cx.notify();
    }

    fn toggle_picker(&mut self, which: OpenPicker, cx: &mut Context<Self>) {
        self.picker = if self.picker == Some(which) {
            None
        } else {
            Some(which)
        };
        let Some(repo) = self.draft.repo().cloned() else {
            cx.notify();
            return;
        };
        match self.picker {
            Some(OpenPicker::Labels) if self.repo_labels.is_idle() => {
                self.repo_labels = Loadable::Loading;
                self.load(
                    cx,
                    move |client| async move { client.repository_labels(&repo).await },
                    |this, labels| this.repo_labels = labels,
                );
            }
            Some(OpenPicker::Assignees) if self.assignable.is_idle() => {
                self.assignable = Loadable::Loading;
                self.load(
                    cx,
                    move |client| async move { client.assignable_users(&repo).await },
                    |this, users| this.assignable = users,
                );
            }
            _ => {}
        }
        cx.notify();
    }

    fn load<T, F, Fut>(
        &mut self,
        cx: &mut Context<Self>,
        fetch: F,
        apply: impl FnOnce(&mut Self, Loadable<Vec<T>>) + 'static,
    ) where
        T: Send + 'static,
        F: FnOnce(GitHubClient) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Vec<T>, GitHubError>> + Send + 'static,
    {
        let Some(client) = self.client(cx) else {
            apply(self, Loadable::Failed("Not authenticated".into()));
            return;
        };
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = Tokio::spawn(&*cx, fetch(client)).await;
            this.update(cx, |this, cx| {
                apply(
                    this,
                    match result {
                        Ok(Ok(items)) => Loadable::Loaded(items),
                        Ok(Err(error)) => Loadable::Failed(error.to_string()),
                        Err(error) => Loadable::Failed(error.to_string()),
                    },
                );
                cx.notify();
            })
            .ok();
        }));
    }

    /// Send the draft. On success the new issue becomes the selection, which
    /// replaces this form with its pane; on failure the form stays, with the
    /// reason, so nothing typed is lost.
    fn create(&mut self, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let title = self.title.read(cx).text().to_string();
        let body = self.body.read(cx).text().to_string();
        let (repo, request) = match self.draft.request(&title, &body) {
            Ok(ready) => ready,
            Err(error) => {
                self.error = Some(capitalised(&error.to_string()));
                cx.notify();
                return;
            }
        };
        let Some(client) = self.client(cx) else {
            self.error = Some("Not authenticated".into());
            cx.notify();
            return;
        };

        self.sending = true;
        self.error = None;
        cx.notify();
        tracing::info!(%repo, labels = request.labels.len(), assignees = request.assignees.len(), "creating issue");

        self.tasks.push(cx.spawn(async move |this, cx| {
            let target = repo.clone();
            let result = Tokio::spawn(
                &*cx,
                async move { client.create_issue(&target, &request).await },
            )
            .await;
            this.update(cx, |this, cx| {
                this.sending = false;
                match result {
                    Ok(Ok(number)) => {
                        this.store.update(cx, |store, cx| {
                            store.refresh_issues(repo.clone(), cx);
                            store.reveal(Selection::Issue { repo, number }, cx);
                        });
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string()),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn on_click(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
        let entity = cx.entity();
        move |_event, _window, cx| entity.update(cx, |this, cx| f(this, cx))
    }

    fn label_toggle(cx: &Context<Self>) -> Toggle {
        let entity = cx.entity();
        Rc::new(move |name: String, _applied, cx: &mut App| {
            entity.update(cx, |this, cx| {
                this.draft.toggle_label(&name);
                cx.notify();
            });
        })
    }

    fn assignee_toggle(cx: &Context<Self>) -> Toggle {
        let entity = cx.entity();
        Rc::new(move |login: String, _assigned, cx: &mut App| {
            entity.update(cx, |this, cx| {
                this.draft.toggle_assignee(&login);
                cx.notify();
            });
        })
    }

    /// The chosen labels as the picker wants them: named, coloured from the
    /// palette when it has loaded.
    fn chosen_labels(&self) -> Vec<Label> {
        let palette = self.repo_labels.loaded();
        self.draft
            .labels()
            .iter()
            .map(|name| {
                palette
                    .and_then(|labels| labels.iter().find(|label| &label.name == name))
                    .cloned()
                    .unwrap_or_else(|| Label {
                        name: name.clone(),
                        color: String::new(),
                    })
            })
            .collect()
    }

    fn chosen_assignees(&self) -> Vec<User> {
        self.draft
            .assignees()
            .iter()
            .map(|login| User {
                login: login.clone(),
                avatar_url: None,
            })
            .collect()
    }
}

/// First letter upper-cased, for an error message written mid-sentence.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl Render for NewIssueForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let repos: Vec<RepoId> = self
            .store
            .read(cx)
            .state
            .repos
            .iter()
            .map(|repo| repo.id.clone())
            .collect();
        let chosen = self.draft.repo().cloned();
        let has_title = !self.title.read(cx).is_empty();
        let sending = self.sending;
        let labels = self.chosen_labels();
        let assignees = self.chosen_assignees();
        let entity = cx.entity();
        let section = |text: &'static str| {
            div()
                .text_size(rems(0.74))
                .text_color(theme.text_subtle)
                .child(text)
        };
        let picker_button = |id: &'static str, which: OpenPicker, closed: &'static str| {
            let open = self.picker == Some(which);
            Button::new(id, if open { "Close" } else { closed })
                .disabled(chosen.is_none())
                .on_click(Self::on_click(cx, move |this, cx| {
                    this.toggle_picker(which, cx)
                }))
        };

        let body = match self.mode {
            BodyMode::Write => self.body.clone().into_any_element(),
            BodyMode::Preview => {
                let text = self.body.read(cx).text().to_string();
                let rendered = match (&chosen, text.trim().is_empty()) {
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
                    .id("new-issue-preview")
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
            .id("new-issue-form")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(
                div()
                    .text_size(rems(1.1))
                    .text_color(theme.text)
                    .child("New issue"),
            )
            .child(section("Repository"))
            .child(
                h_flex()
                    .gap_1p5()
                    .flex_wrap()
                    .children(repos.into_iter().enumerate().map(|(ix, repo)| {
                        let selected = chosen.as_ref() == Some(&repo);
                        Button::new(("new-issue-repo", ix), repo.to_string())
                            .style(if selected {
                                ButtonStyle::Primary
                            } else {
                                ButtonStyle::Subtle
                            })
                            .on_click(Self::on_click(cx, move |this, cx| {
                                this.choose_repo(repo.clone(), cx)
                            }))
                    })),
            )
            .child(section("Title"))
            .child(self.title.clone())
            .child(div().flex_none().child(tab_bar(
                vec![Tab::new("Write"), Tab::new("Preview")],
                match self.mode {
                    BodyMode::Write => 0,
                    BodyMode::Preview => 1,
                },
                cx,
                move |ix, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.mode = if ix == 1 {
                            BodyMode::Preview
                        } else {
                            BodyMode::Write
                        };
                        cx.notify();
                    });
                },
            )))
            .child(body)
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(section("Labels"))
                    .children(labels.iter().enumerate().map(|(ix, label)| {
                        pickers::label_chip(ix, label, sending, &theme, Self::label_toggle(cx))
                    }))
                    .child(picker_button(
                        "new-issue-labels",
                        OpenPicker::Labels,
                        "Labels…",
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(section("Assignees"))
                    .children(assignees.iter().enumerate().map(|(ix, user)| {
                        pickers::assignee_chip(ix, user, sending, &theme, Self::assignee_toggle(cx))
                    }))
                    .child(picker_button(
                        "new-issue-assignees",
                        OpenPicker::Assignees,
                        "Assignees…",
                    )),
            )
            .map(|el| match self.picker {
                Some(OpenPicker::Labels) => el.child(pickers::label_picker(
                    &self.repo_labels,
                    &labels,
                    sending,
                    &theme,
                    Self::label_toggle(cx),
                )),
                Some(OpenPicker::Assignees) => el.child(pickers::assignee_picker(
                    &self.assignable,
                    &assignees,
                    sending,
                    &theme,
                    Self::assignee_toggle(cx),
                )),
                None => el,
            })
            .when_some(self.error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(
                            "new-issue-create",
                            if sending {
                                "Creating…"
                            } else {
                                "Create issue"
                            },
                        )
                        .style(ButtonStyle::Primary)
                        .disabled(sending || !has_title || chosen.is_none())
                        .tooltip(if has_title {
                            "Open the issue on GitHub (ctrl-enter in the body)"
                        } else {
                            "An issue needs a title"
                        })
                        .on_click(Self::on_click(cx, |this, cx| this.create(cx))),
                    )
                    .child(
                        Button::new("new-issue-cancel", "Cancel")
                            .disabled(sending)
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(NewIssueEvent::Cancelled)),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            capitalised("an issue needs a title"),
            "An issue needs a title"
        );
        assert_eq!(capitalised(""), "");
    }
}
