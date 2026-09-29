//! The display of a limiter: the last four seconds of what it sent out, in green under the
//! ceiling line, and how much it took, hanging from the top. The handle at the right end of the
//! ceiling line drags the ceiling up and down. The limiter of the master and the Limiter effect
//! both show it.
//!
//! The owner keeps a [`LimiterHistory`], feeds it one reading per poll, and builds the display
//! from it when it renders, with a handle whose change it handles itself.

use std::collections::VecDeque;

use gpui::{
    App, Bounds, ElementId, Hsla, Pixels, Window, canvas, fill, point, prelude::*, px, size,
};

use crate::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use crate::components::knob::{KnobRange, short};
use crate::components::meter::GainReduction;
use crate::metering::decibels;
use crate::theme::ActiveTheme;

/// The width of the display. With two columns of cells its card is 352 pt.
pub const WIDTH: f32 = 200.;
/// Columns of the history, and the polls each one gathers: 50 columns of 80 ms, four seconds.
pub const COLUMNS: usize = 50;
pub const POLLS_PER_COLUMN: u32 = 5;
/// Where the ceiling handle sits across the display.
const HANDLE_ACROSS: f32 = 0.96;

/// One column of the history: the loudest the limiter sent out and the most it took, in dB,
/// over its 80 ms.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Moment {
    peak_db: f32,
    reduction_db: f32,
}

impl Moment {
    const QUIET: Self = Self {
        peak_db: f32::NEG_INFINITY,
        reduction_db: 0.0,
    };
}

/// The last four seconds, the column being gathered, and the scale of the ceiling they are
/// drawn on.
pub struct LimiterHistory {
    ceiling: KnobRange,
    moments: VecDeque<Moment>,
    gathering: Option<Moment>,
    polls: u32,
}

impl LimiterHistory {
    /// A history drawn on the travel of the ceiling, from its lowest at the bottom of the
    /// display to its highest at the top.
    pub fn new(ceiling: KnobRange) -> Self {
        Self {
            ceiling,
            moments: VecDeque::new(),
            gathering: None,
            polls: 0,
        }
    }

    fn is_quiet(&self, moment: &Moment) -> bool {
        moment.peak_db < self.ceiling.min && moment.reduction_db <= 0.0
    }

    /// One poll: the loudest sample sent out and the largest reduction, as the factor the sound
    /// was above what came out, since the last one. Whether a column was finished that changes
    /// what the display shows.
    pub fn read(&mut self, peak: f32, reduction: f32) -> bool {
        let now = Moment {
            peak_db: decibels(peak),
            reduction_db: decibels(reduction).max(0.0),
        };
        let gathering = self.gathering.get_or_insert(Moment::QUIET);
        gathering.peak_db = gathering.peak_db.max(now.peak_db);
        gathering.reduction_db = gathering.reduction_db.max(now.reduction_db);
        self.polls += 1;
        if self.polls < POLLS_PER_COLUMN {
            return false;
        }
        self.polls = 0;
        let moment = self.gathering.take().unwrap_or(Moment::QUIET);
        // At rest, a quiet column in a quiet history changes nothing and costs no frame.
        if self.is_quiet(&moment) && self.moments.iter().all(|moment| self.is_quiet(moment)) {
            return false;
        }
        if self.moments.len() == COLUMNS {
            self.moments.pop_front();
        }
        self.moments.push_back(moment);
        true
    }

    /// The largest reduction of the last column, for the line under the display.
    pub fn reduction_now(&self) -> f32 {
        self.moments
            .back()
            .map_or(0.0, |moment| moment.reduction_db)
    }

    /// The handle at the right end of the ceiling line. The owner gives it its change.
    pub fn handle(&self, ceiling_db: f32, default_db: f32) -> Handle {
        Handle::new(
            "ceiling",
            Axis::fixed(HANDLE_ACROSS),
            Axis::new(self.ceiling, ceiling_db, default_db),
        )
    }

    /// The history under the ceiling line, with its handle and the ceiling and the reduction
    /// under it.
    pub fn display(
        &self,
        id: impl Into<ElementId>,
        ceiling_db: f32,
        handle: Handle,
        cx: &App,
    ) -> Display {
        let range = self.ceiling;
        let ceiling = range.position(ceiling_db);
        let theme = cx.theme();
        let colors = (theme.green, theme.gray_950);
        let moments: Vec<Moment> = self.moments.iter().copied().collect();
        let history = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_history(bounds, &moments, range, colors, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .w(px(WIDTH))
        .h(px(INSET_HEIGHT));
        let caption = format!(
            "Ceiling {} dB · GR {}",
            short(ceiling_db),
            reduction_readout(self.reduction_now())
        );
        Display::new(id, WIDTH)
            .curve([point(0., ceiling), point(1., ceiling)])
            .handle(handle)
            .caption(caption)
            .child(history)
    }
}

/// The bars of the history: the peaks in green on the scale of the ceiling line, and the
/// reduction hanging from the top, 24 dB for the whole height.
fn paint_history(
    bounds: Bounds<Pixels>,
    moments: &[Moment],
    range: KnobRange,
    (green, reduction): (Hsla, Hsla),
    window: &mut Window,
) {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let inset = 6.;
    let step = (width - 2. * inset) / COLUMNS as f32;
    let bar = (step - 1.).max(1.);
    // The newest column at the right, as time goes.
    let first = COLUMNS - moments.len();
    for (index, moment) in moments.iter().enumerate() {
        let x = inset + (first + index) as f32 * step;
        let up = match moment.peak_db.is_finite() {
            true => range.position(moment.peak_db),
            false => 0.,
        };
        if up > 0. {
            let top = height * (1. - up);
            let body = Bounds::new(
                bounds.origin + point(px(x), px(top)),
                size(px(bar), px(height - top)),
            );
            window.paint_quad(fill(body, green));
        }
        // Narrower than the level and over it, so the two read apart where they meet under
        // the ceiling.
        let down = (moment.reduction_db / GainReduction::RANGE_DB).clamp(0., 1.) * height;
        if down > 0. {
            let thin = (bar / 3.).max(1.);
            let left = x + (bar - thin) / 2.;
            let body = Bounds::new(
                bounds.origin + point(px(left), px(0.)),
                size(px(thin), px(down)),
            );
            window.paint_quad(fill(body, reduction));
        }
    }
}

/// The reduction under the display, to a tenth of a dB: `0 dB`, `-4.1 dB`.
fn reduction_readout(db: f32) -> String {
    match db < 0.05 {
        true => "0 dB".into(),
        false => format!("-{db:.1} dB"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_gathers_the_loudest_of_its_polls_and_rest_costs_nothing() {
        let mut history = LimiterHistory::new(KnobRange::linear(-24., 0.));
        // Silence from the start never draws.
        for _ in 0..3 * POLLS_PER_COLUMN {
            assert!(!history.read(0.0, 0.0));
        }
        let mut finished = Vec::new();
        for poll in 0..POLLS_PER_COLUMN {
            finished.push(history.read(0.1 * poll as f32, 2.0));
        }
        assert_eq!(finished.iter().filter(|done| **done).count(), 1);
        let moment = history.moments[0];
        assert!((moment.peak_db - 20.0 * 0.4_f32.log10()).abs() < 1e-4);
        assert!((moment.reduction_db - 6.0206).abs() < 1e-3);
        // Four seconds at most.
        for _ in 0..(COLUMNS as u32 + 10) * POLLS_PER_COLUMN {
            history.read(0.5, 1.0);
        }
        assert_eq!(history.moments.len(), COLUMNS);
    }

    #[test]
    fn the_reduction_reads_to_a_tenth_of_a_decibel() {
        assert_eq!(reduction_readout(0.0), "0 dB");
        assert_eq!(reduction_readout(0.04), "0 dB");
        assert_eq!(reduction_readout(4.12), "-4.1 dB");
    }
}
