//! Every setting is stable: no blow-up at full resonance, nothing that is not a number, the
//! filter comes to rest, and a render is the same every time.

use filter::{FilterState, FilterType, LFO_DEPTH, LFO_RATE, Slope};
use proptest::prelude::*;
use sound_core::Processor;

use crate::support::{Rig, SAMPLE_RATE, Signal, noise, peak, sine};

/// The loudest the filter makes a full scale input: the peak of full resonance, +11.5 dB or
/// 3.76 times, plus a little for the ring of a step.
const BOUND: f32 = 4.0;

/// The drive curve holds what goes into the filter under 1.5.
const DRIVEN_BOUND: f32 = BOUND * 1.5;

fn assert_bounded(label: &str, bound: f32, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    assert!(loudest < bound, "{label}: peak {loudest}");
}

/// A square at full scale, whose every edge rings a resonant filter.
fn square(hz: f64) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(SAMPLE_RATE);
    Box::new(move || {
        phase = (phase + step).fract();
        [if phase < 0.5 { 1.0 } else { -1.0 }; 2]
    })
}

#[test]
fn full_resonance_at_every_type_slope_and_end_of_the_range_stays_bounded() {
    for kind in FilterType::ALL {
        for slope in Slope::ALL {
            for cutoff_hz in [20.0, 1_000.0, 20_000.0] {
                for drive_db in [0.0, 24.0] {
                    let state = FilterState {
                        kind,
                        slope,
                        cutoff_hz,
                        resonance: 1.0,
                        drive_db,
                        ..FilterState::default()
                    };
                    let label = format!("{kind:?} {slope:?} {cutoff_hz} Hz drive {drive_db}");
                    let bound = if drive_db > 0.0 { DRIVEN_BOUND } else { BOUND };
                    let mut rig = Rig::new(state, square(f64::from(cutoff_hz) / 3.0));
                    assert_bounded(&label, bound, &rig.render(SAMPLE_RATE as usize));
                    let mut rig = Rig::new(state, noise(1.0));
                    assert_bounded(&label, bound, &rig.render(SAMPLE_RATE as usize));
                }
            }
        }
    }
}

/// The LFO at its fastest and deepest pushes the cutoff past both ends of the range, at full
/// resonance.
#[test]
fn the_deepest_fastest_lfo_at_full_resonance_stays_bounded() {
    for slope in Slope::ALL {
        let state = FilterState {
            slope,
            resonance: 1.0,
            lfo_rate_hz: LFO_RATE.max,
            lfo_depth_octaves: LFO_DEPTH.max,
            ..FilterState::default()
        };
        let mut rig = Rig::new(state, noise(1.0));
        assert_bounded(
            &format!("{slope:?}"),
            BOUND,
            &rig.render(2 * SAMPLE_RATE as usize),
        );
    }
}

/// A sample that is not a number, or is infinite, is silence or the limit to the filter. The
/// filter goes on as if nothing had happened.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_output() {
    let mut frame = 0_usize;
    let mut clean = sine(440.0, 0.5);
    let broken: Signal = Box::new(move || {
        frame += 1;
        match frame {
            1_000 => [f32::NAN, f32::INFINITY],
            2_000 => [f32::NEG_INFINITY, f32::NAN],
            3_000 => [f32::MAX, -f32::MAX],
            _ => clean(),
        }
    });
    let state = FilterState {
        resonance: 1.0,
        ..FilterState::default()
    };
    let output = Rig::new(state, broken).render(SAMPLE_RATE as usize);
    for sample in output.iter().flatten() {
        assert!(sample.is_finite());
    }
    // The limit rings a resonant filter, and that ring dies away like any other.
    let [left, _] = &output;
    let tail = &left[SAMPLE_RATE as usize / 2..];
    assert!(peak(tail) < 0.5 * BOUND, "{}", peak(tail));
}

/// After the sound ends the filter rings out and then is exactly silent: its memory lets go,
/// and it does no work.
#[test]
fn after_the_sound_the_filter_comes_to_rest_and_is_silent() {
    let mut frames = 0_usize;
    let mut sound = noise(0.5);
    let burst: Signal = Box::new(move || {
        frames += 1;
        if frames <= 4_800 { sound() } else { [0.0; 2] }
    });
    let state = FilterState {
        slope: Slope::TwentyFour,
        resonance: 1.0,
        cutoff_hz: 20.0,
        ..FilterState::default()
    };
    let mut rig = Rig::new(state, burst);
    let [ringing, _] = rig.render(SAMPLE_RATE as usize);
    assert!(peak(&ringing[4_800..]) > 0.0);
    // The slowest ring, 20 Hz at full resonance, is gone in a few seconds.
    rig.render(10 * SAMPLE_RATE as usize);
    let [rest, other] = rig.render(SAMPLE_RATE as usize);
    assert!(rest.iter().chain(&other).all(|sample| *sample == 0.0));
}

/// A full scale sine that sweeps across the cutoff at full resonance, the loudest thing a
/// resonant filter meets: it stays under the bound, for every cutoff and slope.
#[test]
fn a_full_scale_sweep_across_the_cutoff_at_full_resonance_stays_bounded() {
    for slope in Slope::ALL {
        for cutoff_hz in [100.0, 1_000.0, 10_000.0] {
            let state = FilterState {
                slope,
                cutoff_hz,
                resonance: 1.0,
                ..FilterState::default()
            };
            // Up from two octaves under the cutoff to two over it in two seconds, on a ratio.
            let (from, octaves, seconds) = (f64::from(cutoff_hz) / 4.0, 4.0, 2.0);
            let mut phase = 0.0_f64;
            let mut frame = 0_u64;
            let sweep: Signal = Box::new(move || {
                let time = frame as f64 / f64::from(SAMPLE_RATE);
                let hz = from * 2_f64.powf(octaves * (time / seconds).min(1.0));
                phase = (phase + hz / f64::from(SAMPLE_RATE)).fract();
                frame += 1;
                [(std::f64::consts::TAU * phase).sin() as f32; 2]
            });
            let mut rig = Rig::new(state, sweep);
            let output = rig.render(2 * SAMPLE_RATE as usize);
            let loudest = peak(&output[0]);
            println!("{slope:?} at {cutoff_hz} Hz: peak {loudest:.3}");
            assert_bounded(&format!("{slope:?} {cutoff_hz}"), BOUND, &output);
            // And it did go through the peak. A low cutoff rings slowly, so a sweep this fast
            // gets to half of its steady peak there.
            let steady = filter::response(&state, cutoff_hz, SAMPLE_RATE as f32);
            assert!(loudest > steady * 0.5, "{slope:?} {cutoff_hz}: {loudest}");
        }
    }
}

#[test]
fn renders_are_the_same_every_time_to_the_byte() {
    let state = FilterState {
        kind: FilterType::BandPass,
        slope: Slope::TwentyFour,
        resonance: 0.7,
        drive_db: 9.0,
        mix: 0.8,
        lfo_rate_hz: 3.0,
        lfo_depth_octaves: 2.0,
        ..FilterState::default()
    };
    let render = || {
        let mut rig = Rig::new(state, noise(0.8));
        let first = rig.render(SAMPLE_RATE as usize);
        rig.update(FilterState {
            kind: FilterType::Notch,
            ..state
        });
        let [left, right] = rig.render(SAMPLE_RATE as usize);
        let bytes = |samples: &[f32]| -> Vec<u8> {
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect()
        };
        [first[0].clone(), first[1].clone(), left, right].map(|samples| bytes(&samples))
    };
    assert_eq!(render(), render());
}

#[test]
fn the_filter_has_no_latency() {
    assert_eq!(filter::Filter::new(FilterState::default()).latency(), 0);
}

fn any_state() -> impl Strategy<Value = FilterState> {
    (
        0..4_usize,
        any::<bool>(),
        20.0_f32..=20_000.0,
        0.0_f32..=1.0,
        0.0_f32..=24.0,
        0.0_f32..=1.0,
        0.05_f32..=20.0,
        0.0_f32..=4.0,
    )
        .prop_map(
            |(kind, steep, cutoff_hz, resonance, drive_db, mix, lfo_rate_hz, lfo_depth_octaves)| {
                FilterState {
                    kind: FilterType::ALL[kind],
                    slope: if steep {
                        Slope::TwentyFour
                    } else {
                        Slope::Twelve
                    },
                    cutoff_hz,
                    resonance,
                    drive_db,
                    mix,
                    lfo_rate_hz,
                    lfo_depth_octaves,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any valid records, one after the other every few milliseconds while full scale noise
    /// plays: the output is always a number and bounded.
    #[test]
    fn any_edits_while_it_plays_keep_the_output_bounded(
        states in proptest::collection::vec(any_state(), 2..12),
    ) {
        let mut rig = Rig::new(states[0], noise(1.0));
        for state in &states {
            rig.update(*state);
            let output = rig.render(480);
            for sample in output.iter().flatten() {
                prop_assert!(sample.is_finite());
                prop_assert!(sample.abs() < DRIVEN_BOUND, "{sample} after {state:?}");
            }
        }
    }
}
