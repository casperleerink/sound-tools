//! Where the sounds of the pads are made: never inside an edit, on a thread of their own, the
//! latest ask of a pad winning, and a sample keeps its render and nothing of its file.

use std::time::Instant;

use drum_pad::{DrumPadState, MAX_SAMPLE_SECONDS, SAMPLE_END_SECONDS, Source};
use sound_core::{Changes, Ticks};
use sound_media::AudioAsset;

use crate::support::{Harness, SAMPLE_RATE, hit, id, note, peak, write_sine};

const RIDE: usize = 15;
const PAD: usize = 12;

fn edit(harness: &mut Harness, change: impl FnOnce(&mut DrumPadState)) {
    let drums = harness
        .project
        .resolve::<DrumPadState>(&id("track/instrument"))
        .unwrap();
    let mut state = harness.project.state(&drums).unwrap().clone();
    change(&mut state);
    let mut changes = Changes::new();
    changes.set(&drums, state);
    harness.project.commit("Edit drums", changes).unwrap();
}

/// Plays the notes of the track again from the start.
fn again(harness: &mut Harness, frames: usize, take: bool) -> Vec<f32> {
    harness.project.engine().stop();
    harness.project.engine().seek(Ticks(0));
    harness.render_as_it_is(4_800);
    harness.project.engine().play();
    let played = match take {
        true => harness.render(frames),
        false => harness.render_as_it_is(frames),
    };
    played.sum()
}

/// The edit that asks for the longest sound there is, the ride at the longest decay, returns
/// before the sound is made, and the pad plays its old sound until the new one is in its kit.
#[test]
fn an_edit_makes_no_sound_and_the_new_sound_comes_later() {
    let notes = vec![hit(0, note(RIDE), 110)];
    let mut harness = Harness::with_track(notes.clone(), DrumPadState::default());
    let before = again(&mut harness, 24_000, true);
    assert!(peak(&before) > 0.05);

    for (decay_ms, pitch) in [(10_000.0, 0.37), (10_000.0, 0.41), (9_000.0, 0.43)] {
        let started = Instant::now();
        edit(&mut harness, |drums| {
            drums.pads[RIDE].decay_ms = decay_ms;
            drums.pads[RIDE].pitch_semitones = pitch;
        });
        let edit_took = started.elapsed();
        assert!(
            drum_pad::sounds_pending(),
            "the sound was made inside the edit"
        );
        println!(
            "an edit of the ride to {decay_ms} ms and {pitch} st: {:.2} ms",
            edit_took.as_secs_f64() * 1000.0
        );
    }
    // Still the old ride: the new one is not in the kit. Only its fade over the decay is the
    // new one's, which the pad plays over the old sound: under 0.2 % in the first half second.
    let pending = again(&mut harness, 24_000, false);
    let largest = |a: &[f32], b: &[f32]| {
        a.iter()
            .zip(b)
            .fold(0.0_f32, |largest, (a, b)| largest.max((a - b).abs()))
    };
    let from_before = largest(&pending, &before);
    assert!(from_before < 0.002 * peak(&before), "{from_before}");

    // The latest ask wins: once the sounds are made and taken, the ride is the last edit's.
    let started = Instant::now();
    drum_pad::wait_for_sounds();
    println!(
        "the sounds were made {:.1} ms later",
        started.elapsed().as_secs_f64() * 1000.0
    );
    let after = again(&mut harness, 24_000, true);
    let mut last = DrumPadState::default();
    last.pads[RIDE].decay_ms = 9_000.0;
    last.pads[RIDE].pitch_semitones = 0.43;
    let mut fresh = Harness::with_track(notes, last);
    let expected = again(&mut fresh, 24_000, true);
    assert!(largest(&after, &before) > 0.1 * peak(&before));
    assert_eq!(after, expected);
}

/// A sample is kept as its render, at most the longest decay long, and the file it came from
/// is known by its size and time: written again, it is made again.
#[test]
fn a_sample_is_its_render_and_a_file_written_again_is_made_again() {
    let mut harness = Harness::new();
    let folder = harness.project.root().join("assets/audio");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("long.wav");
    write_sine(&file, 440.0, 30.0, 48_000);
    let mut drums = DrumPadState::default();
    drums.pads[PAD].source = Source::Sample(AudioAsset::new("long.wav").unwrap());
    drums.pads[PAD].decay_ms = 10_000.0;
    drums.pads[PAD].pitch_semitones = 0.0;
    harness.add_track("track", vec![hit(0, note(PAD), 127)], drums);
    drum_pad::wait_for_sounds();
    let ready = drum_pad::take_ready(harness.project.assets());
    let lengths: Vec<usize> = ready
        .iter()
        .flat_map(|(_, sounds)| sounds.iter().map(|sound| sound.frames().len()))
        .collect();
    let longest = (MAX_SAMPLE_SECONDS * f64::from(SAMPLE_RATE)) as usize;
    println!("the renders of a new kit with a 30 s sample: {lengths:?}");
    assert!(lengths.contains(&longest), "{lengths:?}");
    assert!(lengths.iter().all(|length| *length <= longest));
    for (instance, _sounds) in ready {
        harness.project.rebind(&instance).unwrap();
    }
    let first = harness.play(4_800).sum();

    // The same name, another file: a tone an octave up, and a different length.
    write_sine(&file, 880.0, 20.0, 48_000);
    edit(&mut harness, |drums| drums.pads[PAD].volume_db = -0.5);
    let second = again(&mut harness, 4_800, true);
    let crossings = |samples: &[f32]| {
        samples
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count()
    };
    let (low, high) = (crossings(&first), crossings(&second));
    println!("rising zero crossings in 0.1 s: {low} before, {high} after");
    assert!((43..=45).contains(&low) && (87..=89).contains(&high));
}

/// A sample that ends loud fades out over its last 2 ms, so the pad ends at silence.
#[test]
fn a_sample_that_ends_loud_ends_at_silence() {
    let mut harness = Harness::new();
    let folder = harness.project.root().join("assets/audio");
    std::fs::create_dir_all(&folder).unwrap();
    // 0.1 s of a loud sine that stops on a crest: 442.5 Hz is 44 and a quarter cycles.
    write_sine(&folder.join("cut.wav"), 442.5, 0.1, 48_000);
    let mut drums = DrumPadState::default();
    drums.pads[PAD].source = Source::Sample(AudioAsset::new("cut.wav").unwrap());
    drums.pads[PAD].decay_ms = 5_000.0;
    drums.pads[PAD].pitch_semitones = 0.0;
    drums.pads[PAD].pan = 0.0;
    harness.add_track("track", vec![hit(0, note(PAD), 127)], drums);
    let played = harness.play(9_600).left;
    // The file is 4800 frames, and its last frame is silent.
    let end = 4_800;
    assert_eq!(played[end - 1], 0.0);
    assert!(played[end..].iter().all(|sample| *sample == 0.0));
    let ramp = (SAMPLE_END_SECONDS * f64::from(SAMPLE_RATE)).round() as usize;
    let steps: Vec<f32> = played[end - ramp - 1..=end]
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect();
    let largest = steps
        .iter()
        .fold(0.0_f32, |largest, step| largest.max(*step));
    let before_ramp = peak(&played[end - 2 * ramp..end - ramp]);
    println!(
        "the last {ramp} frames: level {before_ramp:.3} before them, largest step {largest:.4}"
    );
    // Loud up to the ramp, and no step at its end larger than the sine's own steps.
    assert!(before_ramp > 0.45);
    assert!(largest < 0.03, "{largest}");
}
