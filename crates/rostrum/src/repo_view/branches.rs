//! The right pane of the repository view when nothing is selected: the
//! branch tree.
//!
//! Each trunk with its distance from the default branch, the pull requests
//! based on it beneath, and pull requests based on those beneath them. The
//! tree itself is built by `rostrum_core::branches::build_tree`; this module
//! only draws [`BranchRow`]s.

use gpui::{AnyElement, Context, Entity, Hsla, Subscription, Window, div, prelude::*, px, rems};
use rostrum_core::{
    Divergence, PrNumber, PullRequest, RepoId, Selection,
    branches::{BranchRow, PullNote, TrunkDrift},
};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, Chip, h_flex, v_flex},
};

use super::model::{Branches, FetchStatus, RepoBranches};
use crate::sync::Store;

/// Indent per level of stacking, in pixels.
const INDENT: f32 = 18.;

pub struct BranchesPane {
    store: Entity<Store>,
    repo: RepoId,
    branches: Entity<RepoBranches>,
    _subscriptions: Vec<Subscription>,
}

impl BranchesPane {
    pub fn new(
        store: Entity<Store>,
        repo: RepoId,
        branches: Entity<RepoBranches>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.observe(&branches, |_, _, cx| cx.notify()),
        ];
        Self {
            store,
            repo,
            branches,
            _subscriptions: subscriptions,
        }
    }

    fn select(&mut self, number: PrNumber, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        self.store.update(cx, |store, cx| {
            store.state.selection = Some(Selection::PullRequest { repo, number });
            cx.notify();
        });
    }

    fn render_row(
        &self,
        ix: usize,
        row: BranchRow,
        prs: &[PullRequest],
        default: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            BranchRow::Trunk { name, drift, pulls } => h_flex()
                .gap_2()
                .px_3()
                .pt_3()
                .pb_1()
                .child(
                    div()
                        .text_size(rems(0.86))
                        .text_color(theme.text)
                        .child(name.to_string()),
                )
                .children(trunk_chips(ix, &name.to_string(), drift, default, theme))
                .child(div().flex_1())
                .child(count_label(pulls, theme))
                .into_any_element(),
            BranchRow::OtherBases => div()
                .px_3()
                .pt_4()
                .pb_1()
                .text_size(rems(0.8))
                .text_color(theme.text_muted)
                .child("Other bases")
                .into_any_element(),
            BranchRow::Base { name, pulls } => h_flex()
                .gap_2()
                .px_3()
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .text_size(rems(0.8))
                        .text_color(theme.text)
                        .child(name),
                )
                .child(Chip::new("unknown base").color(theme.text_subtle).tooltip(
                    ("unknown-base", ix),
                    "Neither a trunk nor the head of exactly one open pull request",
                ))
                .child(div().flex_1())
                .child(count_label(pulls, theme))
                .into_any_element(),
            BranchRow::Pull {
                depth,
                number,
                head,
                base,
                drift,
                note,
            } => {
                let pull = prs.iter().find(|pr| pr.number == number);
                let title = pull.map(|pr| pr.title.clone()).unwrap_or_default();
                let is_draft = pull.is_some_and(|pr| pr.is_draft);
                let merge = pull.map(PullRequest::merge_status);

                h_flex()
                    .id(("branch-pr", ix))
                    .gap_2()
                    .pl(px(12. + INDENT * depth as f32))
                    .pr_3()
                    .py_1()
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.surface_hover))
                    .child(
                        div()
                            .text_size(rems(0.75))
                            .text_color(theme.text_subtle)
                            .child("└"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(220.))
                            .truncate()
                            .text_size(rems(0.76))
                            .text_color(theme.text_muted)
                            .font_family(theme.mono_font.clone())
                            .child(head),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(rems(0.74))
                            .text_color(theme.text_subtle)
                            .child(format!("#{number}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.8))
                            .text_color(if is_draft {
                                theme.text_muted
                            } else {
                                theme.text
                            })
                            .child(title),
                    )
                    .children(divergence_chips(ix, drift, &base, theme))
                    .when(is_draft, |el| {
                        el.child(Chip::new("draft").color(theme.draft))
                    })
                    .when_some(
                        merge.and_then(|merge| merge.chip().map(|text| (merge, text))),
                        |el, (merge, text)| {
                            el.child(
                                Chip::new(text)
                                    .color(theme.merge_color(merge))
                                    .tooltip(("branch-merge", ix), merge.explanation()),
                            )
                        },
                    )
                    .when_some(note, |el, note| {
                        let (text, why) = match note {
                            PullNote::BreaksCycle => (
                                "cycle",
                                "Its base is a pull request stacked on it; the loop is cut here",
                            ),
                            PullNote::AmbiguousBase => (
                                "ambiguous",
                                "More than one open pull request has this base as its head",
                            ),
                        };
                        el.child(
                            Chip::new(text)
                                .color(theme.warning)
                                .tooltip(("branch-note", ix), why),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _window, cx| this.select(number, cx)))
                    .into_any_element()
            }
        }
    }
}

fn count_label(pulls: usize, theme: &Theme) -> impl IntoElement {
    div()
        .text_size(rems(0.72))
        .text_color(theme.text_subtle)
        .child(match pulls {
            0 => "no pull requests".to_string(),
            1 => "1 pull request".to_string(),
            n => format!("{n} pull requests"),
        })
}

fn trunk_chips(
    ix: usize,
    name: &str,
    drift: TrunkDrift,
    default: &str,
    theme: &Theme,
) -> Vec<AnyElement> {
    match drift {
        TrunkDrift::Default => vec![Chip::new("default").color(theme.accent).into_any_element()],
        TrunkDrift::Missing => vec![
            Chip::new("missing")
                .color(theme.danger)
                .tooltip(
                    ("trunk-missing", ix),
                    format!("GitHub has no branch named {name}"),
                )
                .into_any_element(),
        ],
        TrunkDrift::Unknown => divergence_chips(ix, None, default, theme),
        TrunkDrift::Known(divergence) => divergence_chips(ix, Some(divergence), default, theme),
    }
}

/// `↑ahead ↓behind` against `against`, in the feed's colours: behind is the
/// warning, ahead is plain. Both always show, so a branch level with its base
/// reads `↑0 ↓0` rather than nothing. Unknown — a cross-fork head, or not
/// fetched yet — is a `?`, never an error.
fn divergence_chips(
    ix: usize,
    drift: Option<Divergence>,
    against: &str,
    theme: &Theme,
) -> Vec<AnyElement> {
    let Some(drift) = drift else {
        return vec![
            Chip::new("↑? ↓?")
                .color(theme.text_subtle)
                .tooltip(
                    ("drift-unknown", ix),
                    format!(
                        "Distance from {against} unknown: a head in a fork, or not fetched yet"
                    ),
                )
                .into_any_element(),
        ];
    };
    let behind_color: Hsla = if drift.is_behind() {
        theme.warning
    } else {
        theme.text_subtle
    };
    vec![
        Chip::new(format!("↑{}", drift.ahead))
            .color(theme.text_muted)
            .tooltip(
                ("drift-ahead", ix),
                format!("{} commit(s) not on {against}", drift.ahead),
            )
            .into_any_element(),
        Chip::new(format!("↓{}", drift.behind))
            .color(behind_color)
            .tooltip(
                ("drift-behind", ix),
                format!("{} commit(s) on {against} not here", drift.behind),
            )
            .into_any_element(),
    ]
}

impl Render for BranchesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let prs: Vec<PullRequest> = self
            .store
            .read(cx)
            .state
            .repo(&self.repo)
            .map(|repo| repo.prs.clone())
            .unwrap_or_default();
        let branches = self.branches.read(cx);
        let status = branches.status().clone();
        let snapshot = branches.snapshot().cloned();
        let tree = branches.tree(&prs);

        let body: AnyElement = match (&snapshot, tree) {
            (Some(snapshot), Some(tree)) => {
                let default = match &snapshot.branches {
                    Branches::Ready { trunks, .. } => trunks.default_branch().to_string(),
                    Branches::Empty => String::new(),
                };
                let rows = tree.rows();
                v_flex()
                    .pb_4()
                    .children(
                        rows.into_iter()
                            .enumerate()
                            .map(|(ix, row)| self.render_row(ix, row, &prs, &default, &theme, cx)),
                    )
                    .into_any_element()
            }
            (Some(_), None) => notice("This repository has no branches yet", &theme),
            (None, _) => match &status {
                FetchStatus::Failed(message) => notice(message, &theme),
                FetchStatus::Fetching | FetchStatus::Idle => notice("Loading branches…", &theme),
            },
        };

        let status_line = match &status {
            FetchStatus::Fetching if snapshot.is_some() => Some(("refreshing…", theme.text_subtle)),
            FetchStatus::Failed(_) if snapshot.is_some() => {
                Some(("refresh failed; showing the last answer", theme.danger))
            }
            _ => None,
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .flex_none()
                    .gap_3()
                    .p_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_size(rems(1.05))
                            .text_color(theme.text)
                            .child("Branches"),
                    )
                    .child(
                        div()
                            .text_size(rems(0.74))
                            .text_color(theme.text_subtle)
                            .child("ahead/behind: trunks against the default branch, pull requests against their base"),
                    )
                    .child(div().flex_1())
                    .when_some(status_line, |el, (text, color)| {
                        el.child(div().text_size(rems(0.72)).text_color(color).child(text))
                    })
                    .child(
                        Button::new("branches-refresh", "Refresh").on_click(cx.listener(
                            |this, _, _window, cx| {
                                this.branches.update(cx, |branches, cx| branches.fetch(cx))
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .id("branches-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

fn notice(message: &str, theme: &Theme) -> AnyElement {
    div()
        .p_4()
        .text_size(rems(0.85))
        .text_color(theme.text_subtle)
        .child(message.to_string())
        .into_any_element()
}
