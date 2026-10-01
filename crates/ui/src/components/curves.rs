//! The lines that the displays of several devices draw: an envelope with its handles, the
//! shape of an LFO and the response of the state variable filter of the SDK. Places are from
//! 0 to 1 on the display, `y` up, as [`Display`] takes them.

use gpui::{App, ElementId, Point, Window, point};
use sound_core::{EnvelopeCurves, FilterSlope, FilterType, Lfo, LfoShape, svf_response};

use crate::components::display::{Axis, Display, Handle};
use crate::components::gesture::ValueChange;
use crate::components::knob::KnobRange;

/// The four values of an envelope: times in seconds and the sustain as a part of full level.
/// An `Adsr<bool>` says which of them an automation lane moves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adsr<T = f32> {
    pub attack: T,
    pub decay: T,
    pub sustain: T,
    pub release: T,
}

/// A handle of the envelope display, and what it moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeHandle {
    /// The peak: the attack, sideways.
    Attack,
    /// The corner after the decay: the decay sideways and the sustain up and down.
    Decay,
    /// The end: the release, sideways.
    Release,
}

/// Where the envelope sits in its display.
///
/// Each time has a zone of its own across, on the travel of its knob: any time from the start
/// to the end of the range shows, and its handle moves as its knob turns. A stage starts where
/// the one before it ends, so the axis of its handle is the knob's range moved along by that
/// place.
pub mod envelope {
    use crate::components::knob::KnobRange;

    /// Where the attack starts.
    pub const LEFT: f32 = 0.04;
    /// The zone of one time.
    pub const ZONE: f32 = 0.28;
    /// How long a held note is drawn at the sustain level.
    pub const HOLD: f32 = 0.1;
    /// Full level and silence, clear of the edges so a handle there can be taken.
    pub const TOP: f32 = 0.88;
    pub const BOTTOM: f32 = 0.08;

    /// The range of a time whose zone starts at `start`: `time.position(value)` of the knob,
    /// squeezed into the zone and moved to its start. A logarithmic range stays one when it is
    /// stretched and moved, with other ends.
    pub fn time_axis(time: KnobRange, start: f32) -> KnobRange {
        let ratio = time.max / time.min;
        let min = time.min * ratio.powf(-start / ZONE);
        KnobRange::logarithmic(min, min * ratio.powf(1. / ZONE))
    }

    /// The range of the sustain level, from silence at `BOTTOM` to full level at `TOP`.
    pub fn level_axis() -> KnobRange {
        let min = -BOTTOM / (TOP - BOTTOM);
        KnobRange::linear(min, min + 1. / (TOP - BOTTOM))
    }

    /// The places of the stages: the peak after the attack, the end of the decay, the end of
    /// the hold and the end of the release.
    pub fn stages(time: KnobRange, attack: f32, decay: f32, release: f32) -> [f32; 4] {
        let peak = LEFT + ZONE * time.position(attack);
        let decayed = peak + ZONE * time.position(decay);
        let held = decayed + HOLD;
        [peak, decayed, held, held + ZONE * time.position(release)]
    }
}

/// Points of each curved stage.
const STAGE_POINTS: usize = 24;

/// The line of an envelope whose times travel on `time`, each stage bent by its curve of
/// `[attack, decay, release]`, from 0, straight, to 1, as
/// [`EnvelopeCurves`](sound_core::EnvelopeCurves) bends it. The release is drawn from the
/// sustain level over its whole zone.
pub fn envelope_curve(time: KnobRange, adsr: Adsr, curves: [f32; 3]) -> Vec<Point<f32>> {
    use envelope::{BOTTOM, LEFT, TOP};
    let [peak, decayed, held, released] =
        envelope::stages(time, adsr.attack, adsr.decay, adsr.release);
    let level = BOTTOM + adsr.sustain.clamp(0., 1.) * (TOP - BOTTOM);
    let [attack, decay, release] = curves;
    // A stage from `(x, y)` to `(to_x, to_y)`, without its first point.
    let stage = |(x, y): (f32, f32), (to_x, to_y): (f32, f32), curve: f32| {
        (1..=STAGE_POINTS).map(move |step| {
            let part = step as f32 / STAGE_POINTS as f32;
            let progress = EnvelopeCurves::progress(curve, part);
            point(x + (to_x - x) * part, y + (to_y - y) * progress)
        })
    };
    std::iter::once(point(LEFT, BOTTOM))
        .chain(stage((LEFT, BOTTOM), (peak, TOP), attack))
        .chain(stage((peak, TOP), (decayed, level), decay))
        .chain([point(held, level)])
        .chain(stage((held, level), (released, BOTTOM), release))
        .collect()
}

/// A display of an envelope, with a handle at the end of each stage, that drag as the knobs
/// of the times and the sustain do. `on_change` hears which handle moved and how: the
/// owner holds them to the ranges of its fields, since a handle may ask for one past an end.
/// A handle that moves a value an automation lane moves, as `automated` says, does not drag.
/// The owner adds the caption and any control at the top.
pub fn envelope_display(
    id: impl Into<ElementId>,
    width: f32,
    time: KnobRange,
    (adsr, defaults, automated): (Adsr, Adsr, Adsr<bool>),
    curves: [f32; 3],
    on_change: impl Fn((EnvelopeHandle, ValueChange<Point<f32>>), &mut Window, &mut App) + 'static,
) -> Display {
    use envelope::{BOTTOM, LEFT, TOP};
    let [peak, _, held, _] = envelope::stages(time, adsr.attack, adsr.decay, adsr.release);
    let on_change = std::rc::Rc::new(on_change);
    let handle = |which: EnvelopeHandle, name: &'static str, x: Axis, y: Axis| {
        let on_change = on_change.clone();
        let held = match which {
            EnvelopeHandle::Attack => automated.attack,
            EnvelopeHandle::Decay => automated.decay || automated.sustain,
            EnvelopeHandle::Release => automated.release,
        };
        Handle::new(name, x, y)
            .automated(held)
            .on_change(move |change, window, cx| on_change((which, change), window, cx))
    };
    let attack = handle(
        EnvelopeHandle::Attack,
        "attack",
        Axis::new(
            envelope::time_axis(time, LEFT),
            adsr.attack,
            defaults.attack,
        ),
        Axis::fixed(TOP),
    );
    let decay = handle(
        EnvelopeHandle::Decay,
        "decay",
        Axis::new(envelope::time_axis(time, peak), adsr.decay, defaults.decay),
        Axis::new(envelope::level_axis(), adsr.sustain, defaults.sustain),
    );
    let release = handle(
        EnvelopeHandle::Release,
        "release",
        Axis::new(
            envelope::time_axis(time, held),
            adsr.release,
            defaults.release,
        ),
        Axis::fixed(BOTTOM),
    );
    Display::new(id, width)
        .curve(envelope_curve(time, adsr, curves))
        .handle(attack)
        .handle(decay)
        .handle(release)
}

/// Points of an LFO line across the display. Its steps are steep at this many.
const LFO_POINTS: usize = 192;

/// `cycles` of an LFO of `shape` across the display, swinging `swing` up and down about
/// `middle`: the same wave the [`Lfo`] of the SDK plays, a sample and hold with its levels of
/// seed 0.
pub fn lfo_line(shape: LfoShape, cycles: f32, middle: f32, swing: f32) -> Vec<Point<f32>> {
    let lfo = Lfo::default();
    (0..=LFO_POINTS)
        .map(|step| {
            let x = step as f32 / LFO_POINTS as f32;
            // Just short of the end, so the last point is in the last cycle and not the next.
            let phase = (cycles * x).min(cycles - 1e-4);
            point(x, middle + swing * lfo.value(shape, -phase))
        })
        .collect()
}

/// The response of a filter across a display, from 20 Hz to 20 kHz on a scale of ratios, the
/// travel of a cutoff knob.
pub const RESPONSE_ACROSS: KnobRange = KnobRange::logarithmic(20., 20_000.);

/// The gains a response display shows, in dB. The top leaves room for the peak of full
/// resonance, +11.5 dB.
const RESPONSE_DB: (f32, f32) = (-36., 18.);

/// The sample rate a response is drawn for. The curve of another rate differs only near the
/// top of the scale, where the display has no room to show it.
pub const DRAWN_AT: f32 = 48_000.;

/// Points of a response across the display.
const RESPONSE_POINTS: usize = 96;

/// The scale under a response display: the decades of [`response_decades`].
pub const RESPONSE_CAPTION: &str = "100 · 1k · 10k";

/// Where a gain in dB is on a response display, from 0 at the bottom to 1 at the top.
pub fn response_height(db: f32) -> f32 {
    let (bottom, top) = RESPONSE_DB;
    ((db - bottom) / (top - bottom)).clamp(0., 1.)
}

/// The response across the display, from the gain as a factor at each frequency.
pub fn response_curve(gain: impl Fn(f32) -> f32) -> Vec<Point<f32>> {
    (0..=RESPONSE_POINTS)
        .map(|step| {
            let x = step as f32 / RESPONSE_POINTS as f32;
            let gain = gain(RESPONSE_ACROSS.value(x));
            point(x, response_height(20. * gain.max(1e-6).log10()))
        })
        .collect()
}

/// The gain of the state variable filter of the SDK at `hz`, as a factor.
pub fn svf_gain(
    kind: FilterType,
    slope: FilterSlope,
    cutoff_hz: f32,
    resonance: f32,
    hz: f32,
) -> f32 {
    let (real, imaginary) = svf_response(kind, slope, cutoff_hz, resonance, hz, DRAWN_AT);
    real.hypot(imaginary) as f32
}

/// Up and down on a response display is resonance, placed so that a handle at the cutoff sits
/// on the peak of a low or high pass: its gain at the cutoff is the product of the Qs of the
/// sections, which is a straight line in dB over resonance. So the handle is where the curve
/// is, and the range of its travel is that line stretched over the height of the display.
pub fn resonance_travel(slope: FilterSlope) -> KnobRange {
    let db_at = |resonance| {
        let gain = svf_gain(FilterType::LowPass, slope, 1_000., resonance, 1_000.);
        20. * gain.log10()
    };
    let (at_none, at_full) = (response_height(db_at(0.)), response_height(db_at(1.)));
    let per_resonance = at_full - at_none;
    KnobRange::linear(-at_none / per_resonance, (1. - at_none) / per_resonance)
}

/// The places across of 100 Hz, 1 kHz and 10 kHz, the scale under a response display.
pub fn response_decades() -> Vec<f32> {
    [100., 1_000., 10_000.]
        .map(|hz| RESPONSE_ACROSS.position(hz))
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: KnobRange = KnobRange::logarithmic(0.001, 10.);

    /// A handle at the place of a time gives that time back, with the digits its knob gives.
    #[test]
    fn a_time_handle_is_where_its_knob_says_and_gives_its_value_back() {
        for start in [envelope::LEFT, 0.3, 0.62] {
            let axis = envelope::time_axis(TIME, start);
            for value in [0.001, 0.005, 0.2, 1.5, 10.0] {
                let place = axis.position(value);
                let expected = start + envelope::ZONE * TIME.position(value);
                assert!((place - expected).abs() < 1e-4, "{start} {value}: {place}");
                assert!((axis.value(place) - value).abs() <= value * 1e-3, "{value}");
            }
        }
    }

    #[test]
    fn the_sustain_handle_runs_from_silence_to_full_level() {
        let level = envelope::level_axis();
        assert!((level.position(0.) - envelope::BOTTOM).abs() < 1e-6);
        assert!((level.position(1.) - envelope::TOP).abs() < 1e-6);
        assert_eq!(level.value(level.position(0.25)), 0.25);
    }

    /// The longest envelope still fits the display, and the line goes through the corners the
    /// handles are on.
    #[test]
    fn every_envelope_fits_its_display_and_passes_its_handles() {
        let [_, _, _, end] = envelope::stages(TIME, 10., 10., 10.);
        assert!(end <= 1., "{end}");
        let adsr = Adsr {
            attack: 0.01,
            decay: 0.3,
            sustain: 0.5,
            release: 1.,
        };
        let line = envelope_curve(TIME, adsr, [0.5, 0.8, 1.]);
        let [peak, decayed, _, released] = envelope::stages(TIME, 0.01, 0.3, 1.);
        let level = envelope::level_axis().position(0.5);
        for corner in [
            point(peak, envelope::TOP),
            point(decayed, level),
            point(released, envelope::BOTTOM),
        ] {
            let near = line
                .iter()
                .any(|place| (*place - corner).x.abs() < 1e-5 && (place.y - corner.y).abs() < 1e-5);
            assert!(near, "{corner:?}");
        }
    }

    #[test]
    fn an_lfo_line_is_the_wave_of_the_sdk() {
        let sine = lfo_line(LfoShape::Sine, 1., 0.5, 0.25);
        let quarter = sine[LFO_POINTS / 4];
        assert!((quarter.y - 0.75).abs() < 1e-4, "{quarter:?}");
        let square = lfo_line(LfoShape::Square, 2., 0.5, 0.25);
        assert_eq!(square[1].y, 0.75);
        assert_eq!(square[LFO_POINTS / 4 + 1].y, 0.25);
    }

    /// The handle sits on the peak of a low pass, at every resonance and both slopes.
    #[test]
    fn the_resonance_travel_puts_a_handle_on_the_peak() {
        for slope in FilterSlope::ALL {
            for resonance in [0., 0.3, 0.7, 1.] {
                let handle = resonance_travel(slope).position(resonance);
                let gain = svf_gain(FilterType::LowPass, slope, 1_000., resonance, 1_000.);
                let curve = response_height(20. * gain.log10());
                assert!((handle - curve).abs() < 1e-4, "{slope:?} {resonance}");
            }
        }
    }
}
