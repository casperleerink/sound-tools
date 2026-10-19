//! Timelines that loop, the clip launcher of the doc for agents: what they play from their
//! records in a render, the launches a cable sends them, and the notes they hold when what they
//! play into changes.

use crate::owners::{closed, rendered_frames};
use crate::support::{BAR, Harness, first_sound, write};
use sound_core::InstanceId;

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

/// A looping timeline of a bar that waits for a launch, with one note on the second beat.
const HIGH: &str = r#"{"tool": "timeline", "state": {"length": 3840, "loop": true, "plays": false, "notes": [
  {"start": 960, "length": 480, "pitch": 48, "velocity": 100}]}}"#;

/// A looping timeline of a bar with notes on beats 1 and 3, each an eighth long.
const LOW: &str = r#"{"tool": "timeline", "state": {"length": 3840, "loop": true, "notes": [
  {"start": 0, "length": 480, "pitch": 36, "velocity": 100}, {"start": 1920, "length": 480, "pitch": 43, "velocity": 100}]}}"#;

/// A project of the doc's launcher `loops` at the top, connected to the device, with the
/// timeline `low` of `low` and [`HIGH`], a synth that lets go at once, a utility, and
/// the tools of `extra` in `extensions/extra.ts`.
fn loops(low: &str, extra: &str) -> Option<Harness> {
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
    write(root, "state/loops/high.json", HIGH);
    let mut harness = Harness::open(folder);
    harness.write_and_apply("state/loops/low.json", low);
    Some(harness)
}

#[test]
fn a_render_of_the_launcher_of_the_doc_loops_the_timeline_that_plays_on_the_bars() {
    let Some(harness) = loops(LOW, "") else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = rendered_frames(closed(harness).path(), 2 * BAR);
    // Its notes start on beats 1 and 3 and loop after a bar. A note sounds within a
    // millisecond of its frame; `high` waits for a launch.
    let mut from = 0;
    for start in [0, 48_000, 96_000, 144_000] {
        let sound = first_sound(&played, from).unwrap();
        assert!((start..start + 48).contains(&sound), "{sound} for {start}");
        // Each note is an eighth long, 12 000 frames: silent again before the next one.
        from = start + 24_000;
    }
}

#[test]
fn an_edit_of_a_loop_plays_and_a_note_outside_it_is_refused() {
    let record = |start: u64| {
        let note = format!(r#"{{"start": {start}, "length": 240, "pitch": 48, "velocity": 100}}"#);
        format!(
            r#"{{"tool": "timeline", "state": {{"length": 3840, "loop": true, "notes": [{note}]}}}}"#
        )
    };
    let Some(mut harness) = loops(&record(960), "") else {
        return;
    };
    let played = harness.play_from_the_start(BAR);
    // 960 ticks are 24 000 frames.
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(500));

    harness.write_and_apply("state/loops/low.json", &record(2880));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(1500));
    assert_eq!(harness.project.problems(), []);

    // A bar is 3840 ticks: this note starts after the round. The record is refused, so the
    // last one plays on.
    harness.write_and_apply("state/loops/low.json", &record(4000));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(1500));
    let problems = harness.project.problems();
    let problem = problems
        .iter()
        .find(|problem| problem.path == "state/loops/low.json")
        .unwrap_or_else(|| panic!("{problems:?}"));
    let message = "notes[0].start must be less than the length 3840, not 4000";
    assert!(problem.message.contains(message), "{problem:?}");
}

/// Sends the number `first`, 1 until the third beat of the first bar, and `second`, 1 from
/// there.
const CONDUCTOR: &str = r#"import { beat, output, tool } from "./sdk";

tool({
  name: "conductor",
  title: "Conductor",
  when: "Never",
  doc: "Sends what to play.",
  state: {},
  sound: () => {
    output("first", beat.lt(2));
    output("second", beat.ge(2));
    return 0;
  },
});
"#;

#[test]
fn a_number_a_cable_sends_starts_and_stops_loops_at_the_next_bar_in_a_render() {
    let Some(mut harness) = loops(LOW, CONDUCTOR) else {
        return;
    };
    harness.write_and_apply(
        "state/loops/conductor.json",
        r#"{"tool": "conductor", "state": {}}"#,
    );
    let launcher = doc_launcher().replace(
        "connections: [",
        r#"connections: [
      { from: { child: "conductor", number: "first" }, to: { child: "low", number: "launch" } },
      { from: { child: "conductor", number: "second" }, to: { child: "high", number: "launch" } },"#,
    )
    .replace(
        "{ low: \"timeline\", high: \"timeline\",",
        "{ conductor: \"conductor\", low: \"timeline\", high: \"timeline\",",
    );
    let folder = closed(harness);
    write(folder.path(), "extensions/loops.ts", &launcher);
    let played = rendered_frames(folder.path(), 2 * BAR);
    // `low` plays its bar out and stops at the next bar line; `high`, one note on the second
    // beat, starts there.
    for (from, start) in [(0, 0), (24_000, 48_000), (72_000, 120_000)] {
        let sound = first_sound(&played, from).unwrap();
        assert!((start..start + 48).contains(&sound), "{sound} for {start}");
    }
    assert_eq!(first_sound(&played, 144_000), None);
}

/// Plays a loop of one note held for most of a bar into the child `to` names, or none.
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
fn a_note_a_loop_holds_ends_when_it_plays_into_nothing_or_into_another_child() {
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
        let held = r#"{"tool": "timeline", "state": {"length": 3840, "loop": true, "notes": [{"start": 0, "length": 3800, "pitch": 60, "velocity": 100}]}}"#;
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
        // `other` gets the note of the loop at the next round, not before.
        assert_eq!(first_sound(&after, 1_200), None, "{to}");
    }
}

#[test]
fn a_timeline_that_starts_to_loop_shows_that_it_plays() {
    let once = r#"{"tool": "timeline", "state": {"length": 3840, "notes": [
  {"start": 0, "length": 480, "pitch": 36, "velocity": 100}]}}"#;
    let Some(mut harness) = loops(once, "") else {
        return;
    };
    harness.write_and_apply("state/loops/low.json", LOW);
    harness.play_from_the_start(BAR / 2);
    let low = InstanceId::new("loops/low").unwrap();
    let watches = harness.project.watches(&low);
    let playing = watches.iter().find(|(name, _)| name == "playing");
    assert_eq!(playing.map(|(_, watch)| watch.get()), Some(1.0));
}
