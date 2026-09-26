//! The lookahead: it is the latency the compressor reports, the sound comes out that much later
//! and otherwise untouched, and the reduction is in place when a peak arrives.

use compressor::{Compressor, CompressorState, Lookahead};
use sound_core::Processor;

use crate::support::{Rig, SAMPLE_RATE, sine, step};

#[test]
fn the_latency_is_the_lookahead_in_frames() {
    for (lookahead, frames) in [
        (Lookahead::Off, 0),
        (Lookahead::One, 48),
        (Lookahead::Ten, 480),
    ] {
        let state = CompressorState {
            lookahead,
            ..CompressorState::default()
        };
        assert_eq!(Compressor::new(state).latency(), frames);
    }
}

/// Under the threshold the output is the input, delayed by the lookahead, to the bit.
#[test]
fn under_the_threshold_the_sound_is_only_delayed() {
    for lookahead in Lookahead::ALL {
        let state = CompressorState {
            lookahead,
            ..CompressorState::default()
        };
        let delay = lookahead.frames(SAMPLE_RATE as f32);
        let mut rig = Rig::new(state, sine(440.0, 0.05));
        let [output, _] = rig.render(SAMPLE_RATE as usize / 10);
        let mut source = sine(440.0, 0.05);
        let input: Vec<f32> = (0..output.len()).map(|_| source()[0]).collect();
        assert!(output[..delay].iter().all(|sample| *sample == 0.0));
        assert_eq!(&output[delay..], &input[..input.len() - delay]);
    }
}

/// A jump from quiet to loud: without lookahead the first loud frames pass nearly untouched and
/// the reduction catches up; with 10 ms of lookahead and an attack of 1 ms it is in place when
/// the jump comes out.
#[test]
fn with_lookahead_the_reduction_is_in_place_when_the_peak_arrives() {
    let at = SAMPLE_RATE as usize / 2;
    let first_loud_gain_db = |lookahead: Lookahead| {
        let state = CompressorState {
            threshold_db: -20.0,
            ratio: 10.0,
            knee_db: 0.0,
            attack_ms: 1.0,
            lookahead,
            ..CompressorState::default()
        };
        let mut rig = Rig::new(state, step(0.001, at, 1.0));
        let [output, _] = rig.render(SAMPLE_RATE as usize);
        let delay = lookahead.frames(SAMPLE_RATE as f32);
        20.0 * output[at + delay].log10()
    };
    let (without, with) = (
        first_loud_gain_db(Lookahead::Off),
        first_loud_gain_db(Lookahead::Ten),
    );
    println!("first loud frame: {without:.2} dB without lookahead, {with:.2} dB with 10 ms");
    // The static reduction is 18 dB. After 10 attack times it is there to within 0.001 dB.
    assert!(without > -1.0, "{without}");
    assert!((with + 18.0).abs() < 0.01, "{with}");
}
