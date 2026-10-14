//! Clips in editors of the project, see `shared/editors.rs`: the rows of the editor show, a
//! drag and the arrows move by rows, a drum hit is one step, and the editor and the views of a
//! clip survive every edit. A clip that names an editor the project lacks opens as a piano roll.

use std::rc::Rc;

use arrangement::view::rows::Rows;
use gpui::{Modifiers, TestAppContext, point, px};
use sound_core::Changes;
use sound_notes::{Clip, EditorName, Editors, Note, Pitch};

use crate::clip_editors::{c_major, drums};
use crate::support::{self, BAR, Opened, STEP, clip, id, note};

const PART: &str = "arrangement/track-1/part";

fn pitch(number: u8) -> Pitch {
    Pitch::new(number).unwrap()
}

fn part(editor: &str, notes: Vec<Note>) -> Clip {
    let mut part = clip(BAR, 2 * BAR, notes);
    part.editor = Some(EditorName::new(editor).unwrap());
    part
}

/// The project has the drum grid and the scale editor, and the editor is open on `part`.
fn open(cx: &mut TestAppContext, part: Clip) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let editors = project.provided::<Editors>().unwrap();
        assert!(editors.set_project(vec![drums(), c_major()]).is_empty());
        let mut changes = Changes::new();
        changes.create(id(PART), part);
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let place = opened.at(BAR + 100, 0);
    opened.double_click(place);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    opened
}

fn rows(opened: &mut Opened<'_>) -> Rc<Rows> {
    let editor = opened.editor().unwrap();
    opened.cx.read(|cx| editor.read(cx).rows())
}

fn pitches(opened: &mut Opened<'_>) -> Vec<u8> {
    let notes = opened.clip(PART).unwrap().notes;
    notes.iter().map(|note| note.pitch.number()).collect()
}

#[gpui::test]
fn the_drum_grid_shows_its_named_rows_and_draws_hits_of_one_step(cx: &mut TestAppContext) {
    let mut opened = open(cx, part("drums", vec![]));
    let rows = rows(&mut opened);
    assert_eq!((rows.count(), rows.height()), (16, 20.0));
    let snare = rows.look(rows.index_of(pitch(38)).unwrap()).unwrap();
    assert_eq!(snare.label.as_deref(), Some("Snare"));

    // The kick and the snare are at the bottom, under the toms and the cymbals.
    let in_rows = opened.in_editor(BAR, 51);
    opened.scroll(in_rows, 0., -1000.);
    // A double click on the Snare row, and a drag on to the right: a hit of one step.
    let from = opened.in_editor(BAR + 960 + 50, 38);
    let to = opened.in_editor(BAR + 1920 + 50, 38);
    opened.double_press(from);
    opened.drag_to(to);
    opened.release(to);
    assert_eq!(opened.clip(PART).unwrap().notes, [note(960, STEP, 38)]);
    assert_eq!(opened.undo_label().as_deref(), Some("Draw note"));

    // A hit has no end to drag: a drag from its right edge moves it.
    let edge = point(px(rows.height() - 3.), px(0.));
    let (from, to) = (
        opened.in_editor(BAR + 960, 38),
        opened.in_editor(BAR + 1920, 38),
    );
    opened.drag(from + edge, to + edge);
    assert_eq!(opened.clip(PART).unwrap().notes, [note(1920, STEP, 38)]);
    assert_eq!(opened.undo_label().as_deref(), Some("Move note"));
}

#[gpui::test]
fn in_a_scale_a_drag_moves_a_note_by_rows(cx: &mut TestAppContext) {
    let mut opened = open(cx, part("scale", vec![note(0, 480, 64)]));
    // C major from C3 to C6.
    assert_eq!(rows(&mut opened).count(), 22);
    // Up two rows from E4: F4, then G4.
    let from = opened.in_editor(BAR + 100, 64);
    let to = from + point(px(0.), px(-2. * rows(&mut opened).height()));
    opened.drag(from, to);
    assert_eq!(pitches(&mut opened), [67]);
}

#[gpui::test]
fn the_arrows_step_by_rows_shift_goes_an_octave_and_both_stop_at_the_ends(cx: &mut TestAppContext) {
    let mut opened = open(cx, part("scale", vec![note(0, 480, 64)]));
    let on_note = opened.in_editor(BAR + 100, 64);
    opened.click(on_note);
    let steps = [
        ("up", 65),
        ("shift-up", 77),
        // Four rows up to C6, the top row.
        ("shift-up", 84),
        ("up", 84),
        ("shift-down", 72),
        ("down", 71),
    ];
    for (keys, expected) in steps {
        opened.keys(keys);
        assert_eq!(pitches(&mut opened), [expected], "after {keys}");
    }
    opened.keys(&["down"; 30].join(" "));
    assert_eq!(pitches(&mut opened), [48]);
    opened.keys("shift-down");
    assert_eq!(pitches(&mut opened), [48]);
}

#[gpui::test]
fn a_note_off_the_scale_has_a_row_of_its_own_until_it_moves(cx: &mut TestAppContext) {
    let mut opened = open(cx, part("scale", vec![note(0, 480, 66)]));
    let shown = rows(&mut opened);
    let extra = shown.look(shown.index_of(pitch(66)).unwrap()).unwrap();
    assert!(extra.extra);
    assert_eq!(extra.label.as_deref(), Some("F#4"));

    // One row up is G4. The row of F# stays while the drag goes on.
    let from = opened.in_editor(BAR + 100, 66);
    let to = opened.in_editor(BAR + 100, 67);
    opened.press(from);
    opened.drag_to(to);
    assert_eq!(pitches(&mut opened), [67]);
    assert!(rows(&mut opened).index_of(pitch(66)).is_some());
    opened.release(to);
    assert_eq!(rows(&mut opened).index_of(pitch(66)), None);
}

#[gpui::test]
fn the_editor_and_the_views_survive_every_edit(cx: &mut TestAppContext) {
    let accented = |start, pitch| Note {
        view: serde_json::from_str(r#"{"accent": true}"#).unwrap(),
        ..note(start, 480, pitch)
    };
    let mut minor = part("scale", vec![accented(0, 60), accented(960, 63)]);
    minor.view = serde_json::from_str(r#"{"scale": "minor"}"#).unwrap();
    let mut opened = open(cx, minor.clone());
    let kept = |opened: &mut Opened<'_>, clip: &str, what: &str| {
        let clip = opened.clip(clip).unwrap();
        assert_eq!(
            (&clip.editor, &clip.view),
            (&minor.editor, &minor.view),
            "{what}"
        );
        let views = clip.notes.iter().map(|note| &note.view);
        assert!(
            views.clone().all(|view| *view == minor.notes[0].view),
            "{what}"
        );
        clip.notes.len()
    };

    // A drag up a row and a beat on.
    let from = opened.in_editor(BAR + 100, 60);
    let to = opened.in_editor(BAR + 960 + 100, 62);
    opened.drag(from, to);
    assert_eq!(pitches(&mut opened), [62, 63]);
    kept(&mut opened, PART, "move");
    // A copy with alt.
    let (from, to) = (to, opened.in_editor(2 * BAR + 100, 62));
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    opened.drag_with(from, to, alt);
    assert_eq!(kept(&mut opened, PART, "alt copy"), 3);
    opened.keys("secondary-c secondary-v");
    assert_eq!(kept(&mut opened, PART, "copy and paste"), 4);
    opened.keys("secondary-d");
    assert_eq!(kept(&mut opened, PART, "duplicate"), 5);
    opened.keys("right");
    assert_eq!(opened.undo_label().as_deref(), Some("Nudge note"));
    kept(&mut opened, PART, "nudge");
    let first = opened.clip(PART).unwrap().notes[0].start.0;
    let (from, to) = (
        opened.in_lane(BAR + first, 100),
        opened.in_lane(BAR + first, 40),
    );
    opened.drag(from, to);
    assert_eq!(opened.undo_label().as_deref(), Some("Change velocity"));
    kept(&mut opened, PART, "velocity");

    // The clip on the timeline: moved a bar on, and duplicated.
    let (from, to) = (opened.at(BAR + 960, 0), opened.at(2 * BAR + 960, 0));
    opened.drag(from, to);
    assert_eq!(opened.undo_label().as_deref(), Some("Move clip"));
    kept(&mut opened, PART, "clip move");
    opened.keys("secondary-d");
    assert_eq!(opened.undo_label().as_deref(), Some("Duplicate clip"));
    kept(&mut opened, "arrangement/track-1/part-2", "clip duplicate");

    opened.keys("secondary-z secondary-z secondary-z");
    kept(&mut opened, PART, "undo");
    opened.keys("shift-secondary-z shift-secondary-z");
    kept(&mut opened, PART, "redo");
}

#[gpui::test]
fn a_clip_whose_editor_is_missing_opens_as_a_piano_roll_and_keeps_it(cx: &mut TestAppContext) {
    let mut missing = part("drumz", vec![note(0, 480, 60)]);
    missing.view = serde_json::from_str(r#"{"kit": "808"}"#).unwrap();
    let mut opened = open(cx, missing.clone());
    assert_eq!(rows(&mut opened).count(), 128);
    let problems = opened.project(|project| project.problems());
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .starts_with(r#"part.json names the clip editor "drumz""#),
        "{problems:?}"
    );

    let on_note = opened.in_editor(BAR + 100, 60);
    opened.click(on_note);
    opened.keys("up");
    let clip = opened.clip(PART).unwrap();
    assert_eq!(clip.notes, [note(0, 480, 61)]);
    assert_eq!((clip.editor, clip.view), (missing.editor, missing.view));
}
