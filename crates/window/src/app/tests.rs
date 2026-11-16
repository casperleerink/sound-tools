#![allow(clippy::unwrap_used)]

use std::path::Path;

use serde::Deserialize;

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

/// Builds the window's view as Rune does, which binds what moves every frame.
fn build(state: &State) {
    let main = state.main().unwrap();
    if let Some(drawn) = state.drawn(main) {
        tree::view(state, main, "", drawn);
    }
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
    try_act(state, step).unwrap();
}

#[test]
fn playing_moves_the_playhead_and_the_meters_and_runs_no_view() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let runs = state.tools.as_ref().unwrap().view_runs();
    assert!(runs > 0);
    build(&state);
    state.project.engine().play();
    for step in 1..=60 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert!(state.playhead.0 > 0);
    let levels = state.lives.levels();
    assert!(
        levels
            .iter()
            .any(|(name, db)| name == "song/t01/mix" && *db > -60.0),
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
const STEPS: &str = r#"import { boolean, h, input, list, tool } from "./sdk";

export const steps = tool({
  name: "steps", title: "Steps", when: "Never", doc: "Sixteen steps.", state: { on: list(boolean({ default: false }), { max: 16 }) }, sound: () => input,
  view: (self, { change }) => (
    <row gap={1}>
      {Array.from({ length: 16 }, (_, i) => {
        const on = self.state.on[i] ?? false;
        const flip = () => change((draft) => void (draft.state.on = Array.from({ length: 16 }, (_, j) => (j === i ? !on : (self.state.on[j] ?? false)))));
        return <box key={`step-${i}`} label={`Step ${i + 1}`} role="switch" checked={on} w={16} h={24} radius="sm" fill={on ? "text" : "fill"} onPress={flip} actions={{ toggle: flip }} />;
      })}
    </row>
  ),
});
"#;
const AMP: &str = r#"import { input, number, tool } from "./sdk";
export const amp = tool({ name: "amp", title: "Amp", when: "Never", doc: "A gain.", state: { gain: number({ min: 0, max: 1, default: 0.5 }), stages: number({ min: 1, max: 8, integer: true, default: 1 }) }, sound: ({ gain }) => input.times(gain) });
"#;

/// A tool whose actions change twice, fail, or break its view, which a memo keeps a part of.
const COUNTER: &str = r#"import { boolean, h, input, memo, number, tool } from "./sdk";

export const counter = tool({
  name: "counter", title: "Counter", when: "Never", doc: "Counts.", state: { count: number({ min: 0, max: 100, integer: true, default: 0 }), broken: boolean({ default: false }) }, sound: () => input,
  view: (self, { change }) => {
  const pad = memo("pad", [], () => <box key="break" label="Break" onPress={() => change((draft) => void (draft.state.broken = true))} />);
  if (self.state.broken) throw new Error("broken");
  const twice = () => {
    change((draft) => void (draft.state.count += 1));
    change((draft) => void (draft.state.count += 1));
  };
  const set = (count: number) => change((draft) => void (draft.state.count = count));
  return (
    <row>
      {pad}
      <box key="count" label="Count" role="slider" value={self.state.count} actions={{ set }} />
      <box key="twice" label="Twice" onPress={twice} />
      <box key="fail" label="Fail" onPress={() => { throw new Error("it broke"); }} />
    </row>
  );
  },
});
"#;

/// Sixteen steps whose control loop follows where the piece plays.
const SEQUENCE: &str = r#"import { h, input, number, tool } from "./sdk";

export const sequence = tool({
  name: "sequence", title: "Sequence", when: "Never", doc: "Sixteen steps.",
  state: { at: number({ min: 0, max: 15, integer: true, default: 0 }) },
  sound: () => input,
  tick: ({ clock, state, update }) => {
    const at = Math.floor(clock.tick / 240) % 16;
    if (at !== state.at) update("Step", (part) => void (part.state.at = at));
  },
  view: (self) => (
    <row gap={1}>
      {Array.from({ length: 16 }, (_, i) => <box key={`step-${i}`} label={`Step ${i + 1}`} role="switch" checked={i === self.state.at} w={16} h={24} fill={i === self.state.at ? "text" : "fill"} onPress={() => {}} />)}
    </row>
  ),
});
"#;

/// A new project whose first track has `tools` of the project as its effects, by name and code.
fn with_tools(tools: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let chain = folder.join("state/song/t00/chain");
    for (name, code) in tools {
        std::fs::write(folder.join(format!("extensions/{name}.tsx")), code).unwrap();
        std::fs::write(
            chain.join(format!("{name}.json")),
            json!({ "tool": name, "state": {} }).to_string(),
        )
        .unwrap();
    }
    let order: Vec<&str> = tools.iter().map(|(name, _)| *name).collect();
    std::fs::write(
        chain.join("instance.json"),
        json!({ "tool": "chain", "state": { "order": order } }).to_string(),
    )
    .unwrap();
    (scratch, folder)
}

fn try_act(state: &mut State, step: Value) -> Result<()> {
    serde_json::from_value::<Step>(step).unwrap().run(state)
}

fn labels(state: &State) -> Vec<String> {
    let controls = controls::collect(state).controls;
    controls.into_iter().map(|control| control.label).collect()
}

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

#[test]
fn a_run_without_a_window_opens_a_folder_named_by_dots() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let track = folder.join("state/song/t01/instance.json");
    let code = std::fs::read_to_string(&track).unwrap();
    std::fs::write(&track, code.replace("Track 2", "Bass")).unwrap();
    let state = opened(&folder.join("state").join(".."));
    control(&controls::collect(&state).controls, "Mute Bass");
}

#[test]
fn a_new_main_in_the_project_file_shows_at_once() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let file = state.project.root().join(runtime::PROJECT_FILE);
    let code = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        code.replace(r#""main": "song""#, r#""main": "song/t00/synth""#),
    )
    .unwrap();
    state.project.apply_outside_changes(&[file]).unwrap();
    frame(&mut state, 0.1);
    assert!(
        !labels(&state).contains(&"Mute Track 2".to_string()),
        "{:?}",
        labels(&state)
    );
}

#[test]
fn an_action_that_changes_twice_is_one_step_of_undo() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    act(&mut state, json!({ "label": "Twice", "action": "press" }));
    assert_eq!(
        record(&state, "song/t00/chain/counter.json")["state"]["count"],
        2
    );
    act(&mut state, json!({ "action": "undo" }));
    assert_eq!(
        record(&state, "song/t00/chain/counter.json")["state"].get("count"),
        None
    );
}

#[test]
fn a_view_that_failed_builds_its_memos_again_once_it_works() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    let problems = state.project.problems().len();
    act(&mut state, json!({ "label": "Break", "action": "press" }));
    assert!(!labels(&state).contains(&"Twice".to_string()));
    assert_eq!(state.project.problems().len(), problems + 1);
    act(&mut state, json!({ "action": "undo" }));
    control(&controls::collect(&state).controls, "Break");
    assert_eq!(state.project.problems().len(), problems);
}

#[test]
fn an_action_that_fails_or_lacks_its_number_fails_the_steps() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    let failed = try_act(&mut state, json!({ "label": "Fail", "action": "press" })).unwrap_err();
    assert!(failed.to_string().contains("it broke"), "{failed}");
    let unset = try_act(
        &mut state,
        json!({ "label": "Volume Track 1", "action": "set" }),
    )
    .unwrap_err();
    assert!(unset.to_string().contains("set takes a number"), "{unset}");
    assert_eq!(
        record(&state, "song/t00/mix.json")["state"].get("gain_db"),
        None
    );
}

#[test]
fn while_the_code_starts_again_the_view_stays_and_an_action_says_it_did_nothing() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let tools = state.tools.as_mut().unwrap();
    tools.code_changed();
    tools.poll(&mut state.project, &[]);
    let refused = try_act(
        &mut state,
        json!({ "label": "Mute Track 2", "action": "toggle" }),
    );
    assert!(refused.unwrap_err().to_string().contains("did nothing"));
    control(&controls::collect(&state).controls, "Mute Track 2");
}

#[test]
fn a_press_in_a_run_without_a_window_lands_in_its_frame() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let song = InstanceId::new("song").unwrap();
    state.pointed(&song, "mute-t01", "press", None);
    assert_eq!(record(&state, "song/t01/mix.json")["state"]["mute"], true);
}

#[test]
fn a_card_sets_a_whole_number_field_to_a_whole_number() {
    let (_scratch, folder) = with_tools(&[("amp", AMP)]);
    let mut state = opened(&folder);
    act(
        &mut state,
        json!({ "label": "Amp Stages", "action": "set", "value": 2.6 }),
    );
    assert_eq!(
        record(&state, "song/t00/chain/amp.json")["state"]["stages"],
        3
    );
}

#[test]
fn a_control_loop_steps_while_the_piece_plays() {
    let (_scratch, folder) = with_tools(&[("sequence", SEQUENCE)]);
    let mut state = opened(&folder);
    play(&mut state, 0.0, 1.0);
    // A control loop's edits are written once it is quiet: the project holds them at once.
    let id = InstanceId::new("song/t00/chain/sequence").unwrap();
    let held: Value = serde_json::from_str(&state.project.state_json(&id).unwrap()).unwrap();
    assert!(held["at"].as_u64().is_some_and(|at| at > 0), "{held}");
    let lit = controls::collect(&state)
        .controls
        .into_iter()
        .find(|control| control.checked == Some(true) && control.label.starts_with("Step"));
    assert_ne!(lit.map(|control| control.label), Some("Step 1".to_string()));
}

#[test]
fn a_changed_theme_shows_once_the_code_starts_again() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let file = state.project.root().join("extensions/theme.json");
    let code = std::fs::read_to_string(&file).unwrap();
    let mut theme: Value = serde_json::from_str(&code).unwrap();
    theme["color"]["background"] = json!("#123456");
    std::fs::write(&file, theme.to_string()).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    frame(&mut state, 0.2);
    assert_eq!(
        state.theme.color("background"),
        rune::color::hex("#123456").unwrap()
    );
}

/// Plays `seconds` from `from`, a frame each 60th of a second.
fn play(state: &mut State, from: f64, seconds: f64) {
    build(state);
    state.project.engine().play();
    for step in 1..=(seconds * 60.0) as u32 {
        frame(state, from + f64::from(step) / 60.0);
    }
}

#[test]
fn the_meter_of_a_track_taken_away_and_brought_back_moves() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    play(&mut state, 0.0, 0.5);
    let root = state.project.root().to_path_buf();
    let (track, kept) = (root.join("state/song/t01"), scratch.path().join("kept"));
    copy_folder(&track, &kept).unwrap();
    let files = |folder: &Path| -> Vec<std::path::PathBuf> {
        ["", "chain/"]
            .into_iter()
            .flat_map(|inside| std::fs::read_dir(folder.join(inside)).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| track.join(path.strip_prefix(folder).unwrap()))
            .collect()
    };
    let paths = files(&kept);
    std::fs::remove_dir_all(&track).unwrap();
    state.project.apply_outside_changes(&paths).unwrap();
    play(&mut state, 0.5, 0.5);
    copy_folder(&kept, &track).unwrap();
    state.project.apply_outside_changes(&paths).unwrap();
    play(&mut state, 1.0, 1.0);
    let levels = state.lives.levels();
    assert!(
        levels
            .iter()
            .any(|(name, db)| name == "song/t01/mix" && *db > -60.0),
        "{levels:?}"
    );
    let names: Vec<&String> = levels.iter().map(|(name, _)| name).collect();
    assert!(names.is_sorted(), "{names:?}");
}

/// What `sdk/view.ts` sends, read by the window with no field left over.
#[test]
fn every_node_the_template_draws_is_one_the_window_reads() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    let mut read = 0;
    for id in &state.shown {
        let Some(Drawn::View { root, memos }) = state.drawn(id) else {
            continue;
        };
        for node in std::iter::once(root).chain(memos.values().map(|(_, node)| node)) {
            if let Err(error) = tree::Node::deserialize(&**node) {
                panic!("{id}: {error}: {node}");
            }
            read += 1;
        }
    }
    assert!(read > 1);
}

/// What the test helpers of `sdk/view.ts` do, as the window does it.
const VIEW_CHECKS: &str = r#"import { expect, test } from "bun:test";
import { find, h, input, number, press, render, tool } from "./sdk";

const counter = tool({
  name: "counter", title: "Counter", when: "Never", doc: "Counts.", state: { count: number({ min: 0, max: 100, default: 0 }) }, sound: () => input,
  view: (self, { change }) => (
    <row>
      <text key="said">{self.state.count > 5 && "Many "}{self.state.count}</text>
      <box key="twice" label="Twice" onPress={() => change((draft) => { draft.state.count++; draft.state.count++; })} />
    </row>
  ),
});
const self = { name: "counter", state: { count: 0 }, children: [] } as never;

test("two writes to one field in one change add up", () => {
  render(counter, self);
  expect(press("twice")).toEqual([[["state", "count"], 2]]);
});

test("a text leaves out what is false", () => {
  render(counter, self);
  expect(find("said")).toMatchObject({ text: "0" });
});
"#;

#[test]
fn the_test_helpers_change_and_draw_as_the_window_does() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    let extensions = state.project.root().join("extensions");
    std::fs::write(extensions.join("checks.test.tsx"), VIEW_CHECKS).unwrap();
    let bun = sound_typescript::bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let ran = sound_core::process::background_command(&bun)
        .args(["test", "checks.test.tsx"])
        .current_dir(&extensions)
        .output()
        .unwrap();
    let said =
        String::from_utf8_lossy(&ran.stdout).to_string() + &String::from_utf8_lossy(&ran.stderr);
    assert!(ran.status.success(), "{said}");
}

#[test]
fn a_step_after_undo_acts_on_the_view_undo_left() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let mute = json!({ "label": "Mute Track 2", "action": "toggle" });
    for step in [mute.clone(), json!({ "action": "undo" }), mute] {
        try_act(&mut state, step).unwrap();
    }
    assert_eq!(record(&state, "song/t01/mix.json")["state"]["mute"], true);
}

#[test]
fn an_action_whose_change_is_refused_fails_the_steps() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    let refused = try_act(
        &mut state,
        json!({ "label": "Count", "action": "set", "value": 500 }),
    );
    assert!(refused.is_err());
}

#[test]
fn what_an_action_changed_lands_when_the_code_starts_again_meanwhile() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let song = InstanceId::new("song").unwrap();
    state.act_on_view(&song, "mute-t01", "press", None).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    state.settle();
    assert_eq!(record(&state, "song/t01/mix.json")["state"]["mute"], true);
}

/// A control loop that plays its own input, and stops the piece a beat in.
const BEEP: &str = r#"import { number, sine, tool } from "./sdk";
export const beep = tool({
  name: "beep", title: "Beep", when: "Never", doc: "A beep.", state: {},
  inputs: { gain: number({ min: 0, max: 1, default: 0 }) },
  sound: ({ gain }) => sine(440).times(gain),
  tick: ({ clock, send }) => {
    send("gain", 1);
    if (clock.playing && clock.tick >= 960) clock.stop();
  },
});
"#;

#[test]
fn a_control_loop_plays_its_input_and_stops_the_piece() {
    let (_scratch, folder) = with_tools(&[("beep", BEEP)]);
    let mut state = opened(&folder);
    play(&mut state, 0.0, 1.0);
    assert!(!state.playing);
    let levels = state.lives.levels();
    assert!(
        levels
            .iter()
            .any(|(name, db)| name == "song/t00/mix" && *db > -60.0),
        "{levels:?}"
    );
}

/// A control loop slower than its interval, which adds up the time it was told passed.
const SLOW: &str = r#"import { number, tool } from "./sdk";
export const slow = tool({
  name: "slow", title: "Slow", when: "Never", doc: "Slow.",
  state: { total: number({ min: 0, max: 1000, default: 0 }) },
  memory: () => ({ total: 0 }),
  tick: ({ dt, memory, update }) => {
    const end = performance.now() + 50;
    while (performance.now() < end);
    memory.total += dt;
    update("Total", (part) => void (part.state.total = memory.total));
  },
});
"#;

#[test]
fn a_control_loop_slower_than_its_interval_is_told_all_the_time_that_passed() {
    let (_scratch, folder) = with_tools(&[("slow", SLOW)]);
    let mut state = opened(&folder);
    // The first step keeps Bun busy through the others.
    for step in 1..=10 {
        run_loops(&mut state, f64::from(step) * 0.04);
    }
    state.settle();
    assert!(run_loops(&mut state, 0.5));
    state.settle();
    let id = InstanceId::new("song/t00/chain/slow").unwrap();
    let held: Value = serde_json::from_str(&state.project.state_json(&id).unwrap()).unwrap();
    let total = held["total"].as_f64().unwrap();
    assert!((total - 0.5).abs() < 1e-4, "{total}");
}

#[test]
fn a_view_that_failed_says_so_no_more_once_no_code_runs() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let file = folder.join(runtime::PROJECT_FILE);
    let code = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        code.replace(r#""main": "song""#, r#""main": "song/t00/chain/counter""#),
    )
    .unwrap();
    let mut state = opened(&folder);
    act(&mut state, json!({ "label": "Break", "action": "press" }));
    let failed = |state: &State| {
        (state.project.problems().iter()).any(|problem| problem.message.contains("its view failed"))
    };
    assert!(failed(&state));
    for entry in std::fs::read_dir(state.project.root().join("extensions")).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "tsx" || extension == "ts")
            && path.file_name().is_some_and(|name| name != "sdk.ts")
        {
            std::fs::remove_file(path).unwrap();
        }
    }
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    assert!(!failed(&state), "{:?}", state.project.problems());
}

#[test]
fn a_press_that_fails_tells_the_composer_in_plain_words() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    let counter = InstanceId::new("song/t00/chain/counter").unwrap();
    state.pointed(&counter, "fail", "press", None);
    frame(&mut state, 0.1);
    let notice = state.notice.clone().unwrap_or_default();
    assert!(
        !notice.is_empty() && !notice.contains("it broke"),
        "{notice}"
    );
}

#[test]
fn a_peak_held_three_seconds_shows_in_decibels_then_drops() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let song = folder.join("extensions/song.tsx");
    let loudest = r#"<Ruler /><text key="loudest" live={live.peak("t00/mix").hold(3).decibels()} digits={1} />"#;
    let code = std::fs::read_to_string(&song).unwrap();
    std::fs::write(&song, code.replace("<Ruler />", loudest)).unwrap();
    let mut state = opened(&folder);
    play(&mut state, 0.0, 1.0);
    state.project.engine().stop();
    let shown = |state: &State| state.lives.texts().concat();
    let held = shown(&state);
    assert!(held.parse::<f64>().is_ok_and(|db| db > -60.0), "{held}");
    for step in 1..=30 {
        frame(&mut state, 1.0 + f64::from(step) / 60.0);
    }
    assert_eq!(shown(&state), held);
    for step in 31..=240 {
        frame(&mut state, 1.0 + f64::from(step) / 60.0);
    }
    assert_eq!(shown(&state), "-120.0");
}

/// A tool whose control loop holds a note on its child `synth` once, or that has no loop.
fn holder(holds: bool) -> String {
    let tick = match holds {
        true => {
            r#"memory: () => ({ held: false }),
  tick: ({ memory, play }) => {
    if (memory.held) return;
    memory.held = true;
    play(60, { child: "synth", hold: true });
  },"#
        }
        false => "",
    };
    format!(
        r#"import {{ tool }} from "./sdk";
export const holder = tool({{
  name: "holder", title: "Holder", when: "Never", doc: "Holds a note.", state: {{}},
  behaviour: () => ({{ connections: [{{ from: {{ child: "synth" }}, to: {{ port: "audio" }} }}] }}),
  {tick}
}});
"#
    )
}

#[test]
fn a_note_a_control_loop_held_on_a_child_ends_with_its_bun() {
    let (_scratch, folder) = with_tools(&[("holder", &holder(true))]);
    let chain = folder.join("state/song/t00/chain");
    std::fs::remove_file(chain.join("holder.json")).unwrap();
    std::fs::create_dir(chain.join("holder")).unwrap();
    let files = [
        ("instance.json", r#"{"tool": "holder", "state": {}}"#),
        (
            "synth.json",
            r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
        ),
    ];
    for (name, text) in files {
        std::fs::write(chain.join("holder").join(name), text).unwrap();
    }
    let mut state = opened(&folder);
    build(&state);
    let level = |state: &State| {
        let levels = state.lives.levels();
        let mix = levels.iter().find(|(name, _)| name == "song/t00/mix");
        mix.map_or(-120.0, |(_, db)| *db)
    };
    for step in 1..=30 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert!(level(&state) > -60.0, "{}", level(&state));
    let file = state.project.root().join("extensions/holder.tsx");
    std::fs::write(file, holder(false)).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    for step in 31..=90 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert!(level(&state) < -60.0, "{}", level(&state));
}

/// A control loop that keeps the `dt` of its first step, once its code has one.
fn first_step(ticks: bool) -> String {
    let tick = match ticks {
        true => {
            r#"memory: () => ({ told: false }),
  tick: ({ dt, memory, update }) => {
    if (memory.told) return;
    memory.told = true;
    update("First", (part) => void (part.state.dt = dt));
  },"#
        }
        false => "",
    };
    format!(
        r#"import {{ number, tool }} from "./sdk";
export const first = tool({{
  name: "first", title: "First", when: "Never", doc: "First.",
  state: {{ dt: number({{ min: 0, max: 1000, default: 0 }}) }},
  {tick}
}});
"#
    )
}

#[test]
fn the_first_step_of_a_new_control_loop_is_one_interval_long() {
    let (_scratch, folder) = with_tools(&[("first", &first_step(false))]);
    let mut state = opened(&folder);
    for step in 1..=300 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    let file = state.project.root().join("extensions/first.tsx");
    std::fs::write(file, first_step(true)).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    for step in 301..=330 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    let id = InstanceId::new("song/t00/chain/first").unwrap();
    let held: Value = serde_json::from_str(&state.project.state_json(&id).unwrap()).unwrap();
    let first = held["dt"].as_f64().unwrap();
    assert!(first > 0.0 && first < 0.1, "{first}");
}
