//! The card of the built-in filter in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once.

use filter::view::FilterView;
use filter::{FilterState, FilterType};
use gpui::{TestAppContext, point, px};

use crate::support::{self, Opened, id};

const FILTER: &str = "arrangement/track-1/filter";
const FILTER_FILE: &str = "state/arrangement/track-1/filter.json";

/// One track with no instrument, so the only card with knobs is the filter, and its panel
/// open. The filter is added the way a composer adds it: `Add effect`, then `Filter`.
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
    let row = opened.control("menu-filter");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Filter"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> FilterState {
    opened.project(|project| {
        let filter = project.resolve::<FilterState>(&id(FILTER)).unwrap();
        *project.state(&filter).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(FILTER_FILE)).unwrap()
}

#[gpui::test]
fn add_effect_puts_a_filter_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), FilterState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<FilterView>().is_ok());
    // The shown controls, and none of the hidden ones.
    for shown in [
        "knob-cutoff_hz",
        "knob-resonance",
        "knob-drive_db",
        "knob-mix",
        "handle-cutoff-resonance",
        "segment-low_pass",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("knob-lfo_rate_hz"), None);

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(FILTER_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-cutoff_hz");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).cutoff_hz;
    assert!(moving > FilterState::default().cutoff_hz);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).cutoff_hz;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    assert!(file(&mut opened).contains(&format!("\"cutoff_hz\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), FilterState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Filter"));
}

/// Sideways is cutoff and up is resonance, and both are one step.
#[gpui::test]
fn a_drag_of_the_handle_changes_cutoff_and_resonance_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let handle = opened.control("handle-cutoff-resonance");
    opened.drag(handle, point(handle.x + px(30.), handle.y - px(10.)));
    let after = state(&mut opened);
    let default = FilterState::default();
    assert!(after.cutoff_hz > default.cutoff_hz, "{after:?}");
    assert!(after.resonance > default.resonance, "{after:?}");
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Change cutoff and resonance")
    );
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), default);

    // Down past the bottom of the travel: resonance stops at 0.
    let handle = opened.control("handle-cutoff-resonance");
    opened.drag(handle, point(handle.x, handle.y + px(200.)));
    assert_eq!(state(&mut opened).resonance, 0.0);
}

#[gpui::test]
fn the_type_is_one_click_and_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let high = opened.control("segment-high_pass");
    opened.click(high);
    assert_eq!(state(&mut opened).kind, FilterType::HighPass);
    assert_eq!(opened.undo_label().as_deref(), Some("Change filter type"));
    assert!(file(&mut opened).contains(r#""type": "high_pass""#));
}

/// Expand shows the slope and the LFO, is no edit, and is not saved.
#[gpui::test]
fn expand_shows_the_slope_and_the_lfo(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("device-filter-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Filter"));
    for hidden in ["knob-lfo_rate_hz", "knob-lfo_depth_octaves", "segment-24"] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let steep = opened.control("segment-24");
    opened.click(steep);
    assert_eq!(state(&mut opened).slope, filter::Slope::TwentyFour);
    assert_eq!(opened.undo_label().as_deref(), Some("Change slope"));

    let depth = opened.control("knob-lfo_depth_octaves");
    opened.drag(depth, point(depth.x, depth.y - px(50.)));
    assert!(state(&mut opened).lfo_depth_octaves > 0.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change LFO depth"));

    let expand = opened.control("device-filter-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-lfo_rate_hz"), None);
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let path = opened.path(FILTER_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "filter", "state": {"type": "notch", "cutoff_hz": 300.0}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert_eq!(state(&mut opened).kind, FilterType::Notch);
    // The selected segment is the one the file names: clicking it again is no edit.
    let label = opened.undo_label();
    let notch = opened.control("segment-notch");
    opened.click(notch);
    assert_eq!(opened.undo_label(), label);
}
