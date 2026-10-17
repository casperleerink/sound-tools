//! Events that one child of a tool of the project sends out, connected by the parent to another
//! child: notes a sound makes from what it hears into an instrument, a bang into a trigger, a
//! number into a live control. Each lands on its sample, in a render too.

use std::path::Path;

use crate::support::{Harness, first_sound, write};

/// Frames of the hits in `drums.wav`.
const HITS: [usize; 4] = [4_800, 16_000, 30_000, 41_000];

const TOOLS: &str = r#"import { choice, feedback, input, live, max, output, sample, tool, trigger } from "./sdk";

tool({
  name: "tape",
  title: "Tape",
  when: "Never",
  doc: "Plays its sample, a frame at a time.",
  kind: "source",
  state: { sound: sample() },
  sound: ({ sound }) => {
    const frame = feedback();
    frame.set(frame.plus(1));
    return sound.at(frame);
  },
});

tool({
  name: "ears",
  title: "Ears",
  when: "Never",
  doc: "A note of A2 while what it hears is loud, and a bang at each onset.",
  state: {},
  sound: () => {
    const last = feedback();
    const level = max(input.times(input), last.times(0.999));
    last.set(level);
    output("hits", { gate: level.gt(0.01), pitch: 45 });
    output("onset", { bang: level.gt(0.01) });
    output("half", 0.5);
    return input;
  },
});

tool({
  name: "pair",
  title: "Pair",
  when: "Never",
  doc: "Holds C4 on `low` and C5 on `high`, sent in the order `order` names.",
  kind: "source",
  state: { order: choice({ options: ["low first", "high first"], default: "low first" }) },
  sound: ({ order }) => {
    const low = () => output("low", { gate: 1, pitch: 60 });
    const high = () => output("high", { gate: 1, pitch: 72 });
    if (order === "low first") { low(); high(); } else { high(); low(); }
    return 0;
  },
});

tool({
  name: "drop",
  title: "Drop",
  when: "Never",
  doc: "Its trigger, at its live control `gain`.",
  kind: "source",
  state: {},
  controls: { gain: live({ min: 0, max: 1, default: 1 }), hit: trigger() },
  sound: ({ gain, hit }) => hit.times(gain),
});
"#;

/// The drums into `ears`, and `connections` of the parent's behaviour, a list, which plain
/// JavaScript may get wrong where TypeScript would not, and `more` of the declaration. `ears`
/// plays into nothing.
fn listener(connections: &str, more: &str) -> String {
    format!(
        r#"import {{ tool }} from "./sdk";

tool({{
  name: "listener",
  title: "Listener",
  when: "Never",
  doc: "Hears the drums.",
  state: {{}},
  behaviour: () => ({{ connections: [{{ from: {{ child: "drums" }}, to: {{ child: "ears" }} }}, ...{connections}]{more} }}),
}});
"#
    )
}

/// A project of the parent `listener` at the top with `connections`, and its children: the
/// drums, which play `drums.wav`, `ears`, which hears them, a synth and `drop`.
fn project(connections: &str) -> Option<Harness> {
    project_with(connections, "")
}

/// [`project`], with `more` in the declaration.
fn project_with(connections: &str, more: &str) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "listener", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/tools.ts", TOOLS);
    write(root, "extensions/listener.ts", &listener(connections, more));
    drums(root);
    let state = |name: &str, record: &str| write(root, &format!("state/listener/{name}"), record);
    state("instance.json", r#"{"tool": "listener", "state": {}}"#);
    state(
        "drums.json",
        r#"{"tool": "tape", "state": {"sound": "drums.wav"}}"#,
    );
    state("ears.json", r#"{"tool": "ears", "state": {}}"#);
    state("drop.json", r#"{"tool": "drop", "state": {}}"#);
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    state("synth.json", synth);
    Some(Harness::open(folder))
}

/// A second of drum hits at [`HITS`], as a recording of a microphone stands in: each a burst
/// that dies away in 5 ms.
fn drums(root: &Path) {
    let path = root.join("assets/audio/drums.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..48_000 {
        let since = HITS
            .iter()
            .rev()
            .find(|hit| **hit <= frame)
            .map(|hit| frame - hit);
        let burst = since.filter(|since| *since < 240).map_or(0.0, |since| {
            let sign = if since % 2 == 0 { 1.0 } else { -1.0 };
            sign * (1.0 - since as f32 / 240.0)
        });
        writer.write_sample(burst).unwrap();
    }
    writer.finalize().unwrap();
}

fn messages(harness: &Harness) -> Vec<String> {
    let problems = harness.project.problems().into_iter();
    problems.map(|problem| problem.message).collect()
}

#[test]
fn the_hits_of_a_recording_play_a_synth_on_their_samples_through_notes_a_sound_makes() {
    let connections = r#"[
    { from: { child: "ears", notes: "hits" }, to: { child: "synth", notes: "notes" } },
    { from: { child: "synth" }, to: "sound" },
  ]"#;
    let Some(mut harness) = project(connections) else {
        return;
    };
    assert_eq!(messages(&harness), Vec::<String>::new());
    let played = harness.play(48_000);
    let mut from = 0;
    for hit in HITS {
        // The synth sounds within a millisecond of its note, and is silent between them.
        let sound = first_sound(&played, from).unwrap();
        assert!((hit..hit + 48).contains(&sound), "{sound} for {hit}");
        from = hit + 9_600;
    }
}

#[test]
fn a_bang_fires_a_trigger_on_its_sample_and_a_number_moves_a_live_control() {
    let connections = r#"[
    { from: { child: "ears", bang: "onset" }, to: { child: "drop", trigger: "hit" } },
    { from: { child: "drop" }, to: "sound" },
  ]"#;
    let Some(mut harness) = project(connections) else {
        return;
    };
    assert_eq!(messages(&harness), Vec::<String>::new());
    let fired = |played: &[f32]| -> Vec<(usize, f32)> {
        let left = played.iter().step_by(2).copied().enumerate();
        left.filter(|(_, sample)| *sample != 0.0).collect()
    };
    let played = harness.play(48_000);
    assert_eq!(fired(&played), HITS.map(|hit| (hit, 1.0)));

    let connections = r#"[
    { from: { child: "ears", bang: "onset" }, to: { child: "drop", trigger: "hit" } },
    { from: { child: "ears", number: "half" }, to: { child: "drop", live: "gain" } },
    { from: { child: "drop" }, to: "sound" },
  ]"#;
    let Some(mut harness) = project(connections) else {
        return;
    };
    assert_eq!(fired(&harness.play(48_000)), HITS.map(|hit| (hit, 0.5)));
}

#[test]
fn a_cable_between_two_kinds_or_to_a_control_a_child_does_not_have_is_a_problem() {
    let connections = r#"[
    { from: { child: "ears", notes: "hits" }, to: { child: "drop", number: "gain" } },
    { from: { child: "ears", bang: "onset" }, to: { child: "drop", trigger: "gain" } },
    { from: { child: "ears", number: "half" }, to: { child: "drop", live: "level" } },
  ]"#;
    let Some(harness) = project(connections) else {
        return;
    };
    assert_eq!(
        messages(&harness),
        [
            "its behaviour declares connections[1] from notes to a number: a sound goes to { child, port? } or ownSound, notes to { child, notes }, a number to { child, number } or { child, live }, a bang to { child, trigger }",
            "its behaviour names the trigger \"gain\" of \"drop\" at connections[2].to, which it does not have: its triggers are hit",
            "its behaviour names the live control \"level\" of \"drop\" at connections[3].to, which it does not have: its live controls are gain",
        ]
    );
}

#[test]
fn a_behaviour_that_declares_players_says_a_sequencer_plays_patterns_now() {
    let Some(harness) = project_with("[]", ", players: {}") else {
        return;
    };
    let messages = messages(&harness);
    let [message] = messages.as_slice() else {
        panic!("{messages:?}");
    };
    assert!(
        message.contains("`players` are gone: a child of tool `sequencer`"),
        "{message}"
    );
}

#[test]
fn a_held_note_sounds_on_when_new_code_of_its_sender_moves_its_output() {
    let connections = r#"[
    { from: { child: "pair", notes: "high" }, to: { child: "synth", notes: "notes" } },
    { from: { child: "synth" }, to: "sound" },
  ]"#;
    let Some(mut harness) = project(connections) else {
        return;
    };
    let pair = |order: &str| format!(r#"{{"tool": "pair", "state": {{"order": "{order}"}}}}"#);
    harness.write_and_apply("state/listener/pair.json", &pair("low first"));
    assert_eq!(messages(&harness), Vec::<String>::new());
    assert!(first_sound(&harness.play(4_800), 0).is_some());

    harness.write_and_apply("state/listener/pair.json", &pair("high first"));
    // After the fade from the old sound of `pair`, and the synth's release were it let go.
    let played = harness.render(9_600);
    assert!(first_sound(&played, 4_800).is_some());
}
