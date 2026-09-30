//! Slider: a value on a short line, for a row with no room for a knob, such as the amount of a
//! route of a matrix. 24 pt tall to take a press, with a 2 pt track of `alpha/10`, the value as
//! one bright line on it and a 8 pt dot at its end. A bipolar slider draws its line from the
//! middle, as a bipolar knob does from the top.
//!
//! It drags sideways and the dot follows the pointer, which a short line can afford. The rest
//! is the gesture of the knob, see [`gesture`](super::gesture): shift ten times finer, escape
//! puts the value back, double click or backspace sets the default, the arrows step a fiftieth
//! of the travel. Controlled: the owner gives the value on every render and hears a
//! [`ValueChange`].

use std::rc::Rc;

use gpui::{
    App, Bounds, Div, ElementId, Hsla, Pixels, StyleRefinement, Window, canvas, div, fill, point,
    prelude::*, px, size,
};

use crate::components::gesture::{ChangeHandler, GestureState, ValueChange};
use crate::components::knob::{Dragged, KnobRange, drags};
use crate::components::paint;
use crate::theme::ActiveTheme;

pub const HEIGHT: f32 = 24.;
const TRACK: f32 = 2.;
const DOT: f32 = 8.;

#[derive(IntoElement)]
pub struct Slider {
    base: Div,
    id: ElementId,
    width: f32,
    value: f32,
    range: KnobRange,
    default_value: Option<f32>,
    bipolar: bool,
    on_change: Option<ChangeHandler<f32>>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, width: f32) -> Self {
        Self {
            base: div(),
            id: id.into(),
            width,
            value: 0.,
            range: KnobRange::linear(0., 1.),
            default_value: None,
            bipolar: false,
            on_change: None,
        }
    }

    pub fn range(mut self, range: KnobRange) -> Self {
        self.range = range;
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// What a double click and backspace set.
    pub fn default_value(mut self, value: f32) -> Self {
        self.default_value = Some(value);
        self
    }

    /// The line starts in the middle, for a value that goes either way.
    pub fn bipolar(mut self, bipolar: bool) -> Self {
        self.bipolar = bipolar;
        self
    }

    pub fn on_change(mut self, f: impl Fn(ValueChange, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Slider {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// The track, the value on it from `from` to `to`, and the dot at `to`, both from 0 to 1.
fn paint_slider(
    bounds: Bounds<Pixels>,
    (from, to): (f32, f32),
    (track, value, ring): (Hsla, Hsla, Option<Hsla>),
    window: &mut Window,
) {
    // The ends of the line are half a dot in, so the dot at an end stays inside.
    let left = f32::from(bounds.left()) + DOT / 2.;
    let length = f32::from(bounds.size.width) - DOT;
    let middle = f32::from(bounds.center().y);
    let line = |window: &mut Window, from: f32, to: f32, color| {
        let (start, end) = (left + length * from.min(to), left + length * from.max(to));
        let area = Bounds::new(
            point(px(start), px(middle - TRACK / 2.)),
            size(px((end - start).max(0.)), px(TRACK)),
        );
        window.paint_quad(fill(area, color).corner_radii(px(TRACK / 2.)));
    };
    line(window, 0., 1., track);
    line(window, from, to, value);
    let dot = point(px(left + length * to), px(middle));
    paint::circle(window, dot, DOT / 2., value);
    if let Some(ring) = ring {
        paint::ring(window, dot, DOT / 2. + 3., 1.5, ring);
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| GestureState::new(cx));
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(true);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);
        let theme = cx.theme();
        let colors = (
            theme.alpha_at(0.10),
            theme.gray_950,
            ring_shows.then_some(theme.lavender),
        );
        let position = self.range.position(self.value);
        let from = match self.bipolar {
            true => self.range.position(0.),
            false => 0.,
        };
        let drawing = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_slider(bounds, (from, position), colors, window),
        )
        .size_full();
        let dragged = Dragged {
            range: self.range,
            value: self.value,
            default: self.default_value,
            step: None,
            span: self.width - DOT,
            sideways: true,
        };
        // For tests, which find the slider by its id: `slider-<id>`.
        let selector = self.id.clone();
        self.base
            .id(self.id)
            .debug_selector(move || format!("slider-{selector}"))
            .flex_none()
            .w(px(self.width))
            .h(px(HEIGHT))
            .when_some(self.on_change, |slider, on_change| {
                drags(slider, dragged, &state, &focus_handle, on_change)
            })
            .child(drawing)
    }
}
