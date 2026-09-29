//! The spread holds the LFO of the right side behind the left, and the mix goes from the sound
//! as it came to the wet sound only.

use modulation::{Mode, ModulationState};

use crate::support::{Rig, SECOND, correlation, noise, pitch_in_bins, sine};

/// At spread 0 both sides move together: the same sound in makes the same sound out, to the bit.
#[test]
fn at_spread_zero_both_sides_are_the_same() {
    for mode in Mode::ALL {
        let state = ModulationState {
            mode,
            spread: 0.0,
            depth: 1.0,
            feedback: 0.6,
            rate_hz: 3.0,
            ..ModulationState::default()
        };
        let [left, right] = Rig::new(state, sine(440.0, 0.5)).render(SECOND);
        assert_eq!(left, right, "{mode:?}");
    }
}

/// The pitch of the chorus's copy on the left and on the right, in bins of 10 ms, at a rate of
/// 1 Hz: one LFO cycle is 100 bins.
fn pitch_of_both_sides(spread: f32) -> [Vec<f64>; 2] {
    let state = ModulationState {
        mode: Mode::Chorus,
        rate_hz: 1.0,
        depth: 1.0,
        feedback: 0.0,
        spread,
        mix: 1.0,
    };
    let [left, right] = Rig::new(state, sine(1_000.0, 0.5)).render(4 * SECOND);
    [left, right].map(|side| pitch_in_bins(&side[SECOND..], SECOND / 100))
}

/// The right side bends as the left did half the spread in cycles before: at spread 1 half a
/// cycle, 50 bins, where it goes up as the left goes down.
#[test]
fn the_spread_is_how_far_behind_the_right_side_moves() {
    for spread in [0.0, 0.5, 1.0] {
        let [left, right] = pitch_of_both_sides(spread);
        let behind = (50.0 * spread) as usize;
        let length = left.len() - behind;
        let shifted = correlation(&right[behind..], &left[..length]);
        let as_is = correlation(&right[..length], &left[..length]);
        println!("spread {spread}: {shifted:.4} shifted by {behind} bins, {as_is:.4} as is");
        assert!(shifted > 0.999, "{spread}: {shifted}");
        if spread == 1.0 {
            assert!(as_is < -0.8, "{as_is}");
        }
    }
}

/// Mix 0 is the sound as it came in, to the bit, in every mode.
#[test]
fn mix_zero_is_the_dry_sound() {
    for mode in Mode::ALL {
        let state = ModulationState {
            mode,
            mix: 0.0,
            depth: 1.0,
            feedback: 1.0,
            ..ModulationState::default()
        };
        let output = Rig::new(state, noise(0.5)).render(SECOND / 2);
        let mut source = noise(0.5);
        let input: Vec<[f32; 2]> = (0..SECOND / 2).map(|_| source()).collect();
        for (index, frame) in input.iter().enumerate() {
            assert_eq!([output[0][index], output[1][index]], *frame, "{mode:?}");
        }
    }
}

/// Mix 1 is the wet sound only: a still chorus with no feedback is the sound 12 ms later, to the
/// bit, and nothing of the sound as it came.
#[test]
fn mix_one_is_the_wet_sound_only() {
    let state = ModulationState {
        mode: Mode::Chorus,
        depth: 0.0,
        feedback: 0.0,
        mix: 1.0,
        ..ModulationState::default()
    };
    let [left, right] = Rig::new(state, noise(0.5)).render(SECOND / 2);
    let mut source = noise(0.5);
    let input: Vec<[f32; 2]> = (0..SECOND / 2).map(|_| source()).collect();
    let delay = 12 * SECOND / 1_000;
    assert!(left[..delay].iter().all(|sample| *sample == 0.0));
    for (index, frame) in input[..input.len() - delay].iter().enumerate() {
        assert_eq!([left[index + delay], right[index + delay]], *frame);
    }
}
