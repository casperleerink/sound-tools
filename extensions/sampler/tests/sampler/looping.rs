//! A looped note: it goes on past the end of its file while the key is held, forwards and
//! reversed. The crossfade at the loop point is in `clicks.rs`.

use sampler::SamplerState;

use crate::support::{Harness, SAMPLE_RATE, note, playing};

/// Half a second of a ramp up, so every place of the file has its own value.
fn ramp() -> Vec<f32> {
    (0..24_000).map(|frame| frame as f32 / 24_000.0).collect()
}

/// The ramp of a half-second file, looped from its middle and held for two seconds.
fn looped(reverse: bool) -> (Vec<f32>, Vec<f32>) {
    let ramp = ramp();
    let state = playing(
        "ramp.wav",
        SamplerState {
            looping: true,
            loop_start_seconds: Some(0.25),
            reverse,
            attack_seconds: 0.001,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        },
    );
    let mut harness = Harness::with_samples(&[("ramp.wav", SAMPLE_RATE, ramp.clone())]);
    harness.add_track(vec![note(0, 96_000, 60, 127)], state);
    assert_eq!(harness.project.problems(), []);
    (ramp, harness.play(96_000))
}

/// The loop is 12000 frames, from frame 12000 of the file to its end; the last 480 (10 ms) of it
/// fade into its first 480, and it goes on from there. So the note plays the file once, then
/// frames 12480 to 23519 again and again, sample for sample, every 11520 frames, for as long as
/// the key is held: four times the length of the file.
#[test]
fn a_held_note_repeats_the_loop_past_the_end_of_its_file() {
    let (ramp, played) = looped(false);
    assert_eq!(played[100..23_520], ramp[100..23_520]);
    for turn in 0..6 {
        let from = 24_000 + turn * 11_520;
        assert_eq!(
            played[from..from + 11_040],
            ramp[12_480..23_520],
            "turn {turn}"
        );
    }
    assert!(played[95_999] > 0.25, "{}", played[95_999]);
}

/// Reversed, the loop is the same part of the file played backwards: from the end back to the
/// loop start, then from the end again. The part before the loop start never plays.
#[test]
fn a_reversed_loop_plays_its_part_backwards_from_the_end() {
    let (ramp, played) = looped(true);
    let backwards: Vec<f32> = ramp[12_000..24_000].iter().rev().copied().collect();
    assert_eq!(played[100..11_520], backwards[100..11_520]);
    for turn in 0..7 {
        let from = 12_000 + turn * 11_520;
        assert_eq!(
            played[from..from + 11_040],
            backwards[480..11_520],
            "turn {turn}"
        );
    }
    let lowest = played[100..].iter().copied().fold(f32::MAX, f32::min);
    assert!(lowest >= 0.25, "{lowest}");
}
