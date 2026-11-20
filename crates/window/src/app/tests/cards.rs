//! The cards of built-in devices the template draws, driven by the pointer: a handle plays its
//! numbers while it drags, so the sound and the curve follow the hand, and the release is one
//! step of undo.

use serde_json::Value;
use sound_core::InstanceId;

use super::{frame, laid_out, new_project, opened, redraw};
use crate::app::State;

mod effects;

/// A new song whose first track has the effect `name`, of `record`, shown in its panel.
fn with_effect(name: &str, record: &str) -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let chain = folder.join("state/song/t00/chain");
    std::fs::write(chain.join(format!("{name}.json")), record).unwrap();
    let order = serde_json::json!({ "tool": "chain", "state": { "order": [name] } });
    std::fs::write(chain.join("instance.json"), order.to_string()).unwrap();
    (scratch, laid_out(opened(&folder)))
}

/// The record of the effect `name` as the engine plays it, saved or not.
fn played(state: &State, name: &str) -> Value {
    let id = InstanceId::new(&format!("song/t00/chain/{name}")).unwrap();
    serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap()
}

/// A frame `seconds` after the last, as the app runs it: Bun, the session, the engine.
fn wait(window: &mut rune::Runtime<State>, seconds: f64) {
    let at = window.state.last + seconds;
    frame(&mut window.state, at);
    window.invalidate();
    window.frame(seconds);
}

/// Presses the element `key` and moves the pointer `by` in `steps`, a frame after each: where it is.
fn grab_and_move(
    window: &mut rune::Runtime<State>,
    key: &str,
    by: (f32, f32),
    steps: usize,
) -> (f32, f32) {
    let (mut x, mut y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    window.pointer_down(x, y);
    wait(window, 1.0 / 60.0);
    for _ in 0..steps {
        (x, y) = (x + by.0 / steps as f32, y + by.1 / steps as f32);
        window.pointer_move(x, y);
        wait(window, 1.0 / 60.0);
    }
    (x, y)
}

fn let_go(window: &mut rune::Runtime<State>, (x, y): (f32, f32)) {
    window.pointer_up(x, y);
    window.frame(1.0 / 60.0);
    redraw(window);
    wait(window, 1.0 / 60.0);
}

#[test]
fn the_filters_curve_and_sound_follow_its_handle_before_the_release_which_is_one_step() {
    let (_scratch, mut window) = with_effect("filter", r#"{"tool": "filter", "state": {}}"#);
    wait(&mut window, 1.0 / 60.0);
    let drawn = window.state.feeds.drew();
    let at = grab_and_move(
        &mut window,
        "t00/chain/filter:cutoff-resonance",
        (40.0, -20.0),
        8,
    );
    let moving = played(&window.state, "filter");
    assert!(moving["cutoff_hz"].as_f64().unwrap() > 1500.0, "{moving}");
    assert!(moving["resonance"].as_f64().unwrap() > 0.3, "{moving}");
    assert!(
        window.state.feeds.drew() > drawn,
        "the response is drawn again while it drags"
    );
    let_go(&mut window, at);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Change cutoff and resonance"));
    window.state.undo().unwrap();
    let back = played(&window.state, "filter");
    assert_eq!(
        (back["cutoff_hz"].as_f64(), back["resonance"].as_f64()),
        (Some(1000.0), Some(0.2))
    );
}

#[test]
fn an_eq_handle_plays_its_bands_frequency_and_gain_while_it_drags() {
    let (_scratch, mut window) = with_effect("eq", r#"{"tool": "eq", "state": {}}"#);
    wait(&mut window, 1.0 / 60.0);
    let at = grab_and_move(&mut window, "t00/chain/eq:point-3", (30.0, -20.0), 6);
    let band = played(&window.state, "eq")["bands"][2].clone();
    assert!(band["frequency_hz"].as_f64().unwrap() > 2_000.0, "{band}");
    assert!(band["gain_db"].as_f64().unwrap() > 2.0, "{band}");
    let_go(&mut window, at);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Change frequency and gain"));
}
