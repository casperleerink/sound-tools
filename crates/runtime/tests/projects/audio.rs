//! Audio tracks in whole projects: an agent adds a clip from a file by file, a copied project
//! folder renders the same bytes, audio comes back after close and reopen, an outside edit is
//! heard while it plays and undone in one step, and a missing file is a problem and not a
//! crash.

use std::path::Path;

use arrangement::{AudioClip, Colour, TrackState};
use sound_core::{Changes, InstanceId, Ticks};

use crate::support::{BAR, Harness, clip, difference};

const TRACK: &str = "state/arrangement/voice/instance.json";
const CLIP: &str = "state/arrangement/voice/verse-take.json";

/// A stereo 24-bit WAV of a sine in each channel, left at `hz` and right an octave up, at
/// `rate`, written into the project with `hound`.
fn write_wav(harness: &Harness, name: &str, rate: u32, seconds: f64, hz: f64) {
    let path = harness.path(&format!("assets/audio/{name}"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..(seconds * f64::from(rate)) as usize {
        let time = frame as f64 / f64::from(rate);
        for octave in [1.0, 2.0] {
            let value = 0.4 * (std::f64::consts::TAU * hz * octave * time).sin();
            writer
                .write_sample((value * 8_388_607.0).round() as i32)
                .unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn audio_track_record(order: u32) -> String {
    format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "Voice", "kind": "audio", "colour": "peach", "order": {order}}}}}"#
    )
}

fn audio_clip_record(asset: &str, start: u64, extra: &str) -> String {
    format!(
        r#"{{"tool": "arrangement.audio_clip", "state": {{"asset": "{asset}", "start": {start}{extra}}}}}"#
    )
}

/// The default project with a piano, and an audio track with a clip at 48 kHz on bar 2 and one
/// at 44.1 kHz on bar 3 that it covers in part.
fn piece_with_audio() -> Harness {
    let mut harness = Harness::new();
    harness.write_track(
        "piano",
        1,
        0.1,
        &[("chord", clip(0, 15360, &[(0, 15360, 60)]))],
    );
    add_audio(&mut harness);
    harness
}

/// The default project, whose one track is silent, and the audio track of [`piece_with_audio`].
fn audio_only() -> Harness {
    let mut harness = Harness::new();
    add_audio(&mut harness);
    harness
}

fn add_audio(harness: &mut Harness) {
    write_wav(&harness, "voice.wav", 48_000, 3.0, 220.0);
    write_wav(&harness, "guitar.wav", 44_100, 3.0, 330.0);
    let paths = [
        harness.write(TRACK, &audio_track_record(2)),
        harness.write(
            CLIP,
            &audio_clip_record("voice.wav", 3840, r#", "fade_in_ms": 20.0"#),
        ),
        harness.write(
            "state/arrangement/voice/guitar.json",
            &audio_clip_record("guitar.wav", 7680, r#", "gain_db": -6.0, "layer": 1"#),
        ),
    ];
    assert_eq!(harness.apply(&paths), 3);
    assert_eq!(harness.project.problems(), []);
}

/// Plays from the start after the fade-out of whatever sounded has run its course, so two
/// renders of one session can be compared frame for frame.
fn from_the_start(harness: &mut Harness, frames: usize) -> Vec<f32> {
    harness.project.engine().stop();
    harness.project.engine().seek(Ticks(0));
    harness.render(1_000);
    harness.play(frames)
}

fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The sound of the project as bits, so two renders compare exactly.
fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}

#[test]
fn an_agent_adds_an_audio_track_and_a_clip_from_a_file_by_file_as_its_doc_says() {
    let mut harness = Harness::new();
    // The doc's own example records, with the bar of this project's time signature.
    let doc = std::fs::read_to_string(harness.path("agent-docs/audio.md")).unwrap();
    let example = |path: &str| {
        let fence = format!("```json {path}\n");
        let start = doc.find(&fence).unwrap() + fence.len();
        let length = doc[start..].find("```").unwrap();
        doc[start..start + length].to_string()
    };
    // The agent copies a file into assets/audio/ first, then writes the track and the clip.
    write_wav(&harness, "voice-take-1.wav", 48_000, 5.0, 220.0);
    let paths = [
        harness.write(TRACK, &example(TRACK)),
        harness.write(CLIP, &example(CLIP)),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness.project.poll().unwrap();
    let problems = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert_eq!(problems, sound_core::NO_PROBLEMS);

    // Bar 5 is 8 s at 120 bpm. From there it plays 4 s of the file, from 0.5 s in.
    let output = harness.play_from_the_start(7 * BAR);
    let silent = |from: usize, to: usize| output[2 * from..2 * to].iter().all(|s| *s == 0.0);
    assert!(silent(0, 4 * BAR));
    assert!(!silent(4 * BAR, 4 * BAR + 100));
    assert!(silent(6 * BAR, 7 * BAR));
    let summary = runtime::summary(&harness.project);
    assert!(
        summary.contains(
            "audio clip `arrangement/voice/verse-take`: 5:1:000 to 7:1:000, ticks 15360 to 23040, voice-take-1.wav from 0.500 s to 4.500 s of 5.000 s, gain -3 dB, fades 10 ms and 200 ms, layer 0"
        ),
        "{summary}"
    );
}

#[test]
fn a_copied_project_folder_opens_and_renders_the_same_bytes() {
    let mut harness = piece_with_audio();
    let original = harness.play_from_the_start(4 * BAR);
    assert!(original.iter().any(|sample| *sample != 0.0));

    let elsewhere = tempfile::tempdir().unwrap();
    copy_folder(harness.project.root(), &elsewhere.path().join("moved"));
    let copy = tempfile::tempdir().unwrap();
    copy_folder(&elsewhere.path().join("moved"), copy.path());
    let mut copied = Harness::open(copy);
    assert_eq!(copied.project.problems(), []);
    let render = copied.play_from_the_start(4 * BAR);
    assert_eq!(bits(&render), bits(&original));
}

#[test]
fn audio_tracks_and_clips_come_back_after_close_and_reopen_and_render_the_same() {
    let mut harness = piece_with_audio();
    let before = harness.play_from_the_start(4 * BAR);
    let files = [TRACK, CLIP, "state/arrangement/voice/guitar.json"];
    let read = |harness: &Harness| -> Vec<String> {
        let read = |path: &&str| std::fs::read_to_string(harness.path(path)).unwrap();
        files.iter().map(read).collect()
    };
    let records = read(&harness);
    assert!(records[0].contains(r#""kind": "audio""#), "{}", records[0]);

    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    assert_eq!(read(&harness), records);
    let voice = InstanceId::new("arrangement/voice").unwrap();
    let voice = harness.project.resolve::<TrackState>(&voice).unwrap();
    assert_eq!(
        harness.project.state(&voice).unwrap().kind,
        arrangement::TrackKind::Audio
    );
    assert_eq!(bits(&harness.play_from_the_start(4 * BAR)), bits(&before));
}

#[test]
fn an_outside_edit_of_an_audio_clip_while_it_plays_is_heard_and_undone_in_one_step() {
    let mut harness = audio_only();
    let reference = from_the_start(&mut harness, 4 * BAR);
    // One session: the first bar and a half as it was, then an agent moves the clip of bar 2 a
    // bar later and turns it down, while it plays.
    let mut played = from_the_start(&mut harness, BAR + BAR / 2);
    let moved = audio_clip_record(
        "voice.wav",
        7680,
        r#", "gain_db": -12.0, "fade_in_ms": 20.0"#,
    );
    assert_eq!(harness.write_and_apply(CLIP, &moved), 1);
    played.extend(harness.play(BAR / 2 + 2 * BAR));
    // Until the edit, the sound is the one from before, and it changes at the edit.
    let (first, _) = difference(&played, &reference).unwrap();
    // (The sine of the file crosses zero on the very frame of the edit.)
    assert!(
        (BAR + BAR / 2..BAR + BAR / 2 + 2).contains(&first),
        "{first}"
    );
    // After its ramp the rest of bar 2 is silent, where the clip was.
    let silent = |from: usize, to: usize| played[2 * from..2 * to].iter().all(|s| *s == 0.0);
    assert!(silent(BAR + BAR / 2 + 96, 2 * BAR));
    assert!(!silent(2 * BAR, 2 * BAR + 100));
    let clip = InstanceId::new("arrangement/voice/verse-take").unwrap();
    let clip = harness.project.resolve::<AudioClip>(&clip).unwrap();
    assert_eq!(harness.project.state(&clip).unwrap().start, Ticks(7680));

    // One undo takes it back, record and sound.
    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(harness.project.state(&clip).unwrap().start, Ticks(3840));
    assert_eq!(
        bits(&from_the_start(&mut harness, 4 * BAR)),
        bits(&reference)
    );
}

#[test]
fn a_missing_audio_file_is_in_problems_txt_and_the_rest_plays() {
    let mut harness = piece_with_audio();
    let with_both = harness.play_from_the_start(4 * BAR);
    // The same project without the guitar clip: what the rest plays.
    let guitar = "state/arrangement/voice/guitar.json";
    let record = std::fs::read_to_string(harness.path(guitar)).unwrap();
    let guitar_path = harness.path(guitar);
    std::fs::remove_file(&guitar_path).unwrap();
    harness.apply(&[guitar_path]);
    let without_guitar = harness.play_from_the_start(4 * BAR);
    assert!(difference(&with_both, &without_guitar).is_some());

    // The guitar clip is back, and its file is gone.
    std::fs::remove_file(harness.path("assets/audio/guitar.wav")).unwrap();
    assert_eq!(harness.write_and_apply(guitar, &record), 1);
    harness.project.poll().unwrap();
    let problems = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert!(
        problems.contains(
            "state/arrangement/voice/instance.json: the clip \"guitar\" plays assets/audio/guitar.wav, which is not there"
        ),
        "{problems}"
    );
    assert!(harness.path(guitar).exists());
    assert_eq!(
        bits(&harness.play_from_the_start(4 * BAR)),
        bits(&without_guitar)
    );
}

#[test]
fn an_audio_track_and_a_file_from_outside_are_one_undo_step() {
    let mut harness = Harness::new();
    let outside = tempfile::tempdir().unwrap();
    let source = outside.path().join("Lead Vocal.wav");
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        for frame in 0..48_000 {
            writer.write_sample(((frame % 100) * 100) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }
    let arrangement = InstanceId::new("arrangement").unwrap();
    let mut changes = Changes::new();
    let track = arrangement::add_audio_track(
        &harness.project,
        &mut changes,
        &arrangement,
        "Vocal",
        Colour::Pink,
    )
    .unwrap();
    // The track does not exist yet, so the clip goes in the same group by the id it will have.
    harness.project.commit("Add audio track", changes).unwrap();
    let mut changes = Changes::new();
    let clip =
        arrangement::add_audio_file(&harness.project, &mut changes, &track, &source, Ticks(0))
            .unwrap();
    harness.project.commit("Add audio file", changes).unwrap();
    assert_eq!(
        clip.id(),
        &InstanceId::new("arrangement/vocal/lead-vocal").unwrap()
    );
    assert!(harness.path("assets/audio/lead-vocal.wav").exists());
    assert_eq!(harness.project.problems(), []);
    assert!(
        from_the_start(&mut harness, BAR / 4)
            .iter()
            .any(|sample| *sample != 0.0)
    );

    // One undo takes the clip away, and the file stays: an asset is never deleted.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add audio file")
    );
    assert!(harness.project.state(&clip).is_none());
    assert!(
        !harness
            .path("state/arrangement/vocal/lead-vocal.json")
            .exists()
    );
    assert!(harness.path("assets/audio/lead-vocal.wav").exists());
    assert!(
        from_the_start(&mut harness, BAR / 4)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_eq!(
        harness.project.redo().unwrap().as_deref(),
        Some("Add audio file")
    );
    assert!(harness.project.state(&clip).is_some());
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add audio file")
    );
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add audio track")
    );
    assert!(!harness.path("state/arrangement/vocal").exists());
}
