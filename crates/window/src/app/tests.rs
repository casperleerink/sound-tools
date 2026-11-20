#![allow(clippy::unwrap_used)]

use std::path::Path;

use serde::Deserialize;

use serde_json::json;

use super::frame::{frame, run_loops};
use super::headless::copy_folder;
use super::*;
use crate::controls;

mod host;

/// What the live texts of the views show, but the values of the controls a drag turns, such
/// as the template's faders and knobs.
fn shown_texts(state: &State) -> Vec<String> {
    let texts = state.lives.texts().into_iter();
    texts
        .filter(|(shows, _)| !shows.contains("Drag"))
        .map(|(_, text)| text)
        .collect()
}

/// The DAW of a new project, open as a run without a window opens it.
pub(super) fn opened(folder: &Path) -> State {
    let mut state = State::open(Options {
        folder: folder.to_path_buf(),
        headless: true,
        play: None,
        main: None,
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

pub(super) fn new_project() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    runtime::templates::write(&scratch.path().join("song"), &TEMPLATE).unwrap();
    scratch
}

fn record(state: &State, path: &str) -> Value {
    let text = std::fs::read_to_string(state.project().root().join("state").join(path)).unwrap();
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
    state.session.background(|project| project.engine().play());
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
    let song = state.project().root().join("extensions/song.tsx");
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
  view: (self, { update }) => (
    <row gap={1}>
      {Array.from({ length: 16 }, (_, i) => {
        const on = self.state.on[i] ?? false;
        const flip = () => update("Change", (draft) => void (draft.state.on = Array.from({ length: 16 }, (_, j) => (j === i ? !on : (self.state.on[j] ?? false)))));
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
  view: (self, { update }) => {
  const pad = memo("pad", [], () => <box key="break" label="Break" onPress={() => update("Change", (draft) => void (draft.state.broken = true))} />);
  if (self.state.broken) throw new Error("broken");
  const twice = () => {
    update("Change", (draft) => void (draft.state.count += 1));
    update("Change", (draft) => void (draft.state.count += 1));
  };
  const set = (count: number) => update("Change", (draft) => void (draft.state.count = count));
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

/// Runs Bun with `arguments` in `extensions/` of `state`'s project, which must succeed.
fn run_bun(state: &State, arguments: &[&str]) {
    let bun = sound_typescript::bun_program().expect("Bun is installed, see tooling/fetch-bun.sh");
    let ran = sound_core::process::background_command(&bun)
        .args(arguments)
        .current_dir(state.project().root().join("extensions"))
        .output()
        .unwrap();
    let said =
        String::from_utf8_lossy(&ran.stdout).to_string() + &String::from_utf8_lossy(&ran.stderr);
    assert!(ran.status.success(), "bun {arguments:?}: {said}");
}

/// What the agent doc tells an agent to run in a new project: the type check, and its tests.
#[test]
fn a_new_project_type_checks_and_its_tests_pass() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    run_bun(&state, &["check", "-p", "."]);
    run_bun(&state, &["test"]);
}

/// Rune moves a live size only with both: the other in points, never `"full"` or left out.
#[test]
fn a_live_width_or_height_needs_the_other_in_points() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    let sizes = r#"import { h, live } from "./sdk";
const grows = live.elapsed;
export const sizes = [
  <box w={grows} h={4} />,
  <box w={4} h={grows} />,
  <box w="full" h={4} />,
  // @ts-expect-error
  <box w={grows} h="full" />,
  // @ts-expect-error
  <box w="full" h={grows} />,
  // @ts-expect-error
  <box w={grows} />,
];
"#;
    std::fs::write(state.project().root().join("extensions/sizes.tsx"), sizes).unwrap();
    run_bun(&state, &["check", "-p", "."]);
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
    let file = state.project().root().join(runtime::PROJECT_FILE);
    let code = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        code.replace(r#""main": "song""#, r#""main": "song/t00/synth""#),
    )
    .unwrap();
    state
        .session
        .background(|project| project.apply_outside_changes(&[file]))
        .unwrap();
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
    let problems = state.project().problems().len();
    act(&mut state, json!({ "label": "Break", "action": "press" }));
    assert!(!labels(&state).contains(&"Twice".to_string()));
    assert_eq!(state.project().problems().len(), problems + 1);
    act(&mut state, json!({ "action": "undo" }));
    control(&controls::collect(&state).controls, "Break");
    assert_eq!(state.project().problems().len(), problems);
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
    state.session.background(|project| tools.poll(project, &[]));
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
    let held: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
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
    let file = state.project().root().join("extensions/theme.json");
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
    state.session.background(|project| project.engine().play());
    for step in 1..=(seconds * 60.0) as u32 {
        frame(state, from + f64::from(step) / 60.0);
    }
}

#[test]
fn the_meter_of_a_track_taken_away_and_brought_back_moves() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    play(&mut state, 0.0, 0.5);
    let root = state.project().root().to_path_buf();
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
    state
        .session
        .background(|project| project.apply_outside_changes(&paths))
        .unwrap();
    play(&mut state, 0.5, 0.5);
    copy_folder(&kept, &track).unwrap();
    state
        .session
        .background(|project| project.apply_outside_changes(&paths))
        .unwrap();
    // Rune builds the view again once the tree that has the track comes.
    state.settle();
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

/// What the test helpers of `sdk/view.ts` do, as the window does it, and what its types refuse.
const VIEW_CHECKS: &str = include_str!("checks.test.tsx");

#[test]
fn the_test_helpers_change_and_draw_as_the_window_does() {
    let scratch = new_project();
    let state = opened(&scratch.path().join("song"));
    let extensions = state.project().root().join("extensions");
    std::fs::write(extensions.join("checks.test.tsx"), VIEW_CHECKS).unwrap();
    run_bun(&state, &["check", "-p", "."]);
    run_bun(&state, &["test", "checks.test.tsx"]);
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
    let refused = refused.unwrap_err().to_string();
    assert!(refused.contains("was refused"), "{refused}");
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
    let held: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
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
        (state.project().problems().iter())
            .any(|problem| problem.message.contains("its view failed"))
    };
    assert!(failed(&state));
    for entry in std::fs::read_dir(state.project().root().join("extensions")).unwrap() {
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
    assert!(!failed(&state), "{:?}", state.project().problems());
}

#[test]
fn a_press_that_fails_tells_the_composer_in_plain_words() {
    let (_scratch, folder) = with_tools(&[("counter", COUNTER)]);
    let mut state = opened(&folder);
    let counter = InstanceId::new("song/t00/chain/counter").unwrap();
    state.pointed(&counter, "fail", "press", None);
    frame(&mut state, 0.1);
    let notice = state.session.notice().unwrap_or_default().to_string();
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
    let loudest = r#"<Ruler /><text key="loudest" live={live.feed("peak", "t00/mix").hold(3).decibels()} digits={1} />"#;
    let code = std::fs::read_to_string(&song).unwrap();
    std::fs::write(&song, code.replace("<Ruler />", loudest)).unwrap();
    let mut state = opened(&folder);
    play(&mut state, 0.0, 1.0);
    state.session.background(|project| project.engine().stop());
    let shown = |state: &State| shown_texts(state).concat();
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

/// A tool whose control loop holds a note once, or that has no loop: on its child `synth`, or
/// on its own voices.
fn holder(holds: bool, own: bool) -> String {
    let (child, sound) = match own {
        true => (
            "",
            r#"voices: 2,
  sound: () => sine(note.freq).times(note.gate),"#,
        ),
        false => (
            r#"child: "synth", "#,
            r#"behaviour: () => ({ connections: [{ from: { child: "synth" }, to: { port: "audio" } }] }),"#,
        ),
    };
    let tick = match holds {
        true => format!(
            r#"memory: () => ({{ held: false }}),
  tick: ({{ memory, play }}) => {{
    if (memory.held) return;
    memory.held = true;
    play(60, {{ {child}hold: true }});
  }},"#
        ),
        false => String::new(),
    };
    format!(
        r#"import {{ note, sine, tool }} from "./sdk";
export const holder = tool({{
  name: "holder", title: "Holder", when: "Never", doc: "Holds a note.", state: {{}},
  {sound}
  {tick}
}});
"#
    )
}

#[test]
fn a_note_a_control_loop_held_on_a_child_or_its_own_voices_ends_with_its_bun() {
    for own in [false, true] {
        let (_scratch, folder) = with_tools(&[("holder", &holder(true, own))]);
        if !own {
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
        assert!(level(&state) > -60.0, "own {own}: {}", level(&state));
        let file = state.project().root().join("extensions/holder.tsx");
        std::fs::write(file, holder(false, own)).unwrap();
        state.tools.as_mut().unwrap().code_changed();
        for step in 31..=90 {
            frame(&mut state, f64::from(step) / 60.0);
        }
        assert!(level(&state) < -60.0, "own {own}: {}", level(&state));
    }
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
    let file = state.project().root().join("extensions/first.tsx");
    std::fs::write(file, first_step(true)).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    for step in 301..=330 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    let id = InstanceId::new("song/t00/chain/first").unwrap();
    let held: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    let first = held["dt"].as_f64().unwrap();
    assert!(first > 0.0 && first < 0.1, "{first}");
}

/// A gauge of its field `level`, a bar left of it as long as the playhead is past `start`, and
/// a slider of `start`.
const GAUGE: &str = r#"import { h, input, live, number, tool } from "./sdk";
export const gauge = tool({
  name: "gauge", title: "Gauge", when: "Never", doc: "A gauge.",
  state: { level: number({ min: 0, max: 1, default: 0.75 }), start: number({ min: 0, max: 100, integer: true, default: 0 }) },
  sound: () => input,
  view: (self, { update }) => (
    <box>
      <text key="level" live={live.field("level")} digits={2} />
      <box key="bar" at={[-10, 0]} w={live.playhead.minus(self.state.start).plus(5).smooth(100)} h={4} />
      <box key="start" label="Start" role="slider" value={self.state.start} actions={{ set: (start) => update("Change", (draft) => void (draft.state.start = start)) }} />
    </box>
  ),
});
"#;

/// A project that shows the view of [`GAUGE`], whose record leaves every field out.
fn gauge() -> (tempfile::TempDir, State) {
    let (scratch, folder) = with_tools(&[("gauge", GAUGE)]);
    let file = folder.join(runtime::PROJECT_FILE);
    let code = std::fs::read_to_string(&file).unwrap();
    let main = r#""main": "song/t00/chain/gauge""#;
    std::fs::write(&file, code.replace(r#""main": "song""#, main)).unwrap();
    let state = opened(&folder);
    build(&state);
    (scratch, state)
}

#[test]
fn a_live_field_the_record_leaves_out_shows_its_default() {
    let (_scratch, state) = gauge();
    assert_eq!(shown_texts(&state), ["0.75"]);
}

#[test]
fn a_live_place_left_of_its_parent_stays_there() {
    let (_scratch, state) = gauge();
    assert_eq!(state.lives.rects(), [[-10.0, 0.0, 5.0, 4.0]]);
}

#[test]
fn a_live_value_made_anew_in_every_build_keeps_one_binding_and_one_ease() {
    let (_scratch, mut state) = gauge();
    let bindings = state.lives.bindings();
    for start in (0..=100).step_by(10) {
        act(
            &mut state,
            json!({ "label": "Start", "action": "set", "value": start }),
        );
        build(&state);
    }
    frame(&mut state, 0.1);
    assert_eq!(state.lives.bindings(), bindings);
    assert_eq!(state.lives.eased(), 1);
}

#[test]
fn a_part_a_memo_kept_keeps_its_live_values() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    build(&state);
    let bindings = state.lives.bindings();
    // The head of Track 1, with its meter, is a memo this change does not build again.
    act(
        &mut state,
        json!({ "label": "Mute Track 2", "action": "toggle" }),
    );
    assert_eq!(state.lives.bindings(), bindings);
}

/// A beep whose view's buttons play it and the piece, as its control loop could.
const LAUNCHER: &str = r#"import { h, number, sine, tool } from "./sdk";
export const launcher = tool({
  name: "launcher", title: "Launcher", when: "Never", doc: "Launches.", state: {},
  inputs: { gain: number({ min: 0, max: 1, default: 0 }) },
  sound: ({ gain }) => sine(440).times(gain),
  view: (_, { send, clock }) => (
    <row>
      <box key="launch" label="Launch" onPress={() => { send("gain", 1); clock.play(); }} />
      <box key="stop" label="Stop all" onPress={() => clock.stop()} />
    </row>
  ),
});
"#;

#[test]
fn a_press_plays_an_input_and_the_piece_as_a_control_loop_does() {
    let (_scratch, folder) = with_tools(&[("launcher", LAUNCHER)]);
    let mut state = opened(&folder);
    build(&state);
    act(&mut state, json!({ "label": "Launch", "action": "press" }));
    for step in 1..=30 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert!(state.playing);
    let levels = state.lives.levels();
    assert!(
        levels
            .iter()
            .any(|(name, db)| name == "song/t00/mix" && *db > -60.0),
        "{levels:?}"
    );
    act(
        &mut state,
        json!({ "label": "Stop all", "action": "press" }),
    );
    frame(&mut state, 0.6);
    assert!(!state.playing);
}

/// The window of `state` as Rune lays it out and draws it, with the window's fonts.
fn laid_out(state: State) -> rune::Runtime<State> {
    let fonts = rune::text::TextSystem::new(&crate::FONTS, ("Inter Display", "ui-monospace"));
    let env = rune::Env::new(true, 1.0);
    let mut window = rune::Runtime::with_text(state, view, (1470.0, 920.0), env, fonts);
    // As on a Mac with keyboard navigation off, and the app's Tab that reaches every control.
    window.set_full_keyboard_access(false);
    window.set_tab_visits_controls(true);
    window.frame(0.0);
    window
}

/// A frame of the window `seconds` in, as the app runs it: its poll, then Rune's frame.
fn drawn_frame(window: &mut rune::Runtime<State>, seconds: f64) {
    let dt = seconds - window.state.last;
    if frame(&mut window.state, seconds).0 {
        window.invalidate();
    }
    window.frame(dt);
}

fn width(window: &rune::Runtime<State>, key: &str) -> f32 {
    let presented = window.presented(key);
    presented.unwrap_or_else(|| panic!("no {key}")).0[2]
}

#[test]
fn a_live_text_with_no_width_is_as_wide_as_the_widest_it_showed() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let song = folder.join("extensions/song.tsx");
    let loudest = r#"<Ruler /><row><text key="loudest" live={live.feed("peak", "t00/mix").decibels()} unit="db" /></row>"#;
    let code = std::fs::read_to_string(&song).unwrap();
    std::fs::write(&song, code.replace("<Ruler />", loudest)).unwrap();
    let mut window = laid_out(opened(&folder));
    window
        .state
        .session
        .background(|project| project.engine().play());
    for step in 1..=60 {
        drawn_frame(&mut window, f64::from(step) / 60.0);
    }
    let shown = shown_texts(&window.state).concat();
    assert!(shown.ends_with("dB"), "{shown}");
    let measured = rune::text::TextSystem::new(&crate::FONTS, ("Inter Display", "ui-monospace"))
        .measure(&shown, window.state.theme.text("body"), 1.0)
        .0;
    assert!(
        width(&window, "loudest") >= measured,
        "{} < {measured}",
        width(&window, "loudest")
    );
}

/// A device whose view is `tall` points tall.
fn tall(tall: u32) -> String {
    format!(
        r#"import {{ h, input, tool }} from "./sdk";
export const tall = tool({{ name: "tall", title: "Tall", when: "Never", doc: "Tall.", state: {{}}, sound: () => input, view: () => <box key="tall" w={{100}} h={{{tall}}} /> }});
"#
    )
}

#[test]
fn the_devices_take_the_height_they_need_and_scroll_past_the_most() {
    let rect = |window: &rune::Runtime<State>, key: &str| {
        window
            .presented(key)
            .unwrap_or_else(|| panic!("no {key}"))
            .0
    };
    let (_scratch, folder) = with_tools(&[("tall", &tall(230))]);
    let window = laid_out(opened(&folder));
    let [_, top, _, height] = rect(&window, "t00/chain/tall:tall");
    assert!(
        top + height <= rect(&window, "strip-t00")[1],
        "under the mixer"
    );
    let (_scratch, folder) = with_tools(&[("tall", &tall(900))]);
    let mut window = laid_out(opened(&folder));
    let [x, top, ..] = rect(&window, "t00/chain/tall:tall");
    window.wheel(x + 10.0, top + 10.0, -200.0);
    for _ in 0..60 {
        window.frame(1.0 / 60.0);
    }
    assert!(
        rect(&window, "t00/chain/tall:tall")[1] < top - 50.0,
        "it scrolls"
    );
}

#[test]
fn a_card_of_fields_shows_a_few_and_grows_wider_not_taller_when_expanded() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let chain = folder.join("state/song/t00/chain");
    std::fs::write(
        chain.join("comp.json"),
        r#"{"tool": "compressor", "state": {}}"#,
    )
    .unwrap();
    std::fs::write(
        chain.join("instance.json"),
        r#"{"tool": "chain", "state": {"order": ["comp"]}}"#,
    )
    .unwrap();
    let mut window = laid_out(opened(&folder));
    let card = |window: &rune::Runtime<State>| window.presented("t00/chain/comp:card").unwrap().0;
    let shown = |window: &rune::Runtime<State>| {
        let labels = labels(&window.state);
        labels
            .iter()
            .filter(|label| label.starts_with("Compressor "))
            .count()
    };
    let (few, closed) = (shown(&window), card(&window));
    assert_eq!(few, 4);
    act(
        &mut window.state,
        json!({ "label": "More of Compressor", "action": "press" }),
    );
    window.invalidate();
    window.frame(0.1);
    let open = card(&window);
    assert!(shown(&window) > few);
    assert!(open[2] > closed[2], "{open:?} {closed:?}");
    assert_eq!(open[3], closed[3]);
}

/// Eight cells a drag across brings to life.
const CELLS: &str = r#"import { boolean, h, input, list, tool } from "./sdk";
export const cells = tool({
  name: "cells", title: "Cells", when: "Never", doc: "Cells.", state: { alive: list(boolean({ default: false }), { max: 8 }) }, sound: () => input,
  view: (self, { update }) => (
    <row gap={2}>
      {Array.from({ length: 8 }, (_, i) => (
        <box key={`cell-${i}`} label={`Cell ${i + 1}`} w={20} h={20} fill={self.state.alive[i] ? "text" : "fill"}
          onEnter={() => update("Paint", (draft) => void (draft.state.alive = Array.from({ length: 8 }, (_, j) => j === i || (draft.state.alive[j] ?? false))))}>
          <box w={10} h={10} />
        </box>
      ))}
    </row>
  ),
});
"#;

#[test]
fn a_drag_across_cells_paints_each_it_enters_once_as_one_step() {
    let (_scratch, folder) = with_tools(&[("cells", CELLS)]);
    let mut window = laid_out(opened(&folder));
    let centre = |window: &rune::Runtime<State>, cell: usize| {
        window
            .find(&format!("t00/chain/cells:cell-{cell}"))
            .unwrap()
    };
    let ((x, y), (to, _)) = (centre(&window, 0), centre(&window, 5));
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    for step in 1..=20 {
        window.pointer_move(x + (to - x) * step as f32 / 20.0, y);
        window.frame(1.0 / 60.0);
    }
    window.pointer_up(to, y);
    window.frame(1.0 / 60.0);
    let alive = |state: &State| {
        let id = InstanceId::new("song/t00/chain/cells").unwrap();
        let record: Value =
            serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
        record["alive"].clone()
    };
    assert_eq!(
        alive(&window.state),
        json!([true, true, true, true, true, true, false, false])
    );
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(alive(&window.state), Value::Null);
}

/// Track 1's instrument: how many of its keys from C2 sound, and a key a press plays.
const PIANO: &str = r#"import { h, live, Live, note, sine, tool } from "./sdk";
export const piano = tool({
  name: "piano", title: "Piano", when: "Never", doc: "Keys.", state: {}, voices: 4,
  sound: () => sine(note.freq).times(note.gate),
  view: (_, { play }) => (
    <row>
      <text key="down" live={Array.from({ length: 24 }, (_, at) => live.feed(`sounding.${36 + at}`)).reduce((all: Live, one) => all.plus(one), Live.of(0))} digits={0} />
      <box key="c3" label="C3" w={20} h={20} opacity={live.feed("sounding.48")} onPress={() => play(48, { seconds: 0.25 })} />
    </row>
  ),
});
"#;

#[test]
fn a_key_sounds_while_a_press_or_a_clip_plays_it() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(folder.join("extensions/piano.tsx"), PIANO).unwrap();
    std::fs::write(
        folder.join("state/song/t00/synth.json"),
        r#"{"tool": "piano", "state": {}}"#,
    )
    .unwrap();
    let mut state = opened(&folder);
    build(&state);
    let down = |state: &State| shown_texts(state).concat();
    assert_eq!(down(&state), "0");
    act(&mut state, json!({ "label": "C3", "action": "press" }));
    frame(&mut state, 0.1);
    assert_eq!(down(&state), "1");
    for step in 1..=30 {
        frame(&mut state, 0.1 + f64::from(step) / 60.0);
    }
    assert_eq!(down(&state), "0");
    let mut sounded = false;
    state.session.background(|project| project.engine().play());
    for step in 1..=120 {
        frame(&mut state, 1.0 + f64::from(step) / 60.0);
        sounded |= down(&state) != "0";
    }
    assert!(sounded);
}

/// A step with a velocity drag, a menu, a popover with another inside, a name to type, and a
/// level the arrows step and a double click resets.
const KIT: &str = r#"import { boolean, h, input, number, text, tool } from "./sdk";
export const kit = tool({
  name: "kit", title: "Kit", when: "Never", doc: "Kit.",
  state: { on: boolean({ default: false }), velocity: number({ min: 0, max: 1, default: 0.5 }), picked: number({ min: 0, max: 2, default: 0 }), name: text({ default: "" }), level: number({ min: 0, max: 10, default: 5 }) },
  sound: () => input,
  view: (self, { update }) => (
    <row gap={2}>
      <box key="step" label="Step" w={24} h={48} fill="fill" onPress={() => update("Step", (draft) => void (draft.state.on = !draft.state.on))}
        drag={{ y: [-40, 40] }} onDragEnd={({ dy }) => update("Velocity", (draft) => void (draft.state.velocity = 0.5 - dy / 80))} />
      <box key="open" label="Open" w={24} h={24} fill="fill">
        <col overlay={{ open: "press" }} role="menu" group w={80} fill="raised">
          <box key="first" label="First" w={80} h={24} onPress={() => update("Pick", (draft) => void (draft.state.picked = 2))} />
          <box key="pick" label="Pick" w={80} h={24} onPress={() => update("Pick", (draft) => void (draft.state.picked = 1))} />
        </col>
      </box>
      <box key="pop" label="Pop" w={24} h={24} fill="fill">
        <col overlay={{ open: "press" }} role="dialog" w={80} h={80} fill="raised">
          <box key="inner" label="Inner" w={24} h={24} fill="fill">
            <col overlay={{ open: "press" }} w={80} fill="raised">
              <field key="deep" label="Deep" value="" w={80} onCommit={(name) => update("Name", (draft) => void (draft.state.name = name))} />
            </col>
          </box>
        </col>
      </box>
      <field key="name" label="Name" value={self.state.name} w={80} onCommit={(name) => update("Name", (draft) => void (draft.state.name = name))} />
      <row key="item" label="Item" w={48} h={24} fill="fill" onPress={() => update("Item", (draft) => void (draft.state.name = "item"))}>
        <box key="item-menu" label="Item menu" w={24} h={24}>
          <col overlay={{ open: "press" }} role="menu" group w={80} fill="raised">
            <box key="item-pick" label="Item pick" w={80} h={24} onPress={() => {}} />
          </col>
        </box>
      </row>
      <col key="level" label="Level" w={24} h={48} fill="fill" justify="end" actions={{
        increment: (by) => update("Level", (draft) => void (draft.state.level += by)),
        decrement: (by) => update("Level", (draft) => void (draft.state.level -= by)),
        reset: () => update("Level", (draft) => void (draft.state.level = 0)),
      }}>
        <box key="level-value" label="Type level" w={24} h={12}>
          <col overlay={{ open: "press" }} w={80} fill="raised">
            <field key="typed" label="Typed level" value="" w={80} onCommit={(typed) => update("Level", (draft) => void (draft.state.level = Number(typed)))} />
          </col>
        </box>
      </col>
    </row>
  ),
});
"#;

fn kit() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("kit", KIT)]);
    (scratch, laid_out(opened(&folder)))
}

fn kit_record(state: &State) -> Value {
    let id = InstanceId::new("song/t00/chain/kit").unwrap();
    serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap()
}

/// A press and its release at the centre of the element `key`, then a frame that draws it.
fn click(window: &mut rune::Runtime<State>, key: &str) {
    let (x, y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    window.pointer_up(x, y);
    redraw(window);
}

/// The next frame, as the app runs it: its poll, then Rune's frame.
fn redraw(window: &mut rune::Runtime<State>) {
    let seconds = window.state.last + 1.0 / 60.0;
    frame(&mut window.state, seconds);
    window.invalidate();
    window.frame(1.0 / 60.0);
}

fn key(window: &mut rune::Runtime<State>, key: rune::Key) {
    window.key(rune::KeyEvent {
        key,
        mods: rune::Mods::default(),
    });
    redraw(window);
}

#[test]
fn a_drag_on_a_step_sets_how_hard_it_plays_and_never_also_presses_it() {
    let (_scratch, mut window) = kit();
    let (x, y) = window.find("t00/chain/kit:step").unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    for step in 1..=10 {
        window.pointer_move(x, y - step as f32 * 1.6);
        window.frame(1.0 / 60.0);
    }
    // Let go still over the step, where a press ends.
    window.pointer_up(x, y - 16.0);
    redraw(&mut window);
    let held = kit_record(&window.state);
    assert_eq!(held["on"], Value::Null, "{held}");
    assert_eq!(held["velocity"], 0.7);
    click(&mut window, "t00/chain/kit:step");
    assert_eq!(kit_record(&window.state)["on"], true);
}

#[test]
fn an_overlay_closes_on_escape_on_a_press_outside_and_on_a_pick_from_a_menu() {
    let (_scratch, mut window) = kit();
    let opener = "t00/chain/kit:open";
    click(&mut window, opener);
    assert_eq!(window.state.open, [opener]);
    key(&mut window, rune::Key::Escape);
    assert!(window.state.open.is_empty());
    click(&mut window, opener);
    window.pointer_down(700.0, 300.0);
    window.pointer_up(700.0, 300.0);
    redraw(&mut window);
    assert!(window.state.open.is_empty());
    click(&mut window, opener);
    click(&mut window, "t00/chain/kit:pick");
    assert!(window.state.open.is_empty());
    assert_eq!(kit_record(&window.state)["picked"], 1);
}

#[test]
fn a_field_keeps_what_was_typed_on_enter() {
    let (_scratch, mut window) = kit();
    click(&mut window, "t00/chain/kit:name");
    for letter in ["K", "i", "c", "k"] {
        key(&mut window, rune::Key::Text(letter.into()));
    }
    assert_eq!(kit_record(&window.state)["name"], Value::Null);
    key(&mut window, rune::Key::Enter);
    assert_eq!(kit_record(&window.state)["name"], "Kick");
}

#[test]
fn the_arrows_step_a_control_with_steps_that_tab_reaches() {
    let (_scratch, mut window) = kit();
    let level = "t00/chain/kit:level";
    tab_to(&mut window, level);
    assert_eq!(focused(&window).as_deref(), Some(level));
    key(&mut window, rune::Key::Up);
    assert_eq!(kit_record(&window.state)["level"], 6);
}

/// Moves the pointer from `from` by `steps` of `by`, a frame each.
fn travel(
    window: &mut rune::Runtime<State>,
    from: (f32, f32),
    by: (f32, f32),
    steps: usize,
) -> (f32, f32) {
    let mut at = from;
    for _ in 0..steps {
        at = (at.0 + by.0, at.1 + by.1);
        window.pointer_move(at.0, at.1);
        window.frame(1.0 / 60.0);
    }
    at
}

#[test]
fn a_drag_that_comes_back_to_where_it_started_is_no_press() {
    let (_scratch, mut window) = kit();
    let start = window.find("t00/chain/kit:step").unwrap();
    window.pointer_down(start.0, start.1);
    window.frame(1.0 / 60.0);
    let away = travel(&mut window, start, (0.0, -2.0), 6);
    travel(&mut window, away, (0.0, 2.0), 6);
    window.pointer_up(start.0, start.1);
    redraw(&mut window);
    assert_eq!(kit_record(&window.state)["on"], Value::Null);
}

#[test]
fn after_a_drag_let_go_outside_its_element_the_keys_press_it() {
    let (_scratch, mut window) = kit();
    let start = window.find("t00/chain/kit:step").unwrap();
    window.pointer_down(start.0, start.1);
    window.frame(1.0 / 60.0);
    let out = travel(&mut window, start, (0.0, 6.0), 10);
    window.pointer_up(out.0, out.1);
    redraw(&mut window);
    assert_eq!(kit_record(&window.state)["velocity"], 0.0);
    assert_eq!(focused(&window).as_deref(), Some("t00/chain/kit:step"));
    key(&mut window, rune::Key::Text(" ".into()));
    assert_eq!(kit_record(&window.state)["on"], true);
}

#[test]
fn a_double_click_sets_what_reset_sets_and_enter_types_a_value() {
    let (_scratch, mut window) = kit();
    let (x, y) = window.find("t00/chain/kit:level").unwrap();
    for _ in 0..2 {
        window.pointer_down(x, y - 12.0);
        window.pointer_up(x, y - 12.0);
        window.frame(0.05);
    }
    redraw(&mut window);
    assert_eq!(kit_record(&window.state)["level"], 0);
    assert!(window.state.open.is_empty());
    key(&mut window, rune::Key::Enter);
    assert_eq!(window.state.open, ["t00/chain/kit:level-value"]);
    key(&mut window, rune::Key::Text("7".into()));
    key(&mut window, rune::Key::Enter);
    assert_eq!(kit_record(&window.state)["level"], 7);
    assert!(window.state.open.is_empty());
    assert_eq!(focused(&window).as_deref(), Some("t00/chain/kit:level"));
    key(&mut window, rune::Key::Up);
    assert_eq!(kit_record(&window.state)["level"], 8);
}

/// The key of the element that has the keys.
fn focused(window: &rune::Runtime<State>) -> Option<String> {
    let elements = window.elements();
    elements
        .into_iter()
        .find(|element| element.focused)
        .and_then(|element| element.key)
}

/// Tab until the element `key` has the keys.
fn tab_to(window: &mut rune::Runtime<State>, key: &str) {
    for _ in 0..80 {
        if focused(window).as_deref() == Some(key) {
            return;
        }
        self::key(window, rune::Key::Tab);
    }
    panic!("Tab never reaches {key}");
}

#[test]
fn a_row_with_a_menu_in_it_keeps_its_keys_and_tab_reaches_the_menu() {
    let (_scratch, mut window) = kit();
    tab_to(&mut window, "t00/chain/kit:item");
    key(&mut window, rune::Key::Enter);
    assert!(window.state.open.is_empty());
    key(&mut window, rune::Key::Text(" ".into()));
    assert_eq!(kit_record(&window.state)["name"], "item");
    key(&mut window, rune::Key::Tab);
    assert_eq!(focused(&window).as_deref(), Some("t00/chain/kit:item-menu"));
}

#[test]
fn a_drag_with_shift_held_moves_a_tenth_as_far() {
    let (_scratch, mut window) = kit();
    let start = window.find("t00/chain/kit:step").unwrap();
    window.physical_input(rune::PhysicalInput::Modifiers(rune::Mods {
        shift: true,
        ..rune::Mods::default()
    }));
    window.pointer_down(start.0, start.1);
    window.frame(1.0 / 60.0);
    let end = travel(&mut window, start, (0.0, -2.0), 10);
    window.pointer_up(end.0, end.1);
    redraw(&mut window);
    let velocity = kit_record(&window.state)["velocity"].as_f64().unwrap();
    assert!((velocity - 0.525).abs() < 1e-6, "{velocity}");
}

#[test]
fn a_device_without_a_view_has_knobs_that_take_keys_a_double_click_and_a_typed_value() {
    let (_scratch, folder) = with_tools(&[("amp", AMP)]);
    let mut window = laid_out(opened(&folder));
    let gain = || {
        let id = InstanceId::new("song/t00/chain/amp").unwrap();
        move |state: &State| -> Value {
            serde_json::from_str::<Value>(&state.project().state_json(&id).unwrap()).unwrap()["gain"]
                .clone()
        }
    };
    let gain = gain();
    tab_to(&mut window, "t00/chain/amp:gain");
    key(&mut window, rune::Key::Up);
    assert_eq!(gain(&window.state), 0.52);
    let (x, y) = window.find("t00/chain/amp:gain").unwrap();
    for _ in 0..2 {
        window.pointer_down(x, y - 16.0);
        window.pointer_up(x, y - 16.0);
        window.frame(0.05);
    }
    redraw(&mut window);
    assert_eq!(gain(&window.state), 0.5);
    key(&mut window, rune::Key::Enter);
    for typed in ["2", "5", "%"] {
        key(&mut window, rune::Key::Text(typed.into()));
    }
    key(&mut window, rune::Key::Enter);
    assert_eq!(gain(&window.state), 0.25);
}

#[test]
fn an_overlay_opened_inside_another_keeps_it_and_escape_closes_the_inner_first() {
    let (_scratch, mut window) = kit();
    let (pop, inner) = ("t00/chain/kit:pop", "t00/chain/kit:inner");
    click(&mut window, pop);
    click(&mut window, inner);
    assert_eq!(window.state.open, [pop, inner]);
    assert!(window.find("t00/chain/kit:deep").is_some());
    key(&mut window, rune::Key::Escape);
    assert_eq!(window.state.open, [pop]);
    key(&mut window, rune::Key::Escape);
    assert!(window.state.open.is_empty());
}

#[test]
fn the_arrows_move_in_a_menu_and_enter_picks_and_closes_it() {
    let (_scratch, mut window) = kit();
    click(&mut window, "t00/chain/kit:open");
    key(&mut window, rune::Key::Down);
    key(&mut window, rune::Key::Enter);
    assert!(window.state.open.is_empty());
    assert_eq!(kit_record(&window.state)["picked"], 1);
}

#[test]
fn an_agent_sees_and_acts_on_an_overlay_as_a_pointer_does() {
    let (_scratch, mut window) = kit();
    let shown = |state: &State| labels(state).contains(&"Pick".to_string());
    assert!(!shown(&window.state));
    act(
        &mut window.state,
        json!({ "label": "Open", "action": "open" }),
    );
    assert!(shown(&window.state));
    act(
        &mut window.state,
        json!({ "label": "Pick", "action": "press" }),
    );
    assert!(window.state.open.is_empty());
    assert!(!shown(&window.state));
    assert_eq!(kit_record(&window.state)["picked"], 1);
}

fn synth(state: &State) -> Value {
    record(state, "song/t00/synth.json")["state"].clone()
}

#[test]
fn a_built_in_device_changes_from_its_card_one_step_of_undo_each() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    act(
        &mut state,
        json!({ "label": "Synth Cutoff", "action": "set", "value": 500 }),
    );
    assert_eq!(synth(&state)["cutoff_hz"], 500.0);
    act(
        &mut state,
        json!({ "label": "Synth Cutoff", "action": "increment" }),
    );
    let raised = synth(&state)["cutoff_hz"].as_f64().unwrap();
    assert!(raised > 500.0, "{raised}");
    act(&mut state, json!({ "action": "undo" }));
    assert_eq!(synth(&state)["cutoff_hz"], 500.0);
    act(&mut state, json!({ "action": "undo" }));
    assert_eq!(synth(&state).get("cutoff_hz"), None);
}

#[test]
fn a_card_follows_its_instance_becoming_another_tool_and_its_tool_gaining_a_field() {
    let (_scratch, folder) = with_tools(&[("amp", AMP)]);
    let mut state = opened(&folder);
    assert!(labels(&state).contains(&"Amp Gain".to_string()));
    let root = state.project().root().to_path_buf();
    let amp = root.join("extensions/amp.tsx");
    let code = std::fs::read_to_string(&amp).unwrap();
    std::fs::write(
        &amp,
        code.replace(
            "stages:",
            "tone: number({ min: 0, max: 1, default: 0.5 }), stages:",
        ),
    )
    .unwrap();
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    assert!(
        labels(&state).contains(&"Amp Tone".to_string()),
        "{:?}",
        labels(&state)
    );
    let record = root.join("state/song/t00/chain/amp.json");
    std::fs::write(&record, r#"{"tool": "compressor", "state": {}}"#).unwrap();
    state
        .session
        .background(|project| project.apply_outside_changes(&[record]))
        .unwrap();
    frame(&mut state, 0.2);
    state.settle();
    let shown = labels(&state);
    assert!(
        !shown.iter().any(|label| label.starts_with("Amp ")),
        "{shown:?}"
    );
    assert!(
        shown.iter().any(|label| label.starts_with("Compressor ")),
        "{shown:?}"
    );
}

#[test]
fn a_project_without_its_generic_card_says_so_in_plain_words() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::remove_file(folder.join("extensions/ui/blocks/generic-card.tsx")).unwrap();
    let state = opened(&folder);
    let problems = state.project().problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("generic-card.tsx is missing")),
        "{problems:?}"
    );
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(matches!(state.drawn(&synth), Some(Drawn::NoCard(why)) if why.contains("no card")));
}

#[test]
fn a_generic_card_that_hangs_is_blamed_alone_and_the_tools_go_on() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(
        folder.join("extensions/ui/blocks/generic-card.tsx"),
        "export const card = () => { while (true) {} };\n",
    )
    .unwrap();
    let mut state = opened(&folder);
    for step in 1..=8 {
        frame(&mut state, f64::from(step) * 0.5);
        state.settle();
    }
    let problems = state.project().problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.path.contains("generic-card.tsx")),
        "{problems:?}"
    );
    control(&controls::collect(&state).controls, "Mute Track 2");
}

#[test]
fn enter_on_a_fader_types_its_value_and_the_keys_stay_with_it() {
    let scratch = new_project();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    let gain = |state: &State| record(state, "song/t00/mix.json")["state"]["gain_db"].clone();
    tab_to(&mut window, "fader-t00");
    key(&mut window, rune::Key::Enter);
    assert_eq!(window.state.open, ["fader-t00-value"]);
    for typed in ["-", "1", "2"] {
        key(&mut window, rune::Key::Text(typed.into()));
    }
    key(&mut window, rune::Key::Enter);
    assert_eq!(gain(&window.state), -12.0);
    assert_eq!(focused(&window).as_deref(), Some("fader-t00"));
    key(&mut window, rune::Key::Up);
    assert_eq!(gain(&window.state), -11.0);
    key(&mut window, rune::Key::Tab);
    assert_ne!(focused(&window).as_deref(), Some("fader-t00-value"));
}

/// A generic card whose `body` draws for every device without a view.
fn with_card(body: &str) -> (tempfile::TempDir, State) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(
        folder.join("extensions/ui/blocks/generic-card.tsx"),
        format!("import {{ h, type Card }} from \"../../sdk\";\nexport const card: Card = () => {body};\n"),
    )
    .unwrap();
    let state = opened(&folder);
    (scratch, state)
}

fn card_problems(state: &State) -> Vec<String> {
    let problems = state.project().problems();
    (problems.iter())
        .filter(|problem| problem.path.ends_with("generic-card.tsx"))
        .map(|problem| problem.message.clone())
        .collect()
}

#[test]
fn a_generic_card_that_hangs_in_a_handler_is_stopped_alone_and_says_so() {
    let (_scratch, mut state) =
        with_card(r#"<box key="hang" label="Hang" onPress={() => { while (true) {} }} />"#);
    assert!(try_act(&mut state, json!({ "label": "Hang", "action": "press" })).is_err());
    for step in 1..=6 {
        frame(&mut state, f64::from(step) * 0.5);
        state.settle();
    }
    assert!(
        !card_problems(&state).is_empty(),
        "{:?}",
        state.project().problems()
    );
    control(&controls::collect(&state).controls, "Mute Track 2");
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(
        matches!(state.drawn(&synth), Some(Drawn::NoCard(why)) if why.contains("stopped")),
        "the card says why it shows nothing"
    );
}

#[test]
fn a_generic_card_that_fails_to_draw_is_a_problem_of_its_file() {
    let (_scratch, state) = with_card(r#"{ throw new Error("no room") }"#);
    let problems = card_problems(&state);
    assert!(
        problems.iter().any(|problem| problem.contains("no room")),
        "{:?}",
        state.project().problems()
    );
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(matches!(state.drawn(&synth), Some(Drawn::NoCard(why)) if why.contains("mistake")));
}

/// A device of the project with no view, with a declared unit and a plain number.
const RATE: &str = r#"import { input, number, tool } from "./sdk";
export const rate = tool({ name: "rate", title: "Rate", when: "Never", doc: "Rate.", state: { speed: number({ min: 20, max: 20000, unit: "hz", default: 2000 }), seed: number({ min: 0, max: 1, unit: "plain", default: 0.5 }) }, sound: () => input });
"#;

#[test]
fn a_card_keeps_the_unit_a_tool_declares() {
    let (_scratch, folder) = with_tools(&[("rate", RATE)]);
    let state = opened(&folder);
    let id = InstanceId::new("song/t00/chain/rate").unwrap();
    let card = crate::fields::card_of(&state, &id).unwrap();
    assert_eq!(card["fields"][0]["unit"], "hz");
    assert_eq!(card["fields"][0]["log"], true);
    assert_eq!(card["fields"][1]["unit"], Value::Null);
}

#[test]
fn a_tree_that_comes_during_a_drag_leaves_the_drag_where_it_is() {
    let (_scratch, mut window) = kit();
    let key = "t00/chain/kit:step";
    let start = window.find(key).unwrap();
    window.pointer_down(start.0, start.1);
    window.frame(1.0 / 60.0);
    travel(&mut window, start, (0.0, -2.0), 10);
    let moved = window.state.lives.dragged(key).get();
    assert!(moved[1] < -10.0, "{moved:?}");
    // Another control's change draws the view again while the drag goes on.
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    act(
        &mut window.state,
        json!({ "label": "Item", "action": "press" }),
    );
    assert!(window.state.tools.as_ref().unwrap().view_runs() > runs);
    assert_eq!(window.state.lives.dragged(key).get(), moved);
    window.pointer_up(start.0, start.1 - 20.0);
    redraw(&mut window);
    assert_eq!(window.state.lives.dragged(key).get(), [0.0; 2]);
}

/// A lane on the first track of the project in `scratch` that plays `value` into `target`,
/// such as `synth.cutoff_hz`.
fn lane(scratch: &tempfile::TempDir, target: &str, value: f64) {
    let track = scratch.path().join("song/state/song/t00");
    let automation = json!([{ "lane": "lane-held", "target": target }]);
    let instance = json!({ "tool": "track", "state": { "title": "Track 1", "colour": "blue", "automation": automation } });
    std::fs::write(track.join("instance.json"), instance.to_string()).unwrap();
    let points = json!({ "tool": "lane", "state": { "points": [{ "tick": 0, "value": value }] } });
    std::fs::write(track.join("lane-held.json"), points.to_string()).unwrap();
}

/// A new project whose first track's lane plays 8 kHz into its synth's cutoff, saved at 500.
pub(super) fn automated_cutoff() -> tempfile::TempDir {
    let scratch = new_project();
    lane(&scratch, "synth.cutoff_hz", 8000.0);
    let synth = json!({ "tool": "instrument.synth", "state": { "cutoff_hz": 500 } });
    let track = scratch.path().join("song/state/song/t00");
    std::fs::write(track.join("synth.json"), synth.to_string()).unwrap();
    scratch
}

#[test]
fn a_knob_a_lane_moves_shows_what_plays_and_takes_no_change() {
    let scratch = automated_cutoff();
    let mut state = opened(&scratch.path().join("song"));
    frame(&mut state, 0.1);
    build(&state);
    frame(&mut state, 0.2);
    let controls = controls::collect(&state).controls;
    control(&controls, "Synth Cutoff");
    assert!(
        state.lives.texts().iter().any(|(_, text)| text == "8 kHz"),
        "{:?}",
        state.lives.texts()
    );
    assert!(
        try_act(
            &mut state,
            json!({ "label": "Synth Cutoff", "action": "set", "value": 1000 })
        )
        .is_err()
    );
    assert_eq!(synth(&state)["cutoff_hz"], 500);
}

#[test]
fn a_view_runs_once_as_the_project_opens() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    for step in 1..=4 {
        frame(&mut state, f64::from(step) * 0.1);
        state.settle();
    }
    let runs = state.tools.as_ref().unwrap().view_runs();
    assert_eq!(runs as usize, state.shown.len());
}

fn card_file(contents: &str) -> (tempfile::TempDir, State) {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    std::fs::write(
        folder.join("extensions/ui/blocks/generic-card.tsx"),
        contents,
    )
    .unwrap();
    let state = opened(&folder);
    (scratch, state)
}

#[test]
fn a_generic_card_that_does_not_load_says_its_own_mistake() {
    let (_scratch, state) = card_file("export const card = (;\n");
    let said = card_problems(&state);
    assert!(
        said.iter()
            .all(|problem| !problem.contains("fails to draw")),
        "{said:?}"
    );
    assert!(!said.is_empty());
    let (_scratch, state) = card_file("export const cards = () => null;\n");
    let said = card_problems(&state);
    assert!(
        said.iter()
            .any(|problem| problem.contains("exports no `card`")),
        "{said:?}"
    );
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(matches!(state.drawn(&synth), Some(Drawn::NoCard(why)) if why.contains("mistake")));
}

#[test]
fn a_project_of_only_built_in_devices_shows_their_cards() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    for file in [
        "song.tsx",
        "song.test.tsx",
        "gallery.tsx",
        "track.ts",
        "chain.ts",
        "bus.ts",
        "tape.ts",
        "lfo.ts",
    ] {
        std::fs::remove_file(folder.join("extensions").join(file)).unwrap();
    }
    std::fs::remove_dir_all(folder.join("state")).unwrap();
    std::fs::create_dir_all(folder.join("state")).unwrap();
    std::fs::write(
        folder.join("state/comp.json"),
        r#"{"tool": "compressor", "state": {}}"#,
    )
    .unwrap();
    let project = folder.join("project.json");
    let mut file: Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    file["main"] = json!("comp");
    file["connections"] = json!([]);
    std::fs::write(&project, file.to_string()).unwrap();
    let state = opened(&folder);
    assert!(
        labels(&state)
            .iter()
            .any(|label| label.starts_with("Compressor ")),
        "{:?} {:?}",
        labels(&state),
        state.project().problems()
    );
}

#[test]
fn a_held_control_reports_what_plays_and_takes_no_change() {
    let scratch = automated_cutoff();
    let mut state = opened(&scratch.path().join("song"));
    let controls = controls::collect(&state).controls;
    assert_eq!(
        control(&controls, "Synth Cutoff").value,
        Some(json!(8000.0))
    );
    assert!(
        try_act(
            &mut state,
            json!({ "label": "Synth Cutoff", "action": "set", "value": 1000 })
        )
        .is_err()
    );
    assert_eq!(synth(&state)["cutoff_hz"], 500);
}

#[test]
fn the_mixer_holds_a_fader_a_lane_moves() {
    let scratch = new_project();
    lane(&scratch, "mix.gain_db", -24.0);
    let mut state = opened(&scratch.path().join("song"));
    let controls = controls::collect(&state).controls;
    let fader = control(&controls, "Volume Track 1");
    assert_eq!(fader.value, Some(json!(-24.0)));
    assert!(
        try_act(
            &mut state,
            json!({ "label": "Volume Track 1", "action": "increment" })
        )
        .is_err()
    );
}

/// A device of the project whose own view holds its level while a lane moves it.
const LEVEL: &str = r#"import { h, input, live, number, tool } from "./sdk";
import { Knob } from "./ui/knob";
export const level = tool({
  name: "level", title: "Level", when: "Never", doc: "Level.", state: { level: number({ min: 0, max: 10000, default: 500 }) }, sound: ({ level }) => input.times(level.times(0.0001)),
  view: (self, { update, held }) => (
    <Knob id="level" label="Level" value={self.state.level} min={0} max={10000} held={held("level") ? live.field("level") : undefined} onChange={(level) => update("Level", (draft) => void (draft.state.level = level))} />
  ),
});
"#;

#[test]
fn a_tool_s_own_knob_holds_while_a_lane_moves_it() {
    let (scratch, folder) = with_tools(&[("level", LEVEL)]);
    lane(&scratch, "chain/level.level", 8000.0);
    let mut state = opened(&folder);
    let controls = controls::collect(&state).controls;
    let level = control(&controls, "Level");
    assert_eq!(level.value, Some(json!(8000.0)));
    assert!(
        try_act(
            &mut state,
            json!({ "label": "Level", "action": "increment" })
        )
        .is_err()
    );
}

#[test]
fn the_steps_of_act_see_the_lanes_already_bound() {
    let scratch = automated_cutoff();
    let steps = json!([{ "label": "Synth Cutoff", "action": "set", "value": 1000 }]);
    let mut state = State::open(Options {
        folder: scratch.path().join("song"),
        headless: true,
        play: None,
        main: None,
        act: Some(serde_json::from_value(steps).unwrap()),
    })
    .unwrap();
    frame(&mut state, 0.0);
    let failed = FAILED.lock().unwrap_or_else(PoisonError::into_inner).take();
    assert!(failed.is_some_and(|why| why.contains("Synth Cutoff")));
    assert_eq!(synth(&state)["cutoff_hz"], 500);
}

#[test]
fn a_device_whose_code_does_not_load_says_so() {
    let (_scratch, folder) = with_tools(&[("broken", "export const broken = (;\n")]);
    let state = opened(&folder);
    let broken = InstanceId::new("song/t00/chain/broken").unwrap();
    assert!(state.code_failed(&broken));
    let synth = InstanceId::new("song/t00/synth").unwrap();
    assert!(!state.code_failed(&synth));
}

#[test]
fn a_device_whose_code_does_not_load_says_so_where_it_sits_and_as_the_main_view() {
    let (_scratch, folder) = with_tools(&[("broken", "export const broken = (;\n")]);
    let window = laid_out(opened(&folder));
    assert!(window.find("t00/chain/broken:code-failed").is_some());
    let main = State::open(Options {
        folder,
        headless: true,
        play: None,
        main: Some(InstanceId::new("song/t00/chain/broken").unwrap()),
        act: None,
    })
    .unwrap();
    let mut window = laid_out(main);
    drawn_frame(&mut window, 0.1);
    assert!(window.find("code-failed").is_some());
}

/// The song with a button that puts a lane on Track 1's volume.
fn with_lane_button(scratch: &tempfile::TempDir) {
    let song = scratch.path().join("song/extensions/song.tsx");
    let code = std::fs::read_to_string(&song).unwrap();
    let button = r#"<Ruler /><box key="add-lane" label="Add lane" w={20} h={20} onPress={() => update("Lane", (draft) => {
      const first = draft.children.find((child) => child.name === "t00");
      first.state.automation = [{ lane: "lane-held", target: "mix.gain_db" }];
      first.children.push({ name: "lane-held", tool: "lane", state: { points: [{ tick: 0, value: -24 }] }, children: [] });
    })} />"#;
    std::fs::write(&song, code.replacen("<Ruler />", button, 1)).unwrap();
}

fn acted(scratch: &tempfile::TempDir, steps: Value) -> State {
    let mut state = State::open(Options {
        folder: scratch.path().join("song"),
        headless: true,
        play: None,
        main: None,
        act: Some(serde_json::from_value(steps).unwrap()),
    })
    .unwrap();
    frame(&mut state, 0.0);
    state
}

#[test]
fn each_step_of_act_sees_the_lanes_the_step_before_left() {
    let scratch = new_project();
    with_lane_button(&scratch);
    let set = json!({ "label": "Volume Track 1", "action": "set", "value": -6 });
    let state = acted(
        &scratch,
        json!([{ "label": "Add lane", "action": "press" }, set]),
    );
    let failed = FAILED.lock().unwrap_or_else(PoisonError::into_inner).take();
    assert!(failed.is_some_and(|why| why.contains("lane")));
    assert_eq!(
        record(&state, "song/t00/mix.json")["state"].get("gain_db"),
        None
    );
    let scratch = new_project();
    with_lane_button(&scratch);
    let undo = json!({ "action": "undo" });
    let state = acted(
        &scratch,
        json!([{ "label": "Add lane", "action": "press" }, undo, set]),
    );
    assert_eq!(
        record(&state, "song/t00/mix.json")["state"]["gain_db"],
        -6.0
    );
}

#[test]
fn play_plays_all_the_seconds_it_says() {
    let scratch = new_project();
    let mut state = State::open(Options {
        folder: scratch.path().join("song"),
        headless: true,
        play: Some(2.0),
        main: None,
        act: None,
    })
    .unwrap();
    for step in 0..=120 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    assert_eq!(state.playhead.0, 3840);
}

#[test]
fn a_held_control_s_accessibility_value_follows_its_lane() {
    let scratch = new_project();
    let track = scratch.path().join("song/state/song/t00");
    let automation = json!([{ "lane": "lane-held", "target": "synth.cutoff_hz" }]);
    let instance = json!({ "tool": "track", "state": { "title": "Track 1", "colour": "blue", "automation": automation } });
    std::fs::write(track.join("instance.json"), instance.to_string()).unwrap();
    let points = json!([{ "tick": 0, "value": 8000 }, { "tick": 1920, "value": 12000 }]);
    let lane = json!({ "tool": "lane", "state": { "points": points } });
    std::fs::write(track.join("lane-held.json"), lane.to_string()).unwrap();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    window
        .state
        .session
        .background(|project| project.engine().play());
    for step in 1..=15 {
        drawn_frame(&mut window, f64::from(step) / 60.0);
    }
    let spoken: f64 = window
        .state
        .lives
        .spoken("t00/synth:cutoff_hz")
        .unwrap()
        .parse()
        .unwrap();
    let controls = controls::collect(&window.state).controls;
    let value = control(&controls, "Synth Cutoff")
        .value
        .clone()
        .unwrap()
        .as_f64()
        .unwrap();
    assert!(spoken > 8100.0, "{spoken}");
    assert!((spoken - value).abs() < value * 0.01, "{spoken} {value}");
}

/// A parent whose button opens the window of its child: an amp, which has none.
const OPENER: &str = r#"import { h, tool } from "./sdk";

export const opener = tool({
  name: "opener", title: "Opener", when: "Never", doc: "Opens the window of its amp.", state: {}, children: { amp: "amp" },
  behaviour: () => ({ connections: [{ from: { child: "amp" }, to: { port: "audio" } }] }),
  view: (_, { openWindow }) => <box key="open" label="Open amp window" onPress={() => openWindow("amp")} />,
});
"#;

/// The window of a child comes from code, as a plugin card's button opens it; one with none is
/// a problem of that code. Headless, no plugin loads, so `--tree` lists no plugin window.
#[test]
fn code_that_opens_the_window_of_a_child_with_none_has_a_problem() {
    let (_scratch, folder) = with_tools(&[("opener", OPENER)]);
    std::fs::write(folder.join("extensions/amp.ts"), AMP).unwrap();
    let chain = folder.join("state/song/t00/chain");
    std::fs::create_dir(chain.join("opener")).unwrap();
    std::fs::rename(
        chain.join("opener.json"),
        chain.join("opener/instance.json"),
    )
    .unwrap();
    let amp = r#"{"tool": "amp", "state": {}}"#;
    std::fs::write(chain.join("opener/amp.json"), amp).unwrap();
    let mut state = opened(&folder);
    act(
        &mut state,
        json!({ "label": "Open amp window", "action": "press" }),
    );
    frame(&mut state, 0.1);
    let problems = state.report()["problems"].to_string();
    assert!(
        problems.contains(r#"its openWindow(\"amp\") was refused: it has no window of its own"#),
        "{problems}"
    );
    assert_eq!(state.report()["plugin_windows"], json!([]));
}

mod behaviours;
mod frame;
mod projects;
mod transport;
