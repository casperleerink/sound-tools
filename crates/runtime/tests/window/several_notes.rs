//! Several notes in the note editor, step 9b of the third milestone: shift-click, cmd-click and
//! a rectangle, a drag, the arrows and delete of several notes, and copy, cut, paste and
//! duplicate. Every action is driven with the simulated mouse and keys and checked for its
//! records, for exactly one undo step, and for undo giving the files back byte for byte. Also
//! the two things step 9a left: undo of a delete selects the clips again, and a note added with
//! snap off is a sixteenth.

use gpui::{Modifiers, Pixels, Point, TestAppContext, point, px};
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::Clip;

use crate::support::{self, BAR, Opened, STEP, clip, id, mark, note, one_undo_step, write_outside};

const PART: &str = "arrangement/track-1/part";
const OTHER: &str = "arrangement/track-2/other";

/// Bars 2 and 3: two notes in the first bar and one in the second.
fn part() -> Clip {
    clip(
        BAR,
        2 * BAR,
        vec![
            note(960, 480, 60),
            note(1920, 480, 64),
            note(BAR + 960, 480, 67),
        ],
    )
}

fn other() -> Clip {
    clip(4 * BAR, BAR, vec![note(0, 480, 55)])
}

/// Two tracks with a clip each, and the editor open on `part`. No undo history.
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

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn cmd() -> Modifiers {
    Modifiers {
        platform: true,
        ..Modifiers::default()
    }
}

/// The middle of a note of `part`, by its start in the clip and its pitch.
fn on(opened: &mut Opened<'_>, start: u64, pitch: u8) -> Point<Pixels> {
    opened.in_editor(BAR + start + 100, pitch)
}

/// The place of a project tick in the ruler of the note editor.
fn editor_ruler(opened: &mut Opened<'_>, tick: u64) -> Point<Pixels> {
    let x = opened.in_editor(tick, 60).x;
    let height = opened.cx.update(|window, _| window.viewport_size().height);
    let top = f32::from(height) - arrangement::view::roll::EDITOR_HEIGHT;
    point(x, px(top + 16.))
}

fn notes(opened: &mut Opened<'_>, clip: &str) -> Vec<sound_notes::Note> {
    opened.clip(clip).unwrap().notes
}

#[gpui::test]
fn shift_cmd_and_a_rectangle_select_notes_as_for_clips(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (first, second) = (on(&mut opened, 960, 60), on(&mut opened, 1920, 64));
    opened.click(first);
    opened.click_with(second, shift());
    assert_eq!(opened.selected_notes(), [0, 1]);
    // The last one added comes first.
    assert_eq!(opened.selected_note(), Some(1));
    opened.click_with(second, cmd());
    assert_eq!(opened.selected_notes(), [0]);
    opened.click_with(second, cmd());
    assert_eq!(opened.selected_notes(), [0, 1]);
    // A plain click on one of them, without a move, selects it alone.
    opened.click(first);
    assert_eq!(opened.selected_notes(), [0]);

    // A rectangle on empty space from above the first note to below the second selects both,
    // and with shift it adds the third.
    let (from, to) = (on(&mut opened, 0, 66), on(&mut opened, 1900, 59));
    opened.drag(from, to);
    assert_eq!(opened.selected_notes(), [0, 1]);
    let (from, to) = (
        on(&mut opened, BAR + 700, 69),
        on(&mut opened, BAR + 1000, 66),
    );
    opened.drag_with(from, to, shift());
    assert_eq!(opened.selected_notes(), [0, 1, 2]);
    // Escape during a rectangle puts back what was selected.
    opened.click(first);
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(opened.selected_notes(), [2]);
    opened.keys("escape");
    opened.release(to);
    assert_eq!(opened.selected_notes(), [0]);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    // A click on empty space selects nothing, and cmd-a every note.
    opened.click(from);
    assert_eq!(opened.selected_notes(), Vec::<usize>::new());
    opened.keys("cmd-a");
    assert_eq!(opened.selected_notes(), [0, 1, 2]);
    // Selecting is no edit.
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn a_drag_of_several_notes_moves_them_all_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (first, second) = (on(&mut opened, 960, 60), on(&mut opened, 1920, 64));
    opened.click(first);
    opened.click_with(second, shift());
    // A beat later and two semitones up.
    let to = opened.in_editor(BAR + 960 + 100 + 960, 62);
    opened.drag(first, to);
    assert_eq!(
        notes(&mut opened, PART),
        [
            note(1920, 480, 62),
            note(2880, 480, 66),
            note(BAR + 960, 480, 67)
        ]
    );
    assert_eq!(opened.selected_notes(), [0, 1]);
    one_undo_step(&mut opened, "Move notes", &before);

    // Far left: the first stops at the clip start and the second keeps its distance.
    let from = on(&mut opened, 1920, 62);
    let far = opened.in_editor(0, 62);
    opened.drag(from, far);
    let moved = notes(&mut opened, PART);
    assert_eq!(moved[..2], [note(0, 480, 62), note(960, 480, 66)]);
}

#[gpui::test]
fn delete_and_the_arrows_act_on_every_selected_note_and_undo_selects_them_again(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let (first, third) = (on(&mut opened, 960, 60), on(&mut opened, BAR + 960, 67));
    opened.click(first);
    opened.click_with(third, cmd());
    let before = mark(&mut opened);
    opened.keys("right");
    assert_eq!(
        notes(&mut opened, PART),
        [
            note(960 + STEP, 480, 60),
            note(1920, 480, 64),
            note(BAR + 960 + STEP, 480, 67)
        ]
    );
    one_undo_step(&mut opened, "Nudge notes", &before);
    // Up an octave together; the selection goes with them.
    opened.keys("shift-up");
    assert_eq!(notes(&mut opened, PART)[0].pitch.number(), 72);
    assert_eq!(notes(&mut opened, PART)[2].pitch.number(), 79);
    assert_eq!(opened.selected_notes(), [0, 2]);

    let before = mark(&mut opened);
    opened.keys("delete");
    assert_eq!(notes(&mut opened, PART), [note(1920, 480, 64)]);
    assert_eq!(opened.selected_notes(), Vec::<usize>::new());
    one_undo_step(&mut opened, "Delete notes", &before);
    // Undo brings them back selected, so a second delete takes them again.
    opened.keys("cmd-z");
    assert_eq!(notes(&mut opened, PART).len(), 3);
    assert_eq!(opened.selected_notes(), [0, 2]);
    opened.keys("backspace");
    assert_eq!(notes(&mut opened, PART), [note(1920, 480, 64)]);
}

#[gpui::test]
fn a_paste_goes_to_the_playhead_in_the_clip_or_to_the_selected_notes(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (first, second) = (on(&mut opened, 960, 60), on(&mut opened, 1920, 64));
    opened.click(first);
    opened.click_with(second, shift());
    opened.keys("cmd-c");
    assert_eq!(opened.undo_label(), None);

    // The playhead in the second bar of the clip.
    let ruler = editor_ruler(&mut opened, 2 * BAR);
    opened.click(ruler);
    opened.settle();
    assert_eq!(opened.playhead().tick, Ticks(2 * BAR));
    let before = mark(&mut opened);
    opened.keys("cmd-v");
    let pasted = [note(BAR, 480, 60), note(BAR + 960, 480, 64)];
    let now = notes(&mut opened, PART);
    assert!(pasted.iter().all(|note| now.contains(note)), "{now:?}");
    assert_eq!(now.len(), 5);
    // The pasted notes are selected.
    let selected: Vec<_> = opened
        .selected_notes()
        .into_iter()
        .map(|index| now[index])
        .collect();
    assert_eq!(selected, pasted);
    one_undo_step(&mut opened, "Paste notes", &before);

    // The playhead outside the clip: the paste goes to the start of the selected notes.
    opened.keys("cmd-z");
    let outside = opened.ruler(0);
    opened.click(outside);
    opened.settle();
    opened.keys("enter");
    let third = on(&mut opened, BAR + 960, 67);
    opened.click(third);
    opened.keys("cmd-v");
    let now = notes(&mut opened, PART);
    assert!(now.contains(&note(BAR + 960, 480, 60)), "{now:?}");
    assert!(now.contains(&note(BAR + 1920, 480, 64)), "{now:?}");
    // A note that would start past the end of the clip is left out.
    opened.keys("cmd-z");
    let end = opened.in_editor(3 * BAR - 480 + 100, 67);
    opened.double_click(end);
    opened.keys("cmd-v");
    let now = notes(&mut opened, PART);
    assert!(now.contains(&note(2 * BAR - 480, 480, 60)), "{now:?}");
    assert_eq!(now.len(), 5);
    assert_eq!(opened.undo_label().as_deref(), Some("Paste note"));
}

#[gpui::test]
fn cut_and_duplicate_notes_and_a_paste_into_another_clip(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let (first, second) = (on(&mut opened, 960, 60), on(&mut opened, 1920, 64));
    opened.click(first);
    opened.click_with(second, shift());
    let before = mark(&mut opened);
    opened.keys("cmd-x");
    assert_eq!(notes(&mut opened, PART), [note(BAR + 960, 480, 67)]);
    one_undo_step(&mut opened, "Cut notes", &before);

    // A duplicate goes right after the selected notes, and keeps the clipboard.
    let third = on(&mut opened, BAR + 960, 67);
    opened.click(third);
    let before = mark(&mut opened);
    opened.keys("cmd-d");
    assert_eq!(
        notes(&mut opened, PART),
        [note(BAR + 960, 480, 67), note(BAR + 1440, 480, 67)]
    );
    assert_eq!(opened.selected_notes(), [1]);
    one_undo_step(&mut opened, "Duplicate note", &before);

    // The cut notes go into the other clip: the playhead is at the start, outside it, and
    // nothing is selected there, so they go to its start.
    let other_clip = opened.at(4 * BAR + 1920, 1);
    opened.click(other_clip);
    assert_eq!(opened.editor_clip(), Some(id(OTHER)));
    opened.keys("enter");
    let before = mark(&mut opened);
    opened.keys("cmd-v");
    assert_eq!(
        notes(&mut opened, OTHER),
        [note(0, 480, 55), note(0, 480, 60), note(960, 480, 64)]
    );
    one_undo_step(&mut opened, "Paste notes", &before);

    // Notes in the clipboard are not clips: cmd-v in the timeline does nothing.
    let label = opened.undo_label();
    let timeline_place = opened.at(BAR + 100, 0);
    opened.click(timeline_place);
    opened.keys("cmd-v");
    assert_eq!(opened.undo_label(), label);
}

#[gpui::test]
fn notes_written_from_outside_are_selected_and_edited_like_any_other(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    write_outside(
        &mut opened,
        &format!("state/{PART}.json"),
        r#"{"tool": "arrangement.clip", "state": {"start": 3840, "length": 7680, "notes": [
            {"start": 0, "length": 480, "pitch": 61, "velocity": 90},
            {"start": 480, "length": 480, "pitch": 63, "velocity": 90}]}}"#,
    );
    let (from, to) = (opened.in_editor(BAR, 65), opened.in_editor(BAR + 1000, 60));
    opened.press(from - point(px(4.), px(0.)));
    opened.drag_to(to);
    opened.release(to);
    assert_eq!(opened.selected_notes(), [0, 1]);
    opened.keys("cmd-d");
    let now = notes(&mut opened, PART);
    assert_eq!(now.len(), 4);
    assert_eq!(now[2].start, Ticks(960));
}

#[gpui::test]
fn undo_of_a_delete_or_a_cut_of_clips_selects_them_again(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.keys("escape");
    let (on_part, on_other) = (opened.at(BAR + 960, 0), opened.at(4 * BAR + 960, 1));
    opened.click(on_part);
    opened.click_with(on_other, shift());
    let both: Vec<InstanceId> = {
        let mut both = vec![id(PART), id(OTHER)];
        both.sort();
        both
    };
    for key in ["delete", "cmd-x"] {
        opened.keys(key);
        assert_eq!(opened.selected_clips(), Vec::<InstanceId>::new());
        opened.keys("cmd-z");
        assert_eq!(opened.selected_clips(), both, "{key}");
        // The one that came first comes first again.
        assert_eq!(opened.selected_clip(), Some(id(OTHER)), "{key}");
    }
    // A click elsewhere and an edit of another clip do not bring the selection back.
    let empty = opened.at(8 * BAR, 0);
    opened.click(empty);
    write_outside(
        &mut opened,
        &format!("state/{OTHER}.json"),
        r#"{"tool": "arrangement.clip", "state": {"start": 15360, "length": 3840, "notes": []}}"#,
    );
    assert_eq!(opened.selected_clips(), Vec::<InstanceId>::new());
}

#[gpui::test]
fn with_snap_off_a_double_click_adds_a_sixteenth(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let menu = opened.control("snap");
    opened.click(menu);
    let row = opened.control("menu-Off");
    opened.click(row);
    let place = opened.in_editor(BAR + 2 * BAR - 900, 70);
    opened.double_click(place);
    let added = notes(&mut opened, PART);
    let added = added.iter().find(|note| note.pitch.number() == 70).unwrap();
    assert_eq!(added.length.ticks(), Ticks(STEP));
    assert_eq!(opened.undo_label().as_deref(), Some("Draw note"));
}
