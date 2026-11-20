//! The device panel of a track, driven by the pointer and the keys as a composer drives it.

use serde_json::Value;

use super::{build, click, frame, key, laid_out, new_project, opened, peak_db, redraw, travel};
use crate::app::State;

/// A new song whose first track has the effects `effects`, `(name, record)` each, in order.
fn with_effects(effects: &[(&str, &str)]) -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let chain = folder.join("state/song/t00/chain");
    for (name, record) in effects {
        std::fs::write(chain.join(format!("{name}.json")), record).unwrap();
    }
    let order: Vec<&str> = effects.iter().map(|(name, _)| *name).collect();
    let instance = serde_json::json!({ "tool": "chain", "state": { "order": order } });
    std::fs::write(chain.join("instance.json"), instance.to_string()).unwrap();
    let window = laid_out(opened(&folder));
    (scratch, window)
}

fn chain(state: &State) -> Value {
    let id = sound_core::InstanceId::new("song/t00/chain").unwrap();
    serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap()
}

fn order(state: &State) -> Value {
    chain(state)["order"].clone()
}

/// The middle of the element `key`'s drawn rect, from its left by `across` of its width.
fn point(window: &rune::Runtime<State>, key: &str, across: f32) -> (f32, f32) {
    let [x, y, w, h] = window
        .presented(key)
        .unwrap_or_else(|| panic!("no {key}"))
        .0;
    (x + w * across, y + h / 2.0)
}

#[test]
fn an_effect_picked_by_a_typed_search_in_add_effect_joins_the_chain_as_one_step() {
    let (_scratch, mut window) = with_effects(&[]);
    click(&mut window, "add-effect");
    redraw(&mut window);
    for letter in "comp".chars() {
        key(&mut window, rune::Key::Text(letter.to_string()));
    }
    assert!(window.find("add-effect-compressor").is_some());
    assert!(window.find("add-effect-delay").is_none());
    // A heading shows while the search keeps one of its effects.
    assert!(window.find("add-effect-group-Dynamics").is_some());
    assert!(window.find("add-effect-group-Space").is_none());
    key(&mut window, rune::Key::Enter);
    assert_eq!(order(&window.state), serde_json::json!(["compressor"]));
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Add an effect"));
    window.state.undo().unwrap();
    assert_eq!(order(&window.state), serde_json::json!([]));
}

#[test]
fn an_effect_dragged_by_its_header_onto_another_goes_before_it_as_one_step() {
    let (_scratch, mut window) = with_effects(&[
        ("comp", r#"{"tool": "compressor", "state": {}}"#),
        ("dly", r#"{"tool": "delay", "state": {}}"#),
    ]);
    let from = point(&window, "slot-dly-header", 0.6);
    let to = point(&window, "slot-comp-header", 0.6);
    window.pointer_down(from.0, from.1);
    window.frame(1.0 / 60.0);
    let end = travel(&mut window, from, ((to.0 - from.0) / 10.0, 0.0), 10);
    window.pointer_up(end.0, end.1);
    redraw(&mut window);
    assert_eq!(order(&window.state), serde_json::json!(["dly", "comp"]));
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Move an effect"));
    window.state.undo().unwrap();
    assert_eq!(order(&window.state), serde_json::json!(["comp", "dly"]));
}

/// The loudest the track `t00` played in the last `seconds`, in dB, as its meter reads it.
fn level(window: &mut rune::Runtime<State>, from: f64, seconds: f64) -> f64 {
    build(&window.state);
    let state = &mut window.state;
    state.session.background(|project| project.engine().play());
    let mut loudest = f64::NEG_INFINITY;
    for step in 1..=(seconds * 60.0) as u32 {
        frame(state, from + f64::from(step) / 60.0);
        loudest = loudest.max(f64::from(peak_db(state, "song/t00/mix")));
    }
    state.session.background(|project| project.engine().stop());
    loudest
}

#[test]
fn power_off_on_an_effect_that_mutes_lets_the_track_sound() {
    let (_scratch, mut window) =
        with_effects(&[("mute", r#"{"tool": "utility", "state": {"mute": true}}"#)]);
    let muted = level(&mut window, 0.0, 1.0);
    assert!(muted < -90.0, "{muted}");
    // Power shows while the pointer is on the card.
    let header = point(&window, "slot-mute-header", 0.5);
    window.pointer_move(header.0, header.1);
    redraw(&mut window);
    click(&mut window, "slot-mute-power");
    assert_eq!(chain(&window.state)["off"], serde_json::json!(["mute"]));
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Turn an effect off"));
    let open = level(&mut window, 1.0, 1.0);
    assert!(open > -60.0, "{open}");
}

/// The rack keeps its focused Add effect in view as effects come before it, and a wheel that
/// scrolls it away leaves it there.
#[test]
#[ignore = "missing: the focus leaves Add effect for the synth's card when an effect joins (tpl-fix, rack.tsx; or win-sweep)"]
fn the_rack_keeps_the_focused_add_effect_in_view_as_effects_are_added_and_a_wheel_leaves_it() {
    let (_scratch, mut window) = with_effects(&[]);
    window.update(|_, tx| tx.focus("add-effect", false));
    redraw(&mut window);
    assert_eq!(super::focused(&window).as_deref(), Some("add-effect"));
    let shown = |window: &rune::Runtime<State>| {
        let [x, _, w, _] = window.presented("add-effect").unwrap().0;
        x >= 0.0 && x + w <= 1470.0
    };
    let folder = window.state.project().root().to_path_buf();
    let chain = folder.join("state/song/t00/chain");
    let mut order = Vec::new();
    for index in 0..10 {
        let name = format!("u{index}");
        let record = chain.join(format!("{name}.json"));
        std::fs::write(&record, r#"{"tool": "utility", "state": {}}"#).unwrap();
        order.push(name);
        let instance = serde_json::json!({ "tool": "chain", "state": { "order": order } });
        std::fs::write(chain.join("instance.json"), instance.to_string()).unwrap();
        let changed = [record, chain.join("instance.json")];
        (window.state.session)
            .background(|project| project.apply_outside_changes(&changed).unwrap());
        for _ in 0..20 {
            redraw(&mut window);
        }
        assert_eq!(
            super::focused(&window).as_deref(),
            Some("add-effect"),
            "after {}",
            index + 1
        );
        assert!(shown(&window), "after {}", index + 1);
    }
    let (x, y) = point(&window, "add-effect", 0.5);
    window.wheel_xy(x, y, 2000.0, 0.0);
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert!(!shown(&window), "the wheel scrolled it away");
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert!(!shown(&window), "it stays away");
}
