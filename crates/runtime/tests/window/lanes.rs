//! The expression lanes of the note editor: the select shows one, a drag draws its line, alt
//! and a drag erase points, and a double click clears it. Each is one undo step that undo gives
//! back byte for byte, and escape puts a drag back.

use arrangement::view::lanes::{Lane, Shown, lane_y};
use gpui::{Modifiers, TestAppContext};
use sound_core::{Changes, Ticks};
use sound_notes::{Bend, Clip, Point};

use crate::support::{self, BAR, Opened, clip, id, mark, note, one_undo_step};

const PART: &str = "arrangement/track-1/part";

/// Bars 2 and 3, one note, and a bend written by an agent: up at the start of bar 3.
fn part() -> Clip {
    let mut part = clip(BAR, 2 * BAR, vec![note(960, 480, 60)]);
    part.bend = vec![point(0, 0), point(BAR, 4096), point(BAR + 960, 0)];
    part
}

fn point(tick: u64, value: i16) -> Point<Bend> {
    Point {
        tick: Ticks(tick),
        value: Bend::new(value).unwrap(),
    }
}

/// The editor open on `part`, showing the bend. No undo history.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = Changes::new();
        changes.create(id(PART), part());
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let place = opened.at(BAR + 100, 0);
    opened.double_click(place);
    let select = opened.control("editor-lane");
    opened.click(select);
    let bend = opened.control("menu-bend");
    opened.click(bend);
    assert_eq!(shown(&mut opened), Shown::Lane(Lane::Bend));
    opened
}

fn shown(opened: &mut Opened<'_>) -> Shown {
    let editor = opened.editor().unwrap();
    opened.cx.read(|cx| editor.read(cx).shown())
}

fn bend(opened: &mut Opened<'_>) -> Vec<(u64, i16)> {
    let clip = opened.clip(PART).unwrap();
    clip.bend
        .iter()
        .map(|point| (point.tick.0, point.value.value()))
        .collect()
}

fn height(value: i16) -> f32 {
    lane_y(Bend::new(value).unwrap())
}

/// A drag across the lane draws on the sixteenths it passes, at the height of the pointer,
/// over the points that were there, and a level line is its two ends.
#[gpui::test]
fn a_drag_draws_the_line_of_the_pointer_on_the_grid(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let y = height(-4096);
    let (from, to) = (
        opened.in_lane_at(2 * BAR - 480, y),
        opened.in_lane_at(2 * BAR + 480, y),
    );
    opened.press(from);
    // A press is no edit.
    assert!(!opened.gesture_open());
    opened.drag_to(to);
    let drawn = bend(&mut opened);
    let low = drawn[1].1;
    assert!((low + 4096).abs() < 200, "{drawn:?}");
    // Heard at once, written at the end.
    assert!(!opened.clip_file(PART).unwrap().contains(&low.to_string()));
    opened.release(to);
    assert_eq!(
        drawn,
        [(0, 0), (BAR - 480, low), (BAR + 480, low), (BAR + 960, 0)]
    );
    assert!(opened.clip_file(PART).unwrap().contains("\"bend\""));
    one_undo_step(&mut opened, "Draw bend", &before);

    // Escape during a drag puts the lane back.
    let (from, to) = (
        opened.in_lane_at(BAR + 240, height(8000)),
        opened.in_lane_at(2 * BAR, height(8000)),
    );
    opened.press(from);
    opened.drag_to(to);
    assert_ne!(bend(&mut opened), drawn);
    opened.keys("escape");
    opened.release(to);
    assert_eq!(bend(&mut opened), drawn);
}

/// With cmd held the drag does not snap: it draws every few pixels, off the grid.
#[gpui::test]
fn a_drag_with_cmd_draws_off_the_grid(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let cmd = Modifiers {
        platform: true,
        ..Modifiers::default()
    };
    let (from, to) = (
        opened.in_lane_at(BAR + 1000, height(-8192)),
        opened.in_lane_at(BAR + 1700, height(8191)),
    );
    opened.drag_with(from, to, cmd);
    let drawn = bend(&mut opened);
    assert!(drawn.iter().any(|(tick, _)| tick % 120 != 0), "{drawn:?}");
    assert_eq!(opened.undo_label().as_deref(), Some("Draw bend"));
}

/// Alt and a drag erase the points between the press and the pointer.
#[gpui::test]
fn a_drag_with_alt_erases_the_points_it_covers(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    let (from, to) = (
        opened.in_lane_at(2 * BAR - 240, height(0)),
        opened.in_lane_at(2 * BAR + 240, height(0)),
    );
    opened.drag_with(from, to, alt);
    assert_eq!(bend(&mut opened), [(0, 0), (BAR + 960, 0)]);
    one_undo_step(&mut opened, "Erase bend", &before);
}

/// A double click clears the lane, and one on an empty lane is no undo step.
#[gpui::test]
fn a_double_click_clears_the_lane(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let place = opened.in_lane_at(BAR + 500, height(0));
    opened.double_click(place);
    assert!(bend(&mut opened).is_empty());
    assert!(!opened.clip_file(PART).unwrap().contains("\"bend\""));
    one_undo_step(&mut opened, "Clear bend", &before);
    opened.double_click(place);
    assert_eq!(opened.undo_label().as_deref(), Some("Clear bend"));
    // The other lanes of the clip are not touched, and the velocities come back with the
    // select.
    let editor = opened.editor().unwrap();
    opened.cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.show(Shown::Lane(Lane::Pressure), cx)
        })
    });
    let place = opened.in_lane_at(BAR + 500, height(0));
    opened.double_click(place);
    assert_eq!(opened.undo_label().as_deref(), Some("Clear bend"));
    let select = opened.control("editor-lane");
    opened.click(select);
    let velocity = opened.control("menu-velocity");
    opened.click(velocity);
    assert_eq!(shown(&mut opened), Shown::Velocity);
}
