//! The history panel: the points the folder was in, going back to one with a click, and the
//! version a change there starts.

use std::time::Duration;

use gpui::TestAppContext;
use reverb::ReverbState;
use runtime::window::KeepHistory;
use sound_core::{Changes, InstanceId, Project, ProjectError};

use crate::support::{self, Opened};

fn room() -> InstanceId {
    InstanceId::new("room").unwrap()
}

fn add_room(mix: f32) -> impl FnOnce(&mut Project) -> Result<(), ProjectError> {
    move |project| {
        let mut changes = Changes::new();
        let state = ReverbState {
            mix,
            ..ReverbState::default()
        };
        changes.create(room(), state);
        project.commit("Add reverb", changes)
    }
}

/// The history saves once the folder has been quiet a moment.
fn quiet(opened: &mut Opened) {
    opened.cx.executor().advance_clock(Duration::from_secs(1));
    opened.cx.run_until_parked();
}

fn rows(opened: &mut Opened) -> usize {
    (0..)
        .take_while(|row| opened.find(&format!("history-row-{row}")).is_some())
        .count()
}

/// The record of the reverb on disk, as JSON: a point brings back what it held, in a layout of
/// its own.
fn room_file(opened: &mut Opened) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(opened.path("state/room.json")).ok()?;
    Some(serde_json::from_str(&text).unwrap())
}

#[gpui::test]
fn going_back_to_a_point_and_changing_it_starts_a_version(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(KeepHistory));
    let mut opened = support::open_new(cx);
    assert_eq!(rows(&mut opened), 1);

    opened.edit(add_room(0.3));
    quiet(&mut opened);
    assert_eq!(rows(&mut opened), 2);
    let added = room_file(&mut opened);
    assert!(added.is_some());

    // Undo goes back to the first point instead of adding one.
    opened.keys("cmd-z");
    quiet(&mut opened);
    assert_eq!(rows(&mut opened), 2);
    assert_eq!(room_file(&mut opened), None);

    // A click on the newest row goes there: the reverb is back, as one undo step.
    let newest = opened.control("history-row-0");
    opened.click(newest);
    quiet(&mut opened);
    assert_eq!(room_file(&mut opened), added);
    assert_eq!(opened.undo_label().as_deref(), Some("Go to Add reverb"));
    assert!(opened.project(|project| project.tool_of(&room()).is_some()));

    // Back at the start, a change is a new version beside the first.
    let start = opened.control("history-row-1");
    opened.click(start);
    quiet(&mut opened);
    assert_eq!(room_file(&mut opened), None);
    opened.edit(add_room(0.8));
    quiet(&mut opened);
    assert_eq!(rows(&mut opened), 3);

    // Going back to the first version folds the new one away: it never got a moment.
    let first_version = opened.control("history-row-1");
    opened.click(first_version);
    quiet(&mut opened);
    assert_eq!(room_file(&mut opened), added);
    assert_eq!(rows(&mut opened), 2);
}
