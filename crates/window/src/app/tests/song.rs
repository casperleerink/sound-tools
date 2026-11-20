//! The song page of the DAW template, driven by the pointer and the keys as a composer drives it.

use serde_json::{Value, json};

use super::{act, key, laid_out, new_project, opened, record, redraw, travel};
use crate::app::{Drawn, InstanceId, State};

fn song() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let window = laid_out(opened(&scratch.path().join("song")));
    (scratch, window)
}

fn rect(window: &rune::Runtime<State>, key: &str) -> [f32; 4] {
    window
        .presented(key)
        .unwrap_or_else(|| panic!("no {key}"))
        .0
}

/// Presses at `at` and lets go, a frame between.
fn press_at(window: &mut rune::Runtime<State>, at: (f32, f32)) {
    window.pointer_down(at.0, at.1);
    window.frame(1.0 / 60.0);
    window.pointer_up(at.0, at.1);
    redraw(window);
}

/// Drags from `from` by `by` window points in eight steps, and lets go.
fn drag(window: &mut rune::Runtime<State>, from: (f32, f32), by: (f32, f32)) {
    window.pointer_down(from.0, from.1);
    window.frame(1.0 / 60.0);
    let end = travel(window, from, (by.0 / 8.0, by.1 / 8.0), 8);
    window.pointer_up(end.0, end.1);
    redraw(window);
}

fn start(state: &State, clip: &str) -> Value {
    let path = format!("song/{clip}.json");
    let file = state.project().root().join("state").join(&path);
    if file.exists() {
        record(state, &path)["state"]["start"].clone()
    } else {
        Value::Null
    }
}

#[test]
fn a_clip_dragged_lands_on_the_snap_as_one_undo_step() {
    let (_scratch, mut window) = song();
    let before = start(&window.state, "t00/bass");
    let [x, y, ..] = rect(&window, "clip-t00/bass");
    // 61 points at zoom 1 are 2440 ticks: the snap's sixteenths take it to 2400.
    drag(&mut window, (x + 20.0, y + 20.0), (61.0, 0.0));
    assert_eq!(start(&window.state, "t00/bass"), 2400);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(start(&window.state, "t00/bass"), before);
}

/// A child that did not change is the same in every run of the view, so a move of a clip builds
/// its track's row again and keeps the row of every other track.
#[test]
fn a_clip_moved_builds_only_its_row_again() {
    let (_scratch, mut window) = song();
    let builds = |window: &rune::Runtime<State>| {
        let song = InstanceId::new("song").unwrap();
        let Some(Drawn::View { memos, .. }) = window.state.drawn(&song) else {
            panic!("the song has no view");
        };
        ["time-t00", "time-t01"].map(|row| memos[row].0)
    };
    let [x, y, ..] = rect(&window, "clip-t00/bass");
    press_at(&mut window, (x + 20.0, y + 20.0));
    let before = builds(&window);
    drag(&mut window, (x + 20.0, y + 20.0), (61.0, 0.0));
    assert_eq!(start(&window.state, "t00/bass"), 2400);
    let after = builds(&window);
    assert_ne!(after[0], before[0]);
    assert_eq!(after[1], before[1]);
}

/// What Export selection exports follows every edit of the chosen: a move, and its undo.
#[test]
fn the_span_of_the_chosen_follows_a_move_and_its_undo() {
    let (_scratch, mut window) = song();
    let [x, y, ..] = rect(&window, "clip-t00/bass");
    drag(&mut window, (x + 20.0, y + 20.0), (61.0, 0.0));
    let span = |window: &rune::Runtime<State>| {
        let (from, to) = window.state.session.selected_span().unwrap();
        (from.0, to.0)
    };
    assert_eq!(span(&window), (2400, 2400 + 15360));
    act(&mut window.state, json!({ "action": "undo" }));
    redraw(&mut window);
    assert_eq!(span(&window), (0, 15360));
}

#[test]
fn a_drag_over_empty_space_chooses_the_clips_it_covers_and_delete_removes_them() {
    let (_scratch, mut window) = song();
    let [x, y, w, _] = rect(&window, "clip-t00/bass");
    let [_, below, _, tall] = rect(&window, "clip-t01/chords");
    // From right of the first track's clip, left and down over both.
    drag(
        &mut window,
        (x + w + 30.0, y + 4.0),
        (40.0 - w, below + tall / 2.0 - y),
    );
    key(&mut window, rune::Key::Backspace);
    assert_eq!(start(&window.state, "t00/bass"), Value::Null);
    assert_eq!(start(&window.state, "t01/chords"), Value::Null);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_ne!(start(&window.state, "t00/bass"), Value::Null);
    assert_ne!(start(&window.state, "t01/chords"), Value::Null);
}

/// The old rule: audio lands at the nearest beat on the audio track under it, and a MIDI file of
/// one part becomes a clip in the cell under it on that instrument track.
#[test]
fn files_dropped_on_the_timeline_land_on_their_row_and_tick_as_one_undo_step_each() {
    let (_scratch, mut window) = song();
    let outside = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        act(
            &mut window.state,
            json!({ "key": "add-track-kind", "action": "open" }),
        );
        act(
            &mut window.state,
            json!({ "label": "Audio track", "action": "press" }),
        );
        redraw(&mut window);
    }
    let (sound, tune) = (
        outside.path().join("kick.wav"),
        outside.path().join("tune.mid"),
    );
    super::host::wav(&sound);
    super::drops::midi(&tune, &[("Lead", 60)]);
    let drop = |window: &mut rune::Runtime<State>, file: &std::path::Path, x: u32, y: f32| {
        let value = json!({ "files": [file], "x": x, "y": y });
        act(
            &mut window.state,
            json!({ "key": "song", "action": "drop", "value": value }),
        );
        redraw(window);
    };
    // Rows: t00, t01, audio-1, then audio-2, the one dropped on. Bar 3 is tick 7680.
    drop(&mut window, &sound, 7680 + 300, 3.5);
    assert_eq!(start(&window.state, "audio-2/region"), 7680);
    let region = record(&window.state, "song/audio-2/region.json");
    assert_eq!(region["state"]["asset"], "kick.wav");
    drop(&mut window, &tune, 7680 + 100, 1.5);
    assert_eq!(start(&window.state, "t01/clip"), 7680);
    let clip = record(&window.state, "song/t01/clip.json");
    assert_eq!(clip["state"]["notes"][0]["pitch"], 60);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(start(&window.state, "t01/clip"), Value::Null);
    assert_eq!(start(&window.state, "audio-2/region"), 7680);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(start(&window.state, "audio-2/region"), Value::Null);
    let audio_track = window.state.project().root().join("state/song/audio-2");
    assert!(audio_track.exists());
}

#[test]
fn a_track_of_another_instrument_is_added_as_one_undo_step() {
    let (_scratch, mut window) = song();
    act(
        &mut window.state,
        json!({ "key": "add-track-kind", "action": "open" }),
    );
    act(
        &mut window.state,
        json!({ "label": "Sampler track", "action": "press" }),
    );
    redraw(&mut window);
    let synth = |state: &State| {
        let file = state.project().root().join("state/song/track/synth.json");
        file.exists()
            .then(|| record(state, "song/track/synth.json")["tool"].clone())
    };
    assert_eq!(synth(&window.state), Some(json!("sampler")));
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(synth(&window.state), None);
}

#[test]
fn a_double_click_on_a_head_names_its_track_in_place() {
    let (_scratch, mut window) = song();
    let [x, y, ..] = rect(&window, "track-t01");
    press_at(&mut window, (x + 60.0, y + 16.0));
    press_at(&mut window, (x + 60.0, y + 16.0));
    redraw(&mut window);
    assert_eq!(super::focused(&window).as_deref(), Some("rename-t01"));
    for letter in ["B", "a", "s", "s"] {
        key(&mut window, rune::Key::Text(letter.into()));
    }
    key(&mut window, rune::Key::Enter);
    let title = |state: &State| record(state, "song/t01/instance.json")["state"]["title"].clone();
    assert_eq!(title(&window.state), "Bass");
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(title(&window.state), "Track 2");
}

#[test]
fn a_clip_is_named_in_place_from_its_menu() {
    let (_scratch, mut window) = song();
    act(
        &mut window.state,
        json!({ "key": "clip-t00/bass", "action": "press", "value": { "right": true } }),
    );
    act(
        &mut window.state,
        json!({ "label": "Rename", "action": "press" }),
    );
    redraw(&mut window);
    assert_eq!(
        super::focused(&window).as_deref(),
        Some("clip-name-t00/bass")
    );
    for letter in ["R", "o", "o", "t"] {
        key(&mut window, rune::Key::Text(letter.into()));
    }
    key(&mut window, rune::Key::Enter);
    assert_eq!(
        record(&window.state, "song/t00/bass.json")["state"]["title"],
        "Root"
    );
}

/// Enter and Space on an element the keys focused that has a press but no role press it,
/// before the song's own Enter, which would rename the chosen track. A clip focused by a click
/// still opens on Enter.
#[test]
fn enter_and_space_press_a_focused_element_before_the_actions_of_the_song() {
    let (_scratch, mut window) = song();
    let chosen =
        |window: &rune::Runtime<State>| window.state.report()["selected"]["instance"].clone();
    for pressed in [rune::Key::Enter, rune::Key::Text(" ".into())] {
        act(
            &mut window.state,
            json!({ "key": "clip-t00/bass", "action": "press" }),
        );
        redraw(&mut window);
        assert_eq!(chosen(&window), "song/t00/bass");
        window.update(|_, tx| tx.focus("timeline-foot", true));
        redraw(&mut window);
        assert_eq!(super::focused(&window).as_deref(), Some("timeline-foot"));
        key(&mut window, pressed);
        redraw(&mut window);
        // Its press let go of the clip; no name field opened.
        assert_eq!(chosen(&window), "song/t00");
        assert!(window.find("rename-t00").is_none());
    }
    let [x, y, w, h] = rect(&window, "clip-t00/bass");
    press_at(&mut window, (x + w / 2.0, y + h / 2.0));
    assert_eq!(super::focused(&window).as_deref(), Some("clip-t00/bass"));
    key(&mut window, rune::Key::Enter);
    redraw(&mut window);
    assert!(window.find("roll-t00-bass").is_some());
}

/// The heads of the tracks scroll with the timeline: a wheel over the clips moves a head as far
/// as its clips.
#[test]
fn the_heads_of_the_tracks_scroll_with_the_timeline() {
    let scratch = new_project();
    let song = scratch.path().join("song/state/song");
    for track in 2..24 {
        let to = song.join(format!("t{track:02}"));
        std::fs::create_dir_all(to.join("chain")).unwrap();
        for file in [
            "instance.json",
            "mix.json",
            "synth.json",
            "rec.json",
            "chain/instance.json",
        ] {
            if song.join("t00").join(file).exists() {
                std::fs::copy(song.join("t00").join(file), to.join(file)).unwrap();
            }
        }
    }
    let mut window = laid_out(opened(&scratch.path().join("song")));
    for _ in 0..5 {
        redraw(&mut window);
    }
    let head = |window: &rune::Runtime<State>| rect(window, "colour-t00")[1];
    let clip = |window: &rune::Runtime<State>| rect(window, "clip-t00/bass")[1];
    let (head_before, clip_before) = (head(&window), clip(&window));
    let [x, y, ..] = rect(&window, "clip-t00/bass");
    window.wheel_event(rune::WheelEvent::new(0.0, -120.0).at(x + 10.0, y + 10.0));
    for _ in 0..30 {
        redraw(&mut window);
    }
    let moved = clip_before - clip(&window);
    assert!(moved > 50.0, "{moved}");
    assert_eq!(head_before - head(&window), moved);
}

/// The track the song chooses is what the keyboard plays, and a chosen clip is the selection
/// that Export selection exports.
#[test]
fn a_track_the_song_chooses_plays_the_keyboard_and_a_chosen_clip_is_the_selection() {
    let (_scratch, mut window) = song();
    let selected = |window: &rune::Runtime<State>| {
        let selected = window
            .state
            .session
            .selected()
            .map(|id| id.as_str().to_string());
        (selected, window.state.session.selected_span().is_some())
    };
    super::act(
        &mut window.state,
        serde_json::json!({ "label": "Choose Track 2", "action": "press" }),
    );
    redraw(&mut window);
    let (track, span) = selected(&window);
    assert!(
        track
            .as_deref()
            .is_some_and(|id| id.starts_with("song/t01")),
        "{track:?}"
    );
    assert!(!span);
    super::act(
        &mut window.state,
        serde_json::json!({ "label": "Clip at 1.1 on Track 1", "action": "press" }),
    );
    redraw(&mut window);
    let (clip, span) = selected(&window);
    assert!(
        clip.as_deref().is_some_and(|id| id.starts_with("song/t00")),
        "{clip:?}"
    );
    assert!(span);
}
