//! The history tab: undo, redo and a click on a point move through one history of the folder,
//! and a change after going back starts a version beside the first.

use std::time::Duration;

use gpui::TestAppContext;
use reverb::ReverbState;
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

fn set_mix(mix: f32) -> impl FnOnce(&mut Project) -> Result<(), ProjectError> {
    move |project| {
        let mut changes = Changes::new();
        let state = ReverbState {
            mix,
            ..ReverbState::default()
        };
        changes.set(&project.resolve::<ReverbState>(&room()).unwrap(), state);
        project.commit("Mix", changes)
    }
}

/// The tab looks at the history twice a second.
fn look(opened: &mut Opened) {
    opened.cx.executor().advance_clock(Duration::from_secs(1));
    opened.cx.run_until_parked();
}

fn rows(opened: &mut Opened) -> usize {
    (0..)
        .take_while(|row| opened.find(&format!("history-row-{row}")).is_some())
        .count()
}

fn room_file(opened: &mut Opened) -> Option<Vec<u8>> {
    std::fs::read(opened.path("state/room.json")).ok()
}

#[gpui::test]
fn undo_redo_and_a_click_move_through_one_history(cx: &mut TestAppContext) {
    let mut opened = support::open_new(cx);
    let icon = opened.control("left-panel-icon");
    opened.click(icon);
    look(&mut opened);
    assert_eq!(rows(&mut opened), 1);

    opened.edit(add_room(0.3));
    look(&mut opened);
    let added = room_file(&mut opened);
    assert!(added.is_some());
    assert_eq!(rows(&mut opened), 2);

    // Undo goes to the point before, and redo back: no point is added.
    opened.keys("cmd-z");
    look(&mut opened);
    assert_eq!(room_file(&mut opened), None);
    opened.keys("cmd-shift-z");
    look(&mut opened);
    assert_eq!(room_file(&mut opened), added);
    assert_eq!(rows(&mut opened), 2);

    // A change after an undo starts a version beside the first.
    opened.keys("cmd-z");
    opened.edit(add_room(0.8));
    look(&mut opened);
    assert_eq!(rows(&mut opened), 3);

    // A click on the first version's latest point brings its file back byte for byte.
    let first = opened.control("history-row-1");
    opened.click(first);
    look(&mut opened);
    assert_eq!(room_file(&mut opened), added);
    assert_eq!(opened.undo_label().as_deref(), Some("Add reverb"));

    // More changes there fold between its first and its latest point.
    for mix in [0.4, 0.5, 0.6] {
        opened.edit(set_mix(mix));
    }
    look(&mut opened);
    assert_eq!(rows(&mut opened), 4);
    assert!(opened.find("history-fold-2").is_some());
}
