//! What every built-in effect does in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, a drag of a knob is one undo step written once, and an
//! edit from outside shows on its card. Each test goes over the effects the rack offers, so a
//! new device is tested here without a line of its own. What only one device does is in the
//! file of that device.

use arrangement::TrackState;
use gpui::{Pixels, Point, TestAppContext, point, px};
use sound_core::InstanceId;
use sound_ui::{DeviceOffer, Devices, OfferGroup, Slot};

use crate::support::{self, Opened, files, id, mark, one_undo_step, write_outside};

const TRACK: &str = "arrangement/track-1";

/// One track with no instrument, so the only card with knobs is the effect, and its panel
/// open.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        changes.delete(&id("arrangement/track-1/instrument"));
        project.commit("Remove synth", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    opened
}

fn built_in_effects(opened: &mut Opened<'_>) -> Vec<DeviceOffer> {
    let offers = opened.cx.update(|_, cx| Devices::offered(Slot::Effect, cx));
    let built_in = offers
        .into_iter()
        .filter(|offer| offer.group != OfferGroup::Plugins);
    let built_in: Vec<DeviceOffer> = built_in.collect();
    assert!(!built_in.is_empty(), "the rack offers no built-in effect");
    built_in
}

/// Adds the effect the way a composer adds it: `Add effect`, then its row. Gives its slot.
fn add(opened: &mut Opened<'_>, offer: &DeviceOffer) -> InstanceId {
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control(&format!("menu-{}", offer.key));
    opened.click(row);
    opened.project(|project| {
        assert_eq!(project.problems(), []);
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        let slots = arrangement::device_slots(project, &track).unwrap();
        slots.last().unwrap().clone()
    })
}

/// The record in a slot, as JSON.
fn state(opened: &mut Opened<'_>, slot: &InstanceId) -> String {
    opened.project(|project| project.state_json(slot).unwrap())
}

/// The name of every number in a record, also inside its objects and lists.
fn numbers(value: &serde_json::Value, names: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                if value.is_number() {
                    names.push(name.clone());
                } else {
                    numbers(value, names);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| numbers(item, names)),
        _ => {}
    }
}

/// A knob of the card of `slot`: the first number of its record that has one on screen.
fn knob_of(opened: &mut Opened<'_>, slot: &InstanceId) -> Point<Pixels> {
    let record = serde_json::from_str(&state(opened, slot)).unwrap();
    let mut names = Vec::new();
    numbers(&record, &mut names);
    let mut knobs = names.iter().map(|name| format!("knob-{name}"));
    knobs
        .find_map(|knob| opened.find(&knob))
        .unwrap_or_else(|| panic!("no number of {slot} has a knob on screen"))
}

/// Presses a knob and moves it 20 pt: up, or down when it is at the top of its range. Gives
/// where the drag ends, 40 pt from the press.
fn begin_turn(opened: &mut Opened<'_>, slot: &InstanceId, knob: Point<Pixels>) -> Point<Pixels> {
    let record = state(opened, slot);
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    if state(opened, slot) != record {
        return point(knob.x, knob.y - px(40.));
    }
    opened.drag_to(point(knob.x, knob.y + px(20.)));
    point(knob.x, knob.y + px(40.))
}

/// A whole drag of a knob, see [`begin_turn`].
fn turn(opened: &mut Opened<'_>, slot: &InstanceId, knob: Point<Pixels>) {
    let end = begin_turn(opened, slot, knob);
    opened.drag_to(end);
    opened.release(end);
}

#[gpui::test]
fn add_effect_puts_every_built_in_effect_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    for offer in built_in_effects(&mut opened) {
        let before = mark(&mut opened);
        let slot = add(&mut opened, &offer);
        // The record of the tool of the offer, and its card after the empty instrument slot.
        opened.project(|project| assert_eq!(project.tool_of(&slot), Some(offer.key.as_ref())));
        let panel = opened.track_panel().unwrap();
        let (names, has_card) = opened.cx.read(|cx| {
            let panel = panel.read(cx);
            let has_card = panel.device_views().nth(1).is_some_and(|it| it.is_some());
            (panel.device_names(cx), has_card)
        });
        assert_eq!(names.last(), Some(&offer.name));
        assert!(has_card, "{} has no card", offer.name);

        one_undo_step(&mut opened, &format!("Add {}", offer.name), &before);
        // Off again, for the next one.
        opened.keys("cmd-z");
    }
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    for offer in built_in_effects(&mut opened) {
        let slot = add(&mut opened, &offer);
        let knob = knob_of(&mut opened, &slot);
        let before = mark(&mut opened);
        let record = state(&mut opened, &slot);

        let end = begin_turn(&mut opened, &slot, knob);
        // Heard during the drag, not written until it ends.
        let moving = state(&mut opened, &slot);
        assert_ne!(moving, record, "{}", offer.name);
        assert_eq!(files(opened.folder.path()), before.files);
        opened.drag_to(end);
        opened.release(end);
        assert_ne!(state(&mut opened, &slot), moving, "{}", offer.name);
        assert_ne!(files(opened.folder.path()), before.files);

        let label = opened.undo_label().unwrap();
        assert_ne!(Some(&label), before.undo_label.as_ref());
        one_undo_step(&mut opened, &label, &before);
        // The drag and the effect off again, for the next one.
        opened.keys("cmd-z");
        opened.keys("cmd-z");
    }
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    for offer in built_in_effects(&mut opened) {
        let slot = add(&mut opened, &offer);
        let knob = knob_of(&mut opened, &slot);
        // A record that is not the one the card shows: what a drag made, taken back.
        turn(&mut opened, &slot, knob);
        let turned = state(&mut opened, &slot);
        opened.keys("cmd-z");
        assert_ne!(state(&mut opened, &slot), turned);

        let record = format!(r#"{{"tool": "{}", "state": {turned}}}"#, offer.key);
        write_outside(&mut opened, &format!("state/{slot}.json"), &record);
        assert_eq!(state(&mut opened, &slot), turned);
        assert_eq!(opened.undo_label().as_deref(), Some("File change"));
        // The knob is where the file put it: the same drag goes on from there. From the value
        // of before it would end where the file is, and change nothing.
        turn(&mut opened, &slot, knob);
        assert_ne!(state(&mut opened, &slot), turned, "{}", offer.name);

        // The drag, the file change and the effect off again, for the next one.
        for _ in 0..3 {
            opened.keys("cmd-z");
        }
    }
}
