//! The feed header's **Sort** button and its popover.
//!
//! Two sections, one per sort: the keys valid for that sort as a radio-like
//! row of buttons, and a button naming the current direction that reverses
//! it. The item sort is shared by both tabs, so its section is labelled for
//! pull requests and issues alike. Every change goes through a `Store` setter, which re-flattens the feed
//! through the ordinary store-changed path and writes the config — the same
//! funnel every other feed preference uses.
//!
//! Nothing here adds a key context or takes focus, so `j`/`k` keep resolving
//! in the feed's row area and typing keeps resolving in the filter box, as
//! with the authors and repos popovers beside it.

use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px, rems};
use rostrum_core::{ItemSortKey, RepoSortKey, Sort, SortKey};
use rostrum_ui::{
    ActiveTheme, Popover,
    components::{Button, ButtonStyle, h_flex, v_flex},
};

use super::{FeedView, HeaderPopover};
use crate::sync::Store;

/// What a section does when a key is chosen or the direction reversed, and
/// what a key's button says when hovered.
struct SectionActions<K> {
    choose: fn(&mut Store, K, &mut Context<Store>),
    reverse: fn(&mut Store, &mut Context<Store>),
    hint: fn(K) -> Option<&'static str>,
}

/// Issues have no branch, so "pushed" orders them by their last update; the
/// key's button says so rather than leaving the Issues tab to look unsorted.
fn item_key_hint(key: ItemSortKey) -> Option<&'static str> {
    match key {
        ItemSortKey::Pushed => Some("Issues have no branch: they sort by updated"),
        _ => None,
    }
}

impl FeedView {
    /// The Sort button, labelled with both current sorts, and its popover.
    pub(super) fn sort_button(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.popover == Some(HeaderPopover::Sort);
        let sort = self.store.read(cx).state.filter.sort;
        let content = if open {
            Some(self.render_sort_popover(cx))
        } else {
            None
        };
        Popover::new(
            "sort-popover",
            self.sort_anchor.clone(),
            Button::new("sort-button", sort.summary())
                .style(if open {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Subtle
                })
                .tooltip("Sort repositories, pull requests and issues")
                .on_click(
                    cx.listener(|this, _, _window, cx| {
                        this.toggle_popover(HeaderPopover::Sort, cx)
                    }),
                ),
        )
        .open(content)
        .on_dismiss(cx.listener(|this, _, _window, cx| this.close_popover(cx)))
    }

    fn render_sort_popover(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let sort = self.store.read(cx).state.filter.sort;
        let divider = div().h(px(1.)).my_1().bg(cx.theme().border);
        v_flex()
            .gap_1p5()
            .child(self.render_sort_section(
                "Repositories",
                "sort-repos",
                sort.repos,
                SectionActions::<RepoSortKey> {
                    choose: Store::choose_repo_sort,
                    reverse: Store::reverse_repo_sort,
                    hint: |_| None,
                },
                cx,
            ))
            .child(divider)
            .child(self.render_sort_section(
                "Pull requests & issues",
                "sort-items",
                sort.items,
                SectionActions::<ItemSortKey> {
                    choose: Store::choose_item_sort,
                    reverse: Store::reverse_item_sort,
                    hint: item_key_hint,
                },
                cx,
            ))
            .into_any_element()
    }

    /// One sort's section: a heading, the keys valid for it, and the
    /// direction toggle.
    fn render_sort_section<K: SortKey>(
        &mut self,
        title: &'static str,
        id: &'static str,
        current: Sort<K>,
        actions: SectionActions<K>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let SectionActions {
            choose,
            reverse,
            hint,
        } = actions;

        let keys = K::ALL.iter().enumerate().map(|(ix, &key)| {
            let selected = key == current.key();
            Button::new(SharedString::from(format!("{id}-key-{ix}")), key.label())
                .style(if selected {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Subtle
                })
                .when_some(hint(key), |button, hint| button.tooltip(hint))
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.store.update(cx, |store, cx| choose(store, key, cx));
                }))
        });

        v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(rems(0.72))
                    .text_color(theme.text_subtle)
                    .child(title),
            )
            .child(h_flex().gap_1().flex_wrap().children(keys))
            .child(
                h_flex().gap_2().child(
                    Button::new(
                        SharedString::from(format!("{id}-direction")),
                        format!("{} \u{21c5}", current.direction_label()),
                    )
                    .tooltip("Reverse the order")
                    .on_click(cx.listener(
                        move |this, _, _window: &mut Window, cx| {
                            this.store.update(cx, reverse);
                        },
                    )),
                ),
            )
    }
}
