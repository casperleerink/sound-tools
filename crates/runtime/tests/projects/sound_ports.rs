//! Sound ports of a tool of the project besides `audio`: a parent wires a second output of one
//! child into a sound input of another, and `--inspect` lists them.

use crate::support::{Harness, write};

/// `split` plays noise out of `low` and nothing out of `audio`; `pass` plays its `sidechain`.
/// `rig` wires the one into the other, from `low` or from `audio`.
const TOOLS: &str = r#"import { audio, boolean, noise, tool, type Declaration } from "./sdk";

const split = tool({
  name: "split",
  title: "Split",
  when: "Never",
  doc: "Noise out of `low`.",
  state: {},
  outputs: { low: audio() },
  sound: () => ({ audio: 0, low: noise().times(0.25) }),
});

const pass = tool({
  name: "pass",
  title: "Pass",
  when: "Never",
  doc: "Plays its sidechain.",
  state: {},
  inputs: { sidechain: audio() },
  sound: ({ sidechain }) => sidechain,
});

const parts = { split, pass } as const;

tool({
  name: "rig",
  title: "Rig",
  when: "Never",
  doc: "Wires a split into a pass.",
  state: { low: boolean({ default: true }) },
  children: parts,
  behaviour: ({ state }): Declaration<typeof parts> => ({
    connections: [
      { from: { child: "split", port: state.low ? "low" : "audio" }, to: { child: "pass", port: "sidechain" } },
      { from: { child: "pass" }, to: { port: "audio" } },
    ],
  }),
});
"#;

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |most, sample| most.max(sample.abs()))
}

#[test]
fn a_parent_wires_a_second_output_of_a_child_into_a_sound_input_of_another() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "rig", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/tools.ts", TOOLS);
    write(
        root,
        "state/rig/instance.json",
        r#"{"tool": "rig", "state": {}}"#,
    );
    write(
        root,
        "state/rig/split.json",
        r#"{"tool": "split", "state": {}}"#,
    );
    write(
        root,
        "state/rig/pass.json",
        r#"{"tool": "pass", "state": {}}"#,
    );
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    assert!(loudest(&harness.render(4_800)) > 0.1);

    let summary = runtime::summary(&harness.project);
    for ports in [
        "  instance `rig/pass` [pass] {}\n    sound ports: in audio, sidechain; out audio\n",
        "  instance `rig/split` [split] {}\n    sound ports: in audio; out audio, low\n",
    ] {
        assert!(summary.contains(ports), "{ports} in {summary}");
    }

    harness.write_and_apply(
        "state/rig/instance.json",
        r#"{"tool": "rig", "state": {"low": false}}"#,
    );
    harness.render(4_800);
    assert_eq!(loudest(&harness.render(4_800)), 0.0);
}
