//! The warm character: a voice is a full scale saw into the filter, so warm bends its peak at
//! moderate resonance, and its sound comes as late as the oversampling.

use instrument::{FilterCharacter, SynthState, Waveform};
use sound_dsp::Oversampler;

use crate::support::{Harness, SAMPLE_RATE, level_at, note};

const SECOND: usize = SAMPLE_RATE as usize;

/// A3 held, its fourth harmonic on the cutoff at resonance 0.5: half a second once it has
/// settled.
fn held(character: FilterCharacter) -> Vec<f32> {
    let state = SynthState {
        waveform: Waveform::Saw,
        cutoff_hz: 880.0,
        resonance: 0.5,
        character,
        sustain: 1.0,
        gain: 0.25,
        ..SynthState::default()
    };
    let output = Harness::with_track(vec![note(0, 3_840, 57, 127)], state).play(2 * SECOND);
    output[SECOND / 2..SECOND].to_vec()
}

/// The harmonic on the peak comes out quieter when warm; the fundamental, far under the cutoff,
/// keeps its level.
#[test]
fn at_moderate_resonance_a_warm_voice_bends_its_peak() {
    let (clean, warm) = (held(FilterCharacter::Clean), held(FilterCharacter::Warm));
    let db = |samples: &[f32], hz: f32| 20.0 * level_at(samples, hz).log10();
    let peak = db(&warm, 880.0) - db(&clean, 880.0);
    let fundamental = db(&warm, 220.0) - db(&clean, 220.0);
    println!("warm against clean: peak {peak:.2} dB, fundamental {fundamental:.2} dB");
    assert!(peak < -3.0, "{peak}");
    assert!(fundamental.abs() < 1.0, "{fundamental}");
}

/// With the cutoff at the top and no resonance the warm filter bends only the edges of the
/// saw, so a warm voice is closest to the clean one as late as the way up and down: the synth
/// reports no latency for it.
#[test]
fn an_open_warm_voice_is_the_clean_one_as_late_as_the_oversampling() {
    let open = |character| {
        let state = SynthState {
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            character,
            sustain: 1.0,
            ..SynthState::default()
        };
        let output = Harness::with_track(vec![note(0, 3_840, 57, 127)], state).play(SECOND);
        output[SECOND / 2..].to_vec()
    };
    let (clean, warm) = (open(FilterCharacter::Clean), open(FilterCharacter::Warm));
    let apart = |lag: usize| -> f32 {
        let pairs = clean.iter().zip(&warm[lag..]);
        pairs.map(|(clean, warm)| (clean - warm).powi(2)).sum()
    };
    let closest = (0..2 * Oversampler::DELAY_FRAMES)
        .min_by(|a, b| apart(*a).total_cmp(&apart(*b)))
        .unwrap();
    assert_eq!(closest, Oversampler::DELAY_FRAMES);
}
