//! The note editor, driven with a simulated mouse and keys: it opens and closes, notes are
//! drawn, moved, resized, deleted and nudged, they stay inside their clip, and a note that is
//! touched sounds for a moment and then stops.

use arrangement::view::layout::HEADER_WIDTH;
use arrangement::view::roll::EDITOR_HEIGHT;
use gpui::{TestAppContext, point, px};
use runtime::OFFLINE;
use sound_core::Changes;
use sound_notes::Clip;

use crate::support::{self, BAR, Opened, STEP, clip, id, note, peak};

const PART: &str = "arrangement/track-1/part";
const OTHER: &str = "arrangement/track-2/other";

fn part() -> Clip {
    clip(
        BAR,
        2 * BAR,
        vec![note(960, 480, 60), note(BAR + 960, 480, 64)],
    )
}

fn other() -> Clip {
    clip(4 * BAR, BAR, vec![note(0, 480, 55)])
}

/// Two tracks with a clip each, and the editor open on `part` by a double click.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        changes.create(id(OTHER), other());
        project.commit("Add clips", changes).unwrap();
        project.clear_history();
    });
    let place = opened.at(BAR + 100, 0);
    opened.double_click(place);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    opened
}

/// A place just inside the end of a note, where a drag resizes.
fn end_of(opened: &mut Opened<'_>, end_tick: u64, pitch: u8) -> gpui::Point<gpui::Pixels> {
    opened.in_editor(end_tick, pitch) - point(px(3.), px(0.))
}

#[gpui::test]
fn enter_opens_the_editor_for_the_selected_clip_and_escape_closes_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Escape closes and the keys are those of the arrangement again.
    opened.keys("escape");
    assert_eq!(opened.editor_clip(), None);
    opened.keys("right");
    assert_eq!(opened.clip(PART).unwrap().start.0, BAR + STEP);

    opened.keys("enter");
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    // Now the arrows belong to the editor, and no note is selected.
    opened.keys("right");
    assert_eq!(opened.clip(PART).unwrap().start.0, BAR + STEP);

    // The close control, by the mouse and by tab and enter.
    let height = opened.cx.update(|window, _| window.viewport_size().height);
    let top = f32::from(height) - EDITOR_HEIGHT;
    opened.click(point(px(HEADER_WIDTH - 20.), px(top + 16.)));
    assert_eq!(opened.editor_clip(), None);
    opened.keys("enter");
    assert!(opened.editor().is_some());
    opened.keys("tab");
    opened.press_enter();
    assert_eq!(opened.editor_clip(), None);
    // Opening and closing is no edit.
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge clip"));
}

#[gpui::test]
fn a_drag_on_empty_space_draws_a_note_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // From inside the cell at beat 3 of bar 2, to a little past beat 4, on another row.
    let from = opened.in_editor(BAR + 1920 + 50, 67);
    let to = opened.in_editor(BAR + 2880 + 20, 70);
    opened.press(from);
    // A press alone is a note of one snap step already.
    assert_eq!(opened.clip(PART).unwrap().notes[2], note(1920, STEP, 67));
    assert!(opened.gesture_open());
    opened.drag_to(to);
    assert_eq!(opened.clip(PART).unwrap().notes[2], note(1920, 960, 67));
    assert!(!opened.clip_file(PART).unwrap().contains("\"pitch\": 67"));
    opened.release(to);

    // Written once, with the notes in order and the new one selected.
    let drawn = [
        note(960, 480, 60),
        note(1920, 960, 67),
        note(BAR + 960, 480, 64),
    ];
    assert_eq!(opened.clip(PART).unwrap().notes, drawn);
    assert_eq!(opened.selected_note(), Some(1));
    assert_eq!(opened.undo_label().as_deref(), Some("Draw note"));
    let file = opened.clip_file(PART).unwrap();
    assert!(file.contains(r#"{"start": 1920, "length": 960, "pitch": 67, "velocity": 100}"#));

    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
    assert!(!opened.clip_file(PART).unwrap().contains("\"pitch\": 67"));
}

#[gpui::test]
fn a_drag_of_a_note_moves_it_in_time_and_pitch(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = opened.in_editor(BAR + 960 + 100, 60);
    let to = opened.in_editor(BAR + 960 + 100 + 480, 63);
    opened.press(from);
    assert_eq!(opened.selected_note(), Some(0));
    assert!(!opened.gesture_open());
    opened.drag_to(to);
    assert!(opened.gesture_open());
    opened.release(to);
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(1440, 480, 63));
    assert_eq!(opened.undo_label().as_deref(), Some("Move note"));
    assert!(opened.clip_file(PART).unwrap().contains("\"pitch\": 63"));

    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_of_the_end_of_a_note_resizes_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = end_of(&mut opened, BAR + 1440, 60);
    let to = end_of(&mut opened, BAR + 2400, 60);
    opened.drag(from, to);
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(960, 1440, 60));
    assert_eq!(opened.undo_label().as_deref(), Some("Resize note"));

    // Back past its start: one snap step is the least.
    let from = end_of(&mut opened, BAR + 2400, 60);
    let to = opened.in_editor(BAR, 60);
    opened.drag(from, to);
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(960, STEP, 60));
    opened.keys("cmd-z cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
}

#[gpui::test]
fn the_keys_delete_and_nudge_the_selected_note(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.in_editor(BAR + 960 + 100, 60);
    opened.click(place);
    assert_eq!(opened.undo_label(), None);
    opened.keys("right");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(1200, 480, 60));
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge note"));
    opened.keys("left left up");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(720, 480, 61));
    opened.keys("shift-up down");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(720, 480, 72));
    opened.keys("shift-down shift-down");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(720, 480, 48));
    assert!(opened.clip_file(PART).unwrap().contains("\"pitch\": 48"));

    for key in ["delete", "backspace"] {
        let before = opened.clip(PART).unwrap().notes.len();
        let first = opened.clip(PART).unwrap().notes[0];
        let place = opened.in_editor(BAR + first.start.0 + 100, first.pitch.number());
        opened.click(place);
        opened.keys(key);
        assert_eq!(opened.clip(PART).unwrap().notes.len(), before - 1);
        assert_eq!(opened.selected_note(), None);
        assert_eq!(opened.undo_label().as_deref(), Some("Delete note"));
        // With no note selected the key does nothing, and the clip stays.
        opened.keys(key);
        assert_eq!(opened.clip(PART).unwrap().notes.len(), before - 1);
    }
    assert!(opened.clip(PART).unwrap().notes.is_empty());
    assert!(opened.clip_file(PART).unwrap().contains("\"notes\": []"));
}

#[gpui::test]
fn a_nudge_that_passes_another_note_keeps_the_selection_on_its_note(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.in_editor(BAR + 960 + 100, 60);
    opened.click(place);
    // 17 steps to the right puts it after the second note, which starts at 4800.
    for _ in 0..17 {
        opened.keys("right");
    }
    let notes = opened.clip(PART).unwrap().notes;
    assert_eq!(
        notes,
        [note(BAR + 960, 480, 64), note(960 + 17 * STEP, 480, 60)]
    );
    assert_eq!(opened.selected_note(), Some(1));
    opened.keys("up");
    assert_eq!(opened.clip(PART).unwrap().notes[1].pitch.number(), 61);
}

#[gpui::test]
fn notes_never_leave_their_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Far past the end of the clip, and far above the highest row.
    let from = opened.in_editor(2 * BAR + 960 + 100, 64);
    let far = opened.in_editor(9 * BAR, 64);
    opened.press(from);
    opened.drag_to(far);
    opened.drag_to(point(far.x, px(-500.)));
    opened.release(far);
    let last = *opened.clip(PART).unwrap().notes.last().unwrap();
    assert_eq!((last.start.0, last.end().0), (2 * BAR - 480, 2 * BAR));
    assert_eq!(last.pitch.number(), 127);
    opened.keys("right up");
    assert_eq!(*opened.clip(PART).unwrap().notes.last().unwrap(), last);
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));

    // Far before the start of the clip.
    let from = opened.in_editor(BAR + 960 + 100, 60);
    let before = opened.in_editor(0, 60);
    opened.drag(from, before);
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(0, 480, 60));
    opened.keys("left");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(0, 480, 60));
    opened.keys("cmd-z");

    // A resize ends with the clip.
    let from = end_of(&mut opened, 2 * BAR + 1440, 64);
    opened.drag(from, far);
    assert_eq!(
        opened.clip(PART).unwrap().notes[1],
        note(BAR + 960, BAR - 960, 64)
    );
    opened.keys("cmd-z");

    // A drawn note ends with the clip, and outside the clip nothing is drawn.
    let from = opened.in_editor(3 * BAR - 100, 67);
    opened.drag(from, far);
    assert_eq!(
        opened.clip(PART).unwrap().notes[2],
        note(2 * BAR - STEP, STEP, 67)
    );
    opened.keys("cmd-z");
    assert_eq!(opened.undo_label(), None);
    let outside = opened.in_editor(3 * BAR + 100, 67);
    opened.drag(outside, far);
    let before_clip = opened.in_editor(BAR - 100, 67);
    opened.drag(before_clip, from);
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.undo_label(), None);
    assert_eq!(opened.notice(), None);
}

#[gpui::test]
fn escape_cancels_a_note_drag_and_restores_the_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let files = support::files(opened.folder.path());
    let (from, to) = (
        opened.in_editor(BAR + 2000, 67),
        opened.in_editor(BAR + 3000, 67),
    );
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.clip(PART).unwrap().notes.len(), 3);
    opened.keys("escape");
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.selected_note(), None);
    // The first escape ended the drag. The editor is still open.
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    opened.drag_to(from);
    opened.release(from);

    let (from, to) = (
        opened.in_editor(BAR + 1000, 60),
        opened.in_editor(BAR + 2000, 65),
    );
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.clip(PART).unwrap().notes[0].pitch.number(), 65);
    opened.keys("escape");
    opened.release(to);
    assert_eq!(opened.clip(PART), Some(part()));
    assert_eq!(opened.selected_note(), Some(0));
    assert!(!opened.gesture_open());
    assert_eq!((opened.undo_label(), opened.redo_label()), (None, None));
    assert_eq!(support::files(opened.folder.path()), files);
}

#[gpui::test]
fn undo_during_a_note_drag_is_ignored(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let place = opened.in_editor(BAR + 1000, 60);
    opened.click(place);
    opened.keys("up");
    let (from, to) = (
        opened.in_editor(BAR + 1000, 61),
        opened.in_editor(BAR + 2000, 61),
    );
    opened.press(from);
    opened.drag_to(to);
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(1920, 480, 61));
    opened.release(to);
    assert_eq!(opened.undo_label().as_deref(), Some("Move note"));
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART).unwrap().notes[0], note(960, 480, 61));
}

#[gpui::test]
fn the_editor_follows_the_selection_and_closes_when_its_clip_is_deleted(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Another clip is selected: the editor shows it.
    let place = opened.at(4 * BAR + 1920, 1);
    opened.click(place);
    assert_eq!(opened.editor_clip(), Some(id(OTHER)));
    // A click on empty space selects nothing, and the editor stays.
    let empty = opened.at(8 * BAR, 0);
    opened.click(empty);
    assert_eq!(opened.editor_clip(), Some(id(OTHER)));

    // The clip goes to the other track: a new id, the same editor.
    let (from, to) = (opened.at(4 * BAR + 1920, 1), opened.at(5 * BAR + 1920, 0));
    opened.drag(from, to);
    let moved = "arrangement/track-1/other";
    assert_eq!(opened.editor_clip(), Some(id(moved)));
    assert_eq!(opened.clip(moved).unwrap().start.0, 5 * BAR);

    // Deleted from inside.
    opened.keys("delete");
    assert_eq!(opened.clip(moved), None);
    assert_eq!(opened.editor_clip(), None);

    // Deleted from outside, as an agent does.
    let place = opened.at(BAR + 100, 0);
    opened.double_click(place);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    let path = opened.path(&format!("state/{PART}.json"));
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    assert_eq!(opened.editor_clip(), None);
    assert_eq!(opened.notice(), None);
    // The keys of the window still work.
    opened.keys("cmd-z");
    assert_eq!(opened.clip(PART), Some(part()));
}

#[gpui::test]
fn an_outside_edit_during_a_note_drag_is_followed_or_ends_the_drag(cx: &mut TestAppContext) {
    let record = |notes: &[(u64, u8)]| {
        let lines: Vec<String> = notes
            .iter()
            .map(|(start, pitch)| {
                format!(r#"{{"start": {start}, "length": 480, "pitch": {pitch}, "velocity": 100}}"#)
            })
            .collect();
        format!(
            r#"{{"tool": "arrangement.clip", "state": {{"start": 3840, "length": 7680, "notes": [{}]}}}}"#,
            lines.join(", ")
        )
    };
    let mut opened = open(cx);
    let path = opened.path(&format!("state/{PART}.json"));
    let (from, to) = (
        opened.in_editor(BAR + 1000, 60),
        opened.in_editor(BAR + 1000, 62),
    );
    opened.press(from);
    opened.drag_to(to);

    // An agent puts a note before the dragged one, which is at pitch 62 by now.
    std::fs::write(&path, record(&[(0, 50), (960, 62), (4800, 64)])).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    let further = opened.in_editor(BAR + 1000, 65);
    opened.drag_to(further);
    let notes = opened.clip(PART).unwrap().notes;
    assert_eq!(
        notes,
        [note(0, 480, 50), note(960, 480, 65), note(4800, 480, 64)]
    );

    // Then the agent takes the dragged note away: the drag ends, and nothing else moves.
    std::fs::write(&path, record(&[(0, 50), (4800, 64)])).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    let last = opened.in_editor(BAR + 1000, 70);
    opened.drag_to(last);
    opened.release(last);
    assert!(!opened.gesture_open());
    assert_eq!(
        opened.clip(PART).unwrap().notes,
        [note(0, 480, 50), note(4800, 480, 64)]
    );
    assert_eq!(opened.notice(), None);
}

/// A second of the offline engine.
const SECOND: usize = OFFLINE.sample_rate as usize;

#[gpui::test]
fn a_note_that_is_touched_sounds_for_a_moment_while_the_project_is_stopped(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);
    assert!(!opened.playhead().playing);

    // A click on a note.
    let place = opened.in_editor(BAR + 1000, 60);
    opened.click(place);
    assert!(peak(&opened.render(SECOND / 4)) > 0.01);
    opened.render(SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);
    assert_eq!(opened.undo_label(), None);

    // A drawn note, and a key of the strip.
    let place = opened.in_editor(BAR + 2000, 67);
    opened.click(place);
    assert!(peak(&opened.render(SECOND / 4)) > 0.01);
    opened.render(SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);
    let key = point(px(HEADER_WIDTH - 10.), opened.in_editor(BAR, 65).y);
    opened.click(key);
    assert!(peak(&opened.render(SECOND / 4)) > 0.01);
    opened.render(SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);

    // A nudge to a new pitch sounds, a nudge in time does not.
    opened.click(place);
    opened.render(2 * SECOND);
    opened.keys("right");
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);
    opened.keys("up");
    assert!(peak(&opened.render(SECOND / 4)) > 0.01);
}

#[gpui::test]
fn nothing_is_left_sounding_after_a_fast_drag_a_delete_or_a_closed_editor(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // Over an octave of rows, a few frames of audio between the moves.
    let from = opened.in_editor(BAR + 1000, 60);
    opened.press(from);
    for pitch in 61..=72 {
        let to = opened.in_editor(BAR + 1000, pitch);
        opened.drag_to(to);
        assert!(peak(&opened.render(256)) > 0.0);
    }
    let to = opened.in_editor(BAR + 1000, 72);
    opened.release(to);
    opened.render(2 * SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);

    // Touched and deleted at once.
    opened.click(to);
    opened.keys("delete");
    opened.render(2 * SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);

    // Touched, and the editor closes at once.
    let place = opened.in_editor(2 * BAR + 1000, 64);
    opened.click(place);
    opened.keys("escape");
    assert_eq!(opened.editor_clip(), None);
    assert!(peak(&opened.render(SECOND / 8)) > 0.01);
    opened.render(2 * SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);

    // Touched, and the clip is deleted under the editor.
    opened.keys("enter");
    opened.click(place);
    let timeline_place = opened.at(BAR + 100, 0);
    opened.click(timeline_place);
    opened.keys("delete");
    assert_eq!(opened.clip(PART), None);
    opened.render(2 * SECOND);
    assert_eq!(peak(&opened.render(SECOND / 4)), 0.0);
}
