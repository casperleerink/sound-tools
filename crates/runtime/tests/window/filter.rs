//! The card of the built-in filter in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, and every edit of it is one undo step written once. A
//! number a lane of the track moves shows the lane and does not drag.

use arrangement::{AutomationLane, AutomationValue, TrackState};
use filter::view::FilterView;
use filter::{FilterState, FilterType};
use gpui::{TestAppContext, point, px};
use sound_core::Ticks;

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
    let expand = opened.control("card-filter-expand");
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

    let expand = opened.control("card-filter-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-lfo_rate_hz"), None);
}

const TRACK: &str = "arrangement/track-1";

/// Puts these lanes of the cutoff of the filter in the record of the track, or takes them out
/// with none: an edit of the track, as an agent writes it.
fn automate(opened: &mut Opened<'_>, points: &[(u64, f32)]) {
    let lanes = match points {
        [] => Vec::new(),
        points => vec![AutomationLane {
            device: Some("filter".into()),
            parameter: "cutoff_hz".into(),
            points: points
                .iter()
                .map(|&(tick, value)| sound_notes::Point {
                    tick: Ticks(tick),
                    value: AutomationValue(value),
                })
                .collect(),
        }],
    };
    opened.edit(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        let mut state = project.state(&track).unwrap().clone();
        state.automation = lanes;
        let mut changes = sound_core::Changes::new();
        changes.set(&track, state);
        project.commit("Automate", changes)
    });
    opened.project(|project| assert_eq!(project.problems(), []));
}

/// The cutoff as the card shows it.
fn shown_cutoff(opened: &mut Opened<'_>) -> f32 {
    let panel = opened.track_panel().unwrap();
    opened.cx.read(|cx| {
        let view = panel.read(cx).device_views().nth(1).unwrap().cloned();
        let view = view.unwrap().downcast::<FilterView>().unwrap();
        view.read(cx).shown(cx).unwrap().cutoff_hz
    })
}

fn seek(opened: &mut Opened<'_>, tick: u64) {
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, _| session.engine().seek(Ticks(tick)));
    });
    opened.settle();
}

/// A lane of the cutoff in the track: the knob shows the value the lane plays at the playhead,
/// with a mark, and follows a seek. A drag of the knob or of the handle, a key and a reset
/// change nothing. Taking the lane out of the track gives the knob back the record.
#[gpui::test]
fn an_automated_cutoff_shows_its_lane_and_does_not_drag(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    automate(&mut opened, &[(0, 300.), (support::BAR, 3_000.)]);
    assert!(opened.find("automated-cutoff_hz").is_some());
    assert!(opened.find("automated-resonance").is_none());
    assert!((shown_cutoff(&mut opened) - 300.).abs() < 0.01);
    seek(&mut opened, support::BAR);
    assert!((shown_cutoff(&mut opened) - 3_000.).abs() < 0.1);

    let knob = opened.control("knob-cutoff_hz");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    opened.double_click(knob);
    let handle = opened.control("handle-cutoff-resonance");
    opened.drag(handle, point(handle.x - px(30.), handle.y - px(10.)));
    assert_eq!(state(&mut opened), FilterState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Automate"));

    automate(&mut opened, &[]);
    assert!(opened.find("automated-cutoff_hz").is_none());
    let record = FilterState::default().cutoff_hz;
    assert_eq!(shown_cutoff(&mut opened), record);
    let knob = opened.control("knob-cutoff_hz");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    assert!(state(&mut opened).cutoff_hz > record);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
}

/// A lane that arrives during a drag of the cutoff, as an agent writes it: the drag ends at the
/// next mouse move, with the gesture of the session, so undo does not wait for a mouse up.
#[gpui::test]
fn a_lane_that_arrives_during_a_knob_drag_ends_the_gesture(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-cutoff_hz");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    assert!(opened.gesture_open());
    automate(&mut opened, &[(0, 300.)]);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    assert!(!opened.gesture_open());
    assert!((shown_cutoff(&mut opened) - 300.).abs() < 0.01);
    let label = opened.undo_label();
    opened.keys("cmd-z");
    assert_ne!(opened.undo_label(), label);
    opened.release(point(knob.x, knob.y - px(40.)));
    assert!(!opened.gesture_open());
}
