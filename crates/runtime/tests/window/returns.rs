//! Returns and sends: the section of the returns folded and opened, a return added and deleted,
//! and a send knob dragged.

use std::collections::BTreeMap;

use arrangement::{Colour, TrackKind, TrackState};
use gpui::{TestAppContext, point, px};
use sound_core::{Changes, Project};

use crate::support::{self, Opened, files, id, mark, one_undo_step};

const TRACK: &str = "arrangement/track-1";

/// `Track 1` and a return, `Space`, that the track sends to at -12 dB.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        add_space(project);
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        let mut state = project.state(&track).unwrap().clone();
        state.set_send("space", -12.);
        let mut changes = Changes::new();
        changes.set(&track, state);
        project.commit("Send", changes).unwrap();
        project.clear_history();
    })
}

fn add_space(project: &mut Project) {
    let mut space = TrackState::new("Space", Colour::Teal, 0);
    space.kind = TrackKind::Return;
    let mut changes = Changes::new();
    changes.create(id("arrangement/space"), space);
    project.commit("Add return", changes).unwrap();
}

fn returns(opened: &mut Opened<'_>) -> Vec<String> {
    opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let returns = arrangement::returns(project, arrangement.id());
        returns
            .iter()
            .map(|(_, state)| state.name.clone())
            .collect()
    })
}

fn sends(opened: &mut Opened<'_>) -> BTreeMap<String, f32> {
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        project.state(&track).unwrap().sends.clone()
    })
}

fn expanded(opened: &mut Opened<'_>) -> bool {
    let arrangement = opened.arrangement.clone();
    opened
        .cx
        .read(|cx| arrangement.read(cx).returns().read(cx).is_expanded())
}

#[gpui::test]
fn folding_the_returns_writes_nothing_and_is_no_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    assert!(!expanded(&mut opened));
    assert_eq!(opened.find("return-space"), None);
    let header = opened.control("returns-header");
    opened.click(header);
    assert!(expanded(&mut opened));
    assert!(opened.find("return-space").is_some());
    // The open section grows up from the master row, and its header with it.
    let header = opened.control("returns-header");
    opened.click(header);
    assert!(!expanded(&mut opened));
    assert_eq!(files(opened.folder.path()), before.files);
    assert_eq!(opened.undo_label(), before.undo_label);
}

#[gpui::test]
fn the_plus_adds_a_return_and_opens_the_section(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let plus = opened.control("add-return");
    opened.click(plus);
    assert_eq!(returns(&mut opened), ["Space", "Return 2"]);
    assert!(expanded(&mut opened));
    one_undo_step(&mut opened, "Add return", &before);
}

#[gpui::test]
fn the_add_track_menu_adds_the_first_return(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |_| {});
    assert_eq!(opened.find("returns-header"), None);
    let chevron = opened.control("add-track-menu");
    opened.click(chevron);
    let item = opened.control("menu-return-track");
    opened.click(item);
    assert_eq!(returns(&mut opened), ["Return 1"]);
    let kinds = opened.project(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        let tracks = arrangement::tracks(project, arrangement.id());
        tracks
            .iter()
            .map(|(_, state)| state.kind)
            .collect::<Vec<_>>()
    });
    assert_eq!(kinds, [TrackKind::Instrument]);
    assert!(opened.find("return-return-1").is_some());
}

#[gpui::test]
fn a_send_knob_drag_is_one_undo_step_and_a_double_click_takes_the_send_out(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    let before = mark(&mut opened);
    let knob = opened.control("knob-send-space");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    let sent = sends(&mut opened)["space"];
    assert!(sent > -12., "the send went to {sent} dB");
    one_undo_step(&mut opened, "Change send", &before);

    opened.double_click(knob);
    assert_eq!(sends(&mut opened), BTreeMap::new());
    let file = std::fs::read_to_string(opened.path("state/arrangement/track-1/instance.json"));
    assert!(!file.unwrap().contains("sends"));
}

#[gpui::test]
fn deleting_a_return_takes_the_sends_into_it_in_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.control("returns-header");
    opened.click(header);
    let row = opened.control("return-space");
    opened.click(row);
    assert_eq!(opened.panel_track(), Some(id("arrangement/space")));
    let before = mark(&mut opened);
    opened.keys("backspace");
    opened.cx.simulate_prompt_answer("Delete");
    opened.cx.run_until_parked();
    assert_eq!(returns(&mut opened), Vec::<String>::new());
    assert_eq!(sends(&mut opened), BTreeMap::new());
    assert_eq!(opened.track_panel().map(|_| ()), None);
    one_undo_step(&mut opened, "Delete return", &before);
}
