//! The card of the built-in reverb in a rack, with a simulated mouse: every edit of it is one
//! undo step written once.

use gpui::{TestAppContext, point, px};
use reverb::ReverbState;

use crate::rack::record;
use crate::support::{self, Opened, id};

const REVERB: &str = "rack/reverb";
const REVERB_FILE: &str = "state/rack/reverb.json";

/// A rack of the reverb alone, so its card is the only one with knobs.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_rack(cx, &[("reverb", &record("reverb", "{}"))])
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
    let expand = opened.control("card-rack/reverb-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label(), None);
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

    let expand = opened.control("card-rack/reverb-expand");
    opened.click(expand);
    assert_eq!(opened.find("toggle-freeze"), None);
}
