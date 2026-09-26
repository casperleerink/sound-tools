//! The card of the built-in compressor in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

use compressor::view::CompressorView;
use compressor::{CompressorState, Lookahead};
use gpui::{TestAppContext, point, px};
use sound_ui::POLL_INTERVAL;

use crate::support::{self, Opened, clip, id, note};

const COMPRESSOR: &str = "arrangement/track-1/compressor";
const COMPRESSOR_FILE: &str = "state/arrangement/track-1/compressor.json";

/// One track with no instrument, so the only card with knobs is the compressor, and its panel
/// open. The compressor is added the way a composer adds it: `Add effect`, then `Compressor`.
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
    let row = opened.control("menu-compressor");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Compressor"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> CompressorState {
    opened.project(|project| {
        let compressor = project.resolve::<CompressorState>(&id(COMPRESSOR)).unwrap();
        *project.state(&compressor).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(COMPRESSOR_FILE)).unwrap()
}

#[gpui::test]
fn add_effect_puts_a_compressor_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), CompressorState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<CompressorView>().is_ok());
    // The shown controls, and none of the hidden ones.
    for shown in [
        "knob-threshold_db",
        "knob-ratio",
        "knob-attack_ms",
        "knob-release_ms",
        "handle-threshold",
        "handle-ratio",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    for hidden in ["knob-knee_db", "knob-makeup_db", "knob-mix", "segment-10"] {
        assert_eq!(opened.find(hidden), None, "{hidden}");
    }

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(COMPRESSOR_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-ratio");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).ratio;
    assert!(moving > CompressorState::default().ratio);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).ratio;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change ratio"));
    assert!(file(&mut opened).contains(&format!("\"ratio\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), CompressorState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Compressor"));
}

/// The threshold handle moves sideways only, and stops at 0 dBFS.
#[gpui::test]
fn a_drag_of_the_threshold_handle_changes_the_threshold(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = CompressorState::default();
    let handle = opened.control("handle-threshold");
    opened.drag(handle, point(handle.x - px(20.), handle.y - px(30.)));
    let after = state(&mut opened);
    assert!(after.threshold_db < default.threshold_db, "{after:?}");
    assert_eq!(after.ratio, default.ratio);
    assert_eq!(opened.undo_label().as_deref(), Some("Change threshold"));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);

    // Right past the end of the curve: the threshold stops at 0 dBFS.
    let handle = opened.control("handle-threshold");
    opened.drag(handle, point(handle.x + px(200.), handle.y));
    assert_eq!(state(&mut opened).threshold_db, 0.0);
}

/// The ratio handle at the end of the line: up is a gentler ratio, down a steeper one, and it
/// stays on the curve.
#[gpui::test]
fn a_drag_of_the_ratio_handle_changes_the_ratio(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let default = CompressorState::default();
    let handle = opened.control("handle-ratio");
    opened.drag(handle, point(handle.x, handle.y - px(10.)));
    let gentler = state(&mut opened);
    assert!(gentler.ratio < default.ratio, "{gentler:?}");
    assert_eq!(gentler.threshold_db, default.threshold_db);
    assert_eq!(opened.undo_label().as_deref(), Some("Change ratio"));

    // Down past the bottom: the steepest ratio.
    let handle = opened.control("handle-ratio");
    opened.drag(handle, point(handle.x, handle.y + px(200.)));
    assert_eq!(state(&mut opened).ratio, compressor::RATIO.max);
}

/// Expand shows knee, makeup, mix and lookahead, is no edit, and is not saved.
#[gpui::test]
fn expand_shows_knee_makeup_mix_and_lookahead(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-compressor-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Compressor"));
    for hidden in ["knob-knee_db", "knob-makeup_db", "knob-mix", "segment-10"] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let ten = opened.control("segment-10");
    opened.click(ten);
    assert_eq!(state(&mut opened).lookahead, Lookahead::Ten);
    assert_eq!(opened.undo_label().as_deref(), Some("Change lookahead"));
    assert!(file(&mut opened).contains(r#""lookahead_ms": 10"#));

    let makeup = opened.control("knob-makeup_db");
    opened.drag(makeup, point(makeup.x, makeup.y - px(50.)));
    assert!(state(&mut opened).makeup_db > 0.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change makeup"));

    let expand = opened.control("card-compressor-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-knee_db"), None);
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-compressor-expand");
    opened.click(expand);
    let path = opened.path(COMPRESSOR_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "compressor", "state": {"threshold_db": -30.0, "lookahead_ms": 1}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert_eq!(state(&mut opened).lookahead, Lookahead::One);
    // The selected segment is the one the file names: clicking it again is no edit.
    let label = opened.undo_label();
    let one = opened.control("segment-1");
    opened.click(one);
    assert_eq!(opened.undo_label(), label);
}

/// While the track plays, the card shows the level it hears as a dot on the curve; at rest
/// there is none. The level and the reduction come from the audio thread.
#[gpui::test]
fn the_level_shows_on_the_curve_while_the_track_plays(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let chord = vec![note(0, 3840, 48), note(0, 3840, 55), note(0, 3840, 64)];
        let mut changes = sound_core::Changes::new();
        changes.create(id("arrangement/track-1/chord"), clip(0, 3840, chord));
        project.commit("Add chord", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-compressor");
    opened.click(row);
    let file = "state/arrangement/track-1/compressor.json";
    let path = opened.path(file);
    std::fs::write(
        &path,
        r#"{"tool": "compressor", "state": {"threshold_db": -60.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    opened.settle();
    assert_eq!(opened.find("compressor-level"), None);

    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().play());
    });
    opened.settle();
    opened.render(12_000);
    opened.cx.executor().advance_clock(POLL_INTERVAL);
    opened.cx.run_until_parked();
    assert!(opened.find("compressor-level").is_some());

    // Stopped, the sound ends and the dot goes with it.
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().stop());
    });
    opened.settle();
    opened.render(48_000);
    for _ in 0..3 {
        opened.cx.executor().advance_clock(POLL_INTERVAL);
        opened.cx.run_until_parked();
    }
    assert_eq!(opened.find("compressor-level"), None);
}
