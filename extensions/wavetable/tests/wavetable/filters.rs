//! The filters of a voice: each type and slope holds to the exact response of the filter of
//! the SDK, the routings send each oscillator where they say, and the drive bends.

use sound_core::{FilterSlope, FilterType, svf_response};
use wavetable::WavetableState;
use wavetable::state::{Filter, Oscillator, Routing};

use crate::sound::{plain, steady};
use crate::support::{Harness, SAMPLE_RATE, frames, largest_step, level_at, note, ticks};

/// C3, whose 8th harmonic is the cutoff of these tests.
const C3: f32 = 130.812_78;

fn db(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

/// The level of each of the harmonics `numbers` of C3 in `samples`.
fn levels(samples: &[f32], numbers: &[u32]) -> Vec<f32> {
    let level = |number: &u32| level_at(samples, C3 * *number as f32);
    numbers.iter().map(level).collect()
}

fn render(state: WavetableState) -> Vec<f32> {
    let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 48, 127)], state);
    steady(&mut harness)
}

fn with_filter(filter_1: Filter) -> WavetableState {
    WavetableState {
        filter_1,
        ..plain(Oscillator::default())
    }
}

/// A saw through every type and slope: each harmonic comes out as much quieter as the exact
/// response of the filter says, within half a dB.
#[test]
fn every_type_and_slope_holds_to_the_exact_response() {
    let numbers = [1, 2, 4, 8, 12, 16, 24, 32];
    let open = levels(&render(plain(Oscillator::default())), &numbers);
    for kind in FilterType::ALL {
        for slope in FilterSlope::ALL {
            for resonance in [0.0, 0.5] {
                let filter = Filter {
                    kind,
                    slope,
                    resonance,
                    cutoff_hz: 8.0 * C3,
                    ..Filter::default()
                };
                let filtered = levels(&render(with_filter(filter)), &numbers);
                for ((number, open), filtered) in numbers.iter().zip(&open).zip(&filtered) {
                    let hz = C3 * *number as f32;
                    let (real, imaginary) = svf_response(
                        kind,
                        slope,
                        filter.cutoff_hz,
                        resonance,
                        hz,
                        SAMPLE_RATE as f32,
                    );
                    let expected = db(real.hypot(imaginary) as f32);
                    let measured = db(filtered / open);
                    // Far down in a notch or a slope the harmonic is under the leak of the
                    // others; there it only has to be at least that far down.
                    let close =
                        (measured - expected).abs() < 0.5 || (expected < -40.0 && measured < -35.0);
                    assert!(
                        close,
                        "{kind:?} {slope:?} {resonance} harmonic {number}: {measured} dB, not {expected}"
                    );
                }
            }
        }
    }
}

/// A low pass at 300 Hz as filter 1 and a high pass at 3 kHz as filter 2, with oscillator 2
/// an octave up.
fn routed(routing: Routing) -> Vec<f32> {
    let state = WavetableState {
        osc_2: Oscillator {
            octave: 1,
            ..Oscillator::default()
        },
        filter_1: Filter {
            resonance: 0.0,
            cutoff_hz: 300.0,
            ..Filter::default()
        },
        filter_2: Filter {
            kind: FilterType::HighPass,
            resonance: 0.0,
            cutoff_hz: 3_000.0,
            ..Filter::default()
        },
        routing,
        ..plain(Oscillator::default())
    };
    render(state)
}

#[test]
fn the_routings_send_the_oscillators_through_the_filters_as_they_say() {
    // Harmonic 1 of C3 is only in oscillator 1, and low. Harmonic 32 is in both, and high.
    let numbers = [1, 32];
    let [serial, parallel, split] = [Routing::Serial, Routing::Parallel, Routing::Split]
        .map(|routing| levels(&routed(routing), &numbers));
    let open = {
        let state = WavetableState {
            osc_2: Oscillator {
                octave: 1,
                ..Oscillator::default()
            },
            ..plain(Oscillator::default())
        };
        levels(&render(state), &numbers)
    };
    let relative = |levels: &[f32], index: usize| db(levels[index] / open[index]);
    // Serial: a low pass and then a high pass leave neither the low nor the high harmonic.
    assert!(relative(&serial, 0) < -30.0, "{serial:?}");
    assert!(relative(&serial, 1) < -30.0, "{serial:?}");
    // Parallel: half through each, so each harmonic comes out of one filter at half level.
    assert!((relative(&parallel, 0) + 6.0).abs() < 1.0, "{parallel:?}");
    assert!((relative(&parallel, 1) + 6.0).abs() < 1.5, "{parallel:?}");
    // Split: oscillator 1, the whole low harmonic, through the low pass; oscillator 2 through
    // the high pass, which keeps its high harmonic.
    assert!(relative(&split, 0).abs() < 1.0, "{split:?}");
    let osc_2_high = {
        let state = WavetableState {
            osc_1: Oscillator {
                on: false,
                ..Oscillator::default()
            },
            osc_2: Oscillator {
                octave: 1,
                ..Oscillator::default()
            },
            ..plain(Oscillator::default())
        };
        levels(&render(state), &numbers)[1]
    };
    assert!((db(split[1] / osc_2_high)).abs() < 1.0, "{split:?}");
}

#[test]
fn the_drive_adds_harmonics_and_turning_it_on_does_not_click() {
    let sine = Oscillator {
        position: 0.0,
        ..Oscillator::default()
    };
    let open = Filter {
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        ..Filter::default()
    };
    let clean = WavetableState {
        filter_1: open,
        ..plain(sine)
    };
    let driven = WavetableState {
        filter_1: Filter {
            drive_db: 24.0,
            ..open
        },
        ..plain(sine)
    };
    let third = |state| {
        let samples = render(state);
        levels(&samples, &[3])[0] / levels(&samples, &[1])[0]
    };
    assert!(third(clean.clone()) < 0.001);
    assert!(third(driven.clone()) > 0.05);

    let steepest = |state| {
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 48, 127)], state);
        largest_step(&harness.play_left(frames(0.6))[frames(0.5)..])
    };
    let steady = steepest(driven.clone()).max(steepest(clean.clone()));
    let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 48, 127)], clean);
    harness.play_left(frames(0.6));
    harness.edit(driven);
    let [after, _] = harness.render(frames(0.2));
    // The fade from the clean sine into the driven one adds no step of its own.
    assert!(
        largest_step(&after) < 1.05 * steady,
        "{} {steady}",
        largest_step(&after)
    );
    assert!(after.iter().all(|sample| sample.abs() < 0.2));
}
