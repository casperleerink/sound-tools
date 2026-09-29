//! At the defaults every sample comes out as it went in. Every other record does what its
//! matrix says, measured sample by sample, and its pan is the pan of a track.

use utility::{Channels, UtilityState, matrix};

use crate::support::{Rig, SAMPLE_RATE, frames_of, noise};

const FRAMES: usize = SAMPLE_RATE as usize / 2;

/// Noise at full scale, and past it: a utility at its defaults does not even hold what is too
/// loud.
fn loud_noise() -> crate::support::Signal {
    noise(4.0)
}

#[test]
fn at_the_defaults_every_sample_comes_out_as_it_went_in() {
    let input = frames_of(loud_noise(), FRAMES);
    let mut rig = Rig::new(UtilityState::default(), loud_noise());
    assert_eq!(rig.render(FRAMES), input);

    // A bass mono frequency with bass mono off changes nothing either.
    let state = UtilityState {
        bass_mono_hz: 300.0,
        ..UtilityState::default()
    };
    let mut rig = Rig::new(state, loud_noise());
    assert_eq!(rig.render(FRAMES), input);
}

/// After an edit and its undo, once the glide back is over, the sound is as it came again.
#[test]
fn back_at_the_defaults_after_an_edit_every_sample_is_as_it_came_again() {
    let input = frames_of(loud_noise(), 3 * FRAMES);
    let mut rig = Rig::new(UtilityState::default(), loud_noise());
    rig.update(UtilityState {
        gain_db: -6.0,
        width: 1.5,
        bass_mono: true,
        invert_left: true,
        ..UtilityState::default()
    });
    let [edited, _] = rig.render(FRAMES);
    assert_ne!(edited, input[0][..FRAMES]);
    rig.update(UtilityState::default());
    // The glide back, 20 ms, inside one block of the engine.
    rig.render(FRAMES);
    let [left, right] = rig.render(FRAMES);
    assert_eq!(left, input[0][2 * FRAMES..]);
    assert_eq!(right, input[1][2 * FRAMES..]);
}

/// Each output channel is the matrix of the record times the input, frame by frame.
#[test]
fn every_record_does_what_its_matrix_says() {
    let default = UtilityState::default();
    let states = [
        UtilityState {
            gain_db: -6.0,
            ..default
        },
        UtilityState {
            gain_db: 36.0,
            ..default
        },
        UtilityState {
            pan: -0.4,
            ..default
        },
        UtilityState {
            pan: 1.0,
            ..default
        },
        UtilityState {
            width: 0.0,
            ..default
        },
        UtilityState {
            width: 0.5,
            ..default
        },
        UtilityState {
            width: 2.0,
            ..default
        },
        UtilityState {
            channels: Channels::Left,
            ..default
        },
        UtilityState {
            channels: Channels::Right,
            ..default
        },
        UtilityState {
            channels: Channels::Swap,
            invert_right: true,
            ..default
        },
        UtilityState {
            invert_left: true,
            invert_right: true,
            ..default
        },
        UtilityState {
            gain_db: 12.0,
            pan: 0.7,
            width: 1.7,
            channels: Channels::Swap,
            invert_left: true,
            ..default
        },
        UtilityState {
            mute: true,
            ..default
        },
    ];
    let [left_in, right_in] = frames_of(noise(0.5), FRAMES);
    for state in states {
        let [[left_left, left_right], [right_left, right_right]] = matrix(&state);
        let mut rig = Rig::new(state, noise(0.5));
        let [left, right] = rig.render(FRAMES);
        for frame in 0..FRAMES {
            let (l, r) = (left_in[frame], right_in[frame]);
            let expected = [
                left_left * l + left_right * r,
                right_left * l + right_right * r,
            ];
            for (heard, expected) in [left[frame], right[frame]].into_iter().zip(expected) {
                assert!(
                    (heard - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                    "{state:?}, frame {frame}: {heard} where {expected}"
                );
            }
        }
    }
}

/// The pan of a utility is the pan of a track: a sound in the middle keeps its level, panned
/// away one channel is exactly silent and the other 3 dB up.
#[test]
fn the_pan_is_the_pan_law_of_a_track() {
    for pan in [-1.0, -0.3, 0.0, 0.6, 1.0] {
        let state = UtilityState {
            pan,
            ..UtilityState::default()
        };
        let [[left, _], [_, right]] = matrix(&state);
        assert_eq!([left, right], sound_core::pan_gains(1.0, pan), "{pan}");
    }
}
