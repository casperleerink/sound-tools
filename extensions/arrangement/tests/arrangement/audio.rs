//! Audio tracks: where a clip plays, what part of its file, how loud, and that no edge clicks.
//! Every render goes through `Engine::process_block`, so the realtime sanitizer checks the
//! player in CI.

use arrangement::{AudioClip, Colour, DECLICK_SECONDS, decibels};
use sound_core::{Changes, EngineConfig, Ticks};
use sound_media::AudioAsset;

use crate::support::{Harness, SAMPLE_RATE, id, stereo, tempo, write_wav};

/// Frames of the ramp at every edge at 48 kHz: 2 ms.
const RAMP: u64 = 96;

/// A project with a stereo engine and one audio track, `arrangement/voice`.
fn audio_harness() -> Harness {
    assert_eq!((DECLICK_SECONDS * f64::from(SAMPLE_RATE)) as u64, RAMP);
    let mut harness = Harness::with_config(EngineConfig::new(SAMPLE_RATE, 2));
    let mut changes = Changes::new();
    let track = arrangement::add_audio_track(
        &harness.project,
        &mut changes,
        &id("arrangement"),
        "Voice",
        Colour::Peach,
    )
    .unwrap();
    assert_eq!(track.id(), &id("arrangement/voice"));
    harness.project.commit("Add audio track", changes).unwrap();
    harness
}

fn clip(file: &str, start: u64) -> AudioClip {
    AudioClip::new(AudioAsset::new(file).unwrap(), Ticks(start))
}

fn add(harness: &mut Harness, name: &str, clip: AudioClip) {
    let mut changes = Changes::new();
    changes.create(id(&format!("arrangement/voice/{name}")), clip);
    harness.project.commit("Add clip", changes).unwrap();
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// Plays `frames` stereo frames.
fn play(harness: &mut Harness, frames: usize) -> Vec<[f32; 2]> {
    stereo(&harness.play(frames * 2))
}

fn render(harness: &mut Harness, frames: usize) -> Vec<[f32; 2]> {
    stereo(&harness.render(frames * 2))
}

/// The ramp at the two edges of a clip of `length` frames, from the rule and not the code.
fn edges(frame: u64, length: u64) -> f64 {
    let ramp = (RAMP + 1) as f64;
    ((frame + 1) as f64 / ramp)
        .min((length - frame) as f64 / ramp)
        .min(1.0)
}

/// The largest step from one frame to the next, in either channel: a click is a large step.
fn largest_step(frames: &[[f32; 2]]) -> f32 {
    let steps = frames.windows(2).flat_map(|pair| {
        let [before, after] = [pair[0], pair[1]];
        [(after[0] - before[0]).abs(), (after[1] - before[1]).abs()]
    });
    steps.fold(0.0, f32::max)
}

fn steady(level: f32, frames: usize) -> Vec<[f32; 2]> {
    vec![[level, level]; frames]
}

#[test]
fn a_clip_plays_its_file_sample_for_sample_from_the_frame_of_its_tick() {
    let mut harness = audio_harness();
    let file: Vec<[f32; 2]> = (1..=1000)
        .map(|index| [index as f32 / 1000.0, -(index as f32) / 2000.0])
        .collect();
    write_wav(&harness, "count.wav", SAMPLE_RATE, &file);
    // Beat 2 at 120 bpm is tick 960, which is frame 24000.
    add(&mut harness, "count", clip("count.wav", 960));

    let output = play(&mut harness, 26_000);
    assert!(output[..24_000].iter().all(|frame| *frame == [0.0; 2]));
    for (index, (played, file)) in output[24_000..25_000].iter().zip(&file).enumerate() {
        let level = edges(index as u64, 1000);
        for channel in 0..2 {
            let expected = (f64::from(file[channel]) * level) as f32;
            assert!(
                (played[channel] - expected).abs() <= 1e-6,
                "frame {index}: {played:?}, expected {expected}"
            );
        }
    }
    // The first frame of the clip is its first sample on the ramp, not silence.
    assert!(output[24_000][0] > 0.0);
    assert!(output[25_000..].iter().all(|frame| *frame == [0.0; 2]));
}

#[test]
fn a_clip_where_playback_starts_gets_one_ramp_and_not_two() {
    let mut harness = audio_harness();
    let file: Vec<[f32; 2]> = (0..1000).map(|_| [0.5, -0.5]).collect();
    write_wav(&harness, "steady.wav", SAMPLE_RATE, &file);
    add(&mut harness, "steady", clip("steady.wav", 0));
    let output = play(&mut harness, 1_100);
    for (index, played) in output[..1000].iter().enumerate() {
        let expected = (0.5 * edges(index as u64, 1000)) as f32;
        assert!(
            (played[0] - expected).abs() <= 1e-6,
            "frame {index}: {played:?}"
        );
    }
}

#[test]
fn a_file_at_another_rate_plays_at_its_pitch_and_length() {
    let mut harness = audio_harness();
    let sine = |rate: f64, frame: usize| {
        (0.5 * (std::f64::consts::TAU * 1000.0 * frame as f64 / rate).sin()) as f32
    };
    let file: Vec<[f32; 2]> = (0..44_100)
        .map(|frame| [sine(44_100.0, frame); 2])
        .collect();
    write_wav(&harness, "sine.wav", 44_100, &file);
    add(&mut harness, "sine", clip("sine.wav", 0));

    let output = play(&mut harness, 50_000);
    // One second of the file is one second here, 48000 frames, and nothing after it.
    assert!(output[48_000..].iter().all(|frame| *frame == [0.0; 2]));
    assert_ne!(output[47_999], [0.0; 2]);
    let arrangement = id("arrangement");
    assert_eq!(
        arrangement::end(&harness.project, &arrangement),
        Some(Ticks(1920))
    );
    // And in between it is a sine of 1000 Hz at 48 kHz, in phase: the right pitch.
    let worst = output[200..47_800]
        .iter()
        .enumerate()
        .map(|(index, frame)| (f64::from(frame[0]) - f64::from(sine(48_000.0, index + 200))).abs())
        .fold(0.0, f64::max);
    let decibels = 20.0 * (worst / 0.5).log10();
    println!("44.1 kHz file at 48 kHz: worst error {decibels:.1} dB under a 1000 Hz sine");
    assert!(decibels < -70.0, "{decibels}");
}

#[test]
fn trim_gain_and_fades_measure_as_set() {
    let mut harness = audio_harness();
    // Every sample says which frame of the file it is.
    let file: Vec<[f32; 2]> = (0..96_000)
        .map(|frame| [frame as f32 / 200_000.0; 2])
        .collect();
    write_wav(&harness, "ramp.wav", SAMPLE_RATE, &file);
    let mut trimmed = clip("ramp.wav", 0);
    trimmed.file_start_seconds = 0.5;
    trimmed.file_end_seconds = Some(1.5);
    trimmed.gain_db = -6.0;
    trimmed.fade_in_ms = 100.0;
    trimmed.fade_out_ms = 250.0;
    add(&mut harness, "trimmed", trimmed);

    let output = play(&mut harness, 50_000);
    // The part from 0.5 s to 1.5 s: 48000 frames, from file frame 24000.
    assert_ne!(output[47_999], [0.0; 2]);
    assert!(output[48_000..].iter().all(|frame| *frame == [0.0; 2]));
    let gain = f64::from(decibels::amplitude(-6.0));
    let level = |frame: usize| f64::from(output[frame][0]) / f64::from(file[24_000 + frame][0]);
    let measured = [
        ("gain in the middle", level(20_000), gain),
        ("halfway through the fade in", level(2_400), gain * 0.5),
        ("end of the fade in", level(4_800), gain),
        ("halfway through the fade out", level(42_000), gain * 0.5),
        ("a quarter into the fade out", level(39_000), gain * 0.75),
    ];
    for (what, measured, expected) in measured {
        println!("{what}: {measured:.6}, set {expected:.6}");
        assert!(
            (measured - expected).abs() < 1e-5,
            "{what}: {measured} and {expected}"
        );
    }
    // At full level the sample is the file frame 24000 on, so the trim is exact.
    let at = f64::from(output[20_000][0]) / gain * 200_000.0;
    assert!((at - 44_000.0).abs() < 0.1, "{at}");
}

#[test]
fn no_edge_of_a_clip_clicks_and_neither_does_a_stop_or_a_seek() {
    let mut harness = audio_harness();
    // A steady level is the worst case: without a ramp every edge is a step of its full size.
    write_wav(&harness, "high.wav", SAMPLE_RATE, &steady(0.8, 48_000));
    write_wav(&harness, "low.wav", SAMPLE_RATE, &steady(-0.8, 12_000));
    add(&mut harness, "high", clip("high.wav", 100));
    // Over the middle of the first, so it is covered and uncovered.
    let mut low = clip("low.wav", 960);
    low.layer = 1;
    add(&mut harness, "low", low);

    // A ramp takes a level to silence in steps of a 97th. Where one clip covers another of
    // the opposite sign, the last step of the one and the first of the other meet at zero.
    let bound = 0.8 / (RAMP + 1) as f32 + 1e-6;
    let whole = play(&mut harness, 60_000);
    assert_eq!(whole[0], [0.0; 2]);
    let edges = largest_step(&whole);
    println!("largest step at the edges of clips: {edges:.5}, 1.6 without the ramp");
    assert!(edges <= 2.0 * bound, "{edges}");
    assert!(whole.iter().any(|frame| frame[0] == 0.8));
    assert!(whole.iter().any(|frame| frame[0] == -0.8));

    // A stop in the middle of a clip fades out.
    harness.project.engine().stop();
    harness.project.engine().seek(Ticks(1800));
    harness.render(64 * 2);
    let before_stop = play(&mut harness, 1_000);
    harness.project.engine().pause();
    let after_stop = render(&mut harness, 1_000);
    let joined: Vec<[f32; 2]> = before_stop.iter().chain(&after_stop).copied().collect();
    let stop = largest_step(&joined[500..]);
    println!("largest step at a pause in the middle of a clip: {stop:.5}");
    assert!(stop <= bound, "{stop}");
    assert_eq!(*after_stop.last().unwrap(), [0.0; 2]);

    // A seek while it plays, from the high clip into the low one: out and in over one ramp.
    let before_seek = play(&mut harness, 1_000);
    harness.project.engine().seek(Ticks(1_100));
    let after_seek = render(&mut harness, 1_000);
    let joined: Vec<[f32; 2]> = before_seek.iter().chain(&after_seek).copied().collect();
    let seek = largest_step(&joined[500..]);
    println!("largest step at a seek from one clip into another: {seek:.5}, 1.6 without");
    assert!(seek <= 2.0 * bound, "{seek}");
    assert_eq!(after_seek.last().unwrap()[0], -0.8);
}

#[test]
fn where_clips_overlap_the_newest_is_heard_and_the_older_comes_back() {
    let mut harness = audio_harness();
    write_wav(&harness, "old.wav", SAMPLE_RATE, &steady(0.25, 96_000));
    write_wav(&harness, "new.wav", SAMPLE_RATE, &steady(0.5, 24_000));
    add(&mut harness, "old", clip("old.wav", 0));
    // Made after the old one, through the helper that puts a clip on top.
    let track = harness.project.resolve(&id("arrangement/voice")).unwrap();
    let mut changes = Changes::new();
    let added = arrangement::add_audio_clip(
        &harness.project,
        &mut changes,
        &track,
        "new",
        clip("new.wav", 960),
    )
    .unwrap();
    harness.project.commit("Add clip", changes).unwrap();
    let new = harness.project.state(&added).unwrap();
    assert_eq!(new.layer, 1);

    let level = |output: &[[f32; 2]], frame: usize| output[frame][0];
    let output = play(&mut harness, 96_000);
    // The new clip from frame 24000 to 48000, the old one before and after it.
    assert_eq!(level(&output, 12_000), 0.25);
    assert_eq!(level(&output, 36_000), 0.5);
    assert_eq!(level(&output, 60_000), 0.25);
    // Nothing of the old one is heard under the new one: no sum.
    assert!(output[24_200..47_800].iter().all(|frame| frame[0] == 0.5));

    // Deleting the new one brings the old one back whole: it was never changed.
    let mut changes = Changes::new();
    changes.delete(added.id());
    harness.project.commit("Delete clip", changes).unwrap();
    harness.project.engine().seek(Ticks(0));
    let output = play(&mut harness, 96_000);
    assert!(output[200..95_800].iter().all(|frame| frame[0] == 0.25));

    // Of two clips on one layer, the one that starts later is heard.
    let mut changes = Changes::new();
    changes.create(id("arrangement/voice/same"), clip("new.wav", 960));
    harness.project.commit("Add clip", changes).unwrap();
    harness.project.engine().seek(Ticks(0));
    let output = play(&mut harness, 96_000);
    assert_eq!(level(&output, 36_000), 0.5);
}

#[test]
fn a_tempo_change_moves_where_a_clip_starts_and_not_how_long_it_plays() {
    let mut harness = audio_harness();
    write_wav(&harness, "short.wav", SAMPLE_RATE, &steady(0.5, 24_000));
    // Bar 2: frame 96000 at 120 bpm, frame 192000 at 60.
    add(&mut harness, "short", clip("short.wav", 3840));
    let heard = |output: &[[f32; 2]]| {
        let first = output.iter().position(|frame| frame[0] != 0.0).unwrap();
        let last = output.iter().rposition(|frame| frame[0] != 0.0).unwrap();
        (first, last + 1 - first)
    };
    assert_eq!(heard(&play(&mut harness, 130_000)), (96_000, 24_000));

    let mut changes = Changes::new();
    changes.set_tempo_map(tempo(60.0));
    harness.project.commit("Slower", changes).unwrap();
    harness.project.engine().seek(Ticks(0));
    assert_eq!(heard(&play(&mut harness, 230_000)), (192_000, 24_000));
}

#[test]
fn a_file_that_is_not_there_is_a_problem_and_the_rest_plays() {
    let mut harness = audio_harness();
    write_wav(&harness, "here.wav", SAMPLE_RATE, &steady(0.5, 1000));
    let record =
        r#"{"tool": "arrangement.audio_clip", "state": {"asset": "gone.wav", "start": 0}}"#;
    assert_eq!(
        harness.write_and_apply("state/arrangement/voice/gone.json", record),
        1
    );
    add_outside(&mut harness, "here", 960, "here.wav");
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with(
            "state/arrangement/voice/instance.json: the clip \"gone\" plays assets/audio/gone.wav, which is not there"
        ),
        "{problems:?}"
    );
    // The record keeps its place, and the other clip plays.
    assert!(harness.path("state/arrangement/voice/gone.json").exists());
    let output = play(&mut harness, 26_000);
    assert!(output[..24_000].iter().all(|frame| *frame == [0.0; 2]));
    assert_eq!(output[24_500], [0.5; 2]);

    // The file arrives. The clip plays once it changes: the same record again is no change.
    write_wav(&harness, "gone.wav", SAMPLE_RATE, &steady(0.25, 1000));
    assert_eq!(harness.apply(&["state/arrangement/voice/gone.json"]), 0);
    let record =
        r#"{"tool": "arrangement.audio_clip", "state": {"asset": "gone.wav", "start": 480}}"#;
    assert_eq!(
        harness.write_and_apply("state/arrangement/voice/gone.json", record),
        1
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    harness.project.engine().seek(Ticks(0));
    assert_eq!(play(&mut harness, 12_600)[12_500], [0.25; 2]);
}

fn add_outside(harness: &mut Harness, name: &str, start: u64, file: &str) {
    let record = format!(
        r#"{{"tool": "arrangement.audio_clip", "state": {{"asset": "{file}", "start": {start}}}}}"#
    );
    harness.write_and_apply(&format!("state/arrangement/voice/{name}.json"), &record);
}

#[test]
fn an_outside_edit_moves_a_clip_and_changes_its_gain_while_it_plays_and_one_undo_takes_it_back() {
    let mut harness = audio_harness();
    write_wav(&harness, "long.wav", SAMPLE_RATE, &steady(0.5, 240_000));
    add_outside(&mut harness, "long", 0, "long.wav");
    let before = play(&mut harness, 24_000);
    assert_eq!(before[12_000], [0.5; 2]);

    // Quieter by 6 dB, while it plays.
    let record = r#"{"tool": "arrangement.audio_clip", "state": {"asset": "long.wav", "start": 0, "gain_db": -6.0}}"#;
    assert_eq!(
        harness.write_and_apply("state/arrangement/voice/long.json", record),
        1
    );
    let quieter = render(&mut harness, 24_000);
    let joined: Vec<[f32; 2]> = before.iter().chain(&quieter).copied().collect();
    assert!(largest_step(&joined[1_000..]) <= 0.5 / (RAMP + 1) as f32 + 1e-6);
    assert!((quieter[12_000][0] - 0.5 * decibels::amplitude(-6.0)).abs() < 1e-6);

    // Moved a bar later: what plays now is an earlier part of the file, and silence before.
    let record = r#"{"tool": "arrangement.audio_clip", "state": {"asset": "long.wav", "start": 3840, "gain_db": -6.0}}"#;
    harness.write_and_apply("state/arrangement/voice/long.json", record);
    let moved = render(&mut harness, 24_000);
    assert!(moved[1_000..].iter().all(|frame| *frame == [0.0; 2]));
    let clip = harness
        .project
        .resolve::<AudioClip>(&id("arrangement/voice/long"))
        .unwrap();
    assert_eq!(harness.project.state(&clip).unwrap().start, Ticks(3840));

    // One undo takes the move back, and it plays where it was.
    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(harness.project.state(&clip).unwrap().start, Ticks(0));
    let back = render(&mut harness, 24_000);
    assert!((back[12_000][0] - 0.5 * decibels::amplitude(-6.0)).abs() < 1e-6);
}

#[test]
fn clips_in_the_wrong_kind_of_track_are_problems() {
    let mut harness = audio_harness();
    harness.add_track("piano", 1.0);
    write_wav(&harness, "here.wav", SAMPLE_RATE, &steady(0.5, 1000));
    let audio = r#"{"tool": "arrangement.audio_clip", "state": {"asset": "here.wav", "start": 0}}"#;
    harness.write_and_apply("state/arrangement/piano/take.json", audio);
    let notes =
        r#"{"tool": "arrangement.clip", "state": {"start": 0, "length": 960, "notes": []}}"#;
    harness.write_and_apply("state/arrangement/voice/notes.json", notes);
    let problems = harness.problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0]
            .starts_with("state/arrangement/piano/instance.json: take.json is an audio clip")
    );
    assert!(
        problems[1].starts_with("state/arrangement/voice/instance.json: notes.json is a note clip")
    );
}
