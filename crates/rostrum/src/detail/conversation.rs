//! The Conversation tab: body, comments, reviews, inline threads, and events.
//!
//! Rendered as a plain scrolling column rather than a virtualized list. A
//! pull request conversation is bounded in practice (GitHub itself paginates
//! at 100), and every row here contains variable-height rendered markdown, so
//! virtualizing would buy little for a lot of measurement machinery. The diff
//! view, where row counts genuinely run to thousands, does virtualize.

use chrono::{DateTime, Utc};
use gpui::{AnyElement, Context, div, prelude::*, px, rems};
use rostrum_core::{Conversation, ReviewState, ReviewThread, ThreadComment, TimelineItem, User};
use rostrum_ui::{
    ActiveTheme, Theme,
    components::{Button, ButtonStyle, Chip, Initial, h_flex, v_flex},
    markdown,
};

use crate::detail::{Loadable, PrDetail};

pub fn render(detail: &PrDetail, cx: &Context<PrDetail>) -> AnyElement {
    let theme = cx.theme().clone();

    let conversation = match &detail.conversation {
        Loadable::Idle | Loadable::Loading => {
            return centered("Loading conversation…", theme.text_subtle).into_any_element();
        }
        Loadable::Failed(message) => {
            return centered(message.clone(), theme.danger).into_any_element();
        }
        Loadable::Loaded(conversation) => conversation,
    };

    let owner = detail.repo.owner().to_string();
    let repo = detail.repo.name().to_string();

    div()
        .id("conversation")
        .size_full()
        .overflow_y_scroll()
        .p_4()
        .child(
            v_flex()
                .gap_3()
                .children(conversation.items.iter().enumerate().map(|(ix, item)| {
                    render_item(detail, conversation, item, ix, &owner, &repo, &theme, cx)
                })),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_item(
    detail: &PrDetail,
    conversation: &Conversation,
    item: &TimelineItem,
    ix: usize,
    owner: &str,
    repo: &str,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> AnyElement {
    match item {
        TimelineItem::Body {
            author,
            body,
            created_at,
        } => card(
            theme,
            author.as_ref(),
            *created_at,
            Some("description"),
            None,
            body,
            ix,
            owner,
            repo,
            cx,
        ),
        TimelineItem::Comment {
            author,
            body,
            created_at,
            ..
        } => card(
            theme,
            author.as_ref(),
            *created_at,
            None,
            None,
            body,
            ix,
            owner,
            repo,
            cx,
        ),
        TimelineItem::Review {
            author,
            state,
            body,
            created_at,
            thread_ids,
            ..
        } => {
            let (label, color) = review_state_chip(*state, theme);
            let threads: Vec<&ReviewThread> = thread_ids
                .iter()
                .filter_map(|id| conversation.thread(id))
                .collect();

            v_flex()
                .gap_2()
                .child(card(
                    theme,
                    author.as_ref(),
                    *created_at,
                    Some(label),
                    Some(color),
                    body,
                    ix,
                    owner,
                    repo,
                    cx,
                ))
                .children(threads.into_iter().enumerate().map(|(tx, thread)| {
                    render_thread(detail, thread, ix * 1000 + tx, owner, repo, theme, cx)
                }))
                .into_any_element()
        }
        TimelineItem::Event {
            kind,
            actor,
            created_at,
        } => h_flex()
            .gap_2()
            .pl_2()
            .text_size(rems(0.74))
            .text_color(theme.text_subtle)
            .when_some(actor.as_ref(), |el, user| {
                el.child(Initial::new(user.login.clone()))
                    .child(user.login.clone())
            })
            .child(event_text(kind))
            .child(relative_time(*created_at))
            .into_any_element(),
    }
}

#[allow(clippy::too_many_arguments)]
fn card(
    theme: &Theme,
    author: Option<&User>,
    created_at: DateTime<Utc>,
    badge: Option<&str>,
    badge_color: Option<gpui::Hsla>,
    body: &str,
    id_seed: usize,
    owner: &str,
    repo: &str,
    cx: &Context<PrDetail>,
) -> AnyElement {
    let login = author.map(|a| a.login.clone());

    v_flex()
        .rounded_tl(px(6.))
        .rounded_tr(px(6.))
        .rounded_bl(px(6.))
        .rounded_br(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            h_flex()
                .gap_2()
                .px_3()
                .py_2()
                .bg(theme.surface_raised)
                .text_size(rems(0.76))
                .text_color(theme.text_muted)
                .when_some(login.clone(), |el, login| {
                    el.child(Initial::new(login.clone())).child(login)
                })
                .when_some(badge, |el, badge| {
                    el.child(
                        Chip::new(badge.to_string()).color(badge_color.unwrap_or(theme.text_muted)),
                    )
                })
                .child(div().flex_1())
                .child(relative_time(created_at)),
        )
        .child(div().px_3().py_2().child(if body.trim().is_empty() {
            div()
                .text_size(rems(0.78))
                .text_color(theme.text_subtle)
                .child("No description provided")
                .into_any_element()
        } else {
            markdown::render_github(body, owner, repo, id_seed, theme, cx)
        }))
        .into_any_element()
}

fn render_thread(
    detail: &PrDetail,
    thread: &ReviewThread,
    id_seed: usize,
    owner: &str,
    repo: &str,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> AnyElement {
    let location = match thread.line {
        Some(line) => format!("{}:{}", thread.path, line),
        None => format!("{} (outdated)", thread.path),
    };
    // Replies attach to the thread's first comment, which is the id GitHub's
    // reply endpoint expects.
    let reply_target = thread.comments.first().and_then(|c| c.database_id);

    v_flex()
        .ml_4()
        .gap_1()
        .rounded_tl(px(6.))
        .rounded_tr(px(6.))
        .rounded_bl(px(6.))
        .rounded_br(px(6.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            h_flex()
                .gap_2()
                .px_3()
                .py_1p5()
                .text_size(rems(0.72))
                .text_color(theme.text_subtle)
                .font_family(theme.mono_font.clone())
                .child(location)
                .child(div().flex_1())
                .when(thread.is_resolved, |el| {
                    el.child(Chip::new("resolved").color(theme.success))
                })
                .when(thread.is_outdated, |el| {
                    el.child(Chip::new("outdated").color(theme.warning))
                }),
        )
        .children(thread.comments.iter().enumerate().map(|(cx_ix, comment)| {
            render_thread_comment(comment, id_seed * 100 + cx_ix, owner, repo, theme, cx)
        }))
        .child(render_reply(detail, reply_target, theme, cx))
        .into_any_element()
}

fn render_thread_comment(
    comment: &ThreadComment,
    id_seed: usize,
    owner: &str,
    repo: &str,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> AnyElement {
    v_flex()
        .px_3()
        .py_2()
        .gap_1()
        .border_t_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .gap_2()
                .text_size(rems(0.72))
                .text_color(theme.text_muted)
                .when_some(comment.author.as_ref(), |el, user| {
                    el.child(Initial::new(user.login.clone()))
                        .child(user.login.clone())
                })
                .child(div().flex_1())
                .child(relative_time(comment.created_at)),
        )
        .child(markdown::render_github(
            &comment.body,
            owner,
            repo,
            id_seed,
            theme,
            cx,
        ))
        .into_any_element()
}

/// Reply composer for a thread, shown only for the thread being replied to.
fn render_reply(
    detail: &PrDetail,
    target: Option<u64>,
    theme: &Theme,
    cx: &Context<PrDetail>,
) -> AnyElement {
    let Some(target) = target else {
        return div().into_any_element();
    };

    match detail.reply.as_ref() {
        Some((active, input)) if *active == target => v_flex()
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(input.clone())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(("send-reply", target as usize), "Reply")
                            .style(ButtonStyle::Primary)
                            .on_click(PrDetail::on_click(cx, move |this, cx| {
                                let body = this
                                    .reply
                                    .as_ref()
                                    .map(|(_, input)| input.read(cx).text().to_string())
                                    .unwrap_or_default();
                                this.reply = None;
                                this.reply_to_thread(target, body, cx);
                            })),
                    )
                    .child(
                        Button::new(("cancel-reply", target as usize), "Cancel").on_click(
                            PrDetail::on_click(cx, |this, cx| {
                                this.reply = None;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .into_any_element(),
        _ => div()
            .px_3()
            .py_1p5()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Button::new(("open-reply", target as usize), "Reply")
                    .on_click(PrDetail::on_click(cx, move |this, cx| {
                        this.open_reply(target, cx)
                    })),
            )
            .into_any_element(),
    }
}

fn review_state_chip(state: ReviewState, theme: &Theme) -> (&'static str, gpui::Hsla) {
    match state {
        ReviewState::Approved => ("approved", theme.success),
        ReviewState::ChangesRequested => ("requested changes", theme.danger),
        ReviewState::Commented => ("reviewed", theme.text_muted),
        ReviewState::Dismissed => ("dismissed", theme.text_subtle),
        ReviewState::Pending => ("pending", theme.warning),
    }
}

fn event_text(kind: &rostrum_core::EventKind) -> String {
    use rostrum_core::EventKind as E;
    match kind {
        E::Merged => "merged this".into(),
        E::Closed => "closed this".into(),
        E::Reopened => "reopened this".into(),
        E::ReadyForReview => "marked ready for review".into(),
        E::ConvertedToDraft => "converted to draft".into(),
        E::HeadRefForcePushed => "force-pushed".into(),
        E::ReviewRequested { reviewer } => format!("requested a review from {reviewer}"),
        E::Assigned { assignee } => format!("assigned {assignee}"),
        E::Labeled { name } => format!("added the {name} label"),
        E::Unlabeled { name } => format!("removed the {name} label"),
        E::Renamed { from, to } => format!("renamed this from “{from}” to “{to}”"),
        E::Other(kind) => kind.clone(),
    }
}

fn centered(message: impl Into<String>, color: gpui::Hsla) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(rems(0.82))
        .text_color(color)
        .child(message.into())
}

fn relative_time(then: DateTime<Utc>) -> String {
    crate::feed::relative_time(then)
}
