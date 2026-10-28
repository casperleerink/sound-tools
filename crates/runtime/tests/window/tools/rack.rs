//! The blocks the song builds the devices of a track with: the cards of its children, a menu
//! of what the window offers, a text field and rows dragged onto each other.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};
use sound_ui::{Devices, Takes};

use super::{Opened, open};
use crate::song::{BAR, song_files};

/// The song of two tracks of a clip each, the first with a reverb `room` and a utility `trim` in its chain, with
/// the devices of that track showing.
fn open_rack(cx: &mut TestAppContext) -> Opened<'_> {
    let mut files = song_files(2, 2);
    files.retain(|(path, _)| path != "state/song/t00/chain/instance.json");
    for (path, record) in [
        (
            "chain/instance.json",
            r#"{"tool": "chain", "state": {"order": ["room", "trim"]}}"#,
        ),
        ("chain/room.json", r#"{"tool": "reverb", "state": {}}"#),
        ("chain/trim.json", r#"{"tool": "utility", "state": {}}"#),
    ] {
        files.push((format!("state/song/t00/{path}"), record.to_string()));
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut window = open(cx, &files);
    let name = window.until_found("div-name-t00");
    window.click(name);
    window.until_found("div-effect-room");
    window
}

/// The song with the devices of its first track showing, `count` effects `fx0` and on in its
/// chain, of each kind in turn: a rack far wider than the window.
pub(super) fn open_rack_of(cx: &mut TestAppContext, count: usize) -> Opened<'_> {
    let kinds = [
        "eq",
        "compressor",
        "reverb",
        "delay",
        "filter",
        "saturator",
        "gate",
        "utility",
    ];
    let effects: Vec<String> = (0..count).map(|index| format!("fx{index}")).collect();
    let mut files = song_files(2, 2);
    files.retain(|(path, _)| path != "state/song/t00/chain/instance.json");
    files.push((
        "state/song/t00/chain/instance.json".into(),
        serde_json::json!({ "tool": "chain", "state": { "order": effects } }).to_string(),
    ));
    for (name, tool) in effects.iter().zip(kinds.iter().cycle()) {
        files.push((
            format!("state/song/t00/chain/{name}.json"),
            format!(r#"{{"tool": "{tool}", "state": {{}}}}"#),
        ));
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut window = open(cx, &files);
    let name = window.until_found("div-name-t00");
    window.click(name);
    window.until_found(&format!("div-effect-fx{}", count - 1));
    window
}

impl Opened<'_> {
    pub(super) fn click_with(&mut self, position: Point<Pixels>, button: MouseButton) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        let modifiers = Modifiers::default();
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn chain_order(&mut self) -> serde_json::Value {
        self.record("song/t00/chain")["order"].clone()
    }

    pub(super) fn has(&mut self, instance: &str) -> bool {
        let session = self.session.clone();
        let id = crate::support::id(instance);
        self.cx
            .read(|cx| session.read(cx).project().tool_of(&id).is_some())
    }
}

#[gpui::test]
fn a_knob_on_the_card_of_an_effect_turns_its_record_as_one_undo_step(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let before = window.record("song/t00/chain/room");
    let knob = window.until_found("knob-size");
    window.press(knob);
    window.drag_to(knob - point(px(0.), px(20.)));
    window.drag_to(knob - point(px(0.), px(40.)));
    // The knob has the mouse: the card's effect, which drags to another place in the chain,
    // does not go along.
    assert!(!window.cx.update(|_, cx| cx.has_active_drag()));
    window.release(knob - point(px(0.), px(40.)));
    let after = window.record("song/t00/chain/room");
    assert_ne!(after["size"], before["size"]);
    window.undo();
    assert_eq!(window.record("song/t00/chain/room"), before);
    assert_eq!(window.problems(), []);
}

/// A picker lists each group of offers once: the effects come by what they do to the sound,
/// whichever crate registered them first.
#[gpui::test]
fn the_offers_come_group_by_group(cx: &mut TestAppContext) {
    let window = open_rack(cx);
    let groups: Vec<_> = window.cx.update(|_, cx| {
        (Devices::offered(Takes::Audio, cx).iter())
            .map(|offer| offer.group)
            .collect()
    });
    assert!(groups.is_sorted(), "{groups:?}");
}

#[gpui::test]
fn an_effect_picked_from_the_menu_of_offers_joins_the_chain_as_one_undo_step(
    cx: &mut TestAppContext,
) {
    let mut window = open_rack(cx);
    let add = window.until_found("div-add-effect");
    window.click(add);
    // The menu lists what the window offers that takes sound, as a picker of the window does.
    let reverb = window.cx.update(|_, cx| {
        (Devices::offered(Takes::Audio, cx).into_iter())
            .filter(|offer| !offer.is_command())
            .position(|offer| offer.name == "Reverb")
    });
    let item: &'static str = format!("menu-{}", reverb.unwrap()).leak();
    let item = window.until_found(item);
    window.click(item);
    window.until("the reverb added", |window| {
        window
            .chain_order()
            .as_array()
            .is_some_and(|order| order.len() == 3)
    });
    assert_eq!(
        window.chain_order(),
        serde_json::json!(["room", "trim", "reverb"])
    );
    assert!(window.has("song/t00/chain/reverb"));
    window.after_the_runs();
    window.undo();
    assert_eq!(window.chain_order(), serde_json::json!(["room", "trim"]));
    assert!(!window.has("song/t00/chain/reverb"));
    assert_eq!(window.problems(), []);
}

/// The synth card's title lists what takes notes, and the one picked takes the place of the
/// synth, in its file too, as one undo step that gives the synth back.
#[gpui::test]
fn an_instrument_picked_from_the_offers_takes_the_place_of_the_synth(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let change = window.until_found("card-title-song/t00/synth");
    window.click(change);
    let wavetable = window.cx.update(|_, cx| {
        (Devices::offered(Takes::Notes, cx).into_iter())
            .filter(|offer| !offer.is_command())
            .position(|offer| offer.name == "Wavetable")
    });
    let item: &'static str = format!("menu-{}", wavetable.unwrap()).leak();
    let item = window.until_found(item);
    window.click(item);
    let tool = |window: &mut Opened| {
        let id = crate::support::id("song/t00/synth");
        window.project(|project| project.tool_of(&id))
    };
    window.until("the wavetable in place", |window| {
        tool(window) == Some("wavetable")
    });
    assert_eq!(window.problems(), []);
    window.after_the_runs();
    let saved = window.file("state/song/t00/synth.json").unwrap();
    assert!(saved.contains("wavetable"), "{saved}");
    assert_eq!(window.undo().as_deref(), Some("Change the instrument"));
    assert_eq!(tool(&mut window), Some("instrument.synth"));
}

#[gpui::test]
fn the_icons_of_an_effect_card_turn_it_off_and_take_it_out(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let power = window.until_found("card-song/t00/chain/room-power");
    window.click(power);
    window.until("the room turned off", |window| {
        window.record("song/t00/chain")["off"] == serde_json::json!(["room"])
    });
    let close = window.until_found("card-song/t00/chain/trim-close");
    window.click(close);
    window.until("the trim taken out", |window| {
        !window.has("song/t00/chain/trim")
    });
    assert_eq!(window.chain_order(), serde_json::json!(["room"]));
    window.after_the_runs();
    assert_eq!(window.undo().as_deref(), Some("Remove an effect"));
    assert!(window.has("song/t00/chain/trim"));
    assert_eq!(window.chain_order(), serde_json::json!(["room", "trim"]));
}

#[gpui::test]
fn an_effect_dragged_onto_another_goes_before_it_as_one_undo_step(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let room = window.until_found("div-effect-room");
    let trim = window.until_found("div-effect-trim");
    // By the header of its card, above the knobs, which take a drag themselves.
    let (from, to) = (trim - point(px(0.), px(60.)), room - point(px(0.), px(60.)));
    window.press(from);
    for step in [0.2, 0.5, 0.8, 1.0] {
        window.drag_to(from + (to - from) * step);
        window.find("div-effect-room");
    }
    window.release(to);
    window.until("the trim moved", |window| {
        window.chain_order() == serde_json::json!(["trim", "room"])
    });
    window.after_the_runs();
    assert_eq!(window.undo().as_deref(), Some("Move an effect"));
    assert_eq!(window.chain_order(), serde_json::json!(["room", "trim"]));
}

#[gpui::test]
fn an_effect_dragged_onto_add_effect_goes_last(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let room = window.until_found("div-effect-room");
    let to = window.until_found("div-add-effect");
    let from = room - point(px(0.), px(60.));
    window.press(from);
    for step in [0.2, 0.5, 0.8, 1.0] {
        window.drag_to(from + (to - from) * step);
        window.find("div-effect-room");
    }
    window.release(to);
    window.until("the room moved last", |window| {
        window.chain_order() == serde_json::json!(["trim", "room"])
    });
}

#[gpui::test]
fn a_track_is_renamed_in_a_text_field_and_space_types_there(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    let name = window.until_found("div-name-t00");
    window.double_click(name);
    window.until("the name field has the keys", |window| {
        window.cx.update(|window, _| {
            window
                .context_stack()
                .iter()
                .any(|context| context.contains(sound_ui::components::text_input::KEY_CONTEXT))
        })
    });
    window.cx.simulate_input("Bass");
    window.cx.simulate_keystrokes("space");
    assert!(
        !window.playing(),
        "space plays nothing while a field has the keys"
    );
    window.cx.simulate_input("line");
    window.cx.simulate_keystrokes("enter");
    window.until("the title changed", |window| {
        window.record("song/t00")["title"] == "Bass line"
    });
    // Its name shows again, and a click elsewhere tells nothing more.
    let name = window.until_found("div-name-t00");
    window.click(name);
    window.after_the_runs();
    assert_eq!(window.undo().as_deref(), Some("Rename a track"));
    assert_eq!(window.record("song/t00").get("title"), None);
}

#[gpui::test]
fn a_clip_menu_deletes_the_clip(cx: &mut TestAppContext) {
    let mut window = open_rack(cx);
    assert!(window.has("song/t01/c00"));
    let clip = window.on_lane(BAR / 2, 1);
    window.wait_for_bun();
    window.click_with(clip, MouseButton::Right);
    // Cut, copy, duplicate, rename, render with and without the tail, delete.
    let delete = window.until_found("menu-6");
    window.click(delete);
    window.until("the clip deleted", |window| !window.has("song/t01/c00"));
    window.wait_for_bun();
    assert_eq!(window.problems(), []);
}

/// The song with a compressor `comp` after the filter of its first track, whose devices show.
fn open_compressor(cx: &mut TestAppContext) -> Opened<'_> {
    let mut files = song_files(2, 1);
    for (path, record) in &mut files {
        if path == "project.json" {
            *record = record.replace(r#""wavetable""#, r#""wavetable", "compressor""#);
        }
    }
    files.retain(|(path, _)| path != "state/song/t00/chain/instance.json");
    for (path, record) in [
        (
            "chain/instance.json",
            r#"{"tool": "chain", "state": {"order": ["filter", "comp"]}}"#,
        ),
        ("chain/comp.json", r#"{"tool": "compressor", "state": {}}"#),
    ] {
        files.push((format!("state/song/t00/{path}"), record.to_string()));
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut window = open(cx, &files);
    let name = window.until_found("div-name-t00");
    window.click(name);
    window.until_found("div-effect-comp");
    window
}

/// The one key of the song: its track and its tap, `post_fader` when the record leaves it out.
fn keyed(window: &mut Opened<'_>) -> Option<(String, String)> {
    let sidechains = window.record("song")["sidechains"].clone();
    let key = sidechains.as_array()?.first()?.clone();
    let tap = key["tap"].as_str().unwrap_or("post_fader").to_string();
    Some((key["from"].as_str()?.to_string(), tap))
}

/// The sidechain picker is behind the compressor's expand, as the arrangement's was. Another
/// track keys it after its effects and a tap select shows; its own track keys it only before
/// its effects, with no tap; Off takes the key away. Each pick is one undo step.
#[gpui::test]
fn the_sidechain_picker_keys_the_compressor_from_a_track(cx: &mut TestAppContext) {
    let mut window = open_compressor(cx);
    window.wait_for_bun();
    assert_eq!(window.find("select-sidechain-0"), None);
    let expand = window.until_found("card-song/t00/chain/comp-expand");
    window.click(expand);
    let source = window.until_found("select-sidechain-0");
    assert_eq!(window.find("select-sidechain-1"), None);

    // Off, Track 1, Track 2, the audio track.
    window.click(source);
    window.pick("menu-2");
    window.until("the key", |window| keyed(window).is_some());
    window.after_the_runs();
    assert_eq!(keyed(&mut window), Some(("t01".into(), "post_fx".into())));
    assert_eq!(window.undo_label().as_deref(), Some("Turn on sidechain"));
    assert_eq!(window.problems(), []);

    let tap = window.until_found("select-sidechain-1");
    window.click(tap);
    window.pick("menu-2");
    window.until("the tap", |window| {
        keyed(window).is_some_and(|(_, tap)| tap == "post_fader")
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change sidechain tap"));

    let source = window.until_found("select-sidechain-0");
    window.click(source);
    window.pick("menu-1");
    window.until("its own track", |window| {
        keyed(window).is_some_and(|(from, _)| from == "t00")
    });
    window.after_the_runs();
    assert_eq!(keyed(&mut window), Some(("t00".into(), "pre_fx".into())));
    assert_eq!(window.undo_label().as_deref(), Some("Change sidechain"));
    assert_eq!(window.problems(), []);
    assert_eq!(window.find("select-sidechain-1"), None);

    let source = window.until_found("select-sidechain-0");
    window.click(source);
    window.pick("menu-0");
    window.until("no key", |window| keyed(window).is_none());
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Turn off sidechain"));
}

/// A card out of the rack's view is not drawn; scrolled into view, it is.
#[gpui::test]
fn a_card_draws_once_the_rack_scrolls_it_into_view(cx: &mut TestAppContext) {
    let mut window = open_rack_of(cx, 16);
    let last = "card-title-song/t00/chain/fx15";
    assert!(window.find(last).is_none());
    let rack = window.until_found("div-rack");
    window.until(last, |window| {
        window.cx.simulate_event(ScrollWheelEvent {
            position: rack,
            delta: ScrollDelta::Pixels(point(px(-400.), px(0.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        window.find(last).is_some()
    });
    assert!(window.find("card-title-song/t00/chain/fx0").is_none());
}
