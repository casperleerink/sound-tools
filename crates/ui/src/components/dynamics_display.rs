//! The display of a device that turns a level down, the Compressor and the Gate: its transfer
//! curve, the input across and the output up, square, so an untouched sound is a diagonal. The
//! level now is a dot on the curve and the gain reduction a bar at the right edge, with a
//! caption under it.
//!
//! The owner draws its own curve and handles on [`Levels`], keeps a [`Reading`] from its meters
//! and adds them with [`Levels::meters`].

use gpui::{App, div, prelude::*, px};

use crate::components::display::{Display, INSET_HEIGHT};
use crate::components::knob::{KnobRange, short};
use crate::components::meter::GainReduction;
use crate::theme::ActiveTheme;

/// The width of the display.
pub const WIDTH: f32 = 136.;

/// The curve takes this part of the width, so that it is as wide as the display is tall. The
/// gain reduction bar has the rest.
pub const CURVE_WIDTH: f32 = INSET_HEIGHT / WIDTH;

/// The dot of the level and the bar of the gain reduction, in points.
const LEVEL_DOT: f32 = 8.;
const BAR_INSET: f32 = 6.;

/// The levels the curve shows, in dBFS, from the bottom left to the top right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub bottom_db: f32,
    pub top_db: f32,
}

impl Levels {
    /// Where a level is up the display, from 0 at the bottom to 1 at the top.
    pub fn up(self, db: f32) -> f32 {
        ((db - self.bottom_db) / (self.top_db - self.bottom_db)).clamp(0., 1.)
    }

    /// Where a level is across the display, from 0 at the left to [`CURVE_WIDTH`].
    pub fn across(self, db: f32) -> f32 {
        CURVE_WIDTH * self.up(db)
    }

    /// The levels across the whole width, past the top of the curve too, for a handle that
    /// drags a threshold sideways.
    pub fn threshold_travel(self) -> KnobRange {
        let span = self.top_db - self.bottom_db;
        KnobRange::linear(self.bottom_db, self.bottom_db + span / CURVE_WIDTH)
    }

    /// What a device heard and did since the card last looked, from the amplitude of its level
    /// and its reduction in dB: to a quarter of a point of the display and a tenth of a dB, so
    /// that a card whose sound holds still asks for no frame. Silence has no level.
    pub fn reading(self, level: f32, reduction_db: f32) -> Reading {
        let quarter_point = (self.top_db - self.bottom_db) / INSET_HEIGHT / 4.;
        let level_db = match level > 0. {
            true => (20. * level.log10() / quarter_point).round() * quarter_point,
            false => f32::NEG_INFINITY,
        };
        Reading {
            level_db,
            reduction_db: (reduction_db * 10.).round() / 10.,
        }
    }

    /// The dot of the level, where it comes out, the bar and the caption of the reduction.
    /// `selector` names the dot for tests.
    pub fn meters(
        self,
        display: Display,
        reading: Reading,
        selector: &'static str,
        cx: &App,
    ) -> Display {
        let green = cx.theme().green;
        let Reading {
            level_db,
            reduction_db,
        } = reading;
        let level = (level_db > self.bottom_db).then(|| {
            let (x, y) = (self.across(level_db), self.up(level_db - reduction_db));
            div()
                .debug_selector(move || selector.into())
                .absolute()
                .left(px(x * WIDTH - LEVEL_DOT / 2.))
                .top(px((1. - y) * INSET_HEIGHT - LEVEL_DOT / 2.))
                .size(px(LEVEL_DOT))
                .rounded_full()
                .bg(green)
        });
        // In a place of its own: the bar positions itself relative to its own box.
        let bar = div()
            .absolute()
            .top(px(BAR_INSET))
            .right(px(BAR_INSET))
            .child(GainReduction::new(reduction_db).h(px(INSET_HEIGHT - 2. * BAR_INSET)));
        display
            .caption(reduction_readout(reduction_db))
            .children(level)
            .child(bar)
    }
}

/// The peak level in dB and the most a device turned down in dB, as [`Levels::reading`]
/// rounds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub level_db: f32,
    pub reduction_db: f32,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            level_db: f32::NEG_INFINITY,
            reduction_db: 0.,
        }
    }
}

/// The gain reduction under the display, to a tenth of a dB: `GR 0 dB`, `GR -6.8 dB`.
fn reduction_readout(db: f32) -> String {
    match db > 0. {
        true => format!("GR -{} dB", short(db)),
        false => "GR 0 dB".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: Levels = Levels {
        bottom_db: -60.,
        top_db: 0.,
    };

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(reduction_readout(0.0), "GR 0 dB");
        assert_eq!(reduction_readout(6.8), "GR -6.8 dB");
    }

    #[test]
    fn a_reading_of_silence_has_no_level_and_a_steady_one_does_not_move() {
        assert_eq!(LEVELS.reading(0., 0.), Reading::default());
        assert_eq!(LEVELS.reading(0., 0.04), Reading::default());
        let reading = LEVELS.reading(0.5, 6.83);
        assert!((reading.level_db + 6.02).abs() < 0.2, "{reading:?}");
        assert_eq!(reading.reduction_db, 6.8);
        assert_eq!(LEVELS.reading(0.5001, 6.8301), reading);
    }
}
