//! The card of the built-in delay in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

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
