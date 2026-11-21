//! The mixer of the template through the real pointer and keys: a fader's drag plays as it goes
//! and is one step of undo, writes its lane while automation is written, and takes a typed -inf.

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::{act, frame, key, laid_out, lane, new_project, opened, redraw};
use crate::app::State;

const FADER: &str = "fader-t00";

/// The volume of the first track as the engine plays it, saved or not.
fn played(state: &State) -> Value {
    let id = InstanceId::new("song/t00/mix").unwrap();
    let record: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    record["gain_db"].clone()
}

/// The volume of the first track in its file.
fn saved(state: &State) -> Value {
    let file = state.project().root().join("state/song/t00/mix.json");
    let record: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    record["state"]["gain_db"].clone()
}

/// The points of the lane of the volume, as (tick, value).
fn points(state: &State) -> Vec<(u64, f64)> {
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

/// The project in `scratch` with its mixer shown, in a window.
fn mixer(scratch: &tempfile::TempDir) -> rune::Runtime<State> {
    let mut state = opened(&scratch.path().join("song"));
    act(&mut state, json!({ "action": "song.mixer" }));
    let mut window = laid_out(state);
    redraw(&mut window);
    window
}

/// A frame `seconds` after the last, as the app runs it.
fn wait(window: &mut rune::Runtime<State>, seconds: f64) {
    let at = window.state.last + seconds;
    frame(&mut window.state, at);
    window.invalidate();
    window.frame(seconds);
}

/// Grabs the fader on its left level bar, pulls it down 6 points `steps` times, a frame of
/// `seconds` after each, and lets go.
fn pull_down(
    window: &mut rune::Runtime<State>,
    steps: usize,
    seconds: f64,
    during: impl Fn(&State),
) {
    let (middle, mut y) = window.find(FADER).unwrap_or_else(|| panic!("no {FADER}"));
    let x = middle - 4.0;
    window.pointer_down(x, y);
    wait(window, seconds);
    for _ in 0..steps {
        y += 6.0;
        window.pointer_move(x, y);
        wait(window, seconds);
    }
    during(&window.state);
    window.pointer_up(x, y);
    window.frame(1.0 / 60.0);
    redraw(window);
}

/// While the fader drags the engine plays what the hand pulled and the file keeps 0 dB; no view
/// runs. The release keeps what played, as one step "Change the volume", which undo takes back.
#[test]
fn a_mixer_fader_plays_as_it_drags_and_is_one_step_of_undo() {
    let scratch = new_project();
    let mut window = mixer(&scratch);
    let label = window.state.project().undo_label();
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    pull_down(&mut window, 8, 1.0 / 60.0, |state| {
        let hand = played(state).as_f64().unwrap();
        assert!(hand < -3.0, "{hand}");
        assert_eq!(
            saved(state),
            Value::Null,
            "the file says nothing of the volume yet"
        );
        assert_eq!(state.tools.as_ref().unwrap().view_runs(), runs);
    });
    let kept = saved(&window.state).as_f64().unwrap();
    assert!(kept < -3.0, "{kept}");
    assert_eq!(played(&window.state).as_f64(), Some(kept));
    let changed = window.state.project().undo_label();
    assert_eq!(changed.as_deref(), Some("Change the volume"));
    window.state.undo().unwrap();
    assert_eq!(
        (saved(&window.state), played(&window.state)),
        (Value::Null, Value::Null)
    );
    assert_eq!(window.state.project().undo_label(), label);
}

/// With Write automation on while the piece plays, a drag of the fader a lane holds writes what
/// the hand plays into the lane, one step "Write automation", and the record keeps its volume.
#[test]
fn a_mixer_fader_writes_its_volume_lane_while_it_plays() {
    let scratch = new_project();
    lane(&scratch, "mix.gain_db", -6.0);
    let mut window = mixer(&scratch);
    window.state.session.set_writes_automation(true);
    (window.state.session).background(|project| project.engine().play());
    wait(&mut window, 1.0);
    pull_down(&mut window, 8, 0.1, |_| {});
    let written = points(&window.state);
    assert!(
        written.iter().filter(|(_, value)| *value != -6.0).count() > 1,
        "{written:?}"
    );
    assert!(
        written.iter().any(|(_, value)| *value < -6.5),
        "{written:?}"
    );
    assert_eq!(
        saved(&window.state),
        Value::Null,
        "the record keeps its volume"
    );
    let label = window.state.project().undo_label();
    assert_eq!(label.as_deref(), Some("Write automation"));
}

/// Grabbed while it writes, a fader a lane holds starts from what the lane plays, not from what
/// the record says.
#[test]
fn a_held_fader_grabbed_while_it_writes_starts_from_what_plays() {
    let scratch = new_project();
    lane(&scratch, "mix.gain_db", -20.0);
    let mut window = mixer(&scratch);
    window.state.session.set_writes_automation(true);
    (window.state.session).background(|project| project.engine().play());
    wait(&mut window, 1.0);
    pull_down(&mut window, 2, 0.1, |_| {});
    let written: Vec<_> = (points(&window.state).into_iter())
        .filter(|(tick, _)| *tick > 0)
        .collect();
    assert!(!written.is_empty());
    assert!(
        written.iter().all(|(_, db)| *db <= -20.0 && *db > -30.0),
        "{written:?}"
    );
}

/// A click on the fader's number types into it, as in the old app: -inf and Enter silence the track.
#[test]
fn minus_inf_typed_at_a_faders_number_silences_its_track() {
    let scratch = new_project();
    let mix = json!({ "tool": "fader", "state": { "gain_db": -6 } });
    std::fs::write(
        scratch.path().join("song/state/song/t00/mix.json"),
        mix.to_string(),
    )
    .unwrap();
    let mut window = mixer(&scratch);
    let (x, y) = window.find(&format!("{FADER}-value")).unwrap();
    // At 0 dB the number shows under the pointer.
    window.pointer_move(x, y);
    redraw(&mut window);
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    window.pointer_up(x, y);
    redraw(&mut window);
    assert_eq!(window.state.open, [format!("{FADER}-value")]);
    for typed in ["-", "i", "n", "f"] {
        key(&mut window, rune::Key::Text(typed.into()));
    }
    key(&mut window, rune::Key::Enter);
    assert_eq!(saved(&window.state), json!("-inf"));
}
