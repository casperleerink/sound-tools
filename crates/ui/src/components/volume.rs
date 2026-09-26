//! Volume: one control for the level of a track, a fader whose track is the meter. The thumb, a
//! 28 x 6 pt bar with a ring of the window colour, sits across the two bars at the gain, and the
//! level shows through above and below it. A tick at the left marks 0 dB. The readout sits on
//! the value line under it: `-3.5 dB`, `0 dB`, `+2 dB`, `-inf`.
//!
//! The scale is the meter's: -inf at the bottom, 0 dB at 80 % of the height, +6 dB at the top,
//! the same for the thumb and the level. The gesture is that of the knob, see
//! [`gesture`](super::gesture): drag the thumb or anywhere on the meter and it moves from where
//! it is, with the pointer, so a press never jumps; with shift ten times finer. A double click
//! or backspace sets 0 dB. The arrows step 0.5 dB, with shift 0.1 dB. The value is in dB and
//! `f32::NEG_INFINITY` is the bottom: how an owner saves that is its own business.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, Div, ElementId, Hsla, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Point, StyleRefinement, Window, canvas, div, fill, point, prelude::*, px, size,
};

use crate::components::cell::CELL_WIDTH;
use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::meter::{self, BAR_GAP, BAR_WIDTH, Level, Meter, SCALE_TOP};
use crate::theme::ActiveTheme;
use crate::typography;

const THUMB_WIDTH: f32 = 28.;
const THUMB_HEIGHT: f32 = 6.;
/// The ring of the window colour around the thumb, which parts it from the bars.
const THUMB_RING: f32 = 1.5;
const FOCUS_RING: f32 = 2.;
const KEY_STEP_DB: f32 = 0.5;
const FINE_KEY_STEP_DB: f32 = 0.1;
/// Where the arrows go from `-inf`, and under which a step down goes to `-inf`. The lowest
/// level a composer would set by hand; the drag reaches everything under it.
const FLOOR_DB: f32 = -60.;
/// The readout under the meter, on the value line of a cell.
const READOUT_GAP: f32 = 8.;
const READOUT_HEIGHT: f32 = 14.;

/// A level in dB as the readout shows it, to a tenth.
pub fn readout(db: f32) -> String {
    if db == f32::NEG_INFINITY {
        return "-inf".into();
    }
    let tenths = (db * 10.).round() / 10.;
    let text = format!("{tenths:.1}");
    let text = text.trim_end_matches(".0");
    match tenths {
        0. => "0 dB".into(),
        _ if tenths > 0. => format!("+{text} dB"),
        _ => format!("{text} dB"),
    }
}

/// The level at a place on the scale, to a tenth of a dB, as a drag gives it.
fn db_at(position: f32) -> f32 {
    let db = meter::db_at(position);
    match db.is_finite() {
        true => (db * 10.).round() / 10.,
        false => db,
    }
}

/// One arrow step from `db`, or `None` where there is no further to go.
fn step(db: f32, up: bool, fine: bool) -> Option<f32> {
    let size = if fine { FINE_KEY_STEP_DB } else { KEY_STEP_DB };
    let next = match (up, db == f32::NEG_INFINITY) {
        (true, true) => FLOOR_DB,
        (false, true) => return None,
        (true, false) => (db + size).min(meter::MAX_DB),
        (false, false) if db - size < FLOOR_DB => f32::NEG_INFINITY,
        (false, false) => db - size,
    };
    // In tenths, so that steps of a tenth never drift off them.
    let next = if next.is_finite() {
        (next * 10.).round() / 10.
    } else {
        next
    };
    (next != db).then_some(next)
}

#[derive(IntoElement)]
pub struct Volume {
    base: Div,
    id: ElementId,
    db: f32,
    level: Level,
    height: f32,
    disabled: bool,
    on_change: Option<ChangeHandler<f32>>,
    on_clear_clip: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl Volume {
    pub fn new(id: impl Into<ElementId>, db: f32) -> Self {
        Self {
            base: div(),
            id: id.into(),
            db,
            level: Level::SILENT,
            height: 118.,
            disabled: false,
            on_change: None,
            on_clear_clip: None,
        }
    }

    /// What the meter shows.
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// The height of the meter. The readout comes under it.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(
        mut self,
        f: impl Fn(ValueChange, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// A click on the clip light of the meter.
    pub fn on_clear_clip(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear_clip = Some(Rc::new(f));
        self
    }
}

impl Styled for Volume {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// What the thumb and the tick are painted with.
#[derive(Clone, Copy)]
struct ThumbColors {
    thumb: Hsla,
    ring: Hsla,
    tick: Hsla,
    focus: Option<Hsla>,
}

/// The thumb at `position` and the 0 dB tick, over a meter whose bars fill `bounds`.
fn paint_thumb(bounds: Bounds<Pixels>, position: f32, colors: ThumbColors, window: &mut Window) {
    let top = bounds.origin.y + px(SCALE_TOP);
    let length = f32::from(bounds.bottom() - top);
    let y_of = |position: f32| bounds.bottom() - px(position * length);
    let centre_x = bounds.center().x;
    let tick_y = y_of(meter::UNITY);
    let tick = Bounds::new(
        point(bounds.origin.x - px(6.), tick_y),
        size(px(4.), px(1.)),
    );
    window.paint_quad(fill(tick, colors.tick));
    let box_of = |width: f32, height: f32| {
        let centre = point(centre_x, y_of(position));
        Bounds::new(
            centre - point(px(width / 2.), px(height / 2.)),
            size(px(width), px(height)),
        )
    };
    if let Some(focus) = colors.focus {
        let (width, height) = (THUMB_WIDTH, THUMB_HEIGHT);
        let outer = (THUMB_RING + FOCUS_RING) * 2.;
        let ring = box_of(width + outer, height + outer);
        window.paint_quad(fill(ring, focus).corner_radii(px(4.5)));
    }
    let ring = box_of(THUMB_WIDTH + THUMB_RING * 2., THUMB_HEIGHT + THUMB_RING * 2.);
    window.paint_quad(fill(ring, colors.ring).corner_radii(px(3.)));
    let thumb = box_of(THUMB_WIDTH, THUMB_HEIGHT);
    window.paint_quad(fill(thumb, colors.thumb).corner_radii(px(2.)));
}

impl RenderOnce for Volume {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| GestureState::new(cx));
        let disabled = self.disabled;
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(!disabled);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);
        let theme = cx.theme();
        let colors = ThumbColors {
            thumb: theme.gray_950,
            ring: theme.gray_100,
            tick: theme.gray_700,
            focus: ring_shows.then_some(theme.lavender),
        };
        let value_color = theme.gray_950;

        let (db, height) = (self.db, self.height);
        let position = meter::position_of(db);
        let length = height - SCALE_TOP;
        let meter_width = BAR_WIDTH * 2. + BAR_GAP;
        let thumb = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_thumb(bounds, position, colors, window),
        )
        .absolute()
        .top_0()
        .left(px((CELL_WIDTH - meter_width) / 2.))
        .w(px(meter_width))
        .h(px(height));

        let on_change = self.on_change.filter(|_| !disabled);
        let selector = self.id.clone();
        let fader = div()
            .id(self.id.clone())
            .debug_selector(move || format!("volume-{selector}"))
            .relative()
            .w(px(CELL_WIDTH))
            .h(px(height))
            .flex()
            .justify_center()
            .when(disabled, |d| d.cursor_not_allowed())
            .when_some(on_change, |d, on_change| {
                let on_mouse_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let y = -f32::from(event.position.y);
                        let mut travel = Travel::new(y, position, length);
                        let value_at = move |pointer: Point<Pixels>, fine| {
                            match travel.position(-f32::from(pointer.y), fine) {
                                Some(position) => db_at(position),
                                None => db,
                            }
                        };
                        gesture::press(
                            &state,
                            event,
                            db,
                            Some(0.),
                            value_at,
                            &on_change,
                            window,
                            cx,
                        );
                    }
                };
                let on_key_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                        let step = |up, fine| step(db, up, fine);
                        gesture::key_down(&state, event, step, Some(0.), &on_change, window, cx);
                    }
                };
                d.cursor(CursorStyle::ResizeUpDown)
                    .track_focus(&focus_handle)
                    .on_key_down(on_key_down)
                    .on_mouse_down(MouseButton::Left, on_mouse_down)
                    .child(gesture::drag_listeners(state, on_change))
            })
            .child(
                Meter::new("meter", self.level)
                    .height(height)
                    .when_some(self.on_clear_clip, |meter, clear| {
                        meter.on_clear_clip(move |window, cx| clear(window, cx))
                    }),
            )
            .child(thumb);

        self.base
            .flex()
            .flex_col()
            .flex_none()
            .w(px(CELL_WIDTH))
            .when(disabled, |d| d.opacity(0.4))
            .child(fader)
            .child(
                div()
                    .mt(px(READOUT_GAP))
                    .h(px(READOUT_HEIGHT))
                    .flex()
                    .justify_center()
                    .font(typography::tabular())
                    .text_size(px(12.))
                    .line_height(px(READOUT_HEIGHT))
                    .text_color(value_color)
                    .whitespace_nowrap()
                    .child(readout(db)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_is_in_tenths_with_a_sign() {
        assert_eq!(readout(f32::NEG_INFINITY), "-inf");
        assert_eq!(readout(0.), "0 dB");
        assert_eq!(readout(-0.01), "0 dB");
        assert_eq!(readout(-3.5), "-3.5 dB");
        assert_eq!(readout(2.), "+2 dB");
        assert_eq!(readout(-12.34), "-12.3 dB");
        assert_eq!(readout(6.), "+6 dB");
    }

    #[test]
    fn a_drag_gives_tenths_and_minus_inf_at_the_bottom() {
        assert_eq!(db_at(0.), f32::NEG_INFINITY);
        assert_eq!(db_at(meter::UNITY), 0.);
        assert_eq!(db_at(1.), meter::MAX_DB);
        let db = db_at(0.5);
        assert_eq!(db, (db * 10.).round() / 10.);
    }

    #[test]
    fn the_fader_follows_the_pointer_over_the_height_of_the_meter() {
        // A press on the thumb at 0 dB and a drag down by a fifth of the scale.
        let length = 113.;
        let mut travel = Travel::new(-100., meter::UNITY, length);
        let down = travel.position(-100. - length * 0.2, false);
        assert!(down.is_some_and(|position| (position - 0.6).abs() < 1e-5));
        // Anywhere on the meter: the press is where the thumb is, not where the pointer is.
        let mut travel = Travel::new(-10., meter::UNITY, length);
        assert_eq!(travel.position(-10., false), None);
    }

    #[test]
    fn the_arrows_step_half_a_db_and_a_tenth_with_shift() {
        assert_eq!(step(-3.5, true, false), Some(-3.));
        assert_eq!(step(-3.5, false, true), Some(-3.6));
        assert_eq!(step(5.8, true, false), Some(6.));
        assert_eq!(step(6., true, false), None);
        // The bottom: from -inf up to the floor, and from the floor down to -inf.
        assert_eq!(step(f32::NEG_INFINITY, true, false), Some(FLOOR_DB));
        assert_eq!(step(f32::NEG_INFINITY, false, false), None);
        assert_eq!(step(-59.8, false, false), Some(f32::NEG_INFINITY));
        // Tenths stay tenths.
        let mut db = 0.;
        for _ in 0..7 {
            db = step(db, false, true).unwrap_or(db);
        }
        assert_eq!(db, -0.7);
    }
}
