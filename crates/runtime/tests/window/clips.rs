//! Clips in the arrangement, edited with a simulated mouse and keys. Every test checks the
//! project, the undo history and the files.

use gpui::{TestAppContext, point, px};
use sound_core::Changes;

use crate::support::{self, BAR, Opened, STEP, clip, id, note};

const PART: &str = "arrangement/track-1/part";

/// Two tracks. `part` on the first: bars 2 and 3, with a note in each bar.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    })
}

fn part() -> sound_notes::Clip {
    clip(
        BAR,
        2 * BAR,
        vec![note(960, 480, 60), note(BAR + 960, 480, 64)],
    )
}

#[gpui::test]
fn a_double_click_on_empty_track_space_adds_a_clip_of_one_bar(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Bar 6 and a bit, on the second track.
    let place = opened.at(5 * BAR + STEP + 100, 1);
    opened.double_click(place);

    let added = "arrangement/track-2/clip";
    assert_eq!(opened.clip(added), Some(clip(5 * BAR + STEP, BAR, vec![])));
    assert_eq!(opened.selected_clip(), Some(id(added)));
    assert_eq!(opened.undo_label().as_deref(), Some("Add clip"));
    assert!(
        opened
            .clip_file(added)
            .unwrap()
            .contains("\"start\": 19440")
    );

    // The next one on the same track gets the next free name.
    let place = opened.at(8 * BAR, 1);
    opened.double_click(place);
    assert!(opened.clip("arrangement/track-2/clip-2").is_some());

    opened.keys("cmd-z cmd-z");
    assert_eq!(opened.clip(added), None);
    assert_eq!(opened.clip_file(added), None);
    assert_eq!(opened.undo_label(), None);
    // Below the last track there is nothing to add to.
    let below = opened.at(BAR, 2);
    opened.double_click(below);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_of_the_body_moves_a_clip_in_time_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960 + 100, 0));
    opened.press(from);
    opened.drag_to(to);
    // During the drag the project follows and the file waits.
    // A bar and 100 ticks is a bar: a drag moves by whole snap steps.
    assert_eq!(opened.clip(PART).unwrap().start.0, 2 * BAR);
    assert!(opened.gesture_open());
    assert!(opened.clip_file(PART).unwrap().contains("\"start\": 3840"));

    opened.release(to);
    assert!(!opened.gesture_open());
    assert_eq!(opened.clip(PART).unwrap().notes, part().notes);
    assert!(opened.clip_file(PART).unwrap().contains("\"start\": 7680"));
    assert_eq!(opened.undo_label().as_deref(), Some("Move clip"));
    assert_eq!(opened.selected_clip(), Some(id(PART)));

    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
    assert!(opened.clip_file(PART).unwrap().contains("\"start\": 3840"));
    opened.keys("shift-cmd-z");
    assert_eq!(opened.clip(PART).unwrap().start.0, 2 * BAR);
}

#[gpui::test]
fn a_clip_does_not_move_before_tick_zero(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = opened.at(BAR + 960, 0);
    opened.drag(from, point(px(0.), from.y));
    assert_eq!(opened.clip(PART).unwrap().start.0, 0);
}

#[gpui::test]
fn a_drag_to_another_track_moves_the_file_and_keeps_the_clip_selected(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let moved = "arrangement/track-2/part";
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960, 1));
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.clip(PART), None);
    assert_eq!(opened.selected_clip(), Some(id(moved)));
    // Still the old file: nothing is written during a drag.
    assert!(opened.clip_file(PART).is_some());
    assert_eq!(opened.clip_file(moved), None);

    opened.release(to);
    let expected = clip(2 * BAR, 2 * BAR, part().notes);
    assert_eq!(opened.clip(moved), Some(expected));
    assert_eq!(opened.clip_file(PART), None);
    assert!(opened.clip_file(moved).unwrap().contains("\"start\": 7680"));
    assert_eq!(opened.undo_label().as_deref(), Some("Move clip"));

    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.clip(moved), None);
    assert_eq!(opened.clip_file(moved), None);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_to_another_track_and_back_keeps_the_id_of_the_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = opened.at(BAR + 960, 0);
    let (away, back) = (opened.at(BAR + 960, 1), opened.at(2 * BAR + 960, 0));
    opened.press(from);
    opened.drag_to(away);
    assert!(opened.clip("arrangement/track-2/part").is_some());
    opened.drag_to(back);
    opened.release(back);
    assert_eq!(opened.clip(PART).unwrap().start.0, 2 * BAR);
    assert_eq!(opened.clip("arrangement/track-1/part-2"), None);
    assert_eq!(opened.clip("arrangement/track-2/part"), None);
    assert_eq!(opened.selected_clip(), Some(id(PART)));
    let names: Vec<_> = support::files(opened.folder.path())
        .into_iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .filter(|path| path.contains("part"))
        .collect();
    assert_eq!(names, ["state/arrangement/track-1/part.json"]);
}

#[gpui::test]
fn a_drag_of_the_right_edge_resizes_and_going_back_keeps_the_notes(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let edge = opened.at(3 * BAR, 0) - point(px(2.), px(0.));
    // In to the middle of bar 2: the second note is outside now.
    let inside = opened.at(BAR + 1920, 0) - point(px(2.), px(0.));
    opened.press(edge);
    opened.drag_to(inside);
    assert_eq!(
        opened.clip(PART),
        Some(clip(BAR, 1920, vec![note(960, 480, 60)]))
    );
    // Out again in the same drag: the note is back.
    let outside = opened.at(4 * BAR, 0) - point(px(2.), px(0.));
    opened.drag_to(outside);
    opened.release(outside);
    assert_eq!(opened.clip(PART), Some(clip(BAR, 3 * BAR, part().notes)));
    assert_eq!(opened.undo_label().as_deref(), Some("Resize clip"));
    assert!(
        opened
            .clip_file(PART)
            .unwrap()
            .contains("\"length\": 11520")
    );

    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_of_the_left_edge_keeps_the_notes_in_place_and_stops_at_the_first(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let placed: Vec<_> = part().placed_notes().collect();
    let edge = opened.at(BAR, 0) + point(px(2.), px(0.));
    let earlier = opened.at(BAR / 2, 0) + point(px(2.), px(0.));
    opened.drag(edge, earlier);
    let grown = opened.clip(PART).unwrap();
    assert_eq!((grown.start.0, grown.end().0), (BAR / 2, 3 * BAR));
    assert_eq!(grown.placed_notes().collect::<Vec<_>>(), placed);
    assert_eq!(opened.undo_label().as_deref(), Some("Resize clip"));

    // Far to the right: the edge stops at the first note and no note is lost.
    let edge = opened.at(BAR / 2, 0) + point(px(2.), px(0.));
    let far = opened.at(6 * BAR, 0);
    opened.drag(edge, far);
    let shrunk = opened.clip(PART).unwrap();
    assert_eq!((shrunk.start.0, shrunk.end().0), (BAR + 960, 3 * BAR));
    assert_eq!(shrunk.notes[0].start.0, 0);
    assert_eq!(shrunk.placed_notes().collect::<Vec<_>>(), placed);

    opened.keys("cmd-z cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
}

#[gpui::test]
fn delete_and_backspace_delete_the_selected_clip(cx: &mut TestAppContext) {
    for key in ["delete", "backspace"] {
        let mut opened = open(cx);
        // Nothing is selected yet, so the key does nothing.
        opened.keys(key);
        assert!(opened.clip(PART).is_some());

        let place = opened.at(BAR + 960, 0);
        opened.click(place);
        assert_eq!(opened.undo_label(), None);
        opened.keys(key);
        assert_eq!(opened.clip(PART), None);
        assert_eq!(opened.clip_file(PART), None);
        assert_eq!(opened.selected_clip(), None);
        assert_eq!(opened.undo_label().as_deref(), Some("Delete clip"));
        opened.keys("cmd-z");
        assert_eq!(opened.clip(PART), Some(part()));
        assert!(opened.clip_file(PART).is_some());
    }
}

#[gpui::test]
fn arrow_keys_nudge_the_selected_clip_in_time_and_between_tracks(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.at(BAR + 960, 0);
    opened.click(place);
    opened.keys("right right left");
    assert_eq!(opened.clip(PART).unwrap().start.0, BAR + STEP);
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge clip"));
    assert!(opened.clip_file(PART).unwrap().contains("\"start\": 4080"));

    opened.keys("down");
    let moved = "arrangement/track-2/part";
    assert_eq!(opened.clip(PART), None);
    assert_eq!(opened.clip(moved).unwrap().start.0, BAR + STEP);
    assert_eq!(opened.selected_clip(), Some(id(moved)));
    assert!(opened.clip_file(moved).is_some());
    // The last track is the end, and the keys go on with the clip where it is now.
    opened.keys("down right");
    assert_eq!(opened.clip(moved).unwrap().start.0, BAR + 2 * STEP);
    opened.keys("up");
    assert_eq!(opened.clip(PART).unwrap().start.0, BAR + 2 * STEP);
    opened.keys("up");
    assert_eq!(opened.selected_clip(), Some(id(PART)));

    // Each key was one step: six of them changed something.
    opened.keys("cmd-z cmd-z cmd-z cmd-z cmd-z cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_nudge_stops_at_tick_zero(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = Changes::new();
        changes.create(id(PART), clip(STEP, BAR, vec![]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let place = opened.at(STEP + 100, 0);
    opened.click(place);
    opened.keys("left left left");
    assert_eq!(opened.clip(PART).unwrap().start.0, 0);
    opened.keys("cmd-z");
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn escape_cancels_a_drag_and_restores_the_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = support::files(opened.folder.path());
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(4 * BAR, 1));
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.clip(PART), None);
    opened.keys("escape");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.clip("arrangement/track-2/part"), None);
    assert_eq!(opened.selected_clip(), Some(id(PART)));
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label(), None);
    assert_eq!(opened.redo_label(), None);

    // The button is still down. Moving on and letting go changes nothing.
    opened.drag_to(from);
    opened.release(from);
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
    assert_eq!(support::files(opened.folder.path()), before);

    // The same for a resize.
    let edge = opened.at(3 * BAR, 0) - point(px(2.), px(0.));
    let inside = opened.at(BAR + 1920, 0);
    opened.press(edge);
    opened.drag_to(inside);
    assert_eq!(opened.clip(PART).unwrap().notes.len(), 1);
    opened.keys("escape");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn undo_during_a_drag_is_ignored(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.at(BAR + 960, 0);
    opened.click(place);
    opened.keys("right");
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960, 0));
    opened.press(from);
    opened.drag_to(to);
    opened.keys("cmd-z");
    // The nudge is still there and the drag goes on.
    assert_eq!(opened.clip(PART).unwrap().start.0, 2 * BAR + STEP);
    assert!(opened.gesture_open());
    opened.release(to);
    assert_eq!(opened.undo_label().as_deref(), Some("Move clip"));
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART).unwrap().start.0, BAR + STEP);
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge clip"));
}

#[gpui::test]
fn a_click_without_a_move_is_no_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = support::files(opened.folder.path());
    let place = opened.at(BAR + 960, 0);
    opened.click(place);
    // A move of less than half a snap step is no move either.
    opened.drag(place, place + point(px(2.), px(0.)));
    assert_eq!(opened.undo_label(), None);
    assert!(!opened.gesture_open());
    assert_eq!(support::files(opened.folder.path()), before);
}

#[gpui::test]
fn an_outside_delete_of_the_dragged_clip_ends_the_gesture_cleanly(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960, 0));
    opened.press(from);
    opened.drag_to(to);
    assert!(opened.gesture_open());

    // An agent deletes the file while the button is down.
    let path = opened.path(&format!("state/{PART}.json"));
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    assert_eq!(opened.clip(PART), None);
    assert!(!opened.gesture_open());
    assert_eq!(opened.selected_clip(), None);

    // The mouse goes on and lets go: nothing comes back and nothing fails.
    let further = opened.at(3 * BAR, 1);
    opened.drag_to(further);
    opened.release(further);
    assert_eq!(opened.clip(PART), None);
    assert_eq!(opened.clip("arrangement/track-2/part"), None);
    assert_eq!(opened.notice(), None);
    assert_eq!(opened.clip_file(PART), None);

    // The delete was the last write. Undo gives the clip back as it was before the drag.
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
}

#[gpui::test]
fn an_outside_edit_during_a_drag_applies_and_the_drag_goes_on(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960, 0));
    opened.press(from);
    opened.drag_to(to);

    // An agent rewrites the clip with one more note while the button is down.
    let path = opened.path(&format!("state/{PART}.json"));
    let mut edited = part();
    edited.notes.push(note(1920, 480, 67));
    let record = format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": 3840, "length": 7680, "notes": [{}]}}}}"#,
        edited
            .notes
            .iter()
            .map(|note| format!(
                r#"{{"start": {}, "length": 480, "pitch": {}, "velocity": 100}}"#,
                note.start.0,
                note.pitch.number()
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    std::fs::write(&path, record).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    assert_eq!(opened.clip(PART), Some(edited.clone()));

    // The next move is the later write for the start. The new note stays.
    let further = opened.at(3 * BAR + 960, 0);
    opened.drag_to(further);
    opened.release(further);
    let after = opened.clip(PART).unwrap();
    assert_eq!(after.start.0, 3 * BAR);
    assert_eq!(after.notes, edited.notes);
}

#[gpui::test]
fn the_keys_of_the_arrangement_need_its_focus_and_space_still_plays(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.at(BAR + 960, 0);
    opened.click(place);
    opened.keys("space");
    opened.settle();
    assert!(opened.playhead().playing);
    opened.keys("space");
    opened.settle();

    // Tab moves the focus on to the transport. The clip is still selected, and delete is not
    // a key of the transport.
    opened.keys("tab delete right");
    assert_eq!(opened.clip(PART), Some(part()));
    // Shift-tab comes back, and then it is.
    opened.keys("shift-tab delete");
    assert_eq!(opened.clip(PART), None);
}
