//! The sound follows the hand: a knob of the kit plays its number while it drags, and the
//! release is one step of undo, named by the code.

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::{act, automated_cutoff, frame, key, laid_out, new_project, opened, redraw};
use crate::app::State;

const KNOB: &str = "t00/synth:cutoff_hz";

fn synth() -> InstanceId {
    InstanceId::new("song/t00/synth").unwrap()
}

/// The cutoff as the engine plays it: the record as published, saved or not.
fn played(state: &State) -> f64 {
    let record: Value =
        serde_json::from_str(&state.project().state_json(&synth()).unwrap()).unwrap();
    record["cutoff_hz"].as_f64().unwrap()
}

/// The cutoff in the file of the record.
fn saved(state: &State) -> Option<f64> {
    let file = state.project().root().join("state/song/t00/synth.json");
    let record: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    record["state"]["cutoff_hz"].as_f64()
}

/// The points of the lane of the cutoff, as (tick, value).
fn lane(state: &State) -> Vec<(u64, f64)> {
    let id = InstanceId::new("song/t00/lane-held").unwrap();
    let record: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    (record["points"].as_array().unwrap().iter())
        .map(|point| {
            (
                point["tick"].as_u64().unwrap(),
                point["value"].as_f64().unwrap(),
            )
        })
        .collect()
}

fn view_runs(state: &State) -> u64 {
    state.tools.as_ref().unwrap().view_runs()
}

/// A frame `seconds` after the last, as the app runs it: Bun, the session, the engine.
fn wait(window: &mut rune::Runtime<State>, seconds: f64) {
    let at = window.state.last + seconds;
    frame(&mut window.state, at);
    window.invalidate();
    window.frame(seconds);
}

/// Grabs the knob and turns it up `steps` times 4 points, a frame of `seconds` after each move.
fn grab_and_turn(window: &mut rune::Runtime<State>, steps: usize, seconds: f64) -> (f32, f32) {
    let (x, mut y) = window.find(KNOB).unwrap_or_else(|| panic!("no {KNOB}"));
    window.pointer_down(x, y);
    wait(window, seconds);
    for _ in 0..steps {
        y -= 4.0;
        window.pointer_move(x, y);
        wait(window, seconds);
    }
    (x, y)
}

/// Lets go, and gives the code a frame to answer the release.
fn let_go(window: &mut rune::Runtime<State>, (x, y): (f32, f32)) {
    window.pointer_up(x, y);
    window.frame(1.0 / 60.0);
    redraw(window);
}

/// Write automation on, and the piece playing for a second.
fn writing(window: &mut rune::Runtime<State>) {
    window.state.session.set_writes_automation(true);
    (window.state.session).background(|project| project.engine().play());
    wait(window, 1.0);
}

/// While the knob drags, the engine plays what the hand turned and the file keeps its value; no
/// view runs in Bun. The release is one undo step, named by the code's update, which undo takes
/// back whole.
#[test]
fn a_knob_plays_as_it_drags_and_its_release_is_one_step_named_by_the_code() {
    let scratch = new_project();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    let before = (played(&window.state), window.state.project().undo_label());
    let (file, runs) = (saved(&window.state), view_runs(&window.state));
    let at = grab_and_turn(&mut window, 10, 1.0 / 60.0);
    let turned = played(&window.state);
    assert!(turned > before.0, "{turned}");
    assert_eq!(saved(&window.state), file);
    assert_eq!(view_runs(&window.state), runs);
    assert_eq!(window.state.project().undo_label(), before.1);

    let_go(&mut window, at);
    // The code's number, which the hand's was, to the precision of a live value.
    let kept = saved(&window.state).unwrap();
    assert!((kept - turned).abs() < 0.01, "{kept} {turned}");
    assert_eq!(played(&window.state), kept);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Change Cutoff"));
    window.state.undo().unwrap();
    assert_eq!(
        (played(&window.state), window.state.project().undo_label()),
        before
    );
}

#[test]
fn escape_during_a_drag_puts_the_sound_back() {
    let scratch = new_project();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    let before = (played(&window.state), window.state.project().undo_label());
    let at = grab_and_turn(&mut window, 10, 1.0 / 60.0);
    assert_ne!(played(&window.state), before.0);
    key(&mut window, rune::Key::Escape);
    let_go(&mut window, at);
    assert_eq!(
        (played(&window.state), window.state.project().undo_label()),
        before
    );
}

/// A knob a lane plays refuses the drag in the session's words, and its release asks the code
/// for nothing.
#[test]
fn a_knob_a_lane_holds_refuses_its_drag() {
    let scratch = automated_cutoff();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    let label = window.state.project().undo_label();
    let at = grab_and_turn(&mut window, 10, 1.0 / 60.0);
    let_go(&mut window, at);
    let said = window
        .state
        .session
        .notice()
        .unwrap_or_default()
        .to_string();
    assert!(said.contains("turn on Write automation"), "{said}");
    assert_eq!(played(&window.state), 500.0);
    assert_eq!(window.state.project().undo_label(), label);
}

/// While automation is written and the piece plays, a drag of the held cutoff writes what the
/// hand plays into the lane from where it was grabbed to where it was let go: one undo step
/// "Write automation", which keeps the record's cutoff as it was.
#[test]
fn a_drag_of_a_held_knob_while_it_plays_writes_into_its_lane() {
    let scratch = automated_cutoff();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    writing(&mut window);
    let before = (lane(&window.state), window.state.project().undo_label());
    let grabbed = window.state.session.status().playhead_tick.0;
    let at = grab_and_turn(&mut window, 10, 0.1);
    let_go(&mut window, at);
    let released = window.state.session.status().playhead_tick.0;

    let written = lane(&window.state);
    assert_eq!(written.first(), Some(&(0, 8_000.0)), "{written:?}");
    let hand: Vec<_> = (written.iter())
        .filter(|(_, value)| *value != 8_000.0)
        .collect();
    assert!(hand.len() > 1, "{written:?}");
    // The engine says where it played a frame late.
    let frame = 240;
    assert!(
        (hand.iter()).all(|(tick, _)| (grabbed..=released + frame).contains(tick)),
        "{written:?} from {grabbed} to {released}"
    );
    assert_eq!(played(&window.state), 500.0);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Write automation"));
    window.state.undo().unwrap();
    assert_eq!(
        (lane(&window.state), window.state.project().undo_label()),
        before
    );
}

/// Escape during a drag that writes automation leaves the lane, the record and the history as
/// they were.
#[test]
fn escape_during_a_drag_that_writes_automation_leaves_the_lane_as_it_was() {
    let scratch = automated_cutoff();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    writing(&mut window);
    let before = (lane(&window.state), window.state.project().undo_label());
    let at = grab_and_turn(&mut window, 10, 0.1);
    key(&mut window, rune::Key::Escape);
    let_go(&mut window, at);
    assert_eq!(
        (lane(&window.state), window.state.project().undo_label()),
        before
    );
    assert_eq!(played(&window.state), 500.0);
}

/// A value typed on the held cutoff while automation is written and the piece plays lands in
/// the lane where it plays, as one step "Write automation"; the record keeps its cutoff.
#[test]
fn a_value_typed_on_a_held_knob_while_it_plays_lands_in_its_lane() {
    let scratch = automated_cutoff();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    writing(&mut window);
    let playhead = window.state.session.status().playhead_tick.0;
    act(
        &mut window.state,
        json!({ "label": "Synth Cutoff", "action": "set", "value": 1000 }),
    );
    let typed: Vec<_> = (lane(&window.state).into_iter())
        .filter(|(_, value)| *value == 1_000.0)
        .collect();
    assert!(!typed.is_empty(), "{:?}", lane(&window.state));
    assert!(
        typed
            .iter()
            .all(|(tick, _)| tick.abs_diff(playhead) <= 2_000),
        "{typed:?} at {playhead}"
    );
    assert_eq!(played(&window.state), 500.0);
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Write automation"));
}
