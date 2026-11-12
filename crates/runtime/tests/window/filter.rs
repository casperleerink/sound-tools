//! The card of the built-in filter in a rack, with a simulated mouse: every edit of it is one
//! undo step written once. A number a lane moves shows the lane and does not drag.

use filter::view::FilterView;
use filter::{FilterState, FilterType};
use gpui::{TestAppContext, point, px};
use sound_core::Ticks;
use sound_ui::POLL_INTERVAL;

use crate::rack::record;
use crate::support::{self, Opened, id, write_outside};

const FILTER: &str = "rack/filter";
const FILTER_FILE: &str = "state/rack/filter.json";

/// A rack of the filter alone, so its card is the only one with knobs.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_rack(cx, &[("filter", &record("filter", "{}"))])
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
    let expand = opened.control("card-rack/filter-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label(), None);
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

    let expand = opened.control("card-rack/filter-expand");
    opened.click(expand);
    assert_eq!(opened.find("knob-lfo_rate_hz"), None);
}

/// A lane of the cutoff of the filter with these points, or none moving it, as an agent writes
/// it.
fn automate(opened: &mut Opened<'_>, points: &[(u64, f32)]) {
    let points: Vec<_> = (points.iter())
        .map(|(tick, value)| serde_json::json!({ "tick": tick, "value": value }))
        .collect();
    let automation = match points.is_empty() {
        true => serde_json::json!([]),
        false => serde_json::json!([{ "lane": "cutoff", "target": "filter.cutoff_hz" }]),
    };
    if !points.is_empty() {
        let lane = serde_json::json!({ "tool": "lane", "state": { "points": points } });
        write_outside(opened, "state/rack/cutoff.json", &lane.to_string());
    }
    let rack = serde_json::json!({ "tool": "rack", "state": { "order": ["filter"], "automation": automation } });
    write_outside(opened, "state/rack/instance.json", &rack.to_string());
    opened.project(|project| assert_eq!(project.problems(), []));
    let lanes = !points.is_empty();
    opened.until("the lane", |opened| {
        opened.find("automated-cutoff_hz").is_some() == lanes
    });
}

/// The cutoff as the card shows it.
fn shown_cutoff(opened: &mut Opened<'_>) -> f32 {
    let view = opened.card::<FilterView>(FILTER);
    opened
        .cx
        .read(|cx| view.read(cx).shown(cx).unwrap().cutoff_hz)
}

fn seek(opened: &mut Opened<'_>, tick: u64) {
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, _| session.engine().seek(Ticks(tick)));
    });
    opened.settle();
}

/// A lane of the cutoff: the knob shows the value the lane plays at the playhead, with a mark,
/// and follows a seek. A drag of the knob or of the
/// handle, a key and a reset change nothing. Taking the lane out gives the knob back the record.
#[gpui::test]
fn an_automated_cutoff_shows_its_lane_and_does_not_drag(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    automate(&mut opened, &[(0, 300.), (support::BAR, 3_000.)]);
    assert!(opened.find("automated-resonance").is_none());
    // A cable plays a number once per block, so it shows within a few hertz.
    let shown = shown_cutoff(&mut opened);
    assert!((shown - 300.).abs() < 3., "{shown}");
    seek(&mut opened, support::BAR);
    let shown = shown_cutoff(&mut opened);
    assert!((shown - 3_000.).abs() < 30., "{shown}");
    let automated = opened.undo_label();

    let knob = opened.control("knob-cutoff_hz");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    opened.double_click(knob);
    let handle = opened.control("handle-cutoff-resonance");
    opened.drag(handle, point(handle.x - px(30.), handle.y - px(10.)));
    assert_eq!(state(&mut opened), FilterState::default());
    assert_eq!(opened.undo_label(), automated);

    automate(&mut opened, &[]);
    let record = FilterState::default().cutoff_hz;
    assert_eq!(shown_cutoff(&mut opened), record);
    let knob = opened.control("knob-cutoff_hz");
    opened.drag(knob, point(knob.x, knob.y - px(40.)));
    assert!(state(&mut opened).cutoff_hz > record);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
}

/// The filter alone in the window that has the keys, as the app's window does. A window says
/// that the keys left a field only while it is that one, and a test's is not until it is told.
fn open_active(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = open_panel(cx);
    opened.cx.update(|window, _| window.activate_window());
    opened.settle();
    opened
}

/// Types `text` over what the field of the knob `knob` shows. The field opens by a click on
/// the value of the knob.
fn type_on(opened: &mut Opened<'_>, knob: &str, text: &str) {
    let value = opened.control(&format!("{knob}-value"));
    opened.click(value);
    assert!(opened.find(&format!("{knob}-typed")).is_some(), "{knob}");
    opened.cx.simulate_input(text);
    opened.cx.run_until_parked();
}

/// A click on the value of a knob puts a field in its place. Enter reads what was typed in the
/// unit the knob shows, inside its range, as one undo step. Escape, and enter on what the field
/// opened with, change nothing.
#[gpui::test]
fn a_value_typed_on_a_knob_is_read_in_its_unit_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open_active(cx);
    let default = FilterState::default();
    assert_eq!(opened.find("knob-cutoff_hz-typed"), None);

    let before = support::mark(&mut opened);
    type_on(&mut opened, "knob-cutoff_hz", "1.2 kHz");
    assert_eq!(state(&mut opened), default, "typing alone is no edit");
    opened.keys("enter");
    assert_eq!(state(&mut opened).cutoff_hz, 1_200.);
    assert_eq!(opened.find("knob-cutoff_hz-typed"), None);
    support::one_undo_step(&mut opened, "Change cutoff", &before);

    // A part of one is typed as it shows, in percent, with or without the sign.
    type_on(&mut opened, "knob-resonance", "50%");
    opened.keys("enter");
    assert_eq!(state(&mut opened).resonance, 0.5);
    type_on(&mut opened, "knob-resonance", "35");
    opened.keys("enter");
    assert_eq!(state(&mut opened).resonance, 0.35);
    type_on(&mut opened, "knob-drive_db", "6 dB");
    opened.keys("enter");
    assert_eq!(state(&mut opened).drive_db, 6.);
    // Past the end of the travel is the end.
    type_on(&mut opened, "knob-cutoff_hz", "90k");
    opened.keys("enter");
    assert_eq!(state(&mut opened).cutoff_hz, 20_000.);

    // The keys went back to the knob, so enter opens the field again, on the value without
    // its unit. Enter on that, escape after typing, and words are no edit.
    let typed = state(&mut opened);
    let step = opened.undo_label();
    for keys in ["enter", "9 escape", "l o u d enter"] {
        opened.keys("enter");
        assert!(opened.find("knob-cutoff_hz-typed").is_some(), "{keys}");
        opened.keys(keys);
        assert_eq!(opened.find("knob-cutoff_hz-typed"), None, "{keys}");
        assert_eq!(state(&mut opened), typed, "{keys}");
        assert_eq!(opened.undo_label(), step, "{keys}");
    }

    // What was typed counts when the keys go elsewhere: a press on another knob.
    type_on(&mut opened, "knob-cutoff_hz", "440");
    let resonance = opened.control("knob-resonance");
    opened.click(resonance);
    assert_eq!(state(&mut opened).cutoff_hz, 440.);
    assert_eq!(opened.find("knob-cutoff_hz-typed"), None);

    // A double click on the dial still sets the default.
    let knob = opened.control("knob-cutoff_hz");
    opened.double_click(knob);
    assert_eq!(state(&mut opened).cutoff_hz, default.cutoff_hz);
}

/// A value a lane moves cannot be typed, as it cannot be dragged: its value takes no click and
/// enter on the knob opens nothing. A lane that arrives while a value is typed takes the field
/// away, and what was typed is dropped.
#[gpui::test]
fn an_automated_knob_takes_no_typed_value(cx: &mut TestAppContext) {
    let mut opened = open_active(cx);
    type_on(&mut opened, "knob-cutoff_hz", "440");
    automate(&mut opened, &[(0, 300.)]);
    assert_eq!(opened.find("knob-cutoff_hz-typed"), None);
    assert_eq!(state(&mut opened), FilterState::default());
    let automated = opened.undo_label();

    assert_eq!(opened.find("knob-cutoff_hz-value"), None);
    let knob = opened.control("knob-cutoff_hz");
    opened.click(knob);
    opened.keys("enter");
    assert_eq!(opened.find("knob-cutoff_hz-typed"), None);
    opened.cx.simulate_input("440");
    opened.keys("enter");
    assert_eq!(state(&mut opened), FilterState::default());
    assert_eq!(opened.undo_label(), automated);

    // Without the lane the knob takes a typed value again.
    automate(&mut opened, &[]);
    type_on(&mut opened, "knob-cutoff_hz", "440");
    opened.keys("enter");
    assert_eq!(state(&mut opened).cutoff_hz, 440.);
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
    assert!((shown_cutoff(&mut opened) - 300.).abs() < 3.);
    let label = opened.undo_label();
    opened.keys("secondary-z");
    assert_ne!(opened.undo_label(), label);
    opened.release(point(knob.x, knob.y - px(40.)));
    assert!(!opened.gesture_open());
}

/// Plays `frames` and lets the window poll once, as a frame of the display does.
fn play_on(opened: &mut Opened<'_>, frames: usize) {
    opened.render(frames);
    opened.cx.executor().advance_clock(POLL_INTERVAL);
    opened.cx.run_until_parked();
}

fn points(opened: &mut Opened<'_>) -> Vec<(u64, f32)> {
    let lane: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(opened.path("state/rack/cutoff.json")).unwrap(),
    )
    .unwrap();
    let points = lane["state"]["points"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    (points.iter())
        .map(|point| {
            let value = point["value"].as_f64().unwrap_or_default() as f32;
            (point["tick"].as_u64().unwrap_or_default(), value)
        })
        .collect()
}

/// While automation is written and the piece plays, a drag of the cutoff takes it over from its
/// lane: what the hand plays, held still or moving, becomes the lane from where it was grabbed
/// to where it was let go, which goes back to the line it had after. One undo step, which keeps
/// the record's cutoff as it was; the lane plays again after it.
#[gpui::test]
fn a_drag_of_an_automated_cutoff_while_it_plays_writes_into_its_lane(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let far = 64 * support::BAR;
    automate(&mut opened, &[(0, 1_000.), (far, 1_000.)]);
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.set_writes_automation(true, cx);
            session.engine().play();
        });
    });
    play_on(&mut opened, 24_000);
    let before = support::mark(&mut opened);
    let knob = opened.control("knob-cutoff_hz");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // The hand holds still for a second, then moves on.
    for _ in 0..10 {
        play_on(&mut opened, 4_800);
    }
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    for _ in 0..5 {
        play_on(&mut opened, 4_800);
    }
    let played = shown_cutoff(&mut opened);
    opened.release(point(knob.x, knob.y - px(40.)));
    opened.settle();

    let written = points(&mut opened);
    let (first, last) = (written[0], written[written.len() - 1]);
    assert_eq!((first, last), ((0, 1_000.), (far, 1_000.)), "{written:?}");
    // The line holds 1 kHz up to where the hand took it, and goes back to 1 kHz after.
    let (edge, grabbed) = (written[1], written[2]);
    assert_eq!((grabbed.0 - edge.0, edge.1), (1, 1_000.), "{written:?}");
    assert!(grabbed.1 > 1_000., "{written:?}");
    let back = written[written.len() - 2];
    let let_go = written[written.len() - 3];
    assert_eq!((back.0 - let_go.0, back.1), (120, 1_000.), "{written:?}");
    assert_eq!(let_go.1, played, "{written:?}");
    // A second of a still hand is one flat line, above where it started.
    let still = written
        .windows(2)
        .any(|pair| pair[0].1 == pair[1].1 && pair[0].1 > 1_000. && pair[1].0 - pair[0].0 > 1_500);
    assert!(still, "{written:?}");
    assert!(written.len() < 12, "{written:?}");
    assert_eq!(
        state(&mut opened).cutoff_hz,
        FilterState::default().cutoff_hz
    );
    support::one_undo_step(&mut opened, "Write automation", &before);

    // The lane plays again: the knob shows it and holds still without the switch.
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| session.set_writes_automation(false, cx));
    });
    play_on(&mut opened, 4_800);
    assert!(opened.find("automated-cutoff_hz").is_some());
}

/// Escape during a drag that writes automation puts everything back: the lane has its points,
/// there is no undo step, and the lane plays again.
#[gpui::test]
fn escape_during_a_drag_that_writes_automation_leaves_the_lane_as_it_was(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    automate(&mut opened, &[(0, 300.), (support::BAR, 3_000.)]);
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.set_writes_automation(true, cx);
            session.engine().play();
        });
    });
    play_on(&mut opened, 4_800);
    let (lane, label) = (points(&mut opened), opened.undo_label());
    let knob = opened.control("knob-cutoff_hz");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(30.)));
    play_on(&mut opened, 4_800);
    opened.keys("escape");
    opened.release(point(knob.x, knob.y - px(30.)));
    opened.settle();
    assert_eq!(points(&mut opened), lane);
    assert_eq!(opened.undo_label(), label);
    assert_eq!(state(&mut opened), FilterState::default());
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| session.set_writes_automation(false, cx));
    });
    play_on(&mut opened, 4_800);
    assert!(opened.find("automated-cutoff_hz").is_some());
}

/// A hand that holds the cutoff while the loop goes back writes each pass on its own: the
/// second pass writes from the loop's start, where the first had not been.
#[gpui::test]
fn a_drag_held_over_the_end_of_a_loop_writes_each_pass(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    automate(&mut opened, &[(0, 1_000.), (64 * support::BAR, 1_000.)]);
    let session = opened.session.clone();
    opened.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.set_writes_automation(true, cx);
            let range = Ticks(0)..Ticks(support::BAR);
            session.set_loop(sound_ui::Loop { range, on: true }, cx);
            session.engine().play();
        });
    });
    // Half a second into the bar of two seconds, then on past its end.
    play_on(&mut opened, 24_000);
    let knob = opened.control("knob-cutoff_hz");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    for _ in 0..20 {
        play_on(&mut opened, 4_800);
    }
    let held = shown_cutoff(&mut opened);
    opened.release(point(knob.x, knob.y - px(20.)));
    opened.settle();
    let written = points(&mut opened);
    // The line holds what the hand held over `tick`.
    let holds = |tick: u64| {
        let around = written
            .windows(2)
            .find(|pair| pair[0].0 <= tick && tick < pair[1].0);
        around.is_some_and(|pair| pair[0].1 == held && pair[1].1 == held)
    };
    // The hand took it a quarter of the way into the first pass, and held it into the second.
    assert!(holds(500) && holds(2_500), "{written:?}");
}
