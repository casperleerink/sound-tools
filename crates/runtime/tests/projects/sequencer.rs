//! The `sequencer`, a child of a tool of the project that loops note patterns in time with the
//! bars: what it plays from its record in a render, the launches a cable sends it, and the notes
//! it holds when what it plays into changes.

use crate::owners::{closed, rendered_frames};
use crate::support::{BAR, Harness, first_sound, write};

/// The launcher of the doc for agents that write tools, as it is written there.
fn doc_launcher() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let mut blocks = doc
        .split("```ts\n")
        .filter_map(|block| block.split("```").next());
    let block = blocks
        .find(|block| block.contains("number: \"launch\""))
        .unwrap();
    block.to_string()
}

/// The record of the sequencer of its doc, as it is written there.
fn doc_sequencer(harness: &Harness) -> String {
    let doc = (harness.project.agent_docs())
        .find(|doc| doc.name == "sequencer")
        .unwrap()
        .markdown
        .to_string();
    let start = doc.find("```json state/loops/bass.json\n").unwrap();
    let body = &doc[start..];
    body[body.find('\n').unwrap() + 1..body.find("\n```\n").unwrap()].to_string()
}

/// A project of the doc's launcher `loops` at the top, connected to the device, with the
/// sequencer `bass` of `bass`, the doc's when `None`, a synth that lets go at once, a utility,
/// and the tools of `extra` in `extensions/extra.ts`.
fn loops(bass: Option<&str>, extra: &str) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["instrument", "utility"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "loops", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/loops.ts", &doc_launcher());
    write(root, "extensions/extra.ts", extra);
    write(
        root,
        "state/loops/instance.json",
        r#"{"tool": "loops", "state": {}}"#,
    );
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    write(root, "state/loops/synth.json", synth);
    let utility = r#"{"tool": "utility", "state": {}}"#;
    write(root, "state/loops/fx.json", utility);
    let mut harness = Harness::open(folder);
    let bass = bass.map_or_else(|| doc_sequencer(&harness), str::to_string);
    harness.write_and_apply("state/loops/bass.json", &bass);
    Some(harness)
}

#[test]
fn a_render_of_the_launcher_of_the_doc_plays_the_pattern_its_sequencer_names_on_the_bars() {
    let Some(harness) = loops(None, "") else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = rendered_frames(closed(harness).path(), 2 * BAR);
    // Its notes start on beats 1 and 3 and loop after a bar. A note sounds within a
    // millisecond of its frame.
    let mut from = 0;
    for start in [0, 48_000, 96_000, 144_000] {
        let sound = first_sound(&played, from).unwrap();
        assert!((start..start + 48).contains(&sound), "{sound} for {start}");
        // Each note is an eighth long, 12 000 frames: silent again before the next one.
        from = start + 24_000;
    }
}

#[test]
fn an_edit_of_a_pattern_plays_and_a_note_outside_it_is_a_problem() {
    let record = |start: u64| {
        let note = format!(r#"{{"start": {start}, "length": 240, "pitch": 48, "velocity": 100}}"#);
        let patterns = format!(r#"[{{"length": 3840, "notes": [{note}]}}]"#);
        format!(r#"{{"tool": "sequencer", "state": {{"patterns": {patterns}, "plays": 0}}}}"#)
    };
    let Some(mut harness) = loops(Some(&record(960)), "") else {
        return;
    };
    let played = harness.play_from_the_start(BAR);
    // 960 ticks are 24 000 frames.
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(500));

    harness.write_and_apply("state/loops/bass.json", &record(2880));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(1500));
    assert_eq!(harness.project.problems(), []);

    // A bar is 3840 ticks: this note starts after the pattern.
    harness.write_and_apply("state/loops/bass.json", &record(4000));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0), None);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "state/loops/bass.json");
    let message = "patterns[0].notes[0].start must be less than the pattern length 3840, not 4000";
    assert!(problem.message.contains(message), "{problem:?}");
}

/// Sends the number `scene`: 0, then 1 from the third beat of the first bar.
const CONDUCTOR: &str = r#"import { beat, output, tool } from "./sdk";

tool({
  name: "conductor",
  title: "Conductor",
  when: "Never",
  doc: "Sends the scene to play.",
  state: {},
  sound: () => {
    output("scene", beat.ge(2));
    return 0;
  },
});
"#;

#[test]
fn a_number_a_cable_sends_launches_a_pattern_at_the_next_bar_in_a_render() {
    // The second pattern has one note on the second beat.
    let bass = r#"{"tool": "sequencer", "state": {"patterns": [
  {"length": 3840, "notes": [{"start": 0, "length": 480, "pitch": 36, "velocity": 100}, {"start": 1920, "length": 480, "pitch": 43, "velocity": 100}]},
  {"length": 3840, "notes": [{"start": 960, "length": 480, "pitch": 48, "velocity": 100}]}
]}}"#;
    let Some(mut harness) = loops(Some(bass), CONDUCTOR) else {
        return;
    };
    harness.write_and_apply(
        "state/loops/conductor.json",
        r#"{"tool": "conductor", "state": {}}"#,
    );
    let launcher = doc_launcher().replace(
        "connections: [",
        r#"connections: [
      { from: { child: "conductor", number: "scene" }, to: { child: "bass", number: "launch" } },"#,
    );
    let folder = closed(harness);
    write(folder.path(), "extensions/loops.ts", &launcher);
    let played = rendered_frames(folder.path(), 2 * BAR);
    // The first pattern, the cable's 0, plays its bar out; the second from the next bar line.
    for (from, start) in [(0, 0), (24_000, 48_000), (72_000, 120_000)] {
        let sound = first_sound(&played, from).unwrap();
        assert!((start..start + 48).contains(&sound), "{sound} for {start}");
    }
    assert_eq!(first_sound(&played, 144_000), None);
}

/// Plays a pattern of one note held for most of a bar into the child `to` names, or none.
const KEYS: &str = r#"import { text, tool } from "./sdk";

tool({
  name: "keys",
  title: "Keys",
  when: "You want a note held on a child",
  doc: "Holds one note on the child `to` names.",
  state: { to: text({ default: "synth" }) },
  behaviour: ({ state: { to } }) => ({
    connections: [
      ...(to === "none" ? [] : [{ from: { child: "held", notes: "notes" }, to: { child: to, notes: "notes" } }]),
      { from: { child: "synth" }, to: { port: "audio" } },
      { from: { child: "other" }, to: { port: "audio" } },
    ],
  }),
});
"#;

#[test]
fn a_note_a_sequencer_holds_ends_when_it_plays_into_nothing_or_into_another_child() {
    for to in ["none", "other"] {
        if !sound_typescript::has_bun() {
            eprintln!("skipped: Bun is not installed");
            return;
        }
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let project = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "keys", "port": "audio"}, "to": {"device_output": 0}}]}"#;
        write(root, "project.json", project);
        write(root, "extensions/keys.ts", KEYS);
        write(
            root,
            "state/keys/instance.json",
            r#"{"tool": "keys", "state": {}}"#,
        );
        let held = r#"{"tool": "sequencer", "state": {"patterns": [{"length": 3840, "notes": [{"start": 0, "length": 3800, "pitch": 60, "velocity": 100}]}], "plays": 0}}"#;
        write(root, "state/keys/held.json", held);
        let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
        write(root, "state/keys/synth.json", synth);
        write(root, "state/keys/other.json", synth);
        let mut harness = Harness::open(folder);
        assert_eq!(harness.project.problems(), []);
        let held = harness.play(12_000);
        assert!(first_sound(&held, 0).is_some_and(|frame| frame < 48));

        let record = format!(r#"{{"tool": "keys", "state": {{"to": "{to}"}}}}"#);
        harness.write_and_apply("state/keys/instance.json", &record);
        let after = harness.render(24_000);
        // `other` gets the note of the sequencer at the next bar, not before.
        assert_eq!(first_sound(&after, 1_200), None, "{to}");
    }
}
