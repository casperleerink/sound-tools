//! Meter: the level of a stereo signal as two bars, and the gain reduction of a dynamics device.
//!
//! The meter only shows a [`Level`]. Where the level comes from is the owner's business; the
//! owner keeps a [`Ballistics`] to hold the peak and let the bars fall. Colours: green up to
//! -6 dBFS, yellow above, a red clip light over the bars until it is clicked, and a 1 pt peak
//! line. Nothing shows at rest.
//!
//! The scale is that of the fader of [`Volume`](super::volume::Volume), which is drawn over a
//! meter: -inf at the bottom, 0 dB at 80 % of the height and +6 dB at the top.

use std::rc::Rc;

use gpui::{
    App, Bounds, Div, ElementId, Hsla, MouseButton, Pixels, StyleRefinement, Window, canvas, div,
    fill, point, prelude::*, px, size,
};

use crate::theme::ActiveTheme;

/// The top of the scale.
pub const MAX_DB: f32 = 6.;
/// Where 0 dB sits on the scale.
pub const UNITY: f32 = 0.8;
/// Where green ends and yellow begins.
const HOT_DB: f32 = -6.;
/// Decibels of one tenfold step of the place on the scale, so that +6 dB is at the top. The
/// scale is `UNITY * 10^(dB / DECADE)`: a power law on the amplitude, which gives the lower
/// decibels room and reaches the bottom at -inf.
const DECADE: f32 = 61.94;

/// The place of a level on the scale, 0 at -inf and 1 at +6 dB.
pub fn position_of(db: f32) -> f32 {
    (UNITY * 10_f32.powf(db / DECADE)).clamp(0., 1.)
}

/// The level at a place on the scale, `-inf` at 0.
pub fn db_at(position: f32) -> f32 {
    if position <= 0. {
        return f32::NEG_INFINITY;
    }
    (DECADE * (position / UNITY).log10()).min(MAX_DB)
}

/// Bars of a meter and the room between them.
pub const BAR_WIDTH: f32 = 5.;
pub const BAR_GAP: f32 = 2.;
/// The clip light above the bars, and the room under it.
const LIGHT: f32 = 3.;
const LIGHT_GAP: f32 = 2.;
/// Where the scale of a vertical meter starts, from its top: under the clip light.
pub const SCALE_TOP: f32 = LIGHT + LIGHT_GAP;

/// What a meter shows, in dBFS, left and right. `-inf` is silence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub now: [f32; 2],
    /// The peak line, held for a while above the level.
    pub peak: [f32; 2],
    /// A sample went over 0 dBFS since the composer last cleared the light.
    pub clipped: bool,
}

impl Level {
    pub const SILENT: Self = Self {
        now: [f32::NEG_INFINITY; 2],
        peak: [f32::NEG_INFINITY; 2],
        clipped: false,
    };
}

impl Default for Level {
    fn default() -> Self {
        Self::SILENT
    }
}

/// How a meter moves: the bars fall 20 dB a second, and the peak line stays 1.5 s before it
/// falls the same way. The owner feeds it one reading per frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ballistics {
    level: Level,
    /// Seconds each peak line has been held.
    held: [f32; 2],
}

impl Ballistics {
    pub const FALL_DB_PER_SECOND: f32 = 20.;
    pub const HOLD_SECONDS: f32 = 1.5;

    /// One reading: the highest level of each channel since the last one, in dBFS, and the
    /// seconds since the last one. Gives what the meter shows now.
    pub fn read(&mut self, peaks: [f32; 2], seconds: f32) -> Level {
        let fall = Self::FALL_DB_PER_SECOND * seconds;
        for channel in 0..2 {
            let peak = peaks[channel];
            let level = &mut self.level;
            level.now[channel] = peak.max(level.now[channel] - fall);
            if peak >= level.peak[channel] {
                level.peak[channel] = peak;
                self.held[channel] = 0.;
            } else {
                self.held[channel] += seconds;
                let falling = (self.held[channel] - Self::HOLD_SECONDS).clamp(0., seconds);
                let lowered = level.peak[channel] - Self::FALL_DB_PER_SECOND * falling;
                level.peak[channel] = lowered.max(level.now[channel]);
            }
            level.clipped |= peak > 0.;
        }
        self.level
    }

    /// The composer clicked the clip light.
    pub fn clear_clip(&mut self) {
        self.level.clipped = false;
    }
}

type ClearHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The colours of a meter.
#[derive(Clone, Copy)]
struct MeterColors {
    empty: Hsla,
    green: Hsla,
    yellow: Hsla,
    red: Hsla,
    peak: Hsla,
}

impl MeterColors {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            empty: theme.alpha_at(0.06),
            green: theme.green,
            yellow: theme.yellow,
            red: theme.red,
            peak: theme.gray_950,
        }
    }
}

/// Paints the bars of a vertical meter in `bounds`, the clip light at its top.
fn paint_vertical(bounds: Bounds<Pixels>, level: Level, colors: MeterColors, window: &mut Window) {
    let left = bounds.origin.x;
    let top = bounds.origin.y + px(SCALE_TOP);
    let bottom = bounds.bottom();
    let length = f32::from(bottom - top);
    let y_of = |position: f32| bottom - px(position * length);
    for channel in 0..2 {
        let x = left + px(channel as f32 * (BAR_WIDTH + BAR_GAP));
        let span = |from: f32, to: f32| {
            let (low, high) = (y_of(from), y_of(to));
            Bounds::new(point(x, high), size(px(BAR_WIDTH), low - high))
        };
        window.paint_quad(fill(span(0., 1.), colors.empty));
        let now = position_of(level.now[channel]);
        let hot = position_of(HOT_DB);
        if now > 0. {
            window.paint_quad(fill(span(0., now.min(hot)), colors.green));
        }
        if now > hot {
            window.paint_quad(fill(span(hot, now), colors.yellow));
        }
        let peak = position_of(level.peak[channel]);
        if peak > 0. {
            let line = Bounds::new(point(x, y_of(peak)), size(px(BAR_WIDTH), px(1.)));
            window.paint_quad(fill(line, colors.peak));
        }
    }
    if level.clipped {
        let width = px(BAR_WIDTH * 2. + BAR_GAP);
        let light = Bounds::new(bounds.origin, size(width, px(LIGHT)));
        window.paint_quad(fill(light, colors.red).corner_radii(px(1.)));
    }
}

/// A vertical stereo meter: two bars, and the clip light above them.
#[derive(IntoElement)]
pub struct Meter {
    base: Div,
    id: ElementId,
    level: Level,
    height: f32,
    on_clear_clip: Option<ClearHandler>,
}

impl Meter {
    pub fn new(id: impl Into<ElementId>, level: Level) -> Self {
        Self {
            base: div(),
            id: id.into(),
            level,
            height: 118.,
            on_clear_clip: None,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// A click on the clip light, which the owner clears.
    pub fn on_clear_clip(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear_clip = Some(Rc::new(f));
        self
    }
}

impl Styled for Meter {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Meter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (level, colors) = (self.level, MeterColors::of(cx));
        let bars = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_vertical(bounds, level, colors, window),
        )
        .size_full();
        let width = BAR_WIDTH * 2. + BAR_GAP;
        self.base
            .id(self.id)
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(self.height))
            .child(bars)
            .when_some(
                self.on_clear_clip.filter(|_| level.clipped),
                |meter, clear| {
                    // The light is small, so the target is the top of the meter.
                    meter.child(
                        div()
                            .id("clip-light")
                            .absolute()
                            .top(px(-4.))
                            .left(px(-4.))
                            .w(px(width + 8.))
                            .h(px(SCALE_TOP + 8.))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                clear(window, cx);
                            }),
                    )
                },
            )
    }
}

/// Gain reduction: a bar from the top down, 0 to 24 dB. It is not level, so it has none of the
/// colours of a meter. The Compressor and the Limiter draw it inside their display.
#[derive(IntoElement)]
pub struct GainReduction {
    base: Div,
    /// Decibels of reduction, 0 or more.
    db: f32,
}

impl GainReduction {
    /// The whole bar.
    pub const RANGE_DB: f32 = 24.;

    pub fn new(db: f32) -> Self {
        Self {
            base: div().w(px(5.)).h(px(118.)),
            db,
        }
    }
}

impl Styled for GainReduction {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for GainReduction {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track, bar) = (theme.alpha_at(0.06), theme.gray_950);
        let part = (self.db / Self::RANGE_DB).clamp(0., 1.);
        self.base
            .relative()
            .flex_none()
            .rounded(px(1.))
            .bg(track)
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(gpui::relative(part))
                    .bg(bar),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_db_is_at_eighty_percent_and_the_ends_are_minus_inf_and_six() {
        assert_eq!(position_of(0.), UNITY);
        assert!((position_of(MAX_DB) - 1.).abs() < 1e-4);
        assert_eq!(position_of(f32::NEG_INFINITY), 0.);
        assert_eq!(db_at(0.), f32::NEG_INFINITY);
        assert_eq!(db_at(UNITY), 0.);
        assert_eq!(db_at(1.), MAX_DB);
        // Beyond the top is the top.
        assert_eq!(position_of(12.), 1.);
    }

    #[test]
    fn a_level_goes_to_its_place_and_back() {
        for db in [-60., -24., -12., -6., -3.5, -0.1, 0.1, 3., 5.9] {
            let back = db_at(position_of(db));
            assert!((back - db).abs() < 1e-3, "{db} came back as {back}");
        }
        // Lower levels get less room, as on a mixing desk.
        assert!(position_of(-6.) - position_of(-12.) > position_of(-48.) - position_of(-54.));
    }

    #[test]
    fn the_bars_rise_at_once_and_fall_twenty_db_a_second() {
        let mut meter = Ballistics::default();
        assert_eq!(meter.read([-3., -12.], 0.016).now, [-3., -12.]);
        let quiet = [f32::NEG_INFINITY; 2];
        let level = meter.read(quiet, 0.5);
        assert_eq!(level.now, [-13., -22.]);
    }

    #[test]
    fn the_peak_line_is_held_for_a_second_and_a_half_then_falls() {
        let mut meter = Ballistics::default();
        meter.read([-6., -6.], 0.1);
        let quiet = [f32::NEG_INFINITY; 2];
        let held = meter.read(quiet, 1.4);
        assert_eq!(held.peak, [-6., -6.]);
        // 0.1 s past the hold, 2 dB lower.
        let falling = meter.read(quiet, 0.2);
        assert!((falling.peak[0] + 8.).abs() < 1e-4, "{:?}", falling.peak);
        // Never under the bar.
        let mut meter = Ballistics::default();
        meter.read([0., 0.], 0.1);
        let level = meter.read([-1., -1.], 3.);
        assert_eq!(level.peak, level.now);
    }

    #[test]
    fn a_clip_stays_lit_until_it_is_cleared_and_nothing_shows_at_rest() {
        let mut meter = Ballistics::default();
        assert_eq!(meter.read([f32::NEG_INFINITY; 2], 0.1), Level::SILENT);
        assert!(!meter.read([0., -1.], 0.1).clipped);
        assert!(meter.read([0.5, -1.], 0.1).clipped);
        assert!(meter.read([-30., -30.], 5.).clipped);
        meter.clear_clip();
        assert!(!meter.read([-30., -30.], 0.1).clipped);
    }
}
