//! Display: the inset at the left of a device card that shows what the device does to the
//! sound, 118 pt tall, with the line of its numbers or its scale under it on the value line of
//! row 2. A curve is 1.5 pt over a fill of `alpha/5`, grid lines are `alpha/4` and the 0 dB
//! line `alpha/8`. Controls such as a segmented control go at its top as children.
//!
//! The display is not only a picture: its handles drag. A [`Handle`] moves one or two values,
//! sideways and up and down, each on a [`KnobRange`] across the width or the height of the
//! display. It follows the pointer from where it is, so a press never jumps; with shift ten
//! times finer; a double click resets what it moves; escape during a drag puts it back. That is
//! the gesture of the knob, see [`gesture`](super::gesture). A handle is not a tab stop: every
//! value a handle moves also has a knob or a hidden control, which is the path for the keys.
//!
//! The display knows no device. The owner gives the curve as points on the display and the
//! handles with their values, and hears what a handle moves.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, CursorStyle, Div, ElementId, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, SharedString, Window, canvas, div,
    fill, point, prelude::*, px, size,
};

use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::knob::KnobRange;
use crate::theme::ActiveTheme;
use crate::typography;

pub const INSET_HEIGHT: f32 = 118.;
/// From the inset to the line under it, which is on the value line of the second row.
const CAPTION_GAP: f32 = 8.;
const CAPTION_HEIGHT: f32 = 14.;
const CURVE_WIDTH: f32 = 1.5;
const HANDLE: f32 = 10.;
const HANDLE_RING: f32 = 1.5;
/// The target of a handle is larger than its dot: a trackpad is not a mouse.
const HANDLE_TARGET: f32 = 18.;

/// One value a handle moves: where it is on a range, and what a double click sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub range: KnobRange,
    pub value: f32,
    pub default: f32,
    /// Whether the handle moves this value. A handle that only moves sideways still has a
    /// place on the other axis.
    pub drags: bool,
}

impl Axis {
    pub fn new(range: KnobRange, value: f32, default: f32) -> Self {
        let drags = true;
        Self {
            range,
            value,
            default,
            drags,
        }
    }

    /// A place on the display that does not move, from 0 to 1.
    pub fn fixed(position: f32) -> Self {
        let range = KnobRange::linear(0., 1.);
        let drags = false;
        Self {
            range,
            value: position,
            default: position,
            drags,
        }
    }

    fn position(&self) -> f32 {
        self.range.position(self.value)
    }
}

/// The values of a handle: `x` sideways, `y` up and down.
pub type HandleValues = Point<f32>;

#[derive(Clone)]
pub struct Handle {
    id: ElementId,
    x: Axis,
    y: Axis,
    hollow: bool,
    on_change: Option<ChangeHandler<HandleValues>>,
}

impl Handle {
    pub fn new(id: impl Into<ElementId>, x: Axis, y: Axis) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            hollow: false,
            on_change: None,
        }
    }

    /// A secondary handle: a ring and no fill.
    pub fn hollow(mut self, hollow: bool) -> Self {
        self.hollow = hollow;
        self
    }

    pub fn on_change(
        mut self,
        f: impl Fn(ValueChange<HandleValues>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

#[derive(IntoElement)]
pub struct Display {
    base: Div,
    id: ElementId,
    width: f32,
    /// Points from left to right, `x` and `y` from 0 to 1, `y` up.
    curve: Vec<Point<f32>>,
    /// Vertical grid lines at these places across, horizontal ones at these places up.
    grid: (Vec<f32>, Vec<f32>),
    /// The 0 dB line, at this place up.
    zero: Option<f32>,
    handles: Vec<Handle>,
    caption: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl Display {
    pub fn new(id: impl Into<ElementId>, width: f32) -> Self {
        Self {
            base: div(),
            id: id.into(),
            width,
            curve: Vec::new(),
            grid: (Vec::new(), Vec::new()),
            zero: None,
            handles: Vec::new(),
            caption: None,
            children: Vec::new(),
        }
    }

    /// The curve, as points from left to right with `x` and `y` from 0 to 1, `y` up. It is
    /// filled down to the bottom of the display.
    pub fn curve(mut self, points: impl IntoIterator<Item = Point<f32>>) -> Self {
        self.curve = points.into_iter().collect();
        self
    }

    /// Grid lines: vertical ones at places across, horizontal ones at places up, 0 to 1.
    pub fn grid(mut self, across: Vec<f32>, up: Vec<f32>) -> Self {
        self.grid = (across, up);
        self
    }

    /// The 0 dB line, at a place up.
    pub fn zero_line(mut self, up: f32) -> Self {
        self.zero = Some(up);
        self
    }

    pub fn handle(mut self, handle: Handle) -> Self {
        self.handles.push(handle);
        self
    }

    /// The line under the display: its numbers, `A 5 ms · D 350 ms`, or its scale.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }
}

/// Controls at the top of the display, such as the type of a filter.
impl ParentElement for Display {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// The colours of what the display paints.
#[derive(Clone, Copy)]
struct Ink {
    curve: Hsla,
    fill: Hsla,
    grid: Hsla,
    zero: Hsla,
}

/// A place on the display in points, from `x` and `y` from 0 to 1 with `y` up.
fn at(bounds: Bounds<Pixels>, place: Point<f32>) -> Point<Pixels> {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    bounds.origin + point(px(place.x * width), px((1. - place.y) * height))
}

fn paint_display(
    bounds: Bounds<Pixels>,
    curve: &[Point<f32>],
    (across, up): &(Vec<f32>, Vec<f32>),
    zero: Option<f32>,
    ink: Ink,
    window: &mut Window,
) {
    let mask = ContentMask { bounds };
    window.with_content_mask(Some(mask), |window| {
        let line = |window: &mut Window, from: Point<Pixels>, to: Point<Pixels>, color| {
            let size = size((to.x - from.x).max(px(1.)), (to.y - from.y).max(px(1.)));
            window.paint_quad(fill(Bounds::new(from, size), color));
        };
        for x in across {
            let top = at(bounds, point(*x, 1.));
            line(window, top, at(bounds, point(*x, 0.)), ink.grid);
        }
        for y in up {
            let left = at(bounds, point(0., *y));
            line(window, left, at(bounds, point(1., *y)), ink.grid);
        }
        if let Some(y) = zero {
            line(
                window,
                at(bounds, point(0., y)),
                at(bounds, point(1., y)),
                ink.zero,
            );
        }
        let (Some(first), Some(last)) = (curve.first(), curve.last()) else {
            return;
        };
        let mut area = PathBuilder::fill();
        area.move_to(at(bounds, point(first.x, 0.)));
        for place in curve {
            area.line_to(at(bounds, *place));
        }
        area.line_to(at(bounds, point(last.x, 0.)));
        area.close();
        let mut stroke = PathBuilder::stroke(px(CURVE_WIDTH));
        stroke.move_to(at(bounds, *first));
        for place in &curve[1..] {
            stroke.line_to(at(bounds, *place));
        }
        // A path that does not tessellate paints nothing, which is all there is to do about it.
        if let Ok(area) = area.build() {
            window.paint_path(area, ink.fill);
        }
        if let Ok(stroke) = stroke.build() {
            window.paint_path(stroke, ink.curve);
        }
    });
}

/// The value of one axis of a handle for the pointer now.
fn moved(axis: Axis, travel: &mut Travel, pointer: f32, fine: bool) -> f32 {
    match travel.position(pointer, fine) {
        Some(position) if axis.drags => axis.range.value(position),
        _ => axis.value,
    }
}

/// The element of one handle, a dot in a larger target centred on its place.
fn handle_element(
    display: &ElementId,
    handle: Handle,
    (width, height): (f32, f32),
    (dot, ring): (Hsla, Hsla),
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    // Under the id of the display, so that two displays with handles of one name, such as two
    // filters in a rack, keep a drag and a focus each. A display draws before its element
    // pushes its id, so the key names the display itself.
    let key = ElementId::NamedChild(Arc::new(display.clone()), handle.id.to_string().into());
    let state = window.use_keyed_state(key, cx, |_, cx| GestureState::new(cx));
    let focus_handle = state.read(cx).focus_handle.clone();
    let (x, y) = (handle.x, handle.y);
    let place = point(x.position(), y.position());
    let value = point(x.value, y.value);
    // A double click resets what the handle moves, and leaves an axis it does not move alone.
    let reset_of = |axis: Axis| if axis.drags { axis.default } else { axis.value };
    let reset = point(reset_of(x), reset_of(y));
    let cursor = match (x.drags, y.drags) {
        (true, false) => CursorStyle::ResizeLeftRight,
        (false, true) => CursorStyle::ResizeUpDown,
        _ => CursorStyle::Crosshair,
    };
    let (fill, border) = match handle.hollow {
        true => (ring, dot),
        false => (dot, ring),
    };
    let selector = handle.id.clone();
    div()
        .id(handle.id)
        .debug_selector(move || format!("handle-{selector}"))
        .absolute()
        .left(px(place.x * width - HANDLE_TARGET / 2.))
        .top(px((1. - place.y) * height - HANDLE_TARGET / 2.))
        .size(px(HANDLE_TARGET))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(HANDLE))
                .rounded_full()
                .bg(fill)
                .border(px(HANDLE_RING))
                .border_color(border),
        )
        .when_some(handle.on_change, |d, on_change| {
            let on_mouse_down = {
                let (state, on_change) = (state.clone(), on_change.clone());
                let press_focus = focus_handle.clone();
                move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    let pointer = event.position;
                    let (px_x, px_y) = (f32::from(pointer.x), -f32::from(pointer.y));
                    let mut across = Travel::new(px_x, place.x, width);
                    let mut up = Travel::new(px_y, place.y, height);
                    let value_at = move |pointer: Point<Pixels>, fine| {
                        point(
                            moved(x, &mut across, f32::from(pointer.x), fine),
                            moved(y, &mut up, -f32::from(pointer.y), fine),
                        )
                    };
                    let reset = Some(reset);
                    let took = gesture::press(
                        &state, event, value, reset, value_at, &on_change, window, cx,
                    );
                    // A press that opened a drag or reset is the handle's, not the card's. Stopping
                    // it also stops GPUI giving the handle the focus, which escape needs, so the
                    // handle takes it itself.
                    if took {
                        window.focus(&press_focus, cx);
                        cx.stop_propagation();
                    }
                }
            };
            // Only escape: the keys of a value are those of its knob.
            let on_key_down = {
                let (state, on_change) = (state.clone(), on_change.clone());
                move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    gesture::key_down(&state, event, None, None, &on_change, window, cx);
                }
            };
            d.cursor(cursor)
                .track_focus(&focus_handle)
                .on_key_down(on_key_down)
                .on_mouse_down(MouseButton::Left, on_mouse_down)
                .child(gesture::drag_listeners(state, on_change))
        })
}

impl RenderOnce for Display {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let inset = theme.gray_50.opacity(0.7);
        let ink = Ink {
            curve: theme.gray_950,
            fill: theme.alpha_at(0.05),
            grid: theme.alpha_at(0.04),
            zero: theme.alpha_at(0.08),
        };
        let handle_colors = (theme.gray_950, theme.gray_50);
        let caption_color = theme.gray_800;
        let (curve, grid, zero) = (self.curve, self.grid, self.zero);
        let drawing = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_display(bounds, &curve, &grid, zero, ink, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let area = (self.width, INSET_HEIGHT);
        let handles: Vec<_> = self
            .handles
            .into_iter()
            .map(|handle| handle_element(&self.id, handle, area, handle_colors, window, cx))
            .collect();

        self.base
            .id(self.id)
            .flex_none()
            .flex()
            .flex_col()
            .w(px(self.width))
            .child(
                div()
                    .relative()
                    .h(px(INSET_HEIGHT))
                    .rounded(px(6.))
                    .bg(inset)
                    .child(drawing)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .p(px(6.))
                            .flex()
                            .items_start()
                            .children(self.children),
                    )
                    .children(handles),
            )
            .child(
                div()
                    .mt(px(CAPTION_GAP))
                    .h(px(CAPTION_HEIGHT))
                    .flex()
                    .justify_center()
                    .font(typography::tabular())
                    .text_size(px(12.))
                    .line_height(px(CAPTION_HEIGHT))
                    .text_color(caption_color)
                    .whitespace_nowrap()
                    .children(self.caption),
            )
    }
}
