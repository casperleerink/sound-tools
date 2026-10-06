//! The sidechain: while something feeds it, the detector hears it instead of the sound, and the
//! gain still goes on the sound.

use std::f64::consts::TAU;

use compressor::{CompressorState, Lookahead, static_gain_db};

use crate::support::{Rig, SAMPLE_RATE, amplitude_at, largest_step, noise, peak, sine};

/// A quiet sound under the threshold, keyed by a loud tone: it comes out as much quieter as
/// the static curve says for the tone's level, with and without lookahead. The meter shows the
/// tone.
#[test]
fn a_loud_sidechain_turns_a_quiet_sound_down_by_the_static_curve() {
    let (hz, amplitude) = (440.0, 0.01);
    let key_db = -8.0;
    let key_amplitude = 10_f32.powf(key_db / 20.0);
    for lookahead in [Lookahead::Off, Lookahead::Ten] {
        let state = CompressorState {
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 0.0,
            lookahead,
            ..CompressorState::default()
        };
        let mut rig = Rig::new(state, sine(hz, amplitude));
        let key = rig.add_key(sine(1_000.0, key_amplitude));
        rig.connect(key);
        let settle = 2 * SAMPLE_RATE as usize;
        rig.render(settle);
        rig.meters.level.take();
        let window = SAMPLE_RATE as usize / 4;
        let [left, right] = rig.render(window);
        assert_eq!(left, right);
        let latency = lookahead.frames(SAMPLE_RATE as f32);
        let measured = amplitude_at(&left, settle - latency, hz);
        let measured_db = 20.0 * (measured / f64::from(amplitude)).log10();
        // 12 dB over the threshold at 4:1: 9 dB off.
        let expected = f64::from(static_gain_db(&state, key_db));
        assert!(
            (measured_db - expected).abs() < 0.02,
            "{lookahead:?}: {measured_db:.4} dB, expected {expected}"
        );
        let [level, _] = rig.meters.level.take();
        assert!((level - key_amplitude).abs() < 1e-6, "{level}");
    }
}

/// The same noise in the sidechain as in the input sounds exactly like no sidechain, to the bit: with a
/// lookahead the detector reads the sidechain as it comes, as it reads the sound, and links its
/// channels the same way.
#[test]
fn the_sound_itself_in_the_sidechain_is_no_sidechain() {
    let state = CompressorState {
        threshold_db: -30.0,
        ratio: 8.0,
        attack_ms: 0.5,
        release_ms: 20.0,
        lookahead: Lookahead::Ten,
        ..CompressorState::default()
    };
    let mut plain = Rig::new(state, noise(0.5));
    let mut keyed = Rig::new(state, noise(0.5));
    let key = keyed.add_key(noise(0.5));
    keyed.connect(key);
    let frames = SAMPLE_RATE as usize / 2;
    let output = plain.render(frames);
    assert_eq!(keyed.render(frames), output);
    assert!(peak(&output[0]) < 0.5 * 10_f32.powf(-6.0 / 20.0));
}

/// The reduction carries over when a sidechain comes and goes, so neither makes a step in the
/// sound larger than the tone itself takes. Starting over from no reduction would jump by
/// 15 dB at once.
#[test]
fn a_sidechain_that_comes_and_goes_does_not_click() {
    let (hz, amplitude) = (440.0, 0.05);
    let state = CompressorState {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        ..CompressorState::default()
    };
    let mut rig = Rig::new(state, sine(hz, amplitude));
    let key = rig.add_key(sine(1_000.0, 1.0));
    let second = SAMPLE_RATE as usize;
    let mut output = rig.render(second)[0].clone();
    rig.connect(key);
    let keyed = rig.render(second)[0].clone();
    // 20 dB over the threshold at 4:1: 15 dB off.
    let level_db = |samples: &[f32]| 20.0 * (peak(&samples[second / 2..]) / amplitude).log10();
    let ducked = level_db(&keyed);
    assert!((ducked + 15.0).abs() < 0.05, "{ducked}");
    output.extend(keyed);
    rig.disconnect(key);
    let [released, _] = rig.render(second);
    let let_go = level_db(&released);
    assert!(let_go.abs() < 0.05, "{let_go}");
    output.extend(released);
    let tone_step = f64::from(amplitude) * TAU * hz / f64::from(SAMPLE_RATE);
    let step = f64::from(largest_step(&output));
    assert!(step < 1.1 * tone_step, "{step} against {tone_step}");
}
