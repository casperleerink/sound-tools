//! Every record is stable: nothing that is not a number, nothing louder than its matrix allows,
//! the crossover comes to rest, and a sound that is not a number does not poison it.

use std::f32::consts::SQRT_2;

use proptest::prelude::*;
use sound_core::amplitude;
use utility::{Channels, UtilityState, matrix};

use crate::support::{Rig, SAMPLE_RATE, Signal, noise, peak, sine};

/// The crossover can ring a little over a step: its high pass of the fourth order overshoots.
/// So the output of full scale noise stays under this many times the largest row of the matrix.
const RING: f32 = 2.0;

/// The largest row of the matrix of the channels and the width, which is 2 at width 2.
const WIDEST_ROW: f32 = 2.0;

fn states() -> impl Strategy<Value = UtilityState> {
    (
        -36.0_f32..=36.0,
        -1.0_f32..=1.0,
        0.0_f32..=2.0,
        any::<bool>(),
        50.0_f32..=500.0,
        prop::sample::select(Channels::ALL.to_vec()),
        any::<[bool; 3]>(),
    )
        .prop_map(
            |(gain_db, pan, width, bass_mono, bass_mono_hz, channels, switches)| UtilityState {
                gain_db,
                pan,
                width,
                bass_mono,
                bass_mono_hz,
                channels,
                invert_left: switches[0],
                invert_right: switches[1],
                mute: switches[2],
            },
        )
}

/// The most a record can make of a full scale sample: the largest sum of a row of its matrix.
fn loudest(state: &UtilityState) -> f32 {
    matrix(state)
        .iter()
        .map(|row| row.iter().map(|part| part.abs()).sum::<f32>())
        .fold(0.0, f32::max)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Any record, then an edit to any other while noise plays: the output is a number, never
    /// louder than its record allows, and during the glide from one to the other never louder
    /// than the most gain of the two on the widest matrix.
    #[test]
    fn any_record_and_any_edit_stays_bounded(before in states(), after in states()) {
        let bound = |state: &UtilityState| RING * loudest(state) + 1e-6;
        let most_gain = amplitude(before.gain_db.max(after.gain_db)) * SQRT_2;
        let glide_bound = RING * WIDEST_ROW * most_gain;
        let mut rig = Rig::new(before, noise(1.0));
        let settled = rig.render(SAMPLE_RATE as usize / 10);
        rig.update(after);
        let glide = rig.render(SAMPLE_RATE as usize / 10);
        let arrived = rig.render(SAMPLE_RATE as usize / 10);
        for (rendered, bound) in [
            (settled, bound(&before)),
            (glide, glide_bound),
            (arrived, bound(&after)),
        ] {
            for channel in &rendered {
                prop_assert!(channel.iter().all(|sample| sample.is_finite()));
                prop_assert!(peak(channel) <= bound, "{} over {bound}", peak(channel));
            }
        }
    }
}

/// After the sound stops, the crossover lets go of what it held and does no more work: its
/// output is exactly silent, as a processor that returns at once leaves it.
#[test]
fn after_the_sound_the_crossover_comes_to_rest() {
    let state = UtilityState {
        bass_mono: true,
        bass_mono_hz: 50.0,
        width: 1.5,
        ..UtilityState::default()
    };
    let mut played = 0;
    let signal: Signal = {
        let mut noise = noise(1.0);
        Box::new(move || {
            played += 1;
            if played <= SAMPLE_RATE as usize / 2 {
                noise()
            } else {
                [0.0; 2]
            }
        })
    };
    let mut rig = Rig::new(state, signal);
    rig.render(SAMPLE_RATE as usize);
    let [left, right] = rig.render(SAMPLE_RATE as usize / 10);
    assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
}

/// A frame that is not a number, or far too loud, from anyone else: the crossover holds it, and
/// the sound after it plays as it would have.
#[test]
fn a_sound_that_is_not_a_number_does_not_poison_the_crossover() {
    let state = UtilityState {
        bass_mono: true,
        ..UtilityState::default()
    };
    let mut frame = 0;
    let signal: Signal = {
        let mut tone = sine(1_000.0, [0.5, -0.5]);
        Box::new(move || {
            frame += 1;
            let sample = tone();
            match frame {
                100 => [f32::NAN, f32::INFINITY],
                _ => sample,
            }
        })
    };
    let mut rig = Rig::new(state, signal);
    let [first_left, first_right] = rig.render(SAMPLE_RATE as usize / 2);
    let [left, right] = rig.render(SAMPLE_RATE as usize / 10);
    for channel in [&first_left, &first_right, &left, &right] {
        assert!(channel.iter().all(|sample| sample.is_finite()));
    }
    for channel in [&left, &right] {
        assert!((peak(channel) - 0.5).abs() < 0.01, "{}", peak(channel));
    }
}
