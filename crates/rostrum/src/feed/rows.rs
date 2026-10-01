//! Row renderers for the feed: one per [`FeedRow`] variant, plus the
//! container chrome each row draws its share of.

use chrono::{DateTime, Utc};
use gpui::{AnyElement, App, Context, Div, div, prelude::*, px, rems};
use rostrum_core::{
    Chrome, FeedRow, FeedTab, IssueIx, MergeStatus, PrIx, RepoIx, RepoState, ReviewDecision,
    Selection,
};
use rostrum_local::LocalResult;
use rostrum_ui::{
    ActiveTheme,
    components::{Chip, DiffStat, Dot, Initial, h_flex, hex_color, v_flex},
};

use super::{FeedEvent, FeedView, ROW_RADIUS};

impl FeedView {
    pub(super) fn render_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.feed.row(ix) else {
            return div().into_any_element();
        };
        tracing::trace!(ix, ?row, "render row");
        let chrome = self.feed.chrome(ix);

        let tab = self.feed.tab();
        match row {
            FeedRow::Spacer { .. } => div().h(px(10.)).into_any_element(),
            FeedRow::RepoHeader { repo } => self.render_repo_header(repo, chrome, cx),
            FeedRow::PrRow { repo, pr } => self.render_pr_row(repo, pr, chrome, ix, cx),
            FeedRow::IssueRow { repo, issue } => self.render_issue_row(repo, issue, chrome, ix, cx),
            FeedRow::RepoEmpty { repo } => self.render_notice(
                repo,
                chrome,
                match tab {
                    FeedTab::PullRequests => "No open pull requests",
                    FeedTab::Issues => "No open issues",
                },
                cx,
            ),
            FeedRow::RepoLoading { repo } => self.render_notice(repo, chrome, "Loading…", cx),
            FeedRow::RepoError { repo } => {
                let message = self
                    .repo_state(repo, cx)
                    .and_then(|r| {
                        let load = match tab {
                            FeedTab::PullRequests => &r.load,
                            FeedTab::Issues => &r.issues_load,
                        };
                        load.error_message().map(str::to_string)
                    })
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

        let tab = self.feed.tab();
        let name = state.id.to_string();
        let (count, failed) = match tab {
            FeedTab::PullRequests => (state.prs.len(), state.load.is_failed()),
            FeedTab::Issues => (state.issues.len(), state.issues_load.is_failed()),
        };
        let collapsed = state.collapsed;
        let id = state.id.clone();
        let new_issue_repo = state.id.clone();
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
                    })
                    // Creating an issue starts from the repository it goes
                    // in, so the Issues tab offers it on every header.
                    .when(tab == FeedTab::Issues, |el| {
                        el.child(div().flex_1()).child(
                            div()
                                .id(("new-issue", repo.0))
                                .px_1p5()
                                .text_size(rems(0.72))
                                .text_color(cx.theme().text_subtle)
                                .cursor_pointer()
                                .hover(|el| el.text_color(cx.theme().accent))
                                .child("+ New issue")
                                .on_click(cx.listener(move |_, _, _window, cx| {
                                    // The header itself toggles collapse; this
                                    // click is not that.
                                    cx.stop_propagation();
                                    cx.emit(FeedEvent::NewIssue {
                                        repo: Some(new_issue_repo.clone()),
                                    });
                                })),
                        )
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

        let selected = self.store.read(cx).state.selection
            == Some(Selection::PullRequest {
                repo: state.id.clone(),
                number: pull.number,
            });

        let number = pull.number.to_string();
        let title = pull.title.clone();
        let author = pull.author.as_ref().map(|a| a.login.clone());
        let updated = relative_time(pull.updated_at);
        let is_draft = pull.is_draft;
        let additions = pull.additions;
        let deletions = pull.deletions;
        let checks = pull.checks;
        let decision = pull.review_decision;
        let merge = pull.merge_status();
        let base_ref = pull.base_ref.clone();
        // The exact count from the divergence batch, once it has answered.
        // Only "behind" earns a chip: ahead is the normal state of a pull
        // request and says nothing the reviewer must act on.
        let behind = pull
            .base_divergence
            .filter(|d| d.is_behind())
            .map(|d| d.behind);
        // The merge chip's own "behind" carries less than the count, so it
        // yields to the count when both are known; every other merge chip
        // says something the count does not.
        let merge_chip = merge
            .chip()
            .filter(|_| behind.is_none() || merge != MergeStatus::Behind);
        let labels: Vec<_> = pull
            .labels
            .iter()
            .take(3)
            .map(|l| (l.name.clone(), hex_color(&l.color)))
            .collect();
        // The latest "sync all" verdict, when it is one worth a chip.
        let sync_chip = self
            .store
            .read(cx)
            .sync_result(&state.id, pull.number)
            .and_then(|result| {
                result
                    .chip()
                    .map(|text| (text, result.detail(), sync_chip_color(result)))
            });

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
                            .when_some(behind, |el, behind| {
                                el.child(
                                    Chip::new(format!("↓{behind}"))
                                        .color(theme.warning)
                                        // A distinct tag from the merge chip's:
                                        // GPUI element ids must be unique
                                        // within the row, and both can render.
                                        .tooltip(
                                            ("behind-count", ix),
                                            format!("{behind} commit(s) behind {base_ref}"),
                                        ),
                                )
                            })
                            // `chip` returns nothing for draft and unstable:
                            // the draft chip beside this one and the check dot
                            // at the head of the row already say both.
                            .when_some(merge_chip, |el, text| {
                                el.child(
                                    Chip::new(text)
                                        .color(theme.merge_color(merge))
                                        .tooltip(("merge-status", ix), merge.explanation()),
                                )
                            })
                            .when_some(review_label(decision), |el, (text, color)| {
                                el.child(Chip::new(text).color(color(&theme)))
                            })
                            .when_some(sync_chip, |el, (text, detail, color)| {
                                el.child(
                                    Chip::new(text)
                                        .color(color(&theme))
                                        // Its own tag: the merge and behind
                                        // chips can share the row, and GPUI
                                        // element ids must not collide.
                                        .tooltip(("sync-result", ix), detail),
                                )
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
            .on_click(
                cx.listener(move |this, _, _window, cx| {
                    this.select(FeedRow::PrRow { repo, pr }, cx)
                }),
            )
            .into_any_element()
    }

    /// One issue: state dot, number, title, labels; then author, age,
    /// assignees, milestone and comment count.
    fn render_issue_row(
        &mut self,
        repo: RepoIx,
        issue_ix: IssueIx,
        chrome: Chrome,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.repo_state(repo, cx) else {
            return div().into_any_element();
        };
        let Some(issue) = state.issues.get(issue_ix.0) else {
            return div().into_any_element();
        };

        let selected = self.store.read(cx).state.selection
            == Some(Selection::Issue {
                repo: state.id.clone(),
                number: issue.number,
            });
        let number = issue.number.to_string();
        let title = issue.title.clone();
        let author = issue.author.as_ref().map(|a| a.login.clone());
        let updated = relative_time(issue.updated_at);
        let assignees: Vec<String> = issue
            .assignees
            .iter()
            .take(3)
            .map(|user| user.login.clone())
            .collect();
        let milestone = issue.milestone.as_ref().map(|m| m.title.clone());
        let comments = issue.comment_count;
        let labels: Vec<_> = issue
            .labels
            .iter()
            .take(3)
            .map(|l| (l.name.clone(), hex_color(&l.color)))
            .collect();

        let theme = cx.theme().clone();

        card(chrome, cx)
            .id(("issue", ix))
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
                            // Every listed issue is open; green is GitHub's
                            // colour for that.
                            .child(Dot::new(theme.success))
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
                                    .text_color(theme.text)
                                    .text_size(rems(0.82))
                                    .child(title),
                            )
                            .when(comments > 0, |el| {
                                el.child(
                                    div()
                                        .text_color(theme.text_subtle)
                                        .text_size(rems(0.7))
                                        .child(format!("💬 {comments}")),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .pl(px(15.))
                            .flex_wrap()
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
                            .when(!assignees.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_color(theme.text_subtle)
                                        .text_size(rems(0.72))
                                        .child(format!("→ {}", assignees.join(", "))),
                                )
                            })
                            .when_some(milestone, |el, title| {
                                el.child(Chip::new(format!("◷ {title}")).color(theme.text_muted))
                            })
                            .children(labels.into_iter().map(|(name, color)| {
                                let chip = Chip::new(name);
                                match color {
                                    Some(color) => chip.color(color),
                                    None => chip,
                                }
                            })),
                    ),
            )
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.select(
                    FeedRow::IssueRow {
                        repo,
                        issue: issue_ix,
                    },
                    cx,
                )
            }))
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

/// Severity colour for a sync verdict chip: red for what stopped, accent for
/// what was handed on, amber for what git would not start.
fn sync_chip_color(result: &LocalResult) -> ThemeColor {
    match result {
        LocalResult::Conflicted(_) | LocalResult::Failed(_) => |t| t.danger,
        LocalResult::HandedOff { .. } => |t| t.accent,
        LocalResult::Refused(_)
        | LocalResult::NotCheckedOut
        | LocalResult::UpToDate
        | LocalResult::Completed => |t| t.warning,
    }
}

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
}
