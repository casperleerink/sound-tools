//! The note editor of the DAW, driven by the pointer and the keys as a composer drives it.

use super::*;
use crate::app::keys::on_key;

/// A new project with the editor of its first clip open, `t00/bass`.
fn editor() -> (tempfile::TempDir, rune::Runtime<State>) {
    opened_on(new_project())
}

fn opened_on(scratch: tempfile::TempDir) -> (tempfile::TempDir, rune::Runtime<State>) {
    let mut window = laid_out(opened(&scratch.path().join("song")));
    window.on_key = Some(on_key);
    act(
        &mut window.state,
        json!({ "action": "song.select_all_clips" }),
    );
    act(&mut window.state, json!({ "action": "song.open_clip" }));
    settle(&mut window);
    (scratch, window)
}

/// Frames until what came in has slid to its place, so a press lands where it shows.
fn settle(window: &mut rune::Runtime<State>) {
    for _ in 0..30 {
        redraw(window);
    }
}

fn notes(state: &State) -> Vec<Value> {
    record(state, "song/t00/bass.json")["state"]["notes"]
        .as_array()
        .unwrap()
        .clone()
}

fn with_shift(window: &mut rune::Runtime<State>, shift: bool) {
    window.physical_input(rune::PhysicalInput::Modifiers(rune::Mods {
        shift,
        ..rune::Mods::default()
    }));
}

/// A press at `from` window points, a move by `by` in eight steps, and a release.
fn drag_from(window: &mut rune::Runtime<State>, from: (f32, f32), by: (f32, f32)) {
    window.pointer_down(from.0, from.1);
    window.frame(1.0 / 60.0);
    let end = travel(window, from, (by.0 / 8.0, by.1 / 8.0), 8);
    window.pointer_up(end.0, end.1);
    redraw(window);
}

#[test]
fn a_drag_moves_the_chosen_notes_on_the_snap_as_one_step_of_undo() {
    let (_scratch, mut window) = editor();
    let before = notes(&window.state);
    click(&mut window, "note-0");
    with_shift(&mut window, true);
    click(&mut window, "note-1");
    with_shift(&mut window, false);
    let at = window.find("note-0").unwrap();
    // Right about two and a half sixteenths, up two rows.
    drag_from(&mut window, at, (40.0, -24.0));
    let after = notes(&window.state);
    let moved = |index: usize, field: &str| {
        after[index][field].as_i64().unwrap() - before[index][field].as_i64().unwrap()
    };
    let ticks = moved(0, "start");
    assert!(ticks > 0 && ticks % 240 == 0, "moved {ticks} ticks");
    assert_eq!(
        [moved(0, "pitch"), moved(1, "start"), moved(1, "pitch")],
        [2, ticks, 2]
    );
    assert_eq!(after[2..], before[2..]);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(notes(&window.state), before);
}

#[test]
fn a_line_drawn_across_the_velocity_lane_sets_the_notes_it_passes_as_one_step_of_undo() {
    let (_scratch, mut window) = editor();
    let before = notes(&window.state);
    let [x, y, _, h] = window.presented("roll-t00-bass-velocity").unwrap().0;
    drag_from(&mut window, (x + 20.0, y + h - 6.0), (400.0, -(h - 12.0)));
    let velocities: Vec<i64> = notes(&window.state)
        .iter()
        .map(|note| note["velocity"].as_i64().unwrap())
        .collect();
    assert_eq!(velocities[0], 100, "{velocities:?}");
    assert!(
        velocities[1] < 40 && velocities[6] > velocities[1],
        "{velocities:?}"
    );
    assert!(
        velocities[1..7].windows(2).all(|pair| pair[0] < pair[1]),
        "{velocities:?}"
    );
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(notes(&window.state), before);
}

#[test]
fn a_start_typed_in_the_head_moves_the_chosen_note() {
    let (_scratch, mut window) = editor();
    click(&mut window, "note-2");
    settle(&mut window);
    click(&mut window, "note-start");
    redraw(&mut window);
    for letter in "3.2.1".chars() {
        key(&mut window, rune::Key::Text(letter.to_string()));
    }
    key(&mut window, rune::Key::Enter);
    assert_eq!(notes(&window.state)[2]["start"], 2 * 3840 + 960);
}

#[test]
fn the_arrows_move_a_chosen_note_and_with_shift_resize_it() {
    let (_scratch, mut window) = editor();
    let before = notes(&window.state)[0].clone();
    click(&mut window, "note-0");
    key(&mut window, rune::Key::Right);
    key(&mut window, rune::Key::Up);
    window.key(rune::KeyEvent {
        key: rune::Key::Right,
        mods: rune::Mods {
            shift: true,
            ..rune::Mods::default()
        },
    });
    redraw(&mut window);
    let after = &notes(&window.state)[0];
    assert_eq!(after["start"], before["start"].as_i64().unwrap() + 240);
    assert_eq!(after["pitch"], before["pitch"].as_i64().unwrap() + 1);
    assert_eq!(after["length"], before["length"].as_i64().unwrap() + 240);
}

/// The editor of a clip of 2,000 notes, once they show.
fn many_notes() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let clip = scratch.path().join("song/state/song/t00/bass.json");
    let mut record: Value = serde_json::from_str(&std::fs::read_to_string(&clip).unwrap()).unwrap();
    let notes: Vec<Value> = (0..2000)
        .map(|at| json!({ "start": at * 240, "length": 240, "pitch": 36 + (at * 7) % 48, "velocity": 40 + (at * 13) % 87 }))
        .collect();
    record["state"]["notes"] = json!(notes);
    record["state"]["length"] = Value::Null;
    std::fs::write(&clip, record.to_string()).unwrap();
    let (scratch, mut window) = opened_on(scratch);
    for _ in 0..600 {
        if window.find("note-1999").is_some() {
            break;
        }
        redraw(&mut window);
    }
    assert!(
        window.find("note-1999").is_some(),
        "{}",
        window.state.report()["problems"]
    );
    (scratch, window)
}

#[test]
fn two_thousand_notes_of_the_editor_scroll_play_zoom_and_drag_within_their_budgets_and_run_no_view()
{
    let (_scratch, mut window) = many_notes();
    // Some bars across, as a composer edits them.
    act(
        &mut window.state,
        json!({ "key": "roll-t00-bass", "action": "view", "value": { "zoom": 20, "left": 7680, "top": 10 } }),
    );
    settle(&mut window);
    let ([scroll, play, zoom], _) =
        super::budget::measure(&mut window, "editor", "roll-t00-bass", "note-42");
    assert!(
        scroll < super::budget::SCROLL_AND_PLAY,
        "a scroll frame cost {scroll:?}"
    );
    assert!(
        play < super::budget::SCROLL_AND_PLAY,
        "a play frame cost {play:?}"
    );
    assert!(zoom < super::budget::ZOOM, "a zoom frame cost {zoom:?}");
}
