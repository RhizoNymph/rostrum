//! The custom element that shapes and paints a [`TextInput`] and installs the
//! platform IME handler.

use gpui::{
    App, Bounds, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId, PaintQuad,
    Pixels, Point, SharedString, Style, TextRun, UnderlineStyle, Window, WrappedLine, fill, point,
    prelude::*, px, relative, size,
};

use super::{
    TextInput,
    geometry::{LayoutSource, TextLayoutCache, line_starts, position_for_offset, run_lengths},
};
use crate::theme::ActiveTheme;

/// Custom element: shapes the text, paints selection, caret, and glyphs, and
/// installs the platform IME handler.
pub(super) struct TextElement {
    pub input: Entity<TextInput>,
}

pub(super) struct Prepaint {
    lines: Vec<WrappedLine>,
    starts: Vec<usize>,
    source: LayoutSource,
    quads: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    line_height: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();

        let input = self.input.clone();
        let line_height = window.line_height();
        let layout_id =
            window.request_measured_layout(style, move |known, _available, window, cx| {
                let width = known.width.unwrap_or(px(400.));
                let (min, max) = {
                    let input = input.read(cx);
                    (input.min_lines, input.max_lines)
                };
                let rows = shape(&input, width, window, cx)
                    .map(|Shaped { lines, .. }| {
                        lines
                            .iter()
                            .map(|line| line.wrap_boundaries().len() + 1)
                            .sum::<usize>()
                    })
                    .unwrap_or(min);
                size(width, line_height * rows.clamp(min, max) as f32)
            });

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let line_height = window.line_height();
        let Some(Shaped {
            lines,
            starts,
            source,
        }) = shape(&self.input, bounds.size.width, window, cx)
        else {
            return Prepaint {
                lines: Vec::new(),
                starts: Vec::new(),
                source: LayoutSource::Placeholder,
                quads: Vec::new(),
                caret: None,
                line_height,
            };
        };

        let input = self.input.read(cx);
        let theme = cx.theme();
        // Positions are computed against the freshly shaped lines rather than
        // the cached ones, which are only updated at paint time.
        let locate = |offset: usize| -> Option<Point<Pixels>> {
            position_for_offset(&lines, &starts, line_height, offset)
        };
        let selected = input.buffer.selected_range();

        let mut quads = Vec::new();
        if !selected.is_empty()
            && let (Some(start), Some(end)) = (locate(selected.start), locate(selected.end))
        {
            let color = gpui::Hsla {
                a: 0.30,
                ..theme.accent
            };
            if (start.y - end.y).abs() < px(0.5) {
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + start,
                        bounds.origin + point(end.x, end.y + line_height),
                    ),
                    color,
                ));
            } else {
                // First row runs to the right edge, whole rows between are
                // full width, and the last row starts at the left edge.
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + start,
                        bounds.origin + point(bounds.size.width, start.y + line_height),
                    ),
                    color,
                ));
                let mut y = start.y + line_height;
                while y < end.y - px(0.5) {
                    quads.push(fill(
                        Bounds::from_corners(
                            bounds.origin + point(px(0.), y),
                            bounds.origin + point(bounds.size.width, y + line_height),
                        ),
                        color,
                    ));
                    y += line_height;
                }
                quads.push(fill(
                    Bounds::from_corners(
                        bounds.origin + point(px(0.), end.y),
                        bounds.origin + point(end.x, end.y + line_height),
                    ),
                    color,
                ));
            }
        }

        let caret = selected
            .is_empty()
            .then(|| locate(input.buffer.cursor()))
            .flatten()
            .map(|position| {
                fill(
                    Bounds::new(bounds.origin + position, size(px(1.5), line_height)),
                    theme.accent,
                )
            });

        Prepaint {
            lines,
            starts,
            source,
            quads,
            caret,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        for quad in prepaint.quads.drain(..) {
            window.paint_quad(quad);
        }

        let mut origin = bounds.origin;
        for line in &prepaint.lines {
            if let Err(error) = line.paint(
                origin,
                prepaint.line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            ) {
                tracing::warn!(%error, "failed to paint input line");
            }
            origin.y += line.size(prepaint.line_height).height;
        }

        if focus_handle.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }

        let lines = std::mem::take(&mut prepaint.lines);
        let starts = std::mem::take(&mut prepaint.starts);
        let line_height = prepaint.line_height;
        let source = prepaint.source;
        self.input.update(cx, |input, _cx| {
            input.layout = Some(TextLayoutCache {
                lines,
                starts,
                line_height,
                source,
            });
            input.bounds = Some(bounds);
        });
    }
}

/// The shaped text of an input and what it was shaped from.
struct Shaped {
    lines: Vec<WrappedLine>,
    /// Byte offset at which each line begins.
    starts: Vec<usize>,
    source: LayoutSource,
}

/// Shape the input's text, or its placeholder when the text is empty.
fn shape(
    input: &Entity<TextInput>,
    width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Option<Shaped> {
    let input = input.read(cx);
    let style = window.text_style();
    let showing_placeholder = input.buffer.is_empty();
    let (display, source, marked): (SharedString, _, _) = if showing_placeholder {
        (input.placeholder.clone(), LayoutSource::Placeholder, None)
    } else {
        (
            input.buffer.text().into(),
            LayoutSource::Content {
                revision: input.buffer.revision(),
            },
            input.buffer.marked_range(),
        )
    };

    let color = if showing_placeholder {
        cx.theme().text_subtle
    } else {
        style.color
    };

    // Underline the IME pre-edit region so composition is visible.
    let base = TextRun {
        len: 0,
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs: Vec<TextRun> = run_lengths(display.len(), marked)
        .into_iter()
        .map(|(len, underlined)| TextRun {
            len,
            underline: underlined.then_some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.),
                wavy: false,
            }),
            ..base.clone()
        })
        .collect();

    let font_size = style.font_size.to_pixels(window.rem_size());
    let lines = window
        .text_system()
        .shape_text(display.clone(), font_size, &runs, Some(width), None)
        .ok()?;

    Some(Shaped {
        lines: lines.into_iter().collect(),
        starts: line_starts(&display),
        source,
    })
}
