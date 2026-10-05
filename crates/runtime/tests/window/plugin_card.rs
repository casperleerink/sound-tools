//! The parameters on the card of a plugin: pinned from the list on the card, turned there, and
//! taken off again. Each is one undo step that undo gives back byte for byte. An automation lane
//! of the track moves a pin as it moves the knob of a built-in device, and is added from the
//! same select.
//!
//! The plugin is the repository's CLAP test instrument: `Cutoff` takes any value from 20 to
//! 20000, `Wave` is three named steps and `Bright` is off or on.

use std::collections::BTreeMap;

use arrangement::view::layout::{LANES_MIDDLE, NAME_LEFT, TRACK_HEIGHT};
use arrangement::{AutomationLane, AutomationValue, TrackState};
use gpui::{TestAppContext, point, px};
use plugin_host::{Pin, PluginFormat, PluginRecord};

use sound_core::{Changes, Ticks};

use crate::support::{self, Opened, id, mark, one_undo_step, test_plugin_record, write_outside};

const TRACK: &str = "arrangement/track-1";
const SLOT: &str = "arrangement/track-1/instrument";
const SLOT_FILE: &str = "state/arrangement/track-1/instrument.json";
const LIST: &str = "plugin-parameters";

/// `Track 1` playing the CLAP test plugin, with its panel open. The record is written by the
/// project, so undo gives it back byte for byte.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with_test_plugin(cx, |project| {
        let mut changes = Changes::new();
        changes.delete(&id(SLOT));
        project.commit("Remove synth", changes).unwrap();
        let record = PluginRecord::new(PluginFormat::Clap, test_clap_plugin::PLUGIN_ID, "tone");
        let mut changes = Changes::new();
        changes.create(id(SLOT), record.unwrap());
        project.commit("Choose the test plugin", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    opened
}

/// The pins of the record, as id, name and value.
fn pins(opened: &mut Opened<'_>) -> BTreeMap<u32, Pin> {
    opened.project(|project| {
        let record = project.resolve::<PluginRecord>(&id(SLOT)).unwrap();
        project.state(&record).unwrap().parameters.clone()
    })
}

fn pin(name: &str, value: f64) -> Pin {
    Pin {
        name: name.to_string(),
        value,
    }
}

/// Opens the list of parameters, types `search` into it and picks the row of `parameter`.
fn pick(opened: &mut Opened<'_>, search: &str, parameter: u32) {
    let trigger = opened.control(LIST);
    opened.click(trigger);
    opened.cx.simulate_input(search);
    opened.cx.run_until_parked();
    let row = opened.control(&format!("menu-{parameter}"));
    opened.click(row);
}

#[gpui::test]
fn a_parameter_pinned_from_the_card_turns_there_one_undo_step_each(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    assert_eq!(opened.find("knob-pin-0"), None);

    // The search keeps the rows whose name holds what is typed.
    let trigger = opened.control(LIST);
    opened.click(trigger);
    opened.cx.simulate_input("cut");
    opened.cx.run_until_parked();
    assert!(opened.find("menu-0").is_some());
    assert_eq!(opened.find("menu-1"), None, "Wave is not called that");
    opened.keys("escape");

    // Picked, it is on the card at the value the plugin has, as one step.
    let before = mark(&mut opened);
    pick(&mut opened, "cut", test_clap_plugin::CUTOFF);
    assert_eq!(
        pins(&mut opened),
        BTreeMap::from([(0, pin("Cutoff", 1000.0))])
    );
    one_undo_step(&mut opened, "Add Cutoff", &before);

    // A turn of its knob is one step too: a quarter of the travel up from 1000 Hz.
    let before = mark(&mut opened);
    let knob = opened.control("knob-pin-0");
    opened.drag(knob, point(knob.x, knob.y - px(50.)));
    assert_eq!(
        pins(&mut opened),
        BTreeMap::from([(0, pin("Cutoff", 6000.0))])
    );
    one_undo_step(&mut opened, "Change Cutoff", &before);

    // Picked again from the list, it comes off the card.
    let before = mark(&mut opened);
    pick(&mut opened, "", test_clap_plugin::CUTOFF);
    assert_eq!(pins(&mut opened), BTreeMap::new());
    assert_eq!(opened.find("knob-pin-0"), None);
    one_undo_step(&mut opened, "Remove Cutoff", &before);
}

/// Two steps are a toggle and named steps a dropdown, and each sends the exact value of the
/// step it picks.
#[gpui::test]
fn a_toggle_and_a_dropdown_set_the_values_of_their_steps(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    pick(&mut opened, "bri", test_clap_plugin::BRIGHT);
    pick(&mut opened, "wav", test_clap_plugin::WAVE);
    let both = |bright, wave| BTreeMap::from([(1, pin("Wave", wave)), (5, pin("Bright", bright))]);
    assert_eq!(pins(&mut opened), both(0.0, 0.0));

    let before = mark(&mut opened);
    let toggle = opened.control("toggle-pin-5");
    opened.click(toggle);
    assert_eq!(pins(&mut opened), both(1.0, 0.0));
    one_undo_step(&mut opened, "Change Bright", &before);

    let before = mark(&mut opened);
    let select = opened.control("select-pin-1");
    opened.click(select);
    // The rows of a dropdown are its steps, by place.
    let square = opened.control("menu-2");
    opened.click(square);
    assert_eq!(pins(&mut opened), both(1.0, 2.0));
    one_undo_step(&mut opened, "Change Wave", &before);
}

/// A pin the plugin has no parameter for says so in its cell, and comes off from the list like
/// any other.
#[gpui::test]
fn a_pin_the_plugin_does_not_have_says_so_and_comes_off_from_the_list(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let record = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "sound-tools.test-tone", "state_asset": "tone", "parameters": {"77": {"name": "Gone", "value": 0.5}}}}"#;
    write_outside(&mut opened, SLOT_FILE, record);
    assert!(opened.find("missing-77").is_some());

    pick(&mut opened, "gone", 77);
    assert_eq!(pins(&mut opened), BTreeMap::new());
    assert_eq!(opened.find("missing-77"), None);
}

/// A pin taken off while its knob is dragged, by an agent that writes the record, takes the
/// knob with it. The drag ends there, so undo works again at once.
#[gpui::test]
fn a_pin_taken_off_under_a_drag_ends_the_drag(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    pick(&mut opened, "cut", test_clap_plugin::CUTOFF);
    let knob = opened.control("knob-pin-0");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    assert!(opened.gesture_open());

    write_outside(
        &mut opened,
        SLOT_FILE,
        &test_plugin_record(PluginFormat::Clap, "tone"),
    );
    assert_eq!(opened.find("knob-pin-0"), None);
    assert!(!opened.gesture_open());
    opened.release(knob);
    assert_eq!(pins(&mut opened), BTreeMap::new());
}

/// The lane of `Cutoff`, as an agent writes it, or none.
fn automate_cutoff(opened: &mut Opened<'_>, value: Option<f32>) {
    let lanes = value.map(|value| AutomationLane {
        device: Some("instrument".into()),
        parameter: "parameters.0.value".into(),
        points: vec![sound_notes::Point {
            tick: Ticks(0),
            value: AutomationValue(value),
        }],
    });
    opened.edit(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        let mut state = project.state(&track).unwrap().clone();
        state.automation = lanes.into_iter().collect();
        let mut changes = Changes::new();
        changes.set(&track, state);
        project.commit("Automate", changes)
    });
    opened.project(|project| assert_eq!(project.problems(), []));
}

/// A lane of a pin: its knob carries the mark of an automated control and a drag changes
/// nothing. Taking the lane out gives the knob back.
#[gpui::test]
fn the_knob_of_a_pin_a_lane_moves_is_marked_and_does_not_drag(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    pick(&mut opened, "cut", test_clap_plugin::CUTOFF);
    automate_cutoff(&mut opened, Some(5000.0));
    assert!(opened.find("automated-pin-0").is_some());
    let knob = opened.control("knob-pin-0");
    opened.drag(knob, point(knob.x, knob.y - px(50.)));
    assert_eq!(
        pins(&mut opened),
        BTreeMap::from([(0, pin("Cutoff", 1000.0))])
    );
    assert_eq!(opened.undo_label().as_deref(), Some("Automate"));

    automate_cutoff(&mut opened, None);
    assert_eq!(opened.find("automated-pin-0"), None);
    let knob = opened.control("knob-pin-0");
    opened.drag(knob, point(knob.x, knob.y - px(50.)));
    assert_eq!(
        pins(&mut opened),
        BTreeMap::from([(0, pin("Cutoff", 6000.0))])
    );
}

/// The select under the lanes of the track offers a pin that takes a lane, and not one with
/// named steps. A pick adds a lane that holds the value of the record: one undo step.
#[gpui::test]
fn the_select_adds_a_lane_for_a_pin(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    pick(&mut opened, "cut", test_clap_plugin::CUTOFF);
    pick(&mut opened, "wav", test_clap_plugin::WAVE);
    let header = opened.track_header(0);
    let toggle = point(
        px(NAME_LEFT + 8.),
        header.y + px(LANES_MIDDLE - TRACK_HEIGHT / 2.),
    );
    opened.click(toggle);

    let before = mark(&mut opened);
    let select = opened.control("add-lane-track-1");
    opened.click(select);
    assert_eq!(opened.find("menu-instrument/parameters.1.value"), None);
    let cutoff = opened.control("menu-instrument/parameters.0.value");
    opened.click(cutoff);
    let lanes = opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        project.state(&track).unwrap().automation.clone()
    });
    let lane = AutomationLane {
        device: Some("instrument".into()),
        parameter: "parameters.0.value".into(),
        points: vec![sound_notes::Point {
            tick: Ticks(0),
            value: AutomationValue(1000.0),
        }],
    };
    assert_eq!(lanes, [lane]);
    one_undo_step(&mut opened, "Add automation", &before);
}

/// The select also offers every parameter of the plugin that takes a lane and is not pinned,
/// as it offers every knob of a built-in device. A pick pins it at the value it plays and adds
/// the lane, as one undo step that undo takes back whole.
#[gpui::test]
fn the_select_adds_a_lane_for_a_parameter_that_is_not_pinned_and_pins_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    let toggle = point(
        px(NAME_LEFT + 8.),
        header.y + px(LANES_MIDDLE - TRACK_HEIGHT / 2.),
    );
    opened.click(toggle);

    let before = mark(&mut opened);
    let select = opened.control("add-lane-track-1");
    opened.click(select);
    assert!(opened.find("menu-instrument/parameters.0.value").is_some());
    // Named steps and off or on take no lane.
    assert_eq!(opened.find("menu-instrument/parameters.1.value"), None);
    assert_eq!(opened.find("menu-instrument/parameters.5.value"), None);
    // A long list is searched.
    opened.cx.simulate_input("lev");
    opened.cx.run_until_parked();
    assert_eq!(opened.find("menu-instrument/parameters.0.value"), None);
    let level = opened.control("menu-instrument/parameters.4.value");
    opened.click(level);

    assert_eq!(pins(&mut opened), BTreeMap::from([(4, pin("Level", 1.0))]));
    let lanes = opened.project(|project| {
        assert_eq!(project.problems(), []);
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        project.state(&track).unwrap().automation.clone()
    });
    let lane = AutomationLane {
        device: Some("instrument".into()),
        parameter: "parameters.4.value".into(),
        points: vec![sound_notes::Point {
            tick: Ticks(0),
            value: AutomationValue(1.0),
        }],
    };
    assert_eq!(lanes, [lane]);
    assert!(opened.find("automated-pin-4").is_some());
    one_undo_step(&mut opened, "Add automation", &before);
}
