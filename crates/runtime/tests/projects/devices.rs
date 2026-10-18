//! Built-in devices that a tool of the project plays inside its sound.

use crate::support::{Harness, write};

/// `dark` plays noise through the built-in filter, its cutoff a knob of its own. `aligned`
/// takes the filter, which reports a latency, at its dry mix away from what goes into it.
/// `rig` plays its child `hiss` through its child `room`.
const TOOLS: &str = r#"import { devices, noise, tool, type Declaration } from "./sdk";

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
  state: {},
  sound: () => {
    const hiss = noise().times(0.25);
    return hiss.minus(devices.filter(hiss, { mix: 0 }));
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
fn what_goes_round_a_device_with_a_latency_waits_for_it() {
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
