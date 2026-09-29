//! At depth 0 the LFO holds still, and the effect is a fixed filter: what a quiet sine comes out
//! with is what `response` says, in every mode, at every feedback and mix.

use modulation::{Mode, ModulationState, response};

use crate::support::{Rig, SAMPLE_RATE, SECOND, amplitude_at, db, sine};

const AMPLITUDE: f32 = 0.01;

/// The gain of `state` at `hz`, measured once the loop has settled, as a factor.
fn measured(state: ModulationState, hz: f64) -> f64 {
    let mut rig = Rig::new(state, sine(hz, AMPLITUDE));
    // A feedback of 0.9 around 1.5 ms falls by 60 dB in about 100 ms.
    rig.render(SECOND);
    let cycles = (hz * 0.25).ceil();
    let window = (cycles * f64::from(SAMPLE_RATE) / hz).round() as usize;
    let [left, right] = rig.render(window);
    assert_eq!(left, right);
    amplitude_at(&left, SECOND, hz) / f64::from(AMPLITUDE)
}

#[test]
fn a_still_lfo_sounds_as_the_response_says() {
    for mode in Mode::ALL {
        for feedback in [0.0, 0.5, 1.0] {
            for mix in [0.5, 1.0] {
                let state = ModulationState {
                    mode,
                    depth: 0.0,
                    feedback,
                    mix,
                    ..ModulationState::default()
                };
                for hz in [100.0, 333.0, 700.0, 1_000.0, 2_500.0, 9_000.0] {
                    let expected = f64::from(response(&state, hz as f32, SAMPLE_RATE as f32));
                    let heard = measured(state, hz);
                    assert!(
                        (heard - expected).abs() < 0.002 + 0.002 * expected,
                        "{mode:?}, feedback {feedback}, mix {mix}, {hz} Hz: {heard} against {expected}"
                    );
                }
            }
        }
    }
}

/// With the dry sound at half, the copy of a flanger 1.5 ms behind takes out 333 Hz and its odd
/// multiples, and gives back the even ones whole. The middle notch of the phaser is at 1 kHz,
/// where the six allpass filters turn it by 540°.
#[test]
fn the_notches_are_where_the_delay_and_the_allpass_filters_put_them() {
    let still = |mode| ModulationState {
        mode,
        depth: 0.0,
        feedback: 0.0,
        mix: 0.5,
        ..ModulationState::default()
    };
    let flanger = still(Mode::Flanger);
    for notch in [1_000.0 / 3.0, 1_000.0, 5_000.0 / 3.0] {
        let heard = db(measured(flanger, notch));
        assert!(heard < -50.0, "{notch} Hz: {heard} dB");
    }
    for peak in [2_000.0 / 3.0, 4_000.0 / 3.0] {
        let heard = db(measured(flanger, peak));
        assert!(heard.abs() < 0.05, "{peak} Hz: {heard} dB");
    }
    let heard = db(measured(still(Mode::Phaser), 1_000.0));
    assert!(heard < -50.0, "{heard} dB");
}
