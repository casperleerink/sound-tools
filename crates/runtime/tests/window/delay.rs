//! The card of the built-in delay in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

use delay::view::DelayView;
use delay::{DelayState, Division, Feel};
use gpui::{TestAppContext, point, px};

use crate::support::{self, Opened, id};

const DELAY: &str = "arrangement/track-1/delay";
const DELAY_FILE: &str = "state/arrangement/track-1/delay.json";

/// One track with no instrument, so the only card with knobs is the delay, and its panel
/// open. The delay is added the way a composer adds it: `Add effect`, then `Delay`.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        changes.delete(&id("arrangement/track-1/instrument"));
        project.commit("Remove synth", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-delay");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Delay"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> DelayState {
    opened.project(|project| {
        let delay = project.resolve::<DelayState>(&id(DELAY)).unwrap();
        *project.state(&delay).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(DELAY_FILE)).unwrap()
}

#[gpui::test]
fn add_effect_puts_a_delay_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), DelayState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<DelayView>().is_ok());
    // The shown controls, and none of the hidden ones. It syncs, so the time is a division
    // and the feel is at the top of the display.
    for shown in [
        "knob-division",
        "knob-feedback",
        "toggle-sync",
        "knob-mix",
        "handle-time-feedback",
        "segment-dotted",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    for hidden in ["knob-time_ms", "knob-low_cut_hz", "toggle-ping-pong"] {
        assert_eq!(opened.find(hidden), None, "{hidden}");
    }

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(DELAY_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-feedback");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).feedback;
    assert!(moving > DelayState::default().feedback);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).feedback;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change feedback"));
    assert!(file(&mut opened).contains(&format!("\"feedback\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), DelayState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Delay"));
}

/// While it syncs the Time knob steps through the divisions and the feel is one click. Sync
/// off, the knob is the time in ms and the feel is gone, as it does nothing then.
#[gpui::test]
fn the_time_is_a_division_while_it_syncs_and_ms_when_it_does_not(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-division");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    assert_eq!(state(&mut opened).division, Division::Quarter);
    assert_eq!(opened.undo_label().as_deref(), Some("Change time"));

    let dotted = opened.control("segment-dotted");
    opened.click(dotted);
    assert_eq!(state(&mut opened).feel, Feel::Dotted);
    assert_eq!(opened.undo_label().as_deref(), Some("Change feel"));
    assert!(file(&mut opened).contains(r#""feel": "dotted""#));

    let sync = opened.control("toggle-sync");
    opened.click(sync);
    assert!(!state(&mut opened).sync);
    assert_eq!(opened.undo_label().as_deref(), Some("Change sync"));
    assert_eq!(opened.find("knob-division"), None);
    assert_eq!(opened.find("segment-dotted"), None);
    let knob = opened.control("knob-time_ms");
    opened.drag(knob, point(knob.x, knob.y - px(30.)));
    let after = state(&mut opened);
    assert!(after.time_ms > DelayState::default().time_ms, "{after:?}");
    // The division and the feel wait for sync to come back.
    assert_eq!(
        (after.division, after.feel),
        (Division::Quarter, Feel::Dotted)
    );
}

/// The handle on the second repeat: sideways the time, up and down the feedback, one step for
/// both.
#[gpui::test]
fn the_handle_drags_the_time_and_the_feedback(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = DelayState::default();
    let handle = opened.control("handle-time-feedback");
    opened.drag(handle, point(handle.x + px(60.), handle.y - px(20.)));
    let after = state(&mut opened);
    assert!(after.division != default.division, "{after:?}");
    assert!(after.feedback > default.feedback, "{after:?}");
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Change time and feedback")
    );
    // The handle went along with the second repeat.
    let moved = opened.control("handle-time-feedback");
    assert!(moved.x > handle.x && moved.y < handle.y);

    // Far past the bottom: the feedback stops at 0, one repeat.
    let handle = opened.control("handle-time-feedback");
    opened.drag(handle, point(handle.x, handle.y + px(300.)));
    assert_eq!(state(&mut opened).feedback, 0.0);

    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);
}

/// Expand shows the cuts and ping-pong, is no edit, and is not saved. Ping-pong is one click
/// and one step.
#[gpui::test]
fn expand_shows_the_cuts_and_ping_pong(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-delay-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Delay"));
    for hidden in ["knob-low_cut_hz", "knob-high_cut_hz", "toggle-ping-pong"] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let ping_pong = opened.control("toggle-ping-pong");
    opened.click(ping_pong);
    assert!(state(&mut opened).ping_pong);
    assert_eq!(opened.undo_label().as_deref(), Some("Change ping-pong"));
    assert!(file(&mut opened).contains(r#""ping_pong": true"#));

    let low_cut = opened.control("knob-low_cut_hz");
    opened.drag(low_cut, point(low_cut.x, low_cut.y - px(50.)));
    assert!(state(&mut opened).low_cut_hz > DelayState::default().low_cut_hz);
    assert_eq!(opened.undo_label().as_deref(), Some("Change low cut"));

    let expand = opened.control("card-delay-expand");
    opened.click(expand);
    assert_eq!(opened.find("toggle-ping-pong"), None);
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let path = opened.path(DELAY_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "delay", "state": {"sync": false, "time_ms": 120.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert!(!state(&mut opened).sync);
    // The card shows what the file says: the time in ms, and a click turns sync on again.
    assert!(opened.find("knob-time_ms").is_some());
    let sync = opened.control("toggle-sync");
    opened.click(sync);
    assert!(state(&mut opened).sync);
    assert_eq!(opened.undo_label().as_deref(), Some("Change sync"));
}

/// How far apart the two channels of what the track plays are, against its level. The synth
/// plays the same in both, so a track without the delay is 0, and one through a ping-pong delay
/// is not.
fn stereo(opened: &mut Opened<'_>) -> f32 {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().play());
    });
    opened.settle();
    opened.render(12_000);
    let render = opened.render(12_000);
    let apart: f32 = render
        .chunks(2)
        .map(|frame| (frame[0] - frame[1]).abs())
        .sum();
    let level: f32 = render.iter().map(|sample| sample.abs()).sum();
    apart / level
}

/// The delay gets the power icon of every effect: it bypasses the slot, the record of the
/// delay stays, and the track sounds as it does without it. The repeats stop at once, as the
/// slot leaves the chain. One undo step each way.
#[gpui::test]
fn the_power_icon_bypasses_the_delay_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        let note = support::note(0, 4 * support::BAR, 64);
        let part = support::clip(0, 4 * support::BAR, vec![note]);
        changes.create(id("arrangement/track-1/part"), part);
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    assert_eq!(stereo(&mut opened), 0.0);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-delay");
    opened.click(row);
    // In ping-pong the repeats go left and right, so the track is no longer the same in both.
    let path = opened.path(DELAY_FILE);
    std::fs::write(&path, r#"{"tool": "delay", "state": {"ping_pong": true}}"#).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    let wide = stereo(&mut opened);
    assert!(wide > 0.01, "{wide}");

    let power = opened.control("card-delay-power");
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn off Delay"));
    let track = std::fs::read_to_string(opened.path("state/arrangement/track-1/instance.json"));
    assert!(
        track
            .unwrap()
            .contains(r#"{"name": "delay", "bypass": true}"#)
    );
    assert!(opened.path(DELAY_FILE).exists());
    assert_eq!(stereo(&mut opened), 0.0);

    opened.keys("cmd-z");
    assert!(stereo(&mut opened) > 0.01);
    let power = opened.control("card-delay-power");
    opened.click(power);
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn on Delay"));
}
