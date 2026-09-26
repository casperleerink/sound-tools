//! Attack and release: a level step through the compressor, and the time the reduction takes
//! to go 63 % of the way, which is the time constant of a one-pole glide.
//!
//! The step is a constant that jumps from -60 dBFS to 0 dBFS or back, so the gain of each frame
//! is its output over its input and the reduction is read to the frame. Bounds: the attack
//! reaches 63 % within one frame (21 µs at 48 kHz) of `attack_ms`. The release starts 10 to
//! 11 ms after the level fell, when the loud part has left the detector, and from there
//! reaches 63 % within one frame of `release_ms`.

use compressor::{CompressorState, HOLD_SECONDS};

use crate::support::{Rig, SAMPLE_RATE, step};

const LOUD: f32 = 1.0;
const QUIET: f32 = 0.001;
/// The step comes after a second, so the compressor is steady before it.
const AT: usize = SAMPLE_RATE as usize;

/// A hard knee at -20 dB, 4:1: 0 dBFS is turned down by 15 dB, -60 dBFS not at all.
fn state(attack_ms: f32, release_ms: f32) -> CompressorState {
    CompressorState {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        attack_ms,
        release_ms,
        ..CompressorState::default()
    }
}

const FULL_DB: f32 = 15.0;

/// The reduction of each frame from the step on, in dB, for a step between these values.
fn reductions(state: CompressorState, before: f32, after: f32, seconds: f32) -> Vec<f32> {
    let mut rig = Rig::new(state, step(before, AT, after));
    let frames = AT + (seconds * SAMPLE_RATE as f32) as usize;
    let [left, _] = rig.render(frames);
    left[AT..]
        .iter()
        .map(|sample| -20.0 * (sample / after).log10())
        .collect()
}

/// The frames from the step until the reduction has gone 63.2 % of the way from `from` to
/// `to`, counting the frame of the step as the first.
fn frames_to_63(reductions: &[f32], from: f32, to: f32) -> usize {
    let mark = from + (to - from) * (1.0 - (-1.0_f32).exp());
    let past = |reduction: f32| match to > from {
        true => reduction >= mark,
        false => reduction <= mark,
    };
    reductions
        .iter()
        .position(|reduction| past(*reduction))
        .unwrap()
        + 1
}

#[test]
fn the_attack_reaches_63_percent_in_its_time() {
    for attack_ms in [0.1, 1.0, 10.0, 100.0, 300.0] {
        let seconds = 0.1 + 15.0 * attack_ms / 1_000.0;
        let reductions = reductions(state(attack_ms, 120.0), QUIET, LOUD, seconds);
        let frames = frames_to_63(&reductions, 0.0, FULL_DB);
        let expected = attack_ms / 1_000.0 * SAMPLE_RATE as f32;
        println!(
            "attack {attack_ms} ms: 63 % after {frames} frames, {:.3} ms",
            frames as f32 * 1_000.0 / SAMPLE_RATE as f32
        );
        assert!(
            (frames as f32 - expected).abs() <= 1.0,
            "attack {attack_ms} ms: {frames} frames, expected {expected}"
        );
        // And it arrives where the static curve says.
        let last = *reductions.last().unwrap();
        assert!((last - FULL_DB).abs() < 1e-5, "{last}");
    }
}

#[test]
fn the_release_reaches_63_percent_in_its_time_after_the_hold() {
    for release_ms in [1.0, 10.0, 120.0, 1_000.0, 3_000.0] {
        let seconds = 0.1 + 15.0 * release_ms / 1_000.0;
        let reductions = reductions(state(1.0, release_ms), LOUD, QUIET, seconds);
        // The hold: the reduction stays where it was while the loud part is in the detector.
        let hold = reductions
            .iter()
            .position(|reduction| *reduction < FULL_DB - 1e-5)
            .unwrap();
        let hold_ms = hold as f32 * 1_000.0 / SAMPLE_RATE as f32;
        let hold_frames = (HOLD_SECONDS * SAMPLE_RATE as f32).round() as usize;
        assert!(
            (hold_frames..=hold_frames * 11 / 10).contains(&hold),
            "release {release_ms} ms: held for {hold_ms} ms"
        );
        let frames = frames_to_63(&reductions[hold..], FULL_DB, 0.0);
        let expected = release_ms / 1_000.0 * SAMPLE_RATE as f32;
        println!(
            "release {release_ms} ms: held {hold_ms:.2} ms, then 63 % after {frames} frames, {:.3} ms",
            frames as f32 * 1_000.0 / SAMPLE_RATE as f32
        );
        assert!(
            (frames as f32 - expected).abs() <= 1.0,
            "release {release_ms} ms: {frames} frames, expected {expected}"
        );
        // It lets go completely: the gain is exactly 1 again.
        assert_eq!(*reductions.last().unwrap(), 0.0);
    }
}
