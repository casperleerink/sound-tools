#![allow(clippy::unwrap_used)]

use std::path::Path;

use super::*;

/// The DAW of a new project, open as a run without a window opens it.
fn opened(folder: &Path) -> State {
    let mut state = State::open(Options {
        folder: folder.to_path_buf(),
        headless: true,
        play: None,
        act: None,
    })
    .unwrap();
    frame(&mut state, 0.0);
    state
}

fn new_project() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    runtime::templates::write(&scratch.path().join("song"), &TEMPLATE).unwrap();
    scratch
}

fn record(state: &State, path: &str) -> Value {
    let text = std::fs::read_to_string(state.project.root().join("state").join(path)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn control<'a>(controls: &'a [controls::Control], label: &str) -> &'a controls::Control {
    let found = controls.iter().find(|control| control.label == label);
    found.unwrap_or_else(|| {
        panic!(
            "no {label} in {:?}",
            controls
                .iter()
                .map(|control| &control.label)
                .collect::<Vec<_>>()
        )
    })
}

fn act(state: &mut State, step: Value) {
    serde_json::from_value::<Step>(step)
        .unwrap()
        .run(state)
        .unwrap();
    state.settle();
}

#[test]
fn playing_moves_the_playhead_and_the_meters_and_runs_no_view() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let runs = state.tools.as_ref().unwrap().view_runs();
    assert!(runs > 0);
    state.project.engine().play();
    for step in 1..=60 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert!(state.playhead.0 > 0);
    let levels = state.lives.levels();
    assert!(
        levels
            .iter()
            .any(|(name, level)| name == "meter:song/t01/mix" && *level > 0.0),
        "{levels:?}"
    );
    assert_eq!(state.tools.as_ref().unwrap().view_runs(), runs);
}

#[test]
fn an_action_on_mute_writes_the_record_and_undo_takes_it_back() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    act(
        &mut state,
        json!({ "label": "Mute Track 2", "action": "toggle" }),
    );
    assert_eq!(record(&state, "song/t01/mix.json")["state"]["mute"], true);
    assert_eq!(
        control(&controls::collect(&state).controls, "Mute Track 2").checked,
        Some(true)
    );
    act(&mut state, json!({ "action": "undo" }));
    assert_eq!(
        record(&state, "song/t01/mix.json")["state"].get("mute"),
        None
    );
    assert_eq!(
        control(&controls::collect(&state).controls, "Mute Track 2").checked,
        Some(false)
    );
}

#[test]
fn a_changed_tool_file_starts_a_new_bun_and_the_view_follows() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let song = state.project.root().join("extensions/song.tsx");
    let code = std::fs::read_to_string(&song).unwrap();
    std::fs::write(
        &song,
        code.replace("label={`Mute ${title}`}", "label={`Silence ${title}`}"),
    )
    .unwrap();
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    control(&controls::collect(&state).controls, "Silence Track 2");
}

/// A step sequencer an agent writes with a view, and an effect it writes without one.
const STEPS: &str = r#"import { boolean, input, list, tool } from "./sdk";
import { h, view } from "./sdk/view";

export const steps = tool({ name: "steps", title: "Steps", when: "Never", doc: "Sixteen steps.", state: { on: list(boolean({ default: false }), { max: 16 }) }, sound: () => input });

export const stepsView = view(steps, (self, { change }) => (
  <row gap={1}>
    {Array.from({ length: 16 }, (_, i) => {
      const on = self.state.on[i] ?? false;
      const flip = () => change((draft) => void (draft.state.on = Array.from({ length: 16 }, (_, j) => (j === i ? !on : (self.state.on[j] ?? false)))));
      return <box key={`step-${i}`} label={`Step ${i + 1}`} role="switch" checked={on} w={16} h={24} radius="sm" fill={on ? "text" : "fill"} onPress={flip} actions={{ toggle: flip }} />;
    })}
  </row>
));
"#;
const AMP: &str = r#"import { input, number, tool } from "./sdk";
export const amp = tool({ name: "amp", title: "Amp", when: "Never", doc: "A gain.", state: { gain: number({ min: 0, max: 1, default: 0.5 }) }, sound: ({ gain }) => input.times(gain) });
"#;

#[test]
fn a_tool_with_a_view_shows_in_the_panel_and_one_without_shows_its_fields() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(folder.join("extensions/steps.tsx"), STEPS).unwrap();
    std::fs::write(folder.join("extensions/amp.ts"), AMP).unwrap();
    let chain = folder.join("state/song/t00/chain");
    std::fs::write(
        chain.join("instance.json"),
        r#"{"tool": "chain", "state": {"order": ["steps", "amp"]}}"#,
    )
    .unwrap();
    std::fs::write(
        chain.join("steps.json"),
        r#"{"tool": "steps", "state": {}}"#,
    )
    .unwrap();
    std::fs::write(chain.join("amp.json"), r#"{"tool": "amp", "state": {}}"#).unwrap();
    let mut state = opened(&folder);
    let controls = controls::collect(&state).controls;
    assert_eq!(control(&controls, "Step 3").key, "t00/chain/steps:step-2");
    assert_eq!(control(&controls, "Amp Gain").key, "t00/chain/amp:gain");
    act(
        &mut state,
        json!({ "key": "t00/chain/steps:step-2", "action": "toggle" }),
    );
    assert_eq!(
        record(&state, "song/t00/chain/steps.json")["state"]["on"][2],
        true
    );
    act(
        &mut state,
        json!({ "label": "Amp Gain", "action": "set", "value": 0.25 }),
    );
    assert_eq!(
        record(&state, "song/t00/chain/amp.json")["state"]["gain"],
        0.25
    );
}

/// What the agent doc tells an agent to run in a new project: the type check, and its tests.
#[test]
fn a_new_project_type_checks_and_its_tests_pass() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    let bun = sound_typescript::bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let extensions = state.project.root().join("extensions");
    for check in [&["check", "-p", "."][..], &["test"]] {
        let ran = sound_core::process::background_command(&bun)
            .args(check)
            .current_dir(&extensions)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&ran.stdout).to_string()
            + &String::from_utf8_lossy(&ran.stderr);
        assert!(ran.status.success(), "bun {check:?}: {said}");
    }
}
