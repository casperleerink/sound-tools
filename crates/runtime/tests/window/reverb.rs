//! The card of the built-in reverb in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

use gpui::{TestAppContext, point, px};
use reverb::ReverbState;
use reverb::view::ReverbView;

use crate::support::{self, Opened, id};

const REVERB: &str = "arrangement/track-1/reverb";
const REVERB_FILE: &str = "state/arrangement/track-1/reverb.json";

/// One track with no instrument, so the only card with knobs is the reverb, and its panel
/// open. The reverb is added the way a composer adds it: `Add effect`, then `Reverb`.
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
    let row = opened.control("menu-reverb");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Reverb"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> ReverbState {
    opened.project(|project| {
        let reverb = project.resolve::<ReverbState>(&id(REVERB)).unwrap();
        *project.state(&reverb).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(REVERB_FILE)).unwrap()
}

#[gpui::test]
fn add_effect_puts_a_reverb_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), ReverbState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<ReverbView>().is_ok());
    // The shown controls, and none of the hidden ones.
    for shown in [
        "knob-size",
        "knob-damping",
        "knob-width",
        "knob-mix",
        "handle-pre-delay",
        "handle-decay",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    for hidden in ["knob-decay_seconds", "toggle-freeze", "knob-low_cut_hz"] {
        assert_eq!(opened.find(hidden), None, "{hidden}");
    }

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(REVERB_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-size");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).size;
    assert!(moving > ReverbState::default().size);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).size;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change size"));
    assert!(file(&mut opened).contains(&format!("\"size\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), ReverbState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Reverb"));
}

/// The start of the tail is the pre-delay and its end the decay, each one step. The end moves
/// with the start, as the tail starts later.
#[gpui::test]
fn the_handles_drag_the_pre_delay_and_the_decay(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = ReverbState::default();
    let end = opened.control("handle-decay");
    opened.drag(end, point(end.x + px(30.), end.y));
    let after = state(&mut opened);
    assert!(after.decay_seconds > default.decay_seconds, "{after:?}");
    assert_eq!(after.pre_delay_ms, default.pre_delay_ms);
    assert_eq!(opened.undo_label().as_deref(), Some("Change decay"));

    let start = opened.control("handle-pre-delay");
    let end_before = opened.control("handle-decay");
    opened.drag(start, point(start.x + px(10.), start.y));
    let after = state(&mut opened);
    assert!(after.pre_delay_ms > default.pre_delay_ms, "{after:?}");
    assert_eq!(opened.undo_label().as_deref(), Some("Change pre-delay"));
    let end_after = opened.control("handle-decay");
    assert!(end_after.x > end_before.x);

    // Far past the left: the pre-delay stops at its least.
    let start = opened.control("handle-pre-delay");
    opened.drag(start, point(start.x - px(300.), start.y));
    assert_eq!(state(&mut opened).pre_delay_ms, reverb::PRE_DELAY.min);

    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);
}

/// Expand shows the cuts, diffusion, freeze, pre-delay and decay, is no edit, and is not
/// saved. Freeze is one click and one step.
#[gpui::test]
fn expand_shows_the_hidden_controls_and_freeze_is_one_click(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-reverb-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Reverb"));
    for hidden in [
        "knob-low_cut_hz",
        "knob-high_cut_hz",
        "knob-diffusion",
        "knob-pre_delay_ms",
        "knob-decay_seconds",
        "toggle-freeze",
    ] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let freeze = opened.control("toggle-freeze");
    opened.click(freeze);
    assert!(state(&mut opened).freeze);
    assert_eq!(opened.undo_label().as_deref(), Some("Change freeze"));
    assert!(file(&mut opened).contains(r#""freeze": true"#));

    let decay = opened.control("knob-decay_seconds");
    opened.drag(decay, point(decay.x, decay.y - px(50.)));
    assert!(state(&mut opened).decay_seconds > ReverbState::default().decay_seconds);
    assert_eq!(opened.undo_label().as_deref(), Some("Change decay"));

    let expand = opened.control("card-reverb-expand");
    opened.click(expand);
    assert_eq!(opened.find("toggle-freeze"), None);
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-reverb-expand");
    opened.click(expand);
    let path = opened.path(REVERB_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "reverb", "state": {"freeze": true, "size": 0.9}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert!(state(&mut opened).freeze);
    // The toggle shows what the file says: a click turns freeze off, not on again.
    let freeze = opened.control("toggle-freeze");
    opened.click(freeze);
    assert!(!state(&mut opened).freeze);
    assert_eq!(opened.undo_label().as_deref(), Some("Change freeze"));
}

/// How far apart the two channels of what the track plays are, against its level. The synth
/// plays the same in both, so a track without the reverb is 0 and a wide reverb is not.
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

/// The reverb gets the power icon of every effect: it bypasses the slot, the record of the
/// reverb stays, and the track sounds as it does without it. The tail stops at once, as the
/// slot leaves the chain. One undo step each way.
#[gpui::test]
fn the_power_icon_bypasses_the_reverb_as_one_undo_step(cx: &mut TestAppContext) {
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
    let plain = stereo(&mut opened);
    assert_eq!(plain, 0.0);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-reverb");
    opened.click(row);
    let wide = stereo(&mut opened);
    assert!(wide > 0.01, "{wide}");

    let power = opened.control("card-reverb-power");
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn off Reverb"));
    let track = std::fs::read_to_string(opened.path("state/arrangement/track-1/instance.json"));
    assert!(
        track
            .unwrap()
            .contains(r#"{"name": "reverb", "bypass": true}"#)
    );
    assert!(opened.path(REVERB_FILE).exists());
    assert_eq!(stereo(&mut opened), 0.0);

    opened.keys("cmd-z");
    assert!(stereo(&mut opened) > 0.01);
    let power = opened.control("card-reverb-power");
    opened.click(power);
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn on Reverb"));
}
