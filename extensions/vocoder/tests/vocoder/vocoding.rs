//! What the voice lets through of the track.

use sound_core::Processor;
use vocoder::{SIZE, Vocoder, VocoderState};

use crate::support::{Rig, SAMPLE_RATE, amplitude, silence, sines};

/// Half a second, after a second to settle: a whole number of cycles of every sine here.
fn settled(output: &[f32]) -> &[f32] {
    let second = SAMPLE_RATE as usize;
    &output[second..second + second / 2]
}

#[test]
fn a_silent_voice_silences_the_track() {
    let track = sines(&[(220.0, 0.25), (440.0, 0.25), (3_000.0, 0.25)]);
    let output = Rig::new(VocoderState::default(), track)
        .voice(silence())
        .render(SAMPLE_RATE as usize);
    assert!(output.iter().all(|sample| *sample == 0.0));
}

#[test]
fn a_voice_in_one_band_lets_through_only_that_band_at_its_level() {
    let track = sines(&[(300.0, 0.25), (3_000.0, 0.25)]);
    let output = Rig::new(VocoderState::default(), track)
        .voice(sines(&[(3_000.0, 0.1)]))
        .render(2 * SAMPLE_RATE as usize);
    let output = settled(&output);
    let voiced = amplitude(output, 3_000.0);
    assert!((voiced / 0.1 - 1.0).abs() < 0.05, "3 kHz at {voiced}");
    let unvoiced = amplitude(output, 300.0);
    assert!(unvoiced < 1e-3, "300 Hz at {unvoiced}");
}

#[test]
fn the_formant_moves_where_the_voice_opens_the_track() {
    let state = VocoderState {
        formant: 12.0,
        ..VocoderState::default()
    };
    let track = sines(&[(3_000.0, 0.25), (6_000.0, 0.25)]);
    let output = Rig::new(state, track)
        .voice(sines(&[(3_000.0, 0.1)]))
        .render(2 * SAMPLE_RATE as usize);
    let output = settled(&output);
    let (at_voice, octave_up) = (amplitude(output, 3_000.0), amplitude(output, 6_000.0));
    assert!(octave_up > 0.05, "6 kHz at {octave_up}");
    assert!(at_voice < 1e-3, "3 kHz at {at_voice}");
}

#[test]
fn with_no_voice_it_follows_its_own_sound_and_plays_it_as_late_as_it_says() {
    let latency = Vocoder::new(VocoderState::default()).latency() as usize;
    assert_eq!(latency, SIZE - 1);
    let parts = [(220.0, 0.2), (1_100.0, 0.2)];
    let output = Rig::new(VocoderState::default(), sines(&parts)).render(SAMPLE_RATE as usize);
    let mut dry = sines(&parts);
    let input: Vec<f32> = (0..output.len()).map(|_| dry()[0]).collect();
    let start = SAMPLE_RATE as usize / 2;
    for (frame, sample) in output.iter().enumerate().skip(start) {
        let expected = input[frame - latency];
        assert!(
            (sample - expected).abs() < 2e-3,
            "frame {frame}: {sample} for {expected}"
        );
    }
}
