//! The trunk editor: which branches the branch tree treats as trunks.
//!
//! A popover off the repository header. Edits go through
//! [`RepoBranches::set_choice`](super::model::RepoBranches::set_choice),
//! which saves the choice to the config file and fetches again.

use gpui::{Context, SharedString, div, prelude::*, rems};
use rostrum_core::branches::{TrunkChoice, TrunkName};
use rostrum_ui::{
    ActiveTheme, Popover,
    components::{Button, ButtonStyle, h_flex, v_flex},
};

use super::RepoView;

impl RepoView {
    fn toggle_trunk_editor(&mut self, cx: &mut Context<Self>) {
        self.editing_trunks = !self.editing_trunks;
        self.trunk_error = None;
        cx.notify();
    }

    pub(super) fn close_trunk_editor(&mut self, cx: &mut Context<Self>) {
        if self.editing_trunks {
            self.editing_trunks = false;
            self.trunk_error = None;
            cx.notify();
        }
    }

    fn edit_trunks(
        &mut self,
        edit: impl FnOnce(&TrunkChoice, &[TrunkName]) -> TrunkChoice,
        cx: &mut Context<Self>,
    ) {
        let branches = self.branches.clone();
        let detected = branches.read(cx).other_trunks();
        let choice = edit(&branches.read(cx).choice(cx), &detected);
        tracing::info!(repo = %self.repo, ?choice, "trunks changed");
        branches.update(cx, |branches, cx| branches.set_choice(choice, cx));
    }

    pub(super) fn add_trunk(&mut self, cx: &mut Context<Self>) {
        let raw = self.trunk_input.read(cx).text().to_string();
        match TrunkName::parse(&raw) {
            Ok(name) => {
                self.trunk_error = None;
                self.trunk_input.update(cx, |input, cx| input.clear(cx));
                self.edit_trunks(|choice, detected| choice.adding(detected, name), cx);
            }
            Err(error) => self.trunk_error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn remove_trunk(&mut self, name: TrunkName, cx: &mut Context<Self>) {
        self.edit_trunks(|choice, detected| choice.removing(detected, &name), cx);
    }

    fn detect_trunks(&mut self, cx: &mut Context<Self>) {
        self.edit_trunks(|_, _| TrunkChoice::Detected, cx);
    }

    fn render_trunk_editor(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let branches = self.branches.read(cx);
        let choice = branches.choice(cx);
        let detected = branches.other_trunks();
        let default = branches
            .snapshot()
            .and_then(|snapshot| snapshot.meta.default_branch.clone());
        let listed = choice.explicit(&detected);
        let is_detected = choice == TrunkChoice::Detected;

        v_flex()
            .gap_1p5()
            .child(
                div()
                    .text_size(rems(0.72))
                    .text_color(theme.text_subtle)
                    .child(if is_detected {
                        "Detected: whichever of main, master, staging and develop exist"
                    } else {
                        "Configured for this repository"
                    }),
            )
            .when_some(default, |el, default| {
                el.child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(rems(0.76))
                                .text_color(theme.text)
                                .child(default.to_string()),
                        )
                        .child(
                            div()
                                .text_size(rems(0.7))
                                .text_color(theme.text_subtle)
                                .child("default"),
                        ),
                )
            })
            .children(listed.into_iter().enumerate().map(|(ix, name)| {
                let label = name.to_string();
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.76))
                            .text_color(theme.text)
                            .child(label),
                    )
                    .child(
                        div()
                            .id(("remove-trunk", ix))
                            .px_1()
                            .cursor_pointer()
                            .text_size(rems(0.8))
                            .text_color(theme.text_subtle)
                            .hover(|el| el.text_color(theme.danger))
                            .child("×")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.remove_trunk(name.clone(), cx)
                            })),
                    )
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.trunk_input.clone()))
                    .child(
                        Button::new("add-trunk", "Add")
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|this, _, _window, cx| this.add_trunk(cx))),
                    ),
            )
            .when_some(self.trunk_error.clone(), |el, message| {
                el.child(
                    div()
                        .text_size(rems(0.72))
                        .text_color(theme.danger)
                        .child(message),
                )
            })
            .when(!is_detected, |el| {
                el.child(
                    Button::new("detect-trunks", "Detect automatically")
                        .tooltip("Forget this list and use whichever usual names exist")
                        .on_click(cx.listener(|this, _, _window, cx| this.detect_trunks(cx))),
                )
            })
    }

    pub(super) fn trunks_button(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.editing_trunks;
        let content = open.then(|| self.render_trunk_editor(cx).into_any_element());
        Popover::new(
            SharedString::from("trunks-popover"),
            self.trunks_anchor.clone(),
            Button::new("trunks-button", "trunks")
                .style(if open {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Subtle
                })
                .tooltip("Choose which branches are trunks")
                .on_click(cx.listener(|this, _, _window, cx| this.toggle_trunk_editor(cx))),
        )
        .open(content)
        .on_dismiss(cx.listener(|this, _, _window, cx| this.close_trunk_editor(cx)))
    }
}
