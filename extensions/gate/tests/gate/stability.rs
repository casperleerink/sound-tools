//! Input that is not a number, is infinite or is the largest a sample can be does not reach the
//! output, and the gate goes on. Sound under -120 dB is in `shaper.rs`.

use gate::{GateState, SUSTAIN, TRANSIENT};

use crate::support::{Rig, SAMPLE_RATE, Signal, peak, sine};

const FRAMES: usize = 4 * SAMPLE_RATE as usize;

/// A sample that is not a number is silence to the gate, an infinite one the limit, with the
/// gate and both shaper gains at work.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_output() {
    let broken = || -> Signal {
        let mut frame = 0_usize;
        let mut clean = sine(440.0, 0.5);
        Box::new(move || {
            frame += 1;
            match frame {
                1_000 => [f32::NAN, f32::INFINITY],
                2_000 => [f32::NEG_INFINITY, f32::NAN],
                3_000 => [f32::MAX, -f32::MAX],
                4_000 => [f32::MIN_POSITIVE / 4.0, -f32::MIN_POSITIVE / 8.0],
                _ => clean(),
            }
        })
    };
    for (transient_db, sustain_db) in [(TRANSIENT.max, SUSTAIN.min), (TRANSIENT.min, SUSTAIN.max)] {
        let state = GateState {
            threshold_db: -20.0,
            transient_db,
            sustain_db,
            ..GateState::default()
        };
        let [left, right] = Rig::new(state, broken()).render_in_blocks(FRAMES, 480);
        for sample in left.iter().chain(&right) {
            assert!(sample.is_finite(), "{state:?}: {sample}");
        }
        // The sine still plays: the limit is a loud hit, whose tail the shaper still moves
        // after seconds, so the level is not the clean one yet.
        assert!(peak(&left[FRAMES - 4_800..]) > 0.25, "{state:?}");
    }
}
