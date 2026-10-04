//! The card of the built-in compressor in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

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
    for hidden in ["knob-knee_db", "knob-makeup_db", "knob-mix", "lookahead"] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let select = opened.control("lookahead");
    opened.click(select);
    let ten = opened.control("menu-10");
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

/// A card that opens after its compressor played with nobody looking shows what it does now,
/// not the loudest of what it did then.
#[gpui::test]
fn a_card_that_opens_later_does_not_show_what_played_before(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| {
        let chord = vec![note(0, 3840, 48), note(0, 3840, 55), note(0, 3840, 64)];
        let mut changes = sound_core::Changes::new();
        changes.create(id("arrangement/track-1/chord"), clip(0, 3840, chord));
        let track = project
            .resolve::<arrangement::TrackState>(&id("arrangement/track-1"))
            .unwrap();
        let slot = arrangement::add_effect(project, &mut changes, &track, "Compressor").unwrap();
        let sound = CompressorState {
            threshold_db: -60.0,
            ..CompressorState::default()
        };
        changes.create(slot, sound);
        project.commit("Add chord and compressor", changes).unwrap();
        project.clear_history();
    });
    // It plays and stops with its panel closed.
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().play());
    });
    opened.settle();
    opened.render(12_000);
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().stop());
    });
    opened.settle();
    opened.render(48_000);

    let header = opened.track_header(0);
    opened.click(header);
    assert!(opened.find("knob-threshold_db").is_some());
    opened.cx.executor().advance_clock(POLL_INTERVAL);
    opened.cx.run_until_parked();
    assert_eq!(opened.find("compressor-level"), None);
}
