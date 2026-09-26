//! The velocity lane at the bottom of the note editor, step 9b of the third milestone: a drag
//! of a bar sets the velocity of its note, the selected notes change together, a drag across
//! the lane draws several, and alt with the arrows steps them. Each is one undo step that
//! undo gives back byte for byte, and a velocity written from outside shows in the lane at
//! once.

use gpui::{Modifiers, TestAppContext};
use sound_core::Changes;
use sound_notes::{Clip, Note, Velocity};

use crate::support::{self, BAR, Opened, clip, id, mark, note, one_undo_step, write_outside};

const PART: &str = "arrangement/track-1/part";

/// Bars 2 and 3, three notes at velocity 100: two in the first bar, one in the second.
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

/// The editor open on `part`. No undo history.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let place = opened.at(BAR + 100, 0);
    opened.double_click(place);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    opened
}

fn velocities(opened: &mut Opened<'_>) -> Vec<u8> {
    let notes = opened.clip(PART).unwrap().notes;
    notes.iter().map(|note| note.velocity.value()).collect()
}

fn with_velocity(note: Note, velocity: u8) -> Note {
    Note {
        velocity: Velocity::new(velocity).unwrap(),
        ..note
    }
}

#[gpui::test]
fn a_drag_of_a_bar_sets_the_velocity_of_its_note(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let (from, to) = (
        opened.in_lane(BAR + 960, 100),
        opened.in_lane(BAR + 960, 60),
    );
    opened.press(from);
    // A press is no edit, and it selects the note of the bar.
    assert!(!opened.gesture_open());
    assert_eq!(opened.selected_notes(), [0]);
    opened.drag_to(to);
    // Heard at once, written at the end.
    assert_eq!(velocities(&mut opened), [60, 100, 100]);
    assert!(!opened.clip_file(PART).unwrap().contains("\"velocity\": 60"));
    opened.release(to);
    assert!(
        opened
            .clip_file(PART)
            .unwrap()
            .contains(r#"{"start": 960, "length": 480, "pitch": 60, "velocity": 60}"#)
    );
    one_undo_step(&mut opened, "Change velocity", &before);

    // Far above the lane is 127, far below is 1.
    let from = opened.in_lane(BAR + 960, 60);
    opened.press(from);
    opened.drag_to(from - gpui::point(gpui::px(0.), gpui::px(500.)));
    assert_eq!(velocities(&mut opened)[0], 127);
    opened.drag_to(from + gpui::point(gpui::px(0.), gpui::px(500.)));
    assert_eq!(velocities(&mut opened)[0], 1);
    // Escape puts it back.
    opened.keys("escape");
    opened.release(from);
    assert_eq!(velocities(&mut opened), [60, 100, 100]);
}

#[gpui::test]
fn the_selected_notes_change_together_by_the_same_distance(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // The second note is quieter.
    let (from, to) = (
        opened.in_lane(BAR + 1920, 100),
        opened.in_lane(BAR + 1920, 50),
    );
    opened.drag(from, to);
    assert_eq!(velocities(&mut opened), [100, 50, 100]);
    let (first, second) = (
        opened.in_editor(BAR + 960 + 100, 60),
        opened.in_editor(BAR + 1920 + 100, 64),
    );
    opened.click(first);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    opened.click_with(second, shift);
    let before = mark(&mut opened);
    let (from, to) = (
        opened.in_lane(BAR + 960, 100),
        opened.in_lane(BAR + 960, 80),
    );
    opened.drag(from, to);
    assert_eq!(velocities(&mut opened), [80, 30, 100]);
    assert_eq!(opened.selected_notes(), [0, 1]);
    one_undo_step(&mut opened, "Change velocities", &before);

    // Alt with the arrows steps every selected velocity by ten, as one undo step.
    let before = mark(&mut opened);
    opened.keys("alt-up");
    assert_eq!(velocities(&mut opened), [90, 40, 100]);
    one_undo_step(&mut opened, "Change velocities", &before);
    opened.keys("alt-down alt-down");
    assert_eq!(velocities(&mut opened), [70, 20, 100]);
}

#[gpui::test]
fn a_drag_across_the_lane_draws_every_bar_it_passes_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    // From before the first bar to after the last, at the height of velocity 20.
    let (from, to) = (
        opened.in_lane(BAR + 500, 20),
        opened.in_lane(3 * BAR - 200, 20),
    );
    opened.press(from);
    let middle = opened.in_lane(BAR + 1500, 20);
    opened.drag_to(middle);
    assert_eq!(velocities(&mut opened), [20, 100, 100]);
    opened.drag_to(to);
    opened.release(to);
    assert_eq!(velocities(&mut opened), [20, 20, 20]);
    // Drawing selects nothing.
    assert_eq!(opened.selected_notes(), Vec::<usize>::new());
    one_undo_step(&mut opened, "Draw velocities", &before);

    // A slope: the bars take the height of the line where they are.
    let (from, to) = (
        opened.in_lane(BAR + 500, 40),
        opened.in_lane(3 * BAR - 200, 120),
    );
    opened.drag(from, to);
    let drawn = velocities(&mut opened);
    assert!(
        40 < drawn[0] && drawn[0] < drawn[1] && drawn[1] < drawn[2] && drawn[2] < 120,
        "{drawn:?}"
    );
}

#[gpui::test]
fn a_velocity_written_from_outside_shows_in_the_lane_at_once(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let record = serde_json::json!({"tool": "arrangement.clip", "state": clip(BAR, 2 * BAR, vec![
        with_velocity(note(960, 480, 60), 40), note(1920, 480, 64), note(BAR + 960, 480, 67),
    ])});
    write_outside(
        &mut opened,
        &format!("state/{PART}.json"),
        &record.to_string(),
    );
    // A drag from the top of the bar as the file has it lands exactly where it is let go.
    let (from, to) = (opened.in_lane(BAR + 960, 40), opened.in_lane(BAR + 960, 90));
    opened.drag(from, to);
    assert_eq!(velocities(&mut opened), [90, 100, 100]);
    // And undo gives back what the file had.
    opened.keys("cmd-z");
    assert_eq!(velocities(&mut opened), [40, 100, 100]);
}
