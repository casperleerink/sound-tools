//! Loading: a missing file is a problem and silence until it arrives, a new file while notes
//! sound fades them out and never frees the old one on the audio thread, an empty sampler is
//! silent, and a start past the end of the file is a problem.

use std::sync::Arc;

use sampler::SamplerState;
use sound_core::Assets;
use sound_media::AudioAsset;

use crate::support::{Harness, SAMPLE_RATE, largest_step, note, peak, playing, write_sample};

fn steady(level: f32) -> Vec<f32> {
    vec![level; SAMPLE_RATE as usize]
}

#[test]
fn a_missing_file_is_a_problem_and_silence_until_it_arrives() {
    let mut harness = Harness::with_samples(&[]);
    let notes = vec![note(0, 40_000, 60, 127)];
    harness.add_track(notes, playing("kalimba.wav", SamplerState::default()));
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/track/instrument.json");
    assert_eq!(
        problems[0].message,
        "the sample assets/audio/kalimba.wav is not there, so the Sampler is silent. Copy the file into assets/audio/ under that name and it plays, or correct `sample`"
    );
    assert_eq!(peak(&harness.play(4_800)), 0.0);

    // The file arrives, and what the watcher reports of it is enough: no edit of the record.
    write_sample(
        harness.folder.path(),
        "kalimba.wav",
        SAMPLE_RATE,
        &steady(0.25),
    );
    let path = harness.path("assets/audio/kalimba.wav");
    harness.project.apply_outside_changes(&[path]).unwrap();
    assert_eq!(harness.project.problems(), []);
    harness.project.engine().seek(sound_core::Ticks(0));
    let [left, _] = harness.render(4_800);
    assert_eq!(left[2_000], 0.25);
}

#[test]
fn an_empty_sampler_is_silent_and_no_problem() {
    let mut harness = Harness::with_samples(&[]);
    harness.add_track(vec![note(0, 4_800, 60, 127)], SamplerState::default());
    assert_eq!(harness.project.problems(), []);
    assert_eq!(peak(&harness.play(4_800)), 0.0);
}

#[test]
fn a_start_past_the_end_of_the_file_is_a_problem() {
    let mut harness = Harness::with_samples(&[("short.wav", SAMPLE_RATE, steady(0.5))]);
    let state = SamplerState {
        start_seconds: 2.0,
        ..SamplerState::default()
    };
    harness.add_track(vec![note(0, 4_800, 60, 127)], playing("short.wav", state));
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "the Sampler plays nothing: start_seconds 2 is at or past the end of short.wav, which is 1.000 s long"
    );
    assert_eq!(peak(&harness.play(4_800)), 0.0);
}

/// A new file while a note of the old one sounds: the note fades out over 5 ms and the next note
/// plays the new file. The old file goes back to the control side, where it is let go of; the
/// realtime sanitizer, which CI runs on this test, would stop a free on the audio thread.
#[test]
fn a_new_file_while_notes_sound_fades_them_out_and_the_old_one_goes_back() {
    let files = [
        ("up.wav", SAMPLE_RATE, steady(0.5)),
        ("down.wav", SAMPLE_RATE, steady(-0.5)),
        ("third.wav", SAMPLE_RATE, steady(0.25)),
    ];
    let mut harness = Harness::with_samples(&files);
    let state = SamplerState {
        velocity_to_volume: 0.0,
        ..SamplerState::default()
    };
    let notes = vec![note(0, 6_000, 60, 127), note(12_000, 6_000, 60, 127)];
    harness.add_track(notes, playing("up.wav", state));
    // What the test holds of the first file: the project holds it too while it plays.
    let assets = Assets::new(harness.project.root());
    let up = sound_media::load(&assets, &AudioAsset::new("up.wav").unwrap()).unwrap();
    assert!(Arc::strong_count(&up) > 1);

    let mut played = harness.play(3_000);
    let record =
        r#"{"tool": "sampler", "state": {"sample": "down.wav", "velocity_to_volume": 0.0}}"#;
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", record),
        1
    );
    let [left, _] = harness.render(15_000);
    played.extend(left);
    // The first note fades out over 240 frames from where the edit arrived.
    assert_eq!(played[2_999], 0.5);
    let fade = &played[3_000..3_240];
    assert!(fade.windows(2).all(|pair| pair[1] <= pair[0]), "{fade:?}");
    assert_eq!(played[3_241], 0.0);
    let step = largest_step(&played[2_990..3_300]);
    println!("a new file under a note: largest step {step:.5}, a cut 0.5");
    assert!(step <= 0.5 / 240.0 + 1e-6, "{step}");
    // The next note plays the new file.
    assert_eq!(played[14_000], -0.5);

    // The first file is still the previous one, for notes that might still fade. One more file
    // sends it back to the control side, which lets go of it: the test holds the last one.
    let record = r#"{"tool": "sampler", "state": {"sample": "third.wav"}}"#;
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", record),
        1
    );
    harness.render(480);
    harness.project.poll().unwrap();
    assert_eq!(Arc::strong_count(&up), 1);
}
