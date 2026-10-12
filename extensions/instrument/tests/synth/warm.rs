//! The warm character: at full resonance a held note rings at the cutoff on top of its own
//! sound, and at no resonance it sounds as the clean filter does, as late as the oversampling.

use instrument::{FilterCharacter, SynthState, Waveform};
use sound_core::Oversampler;

use crate::support::{Harness, SAMPLE_RATE, level_at, note, peak};

const SECOND: usize = SAMPLE_RATE as usize;

fn held(cutoff_hz: f32, resonance: f32, character: FilterCharacter) -> Vec<f32> {
    let state = SynthState {
        waveform: Waveform::Saw,
        cutoff_hz,
        resonance,
        character,
        sustain: 1.0,
        gain: 0.25,
        ..SynthState::default()
    };
    // C6, 1047 Hz, held for two seconds.
    let output = Harness::with_track(vec![note(0, 3_840, 84, 127)], state).play(2 * SECOND);
    output[SECOND / 2..3 * SECOND / 2].to_vec()
}

/// Half an octave over the note, between its first two harmonics, the warm filter rings on its
/// own; the clean one only lets the note through.
#[test]
fn at_full_resonance_a_warm_voice_rings_at_the_cutoff() {
    for cutoff_hz in [1_500.0, 1_600.0] {
        let warm = level_at(&held(cutoff_hz, 1.0, FilterCharacter::Warm), cutoff_hz);
        let clean = level_at(&held(cutoff_hz, 1.0, FilterCharacter::Clean), cutoff_hz);
        println!("{cutoff_hz} Hz: warm {warm:.4}, clean {clean:.4}");
        assert!(warm > 20.0 * clean, "{cutoff_hz}: {warm} {clean}");
    }
}

/// Without resonance the warm filter barely bends, so it sounds as the clean one, as late as
/// the way up and down: the synth reports no latency for it.
#[test]
fn without_resonance_a_warm_voice_sounds_as_a_clean_one_as_late_as_the_oversampling() {
    let clean = held(2_000.0, 0.0, FilterCharacter::Clean);
    let warm = held(2_000.0, 0.0, FilterCharacter::Warm);
    let loudest = peak(&clean);
    let late = warm[Oversampler::DELAY_FRAMES..].iter();
    let largest = clean
        .iter()
        .zip(late)
        .fold(0.0_f32, |largest, (clean, warm)| {
            largest.max((clean - warm).abs())
        });
    println!("largest difference {largest:.5} of {loudest:.3}");
    assert!(largest < 0.02 * loudest, "{largest} of {loudest}");
}
