//! Reordering the track rack in the window, step 9b of the third milestone: an effect card is
//! dragged by its header onto another card, or moved with cmd and the arrows. The instrument
//! stays first, a plugin moves as a built-in effect does, a bypassed slot keeps its bypass, and
//! each move is one undo step that undo gives back byte for byte. A reorder written from
//! outside shows in the rack at once.

use arrangement::EffectSlot;
use arrangement::TrackState;
use gpui::{Pixels, Point, TestAppContext, point, px};
use plugin_host::{PluginFormat, PluginRecord};

use crate::support::{self, BAR, Opened, id, mark, one_undo_step, test_plugin_id, write_outside};

const TRACK: &str = "arrangement/track-1";
const TRACK_FILE: &str = "state/arrangement/track-1/instance.json";
const PLUGIN: &str = "sound-tools-test-tone";

/// The default track with a Filter and a bypassed Reverb named `space`, written as an agent
/// does, then the repository's test plugin added from the rack, and the panel open. So the rack
/// is: Synth, Filter, Reverb (off), the plugin.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with_test_plugin(cx, |_| {});
    write_outside(
        &mut opened,
        "state/arrangement/track-1/filter.json",
        r#"{"tool": "filter", "state": {}}"#,
    );
    write_outside(
        &mut opened,
        "state/arrangement/track-1/space.json",
        r#"{"tool": "reverb", "state": {}}"#,
    );
    write_outside(
        &mut opened,
        TRACK_FILE,
        r#"{"tool": "arrangement.track", "state": {"name": "Track 1", "order": 0, "effects": ["filter", {"name": "space", "bypass": true}]}}"#,
    );
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = format!(
        "menu-{}",
        PluginRecord::offer_key(PluginFormat::Clap, test_plugin_id(PluginFormat::Clap))
    );
    let row = opened.control(&row);
    opened.click(row);
    opened.project(|project| assert_eq!(project.problems(), []));
    assert_eq!(
        names(&mut opened),
        ["Synth", "Filter", "Reverb", "Sound Tools Test Tone"]
    );
    opened
}

fn names(opened: &mut Opened<'_>) -> Vec<String> {
    let panel = opened.track_panel().unwrap();
    opened.cx.read(|cx| {
        let names = panel.read(cx).device_names(cx);
        names.iter().map(ToString::to_string).collect()
    })
}

fn effects(opened: &mut Opened<'_>) -> Vec<EffectSlot> {
    opened.project(|project| {
        let track = project.resolve::<TrackState>(&id(TRACK)).unwrap();
        project.state(&track).unwrap().effects.clone()
    })
}

fn slot(name: &str, bypass: bool) -> EffectSlot {
    EffectSlot {
        name: name.into(),
        bypass,
    }
}

/// A place on the header of a card that is neither its title nor an icon: just left of the
/// first icon.
fn grip(opened: &mut Opened<'_>, card: &str) -> Point<Pixels> {
    let icon = opened
        .find(&format!("card-{card}-expand"))
        .or_else(|| opened.find(&format!("card-{card}-power")))
        .unwrap();
    icon - point(px(20.), px(0.))
}

/// Drags the header of one card onto the header of another place in the rack.
fn drag_card(opened: &mut Opened<'_>, card: &str, onto: Point<Pixels>) {
    let from = grip(opened, card);
    opened.drag(from, onto);
    opened.settle();
}

fn header(opened: &mut Opened<'_>, card: &str) -> Point<Pixels> {
    opened.control(&format!("card-{card}-header"))
}

#[gpui::test]
fn a_drag_of_an_effect_card_onto_another_moves_it_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let before = mark(&mut opened);
    let onto = header(&mut opened, "space");
    drag_card(&mut opened, "filter", onto);
    // The filter takes the place of the reverb, which keeps its bypass.
    assert_eq!(
        effects(&mut opened),
        [
            slot("space", true),
            slot("filter", false),
            slot(PLUGIN, false)
        ]
    );
    assert_eq!(
        names(&mut opened),
        ["Synth", "Reverb", "Filter", "Sound Tools Test Tone"]
    );
    one_undo_step(&mut opened, "Move Filter", &before);

    // A plugin moves as a built-in effect does.
    let before = mark(&mut opened);
    let onto = header(&mut opened, "space");
    drag_card(&mut opened, PLUGIN, onto);
    assert_eq!(
        effects(&mut opened),
        [
            slot(PLUGIN, false),
            slot("space", true),
            slot("filter", false)
        ]
    );
    one_undo_step(&mut opened, "Move Sound Tools Test Tone", &before);
    // Every record is where it was: only the list of the track changed.
    opened.project(|project| assert_eq!(project.problems(), []));
    assert!(
        opened
            .path("state/arrangement/track-1/filter.json")
            .exists()
    );
}

#[gpui::test]
fn the_instrument_stays_first_and_a_drop_on_it_or_on_add_effect_goes_first_or_last(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    // A drop on the instrument: the first effect.
    let onto = header(&mut opened, "instrument");
    drag_card(&mut opened, PLUGIN, onto);
    assert_eq!(
        effects(&mut opened),
        [
            slot(PLUGIN, false),
            slot("filter", false),
            slot("space", true)
        ]
    );
    assert_eq!(names(&mut opened)[0], "Synth");
    // A drop on `Add effect`: the last.
    let onto = opened.control("add-effect");
    drag_card(&mut opened, PLUGIN, onto);
    assert_eq!(
        effects(&mut opened),
        [
            slot("filter", false),
            slot("space", true),
            slot(PLUGIN, false)
        ]
    );
    // The instrument card has no grip: a drag of its header moves nothing.
    let label = opened.undo_label();
    let from = header(&mut opened, "instrument") + point(px(60.), px(0.));
    let onto = header(&mut opened, "filter");
    opened.drag(from, onto);
    assert_eq!(opened.undo_label(), label);
    // A drop back where it came from is no edit either.
    let onto = header(&mut opened, "filter");
    drag_card(&mut opened, "filter", onto);
    assert_eq!(opened.undo_label(), label);
    assert_eq!(names(&mut opened)[0], "Synth");
}

#[gpui::test]
fn cmd_and_the_arrows_move_the_effect_whose_card_has_the_focus(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    // A press on a knob gives it the focus and changes nothing.
    let knob = opened.control("knob-cutoff_hz");
    opened.click(knob);
    let before = mark(&mut opened);
    opened.keys("cmd-right");
    assert_eq!(
        effects(&mut opened),
        [
            slot("space", true),
            slot("filter", false),
            slot(PLUGIN, false)
        ]
    );
    one_undo_step(&mut opened, "Move Filter", &before);
    opened.keys("cmd-right cmd-right");
    assert_eq!(
        effects(&mut opened),
        [
            slot("space", true),
            slot(PLUGIN, false),
            slot("filter", false)
        ]
    );
    // It never goes before the instrument.
    opened.keys("cmd-left cmd-left cmd-left cmd-left");
    assert_eq!(
        effects(&mut opened),
        [
            slot("filter", false),
            slot("space", true),
            slot(PLUGIN, false)
        ]
    );
    assert_eq!(names(&mut opened)[..2], ["Synth", "Filter"]);
}

#[gpui::test]
fn a_reorder_written_from_outside_shows_in_the_rack_at_once(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    write_outside(
        &mut opened,
        TRACK_FILE,
        &format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "Track 1", "order": 0, "effects": ["{PLUGIN}", {{"name": "space", "bypass": true}}, "filter"]}}}}"#
        ),
    );
    assert_eq!(
        names(&mut opened),
        ["Synth", "Sound Tools Test Tone", "Reverb", "Filter"]
    );
    // And the window moves on from what the file says.
    let onto = header(&mut opened, PLUGIN);
    drag_card(&mut opened, "filter", onto);
    assert_eq!(
        effects(&mut opened),
        [
            slot("filter", false),
            slot(PLUGIN, false),
            slot("space", true)
        ]
    );
}

#[gpui::test]
fn escape_or_a_drop_outside_the_rack_moves_nothing(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let label = opened.undo_label();
    let files = support::files(opened.folder.path());
    // Escape with the card over another: the drop that follows lands nowhere, and the panel
    // stays open.
    let from = grip(&mut opened, "filter");
    let onto = header(&mut opened, "space");
    opened.press(from);
    opened.drag_to(from + point(px(20.), px(0.)));
    opened.drag_to(onto);
    opened.keys("escape");
    opened.release(onto);
    opened.settle();
    assert!(opened.track_panel().is_some());
    // A drop on the timeline, outside the rack.
    let from = grip(&mut opened, "filter");
    let timeline = opened.at(2 * BAR, 0);
    opened.drag(from, timeline);
    opened.settle();
    assert_eq!(
        effects(&mut opened),
        [
            slot("filter", false),
            slot("space", true),
            slot(PLUGIN, false)
        ]
    );
    assert_eq!(opened.undo_label(), label);
    assert_eq!(support::files(opened.folder.path()), files);
}
