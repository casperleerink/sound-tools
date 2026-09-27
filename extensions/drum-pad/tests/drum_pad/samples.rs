//! Sample pads: a file under `assets/audio/` plays at the pitch of the pad, also when its rate
//! is not the engine's, and a file that is not there is a problem and a silent pad.

use drum_pad::{DrumPadState, Source};
use sound_media::AudioAsset;

use crate::support::{
    Harness, SAMPLE_RATE, Stereo, hit, last_above, note, peak, rising_zero_crossings, write_sine,
};

const PAD: usize = 12;

/// The Drum pad with pad 48 playing `file`, in the middle.
fn sample_pad(file: &str, pitch_semitones: f32, decay_ms: f32) -> DrumPadState {
    let mut drums = DrumPadState::default();
    drums.pads[PAD].source = Source::Sample(AudioAsset::new(file).unwrap());
    drums.pads[PAD].pitch_semitones = pitch_semitones;
    drums.pads[PAD].decay_ms = decay_ms;
    drums.pads[PAD].pan = 0.0;
    drums
}

/// A project with a sine of `hz` at `rate` in `assets/audio/tone.wav`, the pad set to play it.
fn play_sine(hz: f64, rate: u32, pitch_semitones: f32, decay_ms: f32) -> Stereo {
    let mut harness = Harness::new();
    let folder = harness.project.root().join("assets/audio");
    std::fs::create_dir_all(&folder).unwrap();
    write_sine(&folder.join("tone.wav"), hz, 2.0, rate);
    let drums = sample_pad("tone.wav", pitch_semitones, decay_ms);
    harness.add_track("track", vec![hit(0, note(PAD), 127)], drums);
    assert!(
        harness.project.problems().is_empty(),
        "{:?}",
        harness.project.problems()
    );
    harness.play(SAMPLE_RATE as usize * 3 / 2)
}

/// The frequency of a steady tone from its rising zero crossings, with the fraction of a frame
/// where each crossing is.
fn frequency(samples: &[f32]) -> f64 {
    let crossings: Vec<f64> = rising_zero_crossings(samples)
        .into_iter()
        .map(|frame| {
            let (before, after) = (f64::from(samples[frame - 1]), f64::from(samples[frame]));
            (frame - 1) as f64 + before / (before - after)
        })
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE) / (last - first)
}

#[test]
fn a_sample_plays_at_the_pitch_of_its_pad_at_any_file_rate() {
    for rate in [44_100, 48_000, 96_000] {
        for (semitones, expected) in [(0.0, 440.0), (7.0, 659.255), (-12.0, 220.0), (12.0, 880.0)] {
            let sound = play_sine(440.0, rate, semitones, 5_000.0);
            let measured = frequency(&sound.left[4_800..24_000]);
            let cents = 1200.0 * (measured / expected).log2();
            println!("{rate} Hz file at {semitones:+} st: {measured:.3} Hz, {cents:+.3} cents");
            assert!(cents.abs() < 0.1, "{cents}");
            // The level of the file, at the start before the fade over the decay counts.
            assert!((peak(&sound.left[..4_800]) - 0.5).abs() < 0.001);
        }
    }
}

#[test]
fn a_sample_is_as_long_as_its_file_or_its_decay() {
    // A file of 2 s at 44.1 kHz an octave up lasts 1 s.
    let sound = play_sine(440.0, 44_100, 12.0, 5_000.0);
    let end = sound
        .left
        .iter()
        .rposition(|sample| *sample != 0.0)
        .unwrap();
    assert!((end as i64 - 48_000).abs() < 100, "{end}");
    // A decay of 100 ms: at half of it the fade is at 1 - 0.5^4, and after it nothing.
    let sound = play_sine(440.0, 48_000, 0.0, 100.0);
    let (half, decay) = (2_400, 4_800);
    let around_half = peak(&sound.left[half - 60..half + 60]);
    assert!(
        (around_half - 0.5 * (1.0 - 0.5_f32.powi(4))).abs() < 0.002,
        "{around_half}"
    );
    assert!(sound.left[decay..].iter().all(|sample| *sample == 0.0));
    assert!(last_above(&sound.left, -20.0) > decay * 3 / 4);
}

#[test]
fn a_missing_sample_is_a_problem_and_its_pad_is_silent_until_the_file_arrives() {
    let mut harness = Harness::new();
    let drums = sample_pad("gone.wav", 0.0, 1_000.0);
    let notes = vec![hit(0, note(PAD), 127), hit(0, note(0), 127)];
    harness.add_track("track", notes, drums);
    let problems: Vec<String> = harness
        .project
        .problems()
        .iter()
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect();
    assert_eq!(
        problems,
        [
            "state/track/instrument.json: pad \"48\" (gone) is silent: assets/audio/gone.wav is not there. The rest of the pads play"
        ]
    );
    // The kick plays, and the pad adds nothing to it.
    let with_missing = harness.play(24_000);
    let mut kick_alone = Harness::with_track(vec![hit(0, note(0), 127)], DrumPadState::default());
    let kick = kick_alone.play(24_000);
    assert!(with_missing.left == kick.left && with_missing.right == kick.right);

    // The file arrives: the pad plays, with no edit of its record.
    let folder = harness.project.root().join("assets/audio");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("gone.wav");
    write_sine(&file, 440.0, 1.0, 48_000);
    harness.project.apply_outside_changes(&[file]).unwrap();
    assert!(harness.project.problems().is_empty());
    harness.project.engine().seek(sound_core::Ticks(0));
    let arrived = harness.render(24_000);
    assert!(peak(&arrived.left) > 0.5);
}
