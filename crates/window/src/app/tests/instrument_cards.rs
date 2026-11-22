//! The template's cards of the drum pad, the sampler and the wavetable, driven as a composer
//! does: a file dropped from the system, a drag on a drawing that plays before release.

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::host::wav;
use super::{act, frame, laid_out, new_project, opened, redraw};
use crate::app::State;

/// A new song whose first track plays `record`, shown in its panel.
fn with_instrument(record: Value) -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(folder.join("state/song/t00/synth.json"), record.to_string()).unwrap();
    (scratch, laid_out(opened(&folder)))
}

/// The instrument's record as the engine plays it, saved or not.
fn played(state: &State) -> Value {
    let id = InstanceId::new("song/t00/synth").unwrap();
    serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap()
}

fn centre(window: &rune::Runtime<State>, key: &str) -> (f32, f32) {
    let ([x, y, w, h], _) = window
        .presented(&format!("t00/synth:{key}"))
        .unwrap_or_else(|| panic!("no {key}"));
    (x + w / 2.0, y + h / 2.0)
}

/// A frame `seconds` after the last, as the app runs it: Bun, the session, the engine.
fn wait(window: &mut rune::Runtime<State>, seconds: f64) {
    let at = window.state.last + seconds;
    frame(&mut window.state, at);
    window.invalidate();
    window.frame(seconds);
}

fn dropped(window: &mut rune::Runtime<State>, key: &str) -> tempfile::TempDir {
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("Clap.wav");
    wav(&file);
    let at = centre(window, key);
    window.drop_files(vec![file], Some(at));
    window.frame(1.0 / 60.0);
    for _ in 0..3 {
        redraw(window);
    }
    outside
}

#[test]
fn a_file_dropped_on_a_drum_pad_makes_that_pad_play_it_as_one_undo_step() {
    let (_scratch, mut window) = with_instrument(json!({ "tool": "drum-pad", "state": {} }));
    let _file = dropped(&mut window, "pad-39");
    assert_eq!(played(&window.state)["pads"]["39"]["sample"], "clap.wav");
    // Half again as long as the file of 0.1 s, so its fade takes almost nothing off its end.
    assert_eq!(
        played(&window.state)["pads"]["39"]["decay_ms"],
        json!(150.0)
    );
    assert_eq!(played(&window.state)["pads"]["38"]["sound"], Value::Null);
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(played(&window.state)["pads"], json!({}));
}

#[test]
fn a_file_dropped_on_the_sampler_becomes_its_sample_from_start_to_end() {
    let record = json!({ "tool": "sampler", "state": { "sample": "gone.wav", "start_seconds": 0.2, "end_seconds": 0.5 } });
    let (_scratch, mut window) = with_instrument(record);
    let _file = dropped(&mut window, "sampler-display");
    let state = played(&window.state);
    assert_eq!(
        (
            &state["sample"],
            &state["start_seconds"],
            &state["end_seconds"]
        ),
        (&json!("clap.wav"), &json!(0.0), &Value::Null)
    );
}

/// The table's drawing names the position as the window reads a number of the record's own
/// group, so the sound follows the hand before it lets go, and the release is one step.
#[test]
fn a_drag_up_the_wavetable_plays_its_position_before_release_as_one_step() {
    let (_scratch, mut window) = with_instrument(json!({ "tool": "wavetable", "state": {} }));
    let (x, y) = centre(&window, "osc_1.position-area");
    window.pointer_down(x, y);
    wait(&mut window, 1.0 / 60.0);
    for step in 1..=4 {
        window.pointer_move(x, y - 4.0 * step as f32);
        wait(&mut window, 1.0 / 60.0);
    }
    let held = played(&window.state)["osc_1"]["position"].as_f64().unwrap();
    assert!(held > 0.55, "the position plays {held} while it is held");
    window.pointer_up(x, y - 16.0);
    for _ in 0..3 {
        redraw(&mut window);
    }
    let let_go = played(&window.state)["osc_1"]["position"].as_f64().unwrap();
    assert!((let_go - held).abs() < 0.02, "{held} held, {let_go} let go");
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(played(&window.state)["osc_1"]["position"], json!(0.5));
}

/// The loudest the feed `name` of the track's instrument read in the last frame.
fn level(state: &State, name: &str) -> f64 {
    let read = state.feeds.read_now();
    (read
        .get(&format!("song/t00/synth {name}"))
        .and_then(Value::as_f64))
    .unwrap_or(0.0)
}

#[test]
fn a_press_on_a_drum_pad_plays_that_pad() {
    let (_scratch, mut window) = with_instrument(json!({ "tool": "drum-pad", "state": {} }));
    let (x, y) = centre(&window, "pad-38");
    window.pointer_down(x, y);
    window.pointer_up(x, y);
    let mut loudest = [0.0f64; 2];
    for _ in 0..20 {
        wait(&mut window, 1.0 / 60.0);
        for (pad, kept) in loudest.iter_mut().enumerate() {
            *kept = kept.max(level(&window.state, &format!("levels.{}", pad + 1)));
        }
    }
    assert!(loudest[1] > 0.1, "pad 2 played {}", loudest[1]);
    assert_eq!(loudest[0], 0.0, "pad 1 played too");
}

/// The library's Download reaches the sampler from its card: here the files are not there to
/// fetch, so the download fails and the card offers it again.
#[test]
fn the_samplers_download_button_starts_its_download() {
    let library = tempfile::tempdir().unwrap();
    sampler::library::set_folder(library.path().join("library"));
    sampler::library::set_source(&format!(
        "file://{}",
        library.path().join("nothing").display()
    ));
    let record = json!({ "tool": "sampler", "state": { "library": "piano/kawai-upright" } });
    let (_scratch, mut window) = with_instrument(record);
    assert!(window.find("t00/synth:download").is_some(), "no Download");
    let drawn = |window: &rune::Runtime<State>| {
        let texts = window
            .elements()
            .into_iter()
            .filter_map(|element| element.text);
        texts.collect::<Vec<_>>().join(" ")
    };
    act(
        &mut window.state,
        json!({ "label": "Sampler download", "action": "press" }),
    );
    for _ in 0..200 {
        wait(&mut window, 1.0 / 60.0);
        if drawn(&window).contains("Try again") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        drawn(&window).contains("The download of"),
        "{}",
        drawn(&window)
    );
    assert!(drawn(&window).contains("Try again"));
}

/// A number of a group of the record, `osc_1.position`, shows what the record holds on its
/// knob: no child is named `osc_1`.
#[test]
fn a_knob_of_a_number_in_a_group_shows_its_record() {
    let record = json!({ "tool": "wavetable", "state": { "osc_1": { "position": 0.35 } } });
    let (_scratch, mut window) = with_instrument(record);
    wait(&mut window, 1.0 / 60.0);
    let texts: Vec<String> = (window.state.lives.texts().into_iter())
        .map(|(_, text)| text)
        .collect();
    assert!(texts.iter().any(|text| text == "35%"), "{texts:?}");
}
