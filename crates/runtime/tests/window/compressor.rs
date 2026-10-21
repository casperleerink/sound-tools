//! The card of the built-in compressor in a rack, with a simulated mouse: every edit of it is
//! one undo step written once.

use compressor::{CompressorState, Lookahead};
use gpui::{TestAppContext, point, px};
use sound_ui::POLL_INTERVAL;

use crate::rack::{notes, record};
use crate::support::{self, Opened, id};

const COMPRESSOR: &str = "rack/compressor";
const COMPRESSOR_FILE: &str = "state/rack/compressor.json";

/// A rack of the compressor alone, so its card is the only one with knobs.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_rack(cx, &[("compressor", &record("compressor", "{}"))])
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
    let expand = opened.control("card-rack/compressor-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label(), None);
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

    let expand = opened.control("card-rack/compressor-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-knee_db"), None);
}

/// While the rack plays, the card shows the level it hears as a dot on the curve; at rest
/// there is none. The level and the reduction come from the audio thread.
#[gpui::test]
fn the_level_shows_on_the_curve_while_it_plays(cx: &mut TestAppContext) {
    let chord = notes(3840, &[(0, 3840, 48), (0, 3840, 55), (0, 3840, 64)]);
    let squeezed = record("compressor", r#"{"threshold_db": -60.0}"#);
    let mut opened = support::open_rack(
        cx,
        &[
            ("instrument", &record("instrument.synth", "{}")),
            ("chord", &chord),
            ("compressor", &squeezed),
        ],
    );
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
