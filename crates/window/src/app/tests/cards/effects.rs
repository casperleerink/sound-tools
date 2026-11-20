//! The cards of the effects that show what they do to the sound while it plays: the level a
//! compressor hears on its curve, how far a limiter turns down, and handles that play as they drag.

use serde_json::Value;

use super::{grab_and_move, let_go, played, wait, with_effect};
use crate::app::State;
use crate::app::tests::play;

/// The number the card of the effect `name` on the first track reads of its feed `feed` this frame.
fn feed(state: &State, name: &str, feed: &str) -> Option<f64> {
    let read = state.feeds.read_now();
    read.get(&format!("song/t00/chain/{name} {feed}"))
        .and_then(Value::as_f64)
}

#[test]
fn the_compressor_shows_the_level_it_hears_and_how_far_it_turns_down_while_it_plays() {
    let record = r#"{"tool": "compressor", "state": {"threshold_db": -40}}"#;
    let (_scratch, mut window) = with_effect("compressor", record);
    play(&mut window.state, 0.0, 1.0);
    let loudest = (0..30)
        .filter_map(|step| {
            play(&mut window.state, 1.0 + f64::from(step) / 60.0, 1.0 / 60.0);
            feed(&window.state, "compressor", "level").zip(feed(
                &window.state,
                "compressor",
                "reduction",
            ))
        })
        .fold((f64::MIN, 0.0_f64), |(level, reduction), (l, r)| {
            (level.max(l), reduction.max(r))
        });
    assert!(loudest.0 > -40.0, "{loudest:?}");
    assert!(loudest.1 > 1.0, "{loudest:?}");
    // The threshold's handle sits on the curve, under the threshold by what the knee turns down there.
    let at = feed(&window.state, "compressor", "threshold_level").unwrap();
    assert!(at < -40.0 && at > -42.0, "{at}");
}

#[test]
fn the_limiter_draws_what_it_sent_out_and_turns_down_while_it_plays() {
    let record = r#"{"tool": "limiter", "state": {"gain_db": 24, "ceiling_db": -12}}"#;
    let (_scratch, mut window) = with_effect("limiter", record);
    play(&mut window.state, 0.0, 1.0);
    let reduction = (0..30)
        .filter_map(|step| {
            play(&mut window.state, 1.0 + f64::from(step) / 60.0, 1.0 / 60.0);
            feed(&window.state, "limiter", "reduction")
        })
        .fold(0.0_f64, f64::max);
    assert!(reduction > 1.0, "{reduction}");
    let history = window.state.feeds.read_now();
    assert_eq!(
        history.get("song/t00/chain/limiter history"),
        Some(&serde_json::json!({ "points": 50 }))
    );
}

#[test]
fn the_compressors_threshold_plays_before_the_release_which_is_one_step() {
    let (_scratch, mut window) =
        with_effect("compressor", r#"{"tool": "compressor", "state": {}}"#);
    wait(&mut window, 1.0 / 60.0);
    let at = grab_and_move(
        &mut window,
        "t00/chain/compressor:threshold-point",
        (-40.0, 0.0),
        8,
    );
    let moving = played(&window.state, "compressor");
    assert!(moving["threshold_db"].as_f64().unwrap() < -25.0, "{moving}");
    let_go(&mut window, at);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Change threshold"));
}

#[test]
fn the_utilitys_handle_plays_gain_and_pan_while_it_drags_and_is_one_step() {
    let (_scratch, mut window) = with_effect("utility", r#"{"tool": "utility", "state": {}}"#);
    wait(&mut window, 1.0 / 60.0);
    let at = grab_and_move(&mut window, "t00/chain/utility:gain-pan", (30.0, -20.0), 6);
    let moving = played(&window.state, "utility");
    assert!(moving["pan"].as_f64().unwrap() > 0.2, "{moving}");
    assert!(moving["gain_db"].as_f64().unwrap() > 3.0, "{moving}");
    let_go(&mut window, at);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Change gain and pan"));
}
