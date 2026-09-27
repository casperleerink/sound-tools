//! What a voice costs, at the steps of keys over the root: 16 voices held for 10 seconds,
//! rendered offline in the dev profile. Run by hand:
//! `cargo nextest run -p sampler --run-ignored only cost_of_a_voice --no-capture`.

use std::time::Instant;

use sampler::SamplerState;
use sound_notes::Pitch;

use crate::support::{Harness, SAMPLE_RATE, note, sine};

#[test]
#[ignore = "a measurement, run by hand"]
fn cost_of_a_voice_at_each_step() {
    // (semitones over the root, the step it plays at)
    for semitones in [0_i32, 1, 12, 24, 36] {
        let key = 48 + semitones as u8;
        let sample = sine(SAMPLE_RATE, 110.0, 0.05, 90.0, 0.0);
        let state = SamplerState {
            root: Pitch::new(48).unwrap(),
            ..SamplerState::default()
        };
        // Sixteen notes of one key, each a voice.
        let notes = (0..16)
            .map(|index| note(index * 100, 20 * 48_000, key, 100))
            .collect();
        let mut harness = Harness::playing(("long.wav", SAMPLE_RATE, sample), notes, state);
        harness.play(4_800);
        let seconds = 10;
        let started = Instant::now();
        harness.render(seconds * SAMPLE_RATE as usize);
        let took = started.elapsed().as_secs_f64();
        let per_voice = took / (16.0 * seconds as f64) * 100.0;
        let step = 2_f64.powf(f64::from(semitones) / 12.0);
        println!(
            "step {step:.2}: 16 voices for {seconds} s took {took:.3} s, {:.1} times realtime; a voice costs {per_voice:.2} % of a core",
            seconds as f64 / took
        );
    }
}
