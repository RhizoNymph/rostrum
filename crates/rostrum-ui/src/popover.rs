//! A button-anchored floating panel.
//!
//! The panel's top-left corner sits on the centre of its trigger, and it floats
//! over whatever is below rather than pushing the layout down. It closes when
//! the user presses outside both the panel and the trigger; the owner decides
//! what `escape` does, since key contexts belong to the view.
//!
//! Placement is done by layout, not by measuring: the trigger is wrapped in a
//! `relative` box, and a zero-size `absolute` box at `left: 50%, top: 50%` of
//! it is where [`anchored`] starts. [`deferred`] paints the panel after every
//! sibling so nothing drawn later covers it.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, ElementId, MouseDownEvent, Pixels, Point, Window, anchored, canvas,
    deferred, div, prelude::*, px, relative,
};

use crate::theme::ActiveTheme;

/// Where the trigger was last painted.
///
/// Owned by the view and handed to [`Popover`] each frame, because a press on
/// the trigger of an open popover must *not* count as a press outside — the
/// trigger's own click is what closes it, and treating the press as outside
/// too would close it on mouse-down and reopen it on the click.
#[derive(Clone, Default)]
pub struct PopoverAnchor(Rc<Cell<Option<Bounds<Pixels>>>>);

impl PopoverAnchor {
    fn record(&self, bounds: Bounds<Pixels>) {
        self.0.set(Some(bounds));
    }

    fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.get()
    }
}

/// Whether a press at `point` outside the panel should close it.
///
/// A press on the trigger is left to the trigger. Before the trigger has been
/// painted there is nothing to exclude.
pub fn press_dismisses(trigger: Option<Bounds<Pixels>>, point: Point<Pixels>) -> bool {
    !trigger.is_some_and(|bounds| bounds.contains(&point))
}

type DismissHandler = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    anchor: PopoverAnchor,
    trigger: AnyElement,
    content: Option<AnyElement>,
    on_dismiss: Option<DismissHandler>,
}

impl Popover {
    /// `trigger` is always drawn; the panel only when [`Popover::open`] was
    /// given content.
    pub fn new(id: impl Into<ElementId>, anchor: PopoverAnchor, trigger: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            anchor,
            trigger: trigger.into_any_element(),
            content: None,
            on_dismiss: None,
        }
    }

    /// Show the panel with `content`, or keep it closed with `None`.
    pub fn open(mut self, content: Option<impl IntoElement>) -> Self {
        self.content = content.map(IntoElement::into_any_element);
        self
    }

    /// Called on a press outside both the panel and the trigger.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let recorder = self.anchor.clone();
        let anchor = self.anchor;
        let on_dismiss = self.on_dismiss;

        div()
            .relative()
            .flex_none()
            .child(self.trigger)
            .child(
                canvas(
                    move |bounds, _window, _cx| recorder.record(bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(self.content, |el, content| {
                el.child(
                    div()
                        .absolute()
                        .left(relative(0.5))
                        .top(relative(0.5))
                        .child(deferred(
                            anchored().snap_to_window_with_margin(px(8.)).child(
                                div()
                                    .id(self.id)
                                    .occlude()
                                    .min_w(px(240.))
                                    .max_w(px(360.))
                                    .max_h(px(420.))
                                    .overflow_y_scroll()
                                    .p_2()
                                    .rounded_tl(px(6.))
                                    .rounded_tr(px(6.))
                                    .rounded_bl(px(6.))
                                    .rounded_br(px(6.))
                                    .border_1()
                                    .border_color(theme.border_strong)
                                    .bg(theme.surface_raised)
                                    .shadow_lg()
                                    .on_mouse_down_out(move |event, window, cx| {
                                        if press_dismisses(anchor.bounds(), event.position)
                                            && let Some(dismiss) = &on_dismiss
                                        {
                                            dismiss(event, window, cx);
                                        }
                                    })
                                    .child(content),
                            ),
                        )),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, size};

    use super::*;

    fn trigger() -> Bounds<Pixels> {
        Bounds::new(point(px(100.), px(20.)), size(px(80.), px(24.)))
    }

    #[test]
    fn a_press_on_the_trigger_is_left_to_the_trigger() {
        assert!(!press_dismisses(Some(trigger()), point(px(140.), px(32.))));
        assert!(!press_dismisses(Some(trigger()), point(px(100.), px(20.))));
    }

    #[test]
    fn a_press_anywhere_else_dismisses() {
        assert!(press_dismisses(Some(trigger()), point(px(10.), px(10.))));
        assert!(press_dismisses(Some(trigger()), point(px(181.), px(32.))));
        assert!(press_dismisses(Some(trigger()), point(px(140.), px(45.))));
    }

    #[test]
    fn before_the_trigger_is_painted_every_press_dismisses() {
        assert!(press_dismisses(None, point(px(140.), px(32.))));
    }

    #[test]
    fn the_anchor_remembers_the_last_painted_bounds() {
        let anchor = PopoverAnchor::default();
        assert_eq!(anchor.bounds(), None);
        anchor.record(trigger());
        assert_eq!(anchor.bounds(), Some(trigger()));
    }
}
