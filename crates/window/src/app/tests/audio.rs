//! Audio clips of the song, driven by the pointer: a trim and a fade are one undo step each, and
//! the sound and the wave follow the hand; a take draws as it records and leaves an audio clip.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use runtime::input::Input;
use sound_core::feeds::{Feed, FeedRequest, Readings, read};

use super::input::{AUDIO_TRACK, Microphone, press, shown_of_take, until, window_of};
use super::*;

/// A tone of `seconds` at 48 kHz, half as loud as it can be: a 16-bit mono WAV.
pub(super) fn tone(path: &Path, seconds: f32) {
    let samples: Vec<i16> = (0..(seconds * 48_000.0) as usize)
        .map(|frame| ((frame as f32 * 220.0 / 48_000.0 * std::f32::consts::TAU).sin() * 16_000.0) as i16)
        .collect();
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(48_000_u32.to_le_bytes());
    bytes.extend(96_000_u32.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The template with an audio track `audio-1`, and on it `tone`, a four-second clip at bar 1.
fn with_audio_track(regions: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let files = AUDIO_TRACK.iter().copied().chain(regions.iter().copied());
    for (path, contents) in files {
        let path = folder.join("state/song").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    tone(&folder.join("assets/audio/tone.wav"), 4.0);
    (scratch, folder)
}

const TONE: (&str, &str) = (
    "audio-1/tone.json",
    r#"{"tool": "region", "state": {"asset": "tone.wav", "start": 0}}"#,
);

fn rect(window: &rune::Runtime<State>, key: &str) -> [f32; 4] {
    window.presented(key).unwrap_or_else(|| panic!("no {key}")).0
}

/// The clip `tone`, chosen alone, once its file was read: it is as long as its four seconds,
/// two bars, and its wave is drawn.
fn chosen_tone() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_audio_track(&[TONE]);
    let mut window = window_of(&folder);
    until(&mut window, "the length of the file", |_| true);
    let start = Instant::now();
    while window.presented("wave-audio-1/tone").is_none() || rect(&window, "clip-audio-1/tone")[2] < 190.0 {
        assert!(start.elapsed().as_secs() < 30, "the clip never took its file's length");
        redraw(&mut window);
    }
    let [x, y, w, h] = rect(&window, "clip-audio-1/tone");
    window.pointer_down(x + w / 2.0, y + h / 2.0);
    redraw(&mut window);
    window.pointer_up(x + w / 2.0, y + h / 2.0);
    redraw(&mut window);
    assert!(window.presented("fade-in-audio-1/tone").is_some(), "chosen alone, it shows its handles");
    (scratch, window)
}

/// The record of `tone` as it plays now, and as its file keeps it.
fn tone_record(state: &State) -> (Value, Value) {
    let id = InstanceId::new("song/audio-1/tone").unwrap();
    let played = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    (played, record(state, "song/audio-1/tone.json")["state"].clone())
}

/// The loudest of the first eighth of the clip `tone`, as its wave shows it.
fn first_of_wave(state: &State) -> f32 {
    let (id, sources) = (InstanceId::new("song/audio-1/tone").unwrap(), state.sources());
    let request = FeedRequest { tick: sound_core::Ticks(0), points: 8, range: Some([0.0, 7680.0]) };
    match read(state.project(), &Readings::new(), &sources, &id, "wave", request) {
        Some(Feed::List(list)) => list[0],
        other => panic!("{other:?}"),
    }
}

fn drag_by(window: &mut rune::Runtime<State>, from: (f32, f32), dx: f32) -> (f32, f32) {
    window.pointer_down(from.0, from.1);
    redraw(window);
    let end = travel(window, from, (dx / 8.0, 0.0), 8);
    redraw(window);
    end
}

#[test]
fn the_end_of_an_audio_clip_trims_on_the_snap_as_one_step_and_its_wave_follows() {
    let (_scratch, mut window) = chosen_tone();
    let wave = rect(&window, "wave-audio-1/tone")[2];
    let [x, y, w, h] = rect(&window, "clip-audio-1/tone");
    // 48 points are a second at 120 bpm: it ends a second earlier.
    let end = drag_by(&mut window, (x + w - 2.0, y + h / 2.0), -48.0);
    window.pointer_up(end.0, end.1);
    redraw(&mut window);
    redraw(&mut window);
    let (played, saved) = tone_record(&window.state);
    assert_eq!(saved["file_end_seconds"], 3.0, "{played}");
    assert_eq!(window.state.project().undo_label().as_deref(), Some("Trim a region"));
    assert!((rect(&window, "wave-audio-1/tone")[2] - wave * 0.75).abs() < 1.0, "the wave is as long as it plays");
    window.state.undo().unwrap();
    assert_eq!(tone_record(&window.state).1.get("file_end_seconds"), None);
}

#[test]
fn a_fade_handle_plays_its_fade_as_it_drags_the_wave_follows_and_its_release_is_one_step() {
    let (_scratch, mut window) = chosen_tone();
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    let before = (first_of_wave(&window.state), window.state.project().undo_label());
    let [x, y, w, h] = rect(&window, "fade-in-audio-1/tone");
    let held = drag_by(&mut window, (x + w / 2.0, y + h / 2.0), 48.0);
    // Held: the engine plays a second's fade in, the file keeps none, and no view ran.
    let (played, saved) = tone_record(&window.state);
    assert!((played["fade_in_ms"].as_f64().unwrap() - 1000.0).abs() < 25.0, "{played}");
    assert_eq!(saved.get("fade_in_ms"), None);
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
    assert!(first_of_wave(&window.state) < before.0 * 0.6, "the wave fades in");
    window.pointer_up(held.0, held.1);
    redraw(&mut window);
    redraw(&mut window);
    let kept = tone_record(&window.state).1["fade_in_ms"].as_f64().unwrap();
    assert!((kept - 1000.0).abs() < 25.0, "{kept}");
    assert_eq!(window.state.project().undo_label().as_deref(), Some("Change fade in"));
    window.state.undo().unwrap();
    assert_eq!(window.state.project().undo_label(), before.1);
}

/// The width of the take on `track` as the window draws it now.
fn take_width(window: &rune::Runtime<State>, track: &str) -> Option<f32> {
    window.presented(&format!("take-{track}")).map(|(rect, ..)| rect[2])
}

#[test]
fn a_take_draws_its_sound_as_it_grows_and_leaves_an_audio_clip_with_its_wave() {
    let (_scratch, folder) = with_audio_track(&[]);
    let mut window = window_of(&folder);
    let microphone = Microphone::default();
    window.state.incoming.live = (window.state.session)
        .background(|project| Input::new(project, microphone.devices(), Arc::new(|| {})));
    act(&mut window.state, json!({ "label": "Arm Audio 1", "action": "toggle" }));
    press(&mut window, "r");
    until(&mut window, "the take", |state| state.session.recording());
    let mut widths = Vec::new();
    for frames in [24_000, 48_000] {
        while window.state.session.status().frames < frames {
            microphone.write_until(window.state.session.status().frames + 2_048);
            redraw(&mut window);
        }
        widths.push(take_width(&window, "audio-1").expect("the take draws while it records"));
    }
    assert!(widths[1] > widths[0] + 20.0, "it grows with the playhead: {widths:?}");
    let (_, sound, ..) = shown_of_take(&window.state, "song/audio-1/rec");
    assert!(sound > 0, "its wave reads the sound so far");

    press(&mut window, "space");
    until(&mut window, "the clip of the take", |state| {
        microphone.write_until(state.session.status().frames + 2_048);
        take_width_gone(state) && !super::input::children_of(state, "song/audio-1", "region").is_empty()
    });
    let region = super::input::children_of(&window.state, "song/audio-1", "region").remove(0);
    let start = Instant::now();
    while window.presented(&format!("wave-audio-1/{region}")).is_none() {
        assert!(start.elapsed().as_secs() < 30, "the clip of the take draws no wave");
        redraw(&mut window);
    }
    // The facts that the take ended reach the view at their next poll.
    let start = Instant::now();
    while take_width(&window, "audio-1").is_some() {
        assert!(start.elapsed().as_secs() < 30, "the take still draws after it ended");
        redraw(&mut window);
    }
}

/// The recorder of `audio-1` no longer says it takes.
fn take_width_gone(state: &State) -> bool {
    shown_of_take(state, "song/audio-1/rec").3.is_null()
}
