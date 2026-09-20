//! The tempo in the transport and the click, driven with the real mouse and keys, and the
//! arrangement view following the playhead.

use arrangement::view::layout::{HEADER_WIDTH, Viewport};
use gpui::{Point, TestAppContext, point, px};
use sound_core::Ticks;

use crate::support::{self, BAR, Opened};

/// The default project: 120 bpm, 4/4, one track, no clips.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |_| {})
}

/// The tempo map as `project.json` holds it.
fn tempo_file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path("project.json")).unwrap()
}

fn tempo_in_file(opened: &mut Opened<'_>) -> String {
    let text = tempo_file(opened);
    let start = text.find("\"tempo_changes\"").unwrap();
    let end = text[start..].find(']').unwrap() + start;
    text[start..end].to_string()
}

/// The middle of the tempo number in the transport.
fn tempo_control(opened: &mut Opened<'_>) -> Point<gpui::Pixels> {
    opened.control("tempo")
}

#[gpui::test]
fn the_transport_shows_the_tempo_at_the_playhead(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    assert_eq!(opened.shown_tempo(), 120.0);

    // A second tempo change at bar 3, written from outside as an agent would.
    let map = r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}, {"tick": 7680, "bpm": 60.0}]}"#;
    write_tempo_map(&mut opened, map);
    assert_eq!(opened.shown_tempo(), 120.0, "the playhead is still at bar 1");

    opened
        .session
        .update(opened.cx, |session, _| session.engine().seek(Ticks(2 * BAR)));
    opened.settle();
    assert_eq!(opened.shown_tempo(), 60.0, "the playhead is past bar 3");
}

/// Writes a whole `project.json` with this tempo map and applies it as the watcher would.
fn write_tempo_map(opened: &mut Opened<'_>, tempo_map: &str) {
    let text = tempo_file(opened);
    let start = text.find("\"tempo_map\"").unwrap();
    let end = text[start..].find("\"connections\"").unwrap() + start;
    let replaced = format!("{}\"tempo_map\": {tempo_map},\n  {}", &text[..start], &text[end..]);
    let path = opened.path("project.json");
    std::fs::write(&path, replaced).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path.clone()]));
    opened.settle();
}

#[gpui::test]
fn an_outside_tempo_edit_shows_in_the_transport_at_once(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let map = r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 93.5}]}"#;
    write_tempo_map(&mut opened, map);
    assert_eq!(opened.shown_tempo(), 93.5);
    assert_eq!(opened.undo_label().as_deref(), Some("File change"));
}

#[gpui::test]
fn a_tempo_drag_is_one_undo_step_and_the_file_follows(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = tempo_control(&mut opened);
    // Up is faster: half a bpm per pixel, landing on whole numbers.
    opened.drag(from, from - point(px(0.), px(40.)));
    assert_eq!(opened.shown_tempo(), 140.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change tempo"));
    assert!(tempo_in_file(&mut opened).contains("140"));
    assert!(!opened.gesture_open());

    // One undo takes the whole drag back, in the window and in the file.
    opened.keys("cmd-z");
    opened.settle();
    assert_eq!(opened.shown_tempo(), 120.0);
    assert_eq!(opened.undo_label(), None);
    assert!(tempo_in_file(&mut opened).contains("120"));
    opened.keys("shift-cmd-z");
    assert_eq!(opened.shown_tempo(), 140.0);
}

#[gpui::test]
fn a_press_without_a_move_is_no_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let at = tempo_control(&mut opened);
    opened.click(at);
    assert_eq!(opened.shown_tempo(), 120.0);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn escape_during_a_tempo_drag_puts_it_back(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let from = tempo_control(&mut opened);
    opened.press(from);
    opened.drag_to(from - point(px(0.), px(20.)));
    assert_eq!(opened.shown_tempo(), 130.0);
    opened.keys("escape");
    assert_eq!(opened.shown_tempo(), 120.0);
    assert_eq!(opened.undo_label(), None);
    opened.release(from);
}

#[gpui::test]
fn the_arrows_change_the_focused_tempo(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let at = tempo_control(&mut opened);
    opened.click(at);
    opened.keys("up up");
    assert_eq!(opened.shown_tempo(), 122.0);
    opened.keys("shift-down");
    assert_eq!(opened.shown_tempo(), 121.9);
    // Each key is one step of its own.
    assert_eq!(opened.undo_label().as_deref(), Some("Change tempo"));
    opened.keys("cmd-z");
    assert_eq!(opened.shown_tempo(), 122.0);
}

#[gpui::test]
fn a_tempo_edit_changes_the_tempo_change_at_the_playhead(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let map = r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}, {"tick": 7680, "bpm": 60.0}]}"#;
    write_tempo_map(&mut opened, map);
    opened
        .session
        .update(opened.cx, |session, _| session.engine().seek(Ticks(2 * BAR)));
    opened.settle();

    let at = tempo_control(&mut opened);
    opened.click(at);
    opened.keys("up");
    assert_eq!(opened.shown_tempo(), 61.0);
    let file = tempo_in_file(&mut opened);
    assert!(file.contains("120"), "the first change is untouched: {file}");
    assert!(file.contains("61"), "the second change moved: {file}");
}

#[gpui::test]
fn toggling_the_click_adds_no_undo_step_and_writes_no_record(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = support::files(&opened.path(""));
    assert!(!opened.click_is_on());

    let at = opened.control("click");
    opened.click(at);
    assert!(opened.click_is_on());
    assert_eq!(opened.undo_label(), None, "the click is not an edit");
    assert_eq!(support::files(&opened.path("")), before);

    // cmd-z with the click on must not toggle it, and must not fail either.
    opened.keys("cmd-z");
    assert!(opened.click_is_on());
    assert_eq!(opened.notice(), None);

    opened.click(at);
    assert!(!opened.click_is_on());
    assert_eq!(opened.undo_label(), None);
    assert_eq!(support::files(&opened.path("")), before);
}

#[gpui::test]
fn the_view_pages_forward_while_playing_and_a_jump_brings_the_playhead_back(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    assert_eq!(scroll(&mut opened), 0.0);

    // Forty seconds of playback at 120 bpm is twenty bars, which is wider than the window.
    play(&mut opened);
    opened.render(40 * 48_000);
    opened.settle();
    assert!(scroll(&mut opened) > 0.0, "the view did not follow the playhead");
    assert!(shows_playhead(&mut opened));

    // A stop is a jump: the view comes back to the start with the playhead.
    opened
        .session
        .update(opened.cx, |session, _| session.engine().stop());
    opened.settle();
    assert_eq!(opened.playhead().tick, Ticks(0));
    assert_eq!(scroll(&mut opened), 0.0);
}

#[gpui::test]
fn scrolling_the_playhead_off_screen_stops_the_following_until_the_next_jump(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    play(&mut opened);
    opened.render(40 * 48_000);
    opened.settle();
    let paged = scroll(&mut opened);
    assert!(paged > 0.0);

    // The composer scrolls back to the start. The playhead is now off screen ahead of them.
    set_scroll(&mut opened, 0.0);
    assert!(!shows_playhead(&mut opened));
    opened.render(10 * 48_000);
    opened.settle();
    assert_eq!(
        scroll(&mut opened),
        0.0,
        "the view was pulled back while the composer had scrolled away"
    );

    // The next jump brings the playhead into view again.
    opened.session.update(opened.cx, |session, _| {
        session.engine().seek(Ticks(40 * crate::support::BAR))
    });
    opened.settle();
    assert!(scroll(&mut opened) > 0.0);
    assert!(shows_playhead(&mut opened));
}

fn play(opened: &mut Opened<'_>) {
    opened
        .session
        .update(opened.cx, |session, _| session.engine().play());
}

fn scroll(opened: &mut Opened<'_>) -> f64 {
    let timeline = opened.timeline.clone();
    opened.cx.read(|cx| timeline.read(cx).viewport().scroll_x)
}

fn set_scroll(opened: &mut Opened<'_>, scroll_x: f64) {
    let timeline = opened.timeline.clone();
    let viewport = Viewport {
        scroll_x,
        ..opened.cx.read(|cx| timeline.read(cx).viewport())
    };
    opened
        .cx
        .update(|_, cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
}

/// Whether the timeline shows the playhead, with the width the test window really has.
fn shows_playhead(opened: &mut Opened<'_>) -> bool {
    let (timeline, tick) = (opened.timeline.clone(), opened.playhead().tick);
    let width = f32::from(opened.cx.update(|window, _| window.viewport_size().width)) - HEADER_WIDTH;
    opened
        .cx
        .read(|cx| timeline.read(cx).viewport().shows(tick, width))
}
