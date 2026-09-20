//! What the arrangement and the note editor paint the same way: text, the bar ruler, the
//! focus ring and the playhead line.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, Entity, FocusHandle, FontWeight, Hsla, Pixels,
    Point, SharedString, TextAlign, TextRun, TruncateFrom, Window, canvas, fill, point, prelude::*,
    px, quad, size,
};
use sound_ui::{ActiveTheme, Playhead, Theme, typography};

use super::layout::{HEADER_WIDTH, RULER_HEIGHT, Rect, Viewport};
use crate::Colour;

/// Track colours are design tokens. The match is exhaustive, so a new colour cannot be
/// left without one.
pub(super) fn accent(colour: Colour, theme: &Theme) -> Hsla {
    match colour {
        Colour::Blue => theme.blue,
        Colour::Sapphire => theme.sapphire,
        Colour::Sky => theme.sky,
        Colour::Teal => theme.teal,
        Colour::Green => theme.green,
        Colour::Yellow => theme.yellow,
        Colour::Peach => theme.peach,
        Colour::Red => theme.red,
        Colour::Maroon => theme.maroon,
        Colour::Mauve => theme.mauve,
        Colour::Pink => theme.pink,
        Colour::Lavender => theme.lavender,
        Colour::Rosewater => theme.rosewater,
        Colour::Flamingo => theme.flamingo,
    }
}

/// A rect of [`super::layout`] in window coordinates, on whole pixels so that edges stay sharp.
pub(super) fn placed(rect: Rect, origin: Point<Pixels>) -> Bounds<Pixels> {
    let (left, top) = (rect.x.round(), rect.y.round());
    let right = (rect.x + rect.width).round().max(left + 1.0);
    let bottom = (rect.y + rect.height).round().max(top + 1.0);
    Bounds::new(
        origin + point(px(left), px(top)),
        size(px(right - left), px(bottom - top)),
    )
}

/// How a label that does not fit is handled.
pub(super) enum Fit {
    /// Ends in an ellipsis at this width.
    Truncate(f32),
    /// Not painted when it would cross this x. Half a number reads as another number.
    SkipPast(Pixels),
    /// Ends at the x of `origin` and is never cut: a short label left of something.
    AlignRight,
}

pub(super) fn paint_text(
    text: SharedString,
    origin: Point<Pixels>,
    font_size: f32,
    weight: FontWeight,
    color: Hsla,
    fit: Fit,
    window: &mut Window,
    cx: &mut App,
) {
    let mut font = typography::tabular();
    font.weight = weight;
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let (text, runs) = match fit {
        Fit::Truncate(width) => {
            let mut wrapper = window.text_system().line_wrapper(font, px(font_size));
            let runs = [run];
            let (text, runs) =
                wrapper.truncate_line(text, px(width), "…", &runs, TruncateFrom::End);
            (text, runs.into_owned())
        }
        Fit::SkipPast(_) | Fit::AlignRight => (text, vec![run]),
    };
    let line = window
        .text_system()
        .shape_line(text, px(font_size), &runs, None);
    if matches!(fit, Fit::SkipPast(right) if origin.x + line.width > right) {
        return;
    }
    let origin = match fit {
        Fit::AlignRight => origin - point(line.width, px(0.)),
        Fit::Truncate(_) | Fit::SkipPast(_) => origin,
    };
    let line_height = px((font_size * 1.4).round());
    // A glyph that cannot be painted leaves a gap in a label. Nothing else depends on it.
    if let Err(error) = line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
        eprintln!("arrangement view: {error}");
    }
}

/// The bar ruler: a short mark and a number per bar of [`Viewport::ruler_bars`].
pub(super) fn paint_ruler(
    bars: &[(u64, f32)],
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = cx.theme();
    let (mark_color, muted) = (theme.alpha_at(0.10), theme.gray_700);
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        for (bar, x) in bars {
            let x = px(x.round());
            let mark = Bounds::new(
                ruler.origin + point(x, px(RULER_HEIGHT - 8.)),
                size(px(1.), px(8.)),
            );
            window.paint_quad(fill(mark, mark_color));
            let label = ruler.origin + point(x + px(8.), px(8.));
            let number = bar.to_string().into();
            let fit = Fit::SkipPast(ruler.right());
            let weight = FontWeight::NORMAL;
            paint_text(number, label, 12., weight, muted, fit, window, cx);
        }
    });
}

/// The accent dot and the name of a track, as in a track header. `top` is the top left of a
/// row of `row_height`, and the name ends in an ellipsis at `name_width`.
pub(super) fn paint_track_label(
    name: SharedString,
    accent: Hsla,
    top: Point<Pixels>,
    row_height: f32,
    name_width: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let text = cx.theme().gray_900;
    let dot = Bounds::new(
        top + point(px(24.), px(row_height / 2. - 4.)),
        size(px(8.), px(8.)),
    );
    window.paint_quad(quad(
        dot,
        px(4.),
        accent,
        px(0.),
        accent,
        BorderStyle::Solid,
    ));
    let origin = top + point(px(44.), px(row_height / 2. - 10.));
    let fit = Fit::Truncate(name_width);
    let weight = FontWeight::MEDIUM;
    paint_text(name, origin, 14., weight, text, fit, window, cx);
}

/// A thin ring inside the view, only while it has the focus from the keyboard. A click
/// focuses the view too, and then the pointer already says where the composer is.
pub(super) fn paint_focus_ring(
    bounds: Bounds<Pixels>,
    focus_handle: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    if !focus_handle.is_focused(window) || !window.last_input_was_keyboard() {
        return;
    }
    let clear = Hsla::transparent_black();
    let ring = cx.theme().lavender;
    window.paint_quad(quad(
        bounds,
        px(6.),
        clear,
        px(1.),
        ring,
        BorderStyle::Solid,
    ));
}

/// The playhead: one line over the ruler and what is below it. It repaints on every playhead
/// change, so it reads nothing from the project. The arrangement and the note editor each
/// have one, over their own viewport.
pub(super) struct PlayheadLine {
    playhead: Entity<Playhead>,
    painted: Rc<Cell<Viewport>>,
}

impl PlayheadLine {
    /// `painted` is the viewport that `view` painted last.
    pub fn new<V: 'static>(
        playhead: Entity<Playhead>,
        view: &Entity<V>,
        painted: Rc<Cell<Viewport>>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&playhead, |_, _, cx| cx.notify()).detach();
        // A scroll or a zoom moves the line too.
        cx.observe(view, |_, _, cx| cx.notify()).detach();
        Self { playhead, painted }
    }
}

impl Render for PlayheadLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tick = self.playhead.read(cx).tick;
        let painted = self.painted.clone();
        let color = cx.theme().gray_950;
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                // The view painted before this line, so the viewport is this frame's.
                let x = painted.get().x_of(tick).round();
                if x < 0.0 || x >= f32::from(bounds.size.width) - HEADER_WIDTH {
                    return;
                }
                let top = bounds.origin + point(px(HEADER_WIDTH + x), px(RULER_HEIGHT / 2.));
                let line = Bounds::new(
                    top,
                    size(px(1.), bounds.size.height - px(RULER_HEIGHT / 2.)),
                );
                let head = Bounds::new(top - point(px(3.), px(3.)), size(px(7.), px(7.)));
                window.paint_quad(fill(line, color));
                window.paint_quad(quad(
                    head,
                    px(3.5),
                    color,
                    px(0.),
                    color,
                    BorderStyle::Solid,
                ));
            },
        )
        .absolute()
        .inset_0()
    }
}
