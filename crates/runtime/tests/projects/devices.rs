//! Built-in devices that a tool of the project plays inside its sound.

use crate::support::{Harness, write};

/// `dark` plays noise through the built-in filter, its cutoff a knob of its own. `aligned`
/// takes the filter, which reports a latency, at its dry mix away from what goes into it, with
/// both sides made loud by a knob far past what a sound may play.
/// `rig` plays its child `hiss` through its child `room`.
const TOOLS: &str = r#"import { devices, knob, noise, tool, type Declaration } from "./sdk";

tool({
  name: "dark",
  title: "Dark",
  when: "Never",
  doc: "Noise through the built-in filter.",
  kind: "source",
  state: { cutoff_hz: devices.filter.fields.cutoff_hz },
  sound: ({ cutoff_hz }) => devices.filter(noise().times(0.25), { cutoff_hz }),
});

tool({
  name: "aligned",
  title: "Aligned",
  when: "Never",
  doc: "Noise less itself through a filter that lets it through.",
  kind: "source",
  state: { gain: knob({ min: 1, max: 1000, default: 1000 }) },
  sound: ({ gain }) => {
    const hiss = noise().times(0.25);
    return hiss.times(gain).minus(devices.filter(hiss, { mix: 0 }).times(gain)).over(gain);
  },
});

tool({
  name: "hiss",
  title: "Hiss",
  when: "Never",
  doc: "Noise.",
  kind: "source",
  state: {},
  sound: () => noise().times(0.25),
});

tool({
  name: "rig",
  title: "Rig",
  when: "Never",
  doc: "Plays hiss through room.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "hiss" }, to: { child: "room" } },
      { from: { child: "room" }, to: { port: "audio" } },
    ],
  }),
});
"#;

/// The example of the doc for agents that plays built-in devices: the effect `swept-room`.
fn doc_example() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let block = (doc.split("```ts\n")).find(|block| block.contains("devices.reverb(swept"));
    block.unwrap().split("```").next().unwrap().to_string()
}

/// A project that plays the instance `top` of `TOOLS`, with the doc's example and `records`.
fn project(top: &str, records: &[(&str, &str)]) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = format!(
        r#"{{"format": 1, "extensions": [],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{{"from": {{"instance": "{top}", "port": "audio"}}, "to": {{"device_output": 0}}}}]}}"#
    );
    write(root, "project.json", &project);
    write(root, "extensions/tools.ts", TOOLS);
    write(root, "extensions/swept-room.ts", &doc_example());
    for (path, record) in records {
        write(root, path, record);
    }
    Some(Harness::open(folder))
}

fn record(tool: &str, state: &str) -> String {
    format!(r#"{{"tool": "{tool}", "state": {state}}}"#)
}

/// How far the left channel moves from one frame to the next, on average: noise through a low
/// cutoff moves little.
fn brightness(samples: &[f32]) -> f32 {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let moves = left.windows(2).map(|pair| (pair[1] - pair[0]).abs());
    moves.sum::<f32>() / left.len() as f32
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn a_knob_of_a_tool_turns_the_cutoff_of_the_filter_its_sound_plays_through() {
    let dark = record("dark", r#"{"cutoff_hz": 200}"#);
    let Some(mut harness) = project("dark", &[("state/dark.json", &dark)]) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let dark = brightness(&harness.render(24_000)[2 * 4_800..]);
    harness.write_and_apply(
        "state/dark.json",
        &record("dark", r#"{"cutoff_hz": 20000}"#),
    );
    let bright = brightness(&harness.render(24_000)[2 * 4_800..]);
    assert!(bright > 8.0 * dark, "dark {dark}, bright {bright}");
}

#[test]
fn what_goes_round_a_device_keeps_its_value_and_waits_for_its_latency() {
    let aligned = record("aligned", "{}");
    let Some(mut harness) = project("aligned", &[("state/aligned.json", &aligned)]) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = harness.render(24_000);
    // Noise of 0.25 is about 0.14 RMS: a frame off, the difference would be as loud.
    assert!(rms(&played[2 * 4_800..]) < 1e-3, "{}", rms(&played));
}

#[test]
fn the_device_example_of_the_doc_plays_as_the_child_of_a_parent() {
    let records = [
        ("state/rig/instance.json", record("rig", "{}")),
        ("state/rig/hiss.json", record("hiss", "{}")),
        ("state/rig/room.json", record("swept-room", "{}")),
    ];
    let records: Vec<(&str, &str)> = (records.iter())
        .map(|(path, record)| (*path, record.as_str()))
        .collect();
    let Some(mut harness) = project("rig", &records) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = harness.play(24_000);
    assert!(rms(&played[2 * 4_800..]) > 0.01, "{}", rms(&played));
}

#[test]
fn a_setting_a_device_refuses_is_a_problem_of_the_tool_as_soon_as_it_loads() {
    let wrong = r#"import { devices, noise, tool } from "./sdk";

tool({
  name: "wrong",
  title: "Wrong",
  when: "Never",
  doc: "A filter of a type it does not have.",
  kind: "source",
  state: {},
  sound: () => devices.filter(noise(), { type: "hi_pass" }),
});
"#;
    let dark = record("dark", "{}");
    let records = [
        ("state/dark.json", dark.as_str()),
        ("extensions/wrong.ts", wrong),
    ];
    let Some(harness) = project("dark", &records) else {
        return;
    };
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/wrong.ts");
    assert!(
        (problem.message).contains("devices.filter: type: unknown variant `hi_pass`"),
        "{problem:?}"
    );
}

/// So an agent reads in `sdk.ts` what each field of a built-in record does: from its doc
/// comment, its `Parameter` or its default.
#[test]
fn every_field_of_a_built_in_record_has_its_doc_in_the_sdk() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("extensions")).unwrap();
    let schemas = runtime::tool_schemas().unwrap();
    assert!(sound_typescript::Extensions::load(project.path(), &schemas).is_some());
    let sdk = std::fs::read_to_string(project.path().join("extensions/sdk.ts")).unwrap();
    let records = sdk.split("// Built-in tools").nth(1).unwrap();
    let records = records.split("export interface Tools").next().unwrap();
    let lines: Vec<&str> = records.lines().collect();
    let field = |line: &str| {
        let line = line.trim_start();
        let name = line.split(['?', ':']).next().unwrap_or_default();
        line.starts_with(|c: char| c.is_ascii_lowercase())
            && line.contains(':')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let undocumented: Vec<&str> = (lines.windows(2))
        .filter(|pair| field(pair[1]) && !pair[0].trim_end().ends_with("*/"))
        .map(|pair| pair[1].trim())
        .collect();
    assert!(undocumented.is_empty(), "{undocumented:#?}");
}
