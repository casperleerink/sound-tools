//! Numbers that one child of a tool of the project sends out, connected by the parent to a
//! number of another child: a knob of a built-in device or of a tool of the project.

use crate::support::{Harness, write};

/// Noise, and the number `cutoff` at 200 Hz for the first half of each second and 20 kHz for the
/// second.
const SOURCES: &str = r#"import { knob, mix, noise, output, phasor, sine, tool } from "./sdk";

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
  name: "square-lfo",
  title: "Square LFO",
  when: "Never",
  doc: "Sends `cutoff` low, then high, once a second.",
  kind: "source",
  state: {},
  sound: () => {
    output("cutoff", mix(200, 20000, phasor(1).ge(0.5)));
    output("pitch", 440);
    return 0;
  },
});

tool({
  name: "drone",
  title: "Drone",
  when: "Never",
  doc: "A sine.",
  kind: "source",
  state: { hz: knob({ min: 20, max: 2000, default: 220, unit: "hz" }) },
  sound: ({ hz }) => sine(hz).times(0.25),
});
"#;

/// Plays `lead` through the child `filter`, and connects `send` of the child `lfo` to the number
/// `to` of the child `target` while `to` is not empty.
const RIG: &str = r#"import { text, tool, type Declaration } from "./sdk";

tool({
  name: "rig",
  title: "Rig",
  when: "Never",
  doc: "Plays a child through a filter, with a number of an LFO into one of a child.",
  state: { lead: text({ default: "hiss" }), send: text(), target: text(), to: text() },
  behaviour: ({ state: { lead, send, target, to } }): Declaration => ({
    connections: [
      { from: { child: lead }, to: { child: "filter" } },
      { from: { child: "filter" }, to: "sound" },
      ...(to ? [{ from: { child: "lfo", number: send }, to: { child: target, number: to } }] : []),
    ],
  }),
});
"#;

fn rig(state: &str) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["filter"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "rig", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/sources.ts", SOURCES);
    write(root, "extensions/rig.ts", RIG);
    write(root, "state/rig/instance.json", &record(state));
    write(
        root,
        "state/rig/hiss.json",
        r#"{"tool": "hiss", "state": {}}"#,
    );
    write(
        root,
        "state/rig/lfo.json",
        r#"{"tool": "square-lfo", "state": {}}"#,
    );
    write(
        root,
        "state/rig/drone.json",
        r#"{"tool": "drone", "state": {}}"#,
    );
    let filter = r#"{"tool": "filter", "state": {"cutoff_hz": 20000}}"#;
    write(root, "state/rig/filter.json", filter);
    Some(Harness::open(folder))
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "rig", "state": {state}}}"#)
}

/// How far the left channel moves from one frame to the next, on average: noise through a low
/// cutoff moves little.
fn brightness(samples: &[f32]) -> f32 {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let moves = left.windows(2).map(|pair| (pair[1] - pair[0]).abs());
    moves.sum::<f32>() / left.len() as f32
}

/// How often the left channel rises through zero.
fn rises(samples: &[f32]) -> usize {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let pairs = left.windows(2);
    pairs.filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0).count()
}

#[test]
fn a_number_of_a_child_moves_the_cutoff_of_a_built_in_filter_in_time() {
    let state = r#"{"send": "cutoff", "target": "filter", "to": "cutoff_hz"}"#;
    let Some(mut harness) = rig(state) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = harness.play(48_000);
    // Leave out where the filter glides from one to the other.
    let dark = brightness(&played[2 * 4_800..2 * 19_200]);
    let bright = brightness(&played[2 * 28_800..2 * 43_200]);
    assert!(bright > 8.0 * dark, "dark {dark}, bright {bright}");
}

#[test]
fn a_number_drives_a_knob_of_a_tool_of_the_project_until_its_cable_goes() {
    let state = r#"{"lead": "drone", "send": "pitch", "target": "drone", "to": "hz"}"#;
    let Some(mut harness) = rig(state) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let driven = harness.play(48_000);
    assert!(
        (rises(&driven) as i64 - 440).abs() <= 2,
        "{}",
        rises(&driven)
    );

    // With no cable the knob glides back to its record.
    harness.write_and_apply("state/rig/instance.json", &record(r#"{"lead": "drone"}"#));
    let free = harness.render(48_000 + 4_800);
    let free = &free[2 * 4_800..];
    assert!((rises(free) as i64 - 220).abs() <= 2, "{}", rises(free));
}

#[test]
fn a_number_a_child_does_not_send_or_take_is_a_problem_that_names_what_it_has() {
    let state = r#"{"send": "cutoff", "target": "filter", "to": "cutoff"}"#;
    let Some(mut harness) = rig(state) else {
        return;
    };
    let messages = |harness: &Harness| -> Vec<String> {
        let problems = harness.project.problems().into_iter();
        problems.map(|problem| problem.message).collect()
    };
    assert_eq!(
        messages(&harness),
        [
            "its behaviour names the number \"cutoff\" of \"filter\" at connections[2].to, which it does not take: it takes cutoff_hz, resonance, drive_db, mix, lfo_rate_hz, lfo_depth_octaves"
        ]
    );
    let state = r#"{"send": "cutof", "target": "filter", "to": "cutoff_hz"}"#;
    harness.write_and_apply("state/rig/instance.json", &record(state));
    harness.render(64);
    assert_eq!(
        messages(&harness),
        [
            "its behaviour names the output \"cutof\" of \"lfo\" at connections[2].from, which it does not have: its outputs are audio, cutoff, pitch"
        ]
    );
}
