//! Returns and sends: the section of the returns folded and opened, a return added and deleted,
//! and a send knob dragged.

use std::collections::BTreeMap;

use arrangement::{Colour, TrackKind, TrackState};
use gpui::{TestAppContext, point, px};
use sound_core::{Changes, Project};

use crate::support::{self, BAR, Opened, clip, files, id, mark, note, one_undo_step};

const TRACK: &str = "arrangement/track-1";
const PART: &str = "arrangement/track-1/part";

/// `Track 1` with the clip `part` in bars 2 and 3, and a return, `Space`, that the track sends
/// to at -12 dB.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        add_space(project);
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        let mut state = project.state(&track).unwrap().clone();
        state.set_send("space", -12.);
        let mut changes = Changes::new();
        changes.set(&track, state);
        changes.create(id(PART), clip(BAR, 2 * BAR, vec![note(0, 480, 60)]));
        project.commit("Send", changes).unwrap();
        project.clear_history();
    })
}

/// Opens the section and selects `Space`, whose row then has the focus.
fn select_space(opened: &mut Opened<'_>) {
    let header = opened.control("returns-header");
    opened.click(header);
    let row = opened.control("return-space");
    opened.click(row);
    assert_eq!(opened.panel_track(), Some(id("arrangement/space")));
}

fn clips(opened: &mut Opened<'_>) -> usize {
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        arrangement::clips(project, track.id()).len()
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
    select_space(&mut opened);
    let before = mark(&mut opened);
    opened.keys("backspace");
    opened.cx.simulate_prompt_answer("Delete");
    opened.cx.run_until_parked();
    assert_eq!(returns(&mut opened), Vec::<String>::new());
    assert_eq!(sends(&mut opened), BTreeMap::new());
    assert_eq!(opened.track_panel().map(|_| ()), None);
    one_undo_step(&mut opened, "Delete return", &before);
}

/// The focus was on the row of the return. It goes to the timeline, so the keys of the
/// arrangement work on.
#[gpui::test]
fn deleting_the_focused_return_gives_the_focus_to_the_timeline(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    select_space(&mut opened);
    opened.keys("backspace");
    opened.cx.simulate_prompt_answer("Delete");
    opened.cx.run_until_parked();
    assert_eq!(returns(&mut opened), Vec::<String>::new());
    opened.keys("secondary-a");
    assert_eq!(opened.selected_clips(), [id(PART)]);
}

/// Enter gives the focus back to the row: its keys work on, and cmd-v there pastes nothing on
/// the first track, where the timeline puts a paste when no track of its own is selected.
#[gpui::test]
fn after_a_rename_the_return_keeps_the_focus_and_its_keys(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(2 * BAR, 0);
    opened.click(part);
    opened.keys("secondary-c");
    select_space(&mut opened);
    opened.keys("enter");
    opened.cx.simulate_input("Hall");
    opened.keys("enter");
    assert_eq!(returns(&mut opened), ["Hall"]);
    opened.keys("secondary-v");
    assert_eq!(clips(&mut opened), 1);
    assert_eq!(opened.undo_label().as_deref(), Some("Rename track"));
    opened.keys("backspace");
    opened.cx.simulate_prompt_answer("Delete");
    opened.cx.run_until_parked();
    assert_eq!(returns(&mut opened), Vec::<String>::new());
}

/// An agent deletes a return while its send knob is dragged. The knob goes, and the gesture
/// ends with it: nothing is left open to hold undo back.
#[gpui::test]
fn deleting_a_return_during_a_drag_of_its_send_ends_the_gesture(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    let knob = opened.control("knob-send-space");
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-40.)));
    assert!(opened.gesture_open());

    let folder = opened.path("state/arrangement/space");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    assert!(!opened.gesture_open());
    assert_eq!(opened.find("knob-send-space"), None);
    opened.drag_to(knob + point(px(0.), px(-80.)));
    opened.release(knob + point(px(0.), px(-80.)));
    assert!(!opened.gesture_open());
    assert_eq!(opened.notice(), None);

    // Undo takes the drag back, then brings the return back.
    opened.keys("secondary-z");
    assert_eq!(
        sends(&mut opened),
        BTreeMap::from([("space".to_string(), -12.)])
    );
    opened.keys("secondary-z");
    assert_eq!(returns(&mut opened), ["Space"]);
}

/// A click on empty space in the timeline keeps the return selected, and cmd-v there is
/// refused: a return plays no clips, and the first track is not what is selected.
#[gpui::test]
fn a_paste_with_a_return_selected_lands_on_no_track(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let part = opened.at(2 * BAR, 0);
    opened.click(part);
    opened.keys("secondary-c");
    select_space(&mut opened);
    let empty = opened.at(8 * BAR, 0);
    opened.click(empty);
    assert_eq!(opened.selected_track(), Some(id("arrangement/space")));
    let before = mark(&mut opened);
    opened.keys("secondary-v");
    assert_eq!(clips(&mut opened), 1);
    assert_eq!(opened.undo_label(), before.undo_label);
    let notice = opened.notice().unwrap_or_default();
    assert!(notice.contains("a return track plays no clips"), "{notice}");
}

/// Twenty returns open, with the panel of a track: the section scrolls inside a few rows, so
/// the master row and the panel stay in the window, and tab scrolls the row it reaches into
/// view.
#[gpui::test]
fn many_open_returns_scroll_and_keep_the_master_row_in_the_window(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        for _ in 0..20 {
            runtime::add_track_of_kind(project, &arrangement, TrackKind::Return).unwrap();
        }
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let header = opened.control("returns-header");
    opened.click(header);
    let height = opened.cx.update(|window, _| window.viewport_size().height);
    let panel = opened.bounds("track-panel").unwrap();
    assert!(panel.bottom() <= height, "{panel:?} in {height:?}");
    let master = opened.bounds("master-row").unwrap();
    let last = opened.bounds("return-return-20").unwrap();
    assert!(last.top() >= master.top(), "{last:?} is not hidden");

    let first = opened.control("return-return-1");
    opened.click(first);
    // A view hears the focus move only in the window in front.
    opened.cx.update(|window, _| window.activate_window());
    for _ in 1..20 {
        opened.keys("tab");
    }
    let last = opened.bounds("return-return-20").unwrap();
    assert!(last.bottom() <= master.top(), "{last:?} is not in view");
}

/// An agent deletes a return while the volume of a track is dragged. The drag goes on as one
/// gesture, so one undo takes all of it back.
#[gpui::test]
fn a_volume_drag_stays_one_undo_step_when_a_return_is_deleted(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    let knob = opened.control("volume-gain_db");
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(20.)));

    let folder = opened.path("state/arrangement/space");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    assert!(opened.gesture_open());
    opened.drag_to(knob + point(px(0.), px(50.)));
    opened.release(knob + point(px(0.), px(50.)));
    assert_eq!(opened.undo_label().as_deref(), Some("Change volume"));

    opened.keys("secondary-z");
    let gain = opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        project.state(&track).unwrap().gain_db
    });
    assert_eq!(gain, 0.);
    assert_ne!(opened.undo_label().as_deref(), Some("Change volume"));
}

/// An agent deletes a return while its name is edited. The focus goes to the timeline, so the
/// keys of the arrangement work on.
#[gpui::test]
fn deleting_a_return_during_its_rename_gives_the_focus_to_the_timeline(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    select_space(&mut opened);
    opened.keys("enter");
    let folder = opened.path("state/arrangement/space");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    assert_eq!(returns(&mut opened), Vec::<String>::new());
    opened.keys("secondary-a");
    assert_eq!(opened.selected_clips(), [id(PART)]);
}
