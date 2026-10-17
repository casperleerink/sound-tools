//! Tools of the project that own children: a behaviour in TypeScript says how the children
//! connect, Bun works it out while the sound plays, and a render waits for it. Bun is not part
//! of the build, so where it is not installed, these tests say so and pass.

use std::path::Path;

use sound_core::InstanceId;

use crate::plugin_hosts::open_read_only;
use crate::support::{BAR, Harness, write};

/// A sine at `hz` that runs all the time.
const DRONE: &str = r#"import { knob, sine, tool } from "./sdk";

tool({
  name: "drone",
  title: "Drone",
  when: "You want a drone",
  doc: "A sine that never stops.",
  kind: "source",
  state: { hz: knob({ min: 20, max: 2000, default: 220, unit: "hz" }) },
  sound: ({ hz }) => sine(hz).times(0.25),
});
"#;

/// Plays the child its record names through the effects it lists, into its own sound. It throws
/// for a lead named `boom`.
const BAND: &str = r#"import { input, knob, list, text, tool, type Connection } from "./sdk";

tool({
  name: "band",
  title: "Band",
  when: "You want a band",
  doc: "Plays one child through effects.",
  state: {
    lead: text({ default: "low" }),
    effects: list(text(), { max: 4 }),
    level: knob({ min: 0, max: 1, default: 1 }),
  },
  sound: ({ level }) => input.times(level),
  behaviour: ({ state: { lead, effects } }) => {
    if (lead === "boom") throw new Error("boom");
    const chain = [lead, ...effects];
    const connections: Connection[] = chain.slice(1).map((effect, index) => ({
      from: { child: chain[index] },
      to: { child: effect },
    }));
    connections.push({ from: { child: chain[chain.length - 1] }, to: "sound" });
    return { connections };
  },
});
"#;

/// The owner of the doc for agents that write tools, as it is written there.
fn doc_owner() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let mut blocks = doc.split("```ts\n");
    let block = blocks.find(|block| block.contains("behaviour:")).unwrap();
    block.split("```").next().unwrap().to_string()
}

/// A project of `band` at the top, connected to the device, with three drones and a delay.
fn band(record: &str) -> Option<Harness> {
    band_of(BAND, record)
}

/// [`band`], with the tool `band` of `code`.
fn band_of(code: &str, record: &str) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["delay"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "band", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/drone.ts", DRONE);
    write(root, "extensions/band.ts", code);
    write(root, "state/band/instance.json", record);
    write(
        root,
        "state/band/drone.json",
        r#"{"tool": "drone", "state": {"hz": 330}}"#,
    );
    write(
        root,
        "state/band/low.json",
        r#"{"tool": "drone", "state": {}}"#,
    );
    write(
        root,
        "state/band/high.json",
        r#"{"tool": "drone", "state": {"hz": 880}}"#,
    );
    write(
        root,
        "state/band/echo.json",
        r#"{"tool": "delay", "state": {}}"#,
    );
    Some(Harness::open(folder))
}

/// How often the left channel rises through zero in a second of `samples`.
fn rises(samples: &[f32]) -> usize {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let pairs = left.windows(2);
    pairs.filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0).count()
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "band", "state": {state}}}"#)
}

#[test]
fn an_owner_plays_its_source_child_through_its_effect_child_into_its_own_sound() {
    let Some(mut harness) = band_of(&doc_owner(), &record(r#"{"effects": ["echo"]}"#)) else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = harness.play(48_000);
    let peak = played
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.1, "peak {peak}");
    // The drone, through the delay, whose echo of it is in tune with it.
    assert!(
        (rises(&played) as i64 - 330).abs() <= 2,
        "{}",
        rises(&played)
    );
}

#[test]
fn a_change_of_the_record_plays_the_last_wiring_until_bun_answers() {
    let Some(mut harness) = band(&record("{}")) else {
        return;
    };
    let low = harness.play(48_000);
    assert!((rises(&low) as i64 - 220).abs() <= 2, "{}", rises(&low));

    harness.write_and_apply("state/band/instance.json", &record(r#"{"lead": "high"}"#));
    // Nothing waited for Bun: the engine plays what was declared before.
    let mut block = vec![0.0; 512 * 2];
    harness.engine.process_block(&mut block);
    assert!(rises(&block) <= 3, "{}", rises(&block));
    // A render waits for its answer, and plays it from its first block.
    let high = harness.render(48_000);
    assert!((rises(&high) as i64 - 880).abs() <= 2, "{}", rises(&high));
    assert_eq!(harness.project.problems(), []);

    // A behaviour that throws keeps what it declared last, and says why.
    harness.write_and_apply("state/band/instance.json", &record(r#"{"lead": "boom"}"#));
    let kept = harness.render(48_000);
    assert!((rises(&kept) as i64 - 880).abs() <= 2, "{}", rises(&kept));
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "state/band/instance.json");
    assert!(
        problem
            .message
            .contains("its behaviour failed: Error: boom"),
        "{problem:?}"
    );
}

#[test]
fn a_project_opened_again_plays_nothing_an_earlier_opening_declared() {
    let Some(mut harness) = band(&record(r#"{"lead": "high"}"#)) else {
        return;
    };
    let high = harness.play(48_000);
    assert!((rises(&high) as i64 - 880).abs() <= 2, "{}", rises(&high));

    // The behaviour throws from the start of the next opening, which has declared nothing.
    harness.write("state/band/instance.json", &record(r#"{"lead": "boom"}"#));
    let mut harness = harness.reopen();
    let played = harness.play(4_800);
    assert!(played.iter().all(|sample| *sample == 0.0));
    let problems = harness.project.problems();
    assert!(problems[0].message.contains("Error: boom"), "{problems:?}");
}

#[test]
fn a_missing_child_is_a_problem_that_names_it_and_the_rest_plays() {
    let Some(mut harness) = band(&record(r#"{"effects": ["reverb", "echo"]}"#)) else {
        return;
    };
    let problems: Vec<String> = (harness.project.problems().into_iter())
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect();
    let missing = |at: &str| {
        format!(
            "state/band/instance.json: its behaviour names the child \"reverb\" at connections[{at}, which it does not own: its children are drone, echo, high, low"
        )
    };
    assert_eq!(problems, [missing("0].to"), missing("1].from")]);
    // `echo` plays into the sound, and nothing reaches it.
    let silent = harness.play(4_800);
    assert!(silent.iter().all(|sample| *sample == 0.0));

    harness.write_and_apply("state/band/instance.json", &record(r#"{"lead": "ghost"}"#));
    let played = harness.render(4_800);
    assert!(played.iter().all(|sample| *sample == 0.0));
    let problems = harness.project.problems();
    assert!(problems[0].message.contains("\"ghost\""), "{problems:?}");
}

/// The folder of the project, which is closed.
fn closed(harness: Harness) -> tempfile::TempDir {
    let Harness {
        project,
        engine,
        folder,
        ..
    } = harness;
    drop((project, engine));
    folder
}

/// The bytes of a render from the start, as `--render` opens the project.
fn rendered(folder: &Path) -> Vec<f32> {
    rendered_frames(folder, 48_000)
}

fn rendered_frames(folder: &Path, frames: usize) -> Vec<f32> {
    let (mut project, mut engine, plugins) = open_read_only(folder);
    project.engine().play();
    runtime::render(&mut project, &mut engine, &plugins, frames).unwrap()
}

#[test]
fn a_render_plays_the_declaration_from_its_first_block_and_the_same_every_time() {
    let Some(harness) = band(&record(r#"{"lead": "high", "effects": ["echo"]}"#)) else {
        return;
    };
    let folder = closed(harness);
    let first = rendered(folder.path());
    assert!((rises(&first[..2 * 4_800]) as i64 - 88).abs() <= 1);
    assert_eq!(first, rendered(folder.path()));
}

#[test]
fn a_render_starts_with_every_owner_wired_also_more_than_the_engine_takes_in_one_block() {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return;
    }
    // More owners than the 64 edits the engine takes between two polls, each one an edit once
    // Bun answers.
    const OWNERS: usize = 80;
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let connections: Vec<String> = (0..OWNERS)
        .map(|owner| {
            format!(r#"{{"from": {{"instance": "band-{owner}", "port": "audio"}}, "to": {{"device_output": 0}}}}"#)
        })
        .collect();
    let project = format!(
        r#"{{"format": 1, "extensions": [],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{}]}}"#,
        connections.join(", ")
    );
    write(root, "project.json", &project);
    write(root, "extensions/drone.ts", &DRONE.replace("0.25", "0.01"));
    write(root, "extensions/band.ts", BAND);
    for owner in 0..OWNERS {
        write(
            root,
            &format!("state/band-{owner}/instance.json"),
            &record("{}"),
        );
        let drone = r#"{"tool": "drone", "state": {}}"#;
        write(root, &format!("state/band-{owner}/low.json"), drone);
    }
    let played = rendered_frames(root, 4_800);
    let peak = |samples: &[f32]| {
        samples
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    };
    // The drones start together in phase: every one of them sounds in the first block.
    let all = OWNERS as f32 * 0.01;
    assert!(
        (peak(&played[..2 * 512]) - all).abs() < 0.01,
        "{}",
        peak(&played[..2 * 512])
    );
    assert!((peak(&played) - all).abs() < 0.01, "{}", peak(&played));
}

#[test]
fn the_example_of_the_doc_of_an_owner_loads() {
    let Some(harness) = band(&record("{}")) else {
        return;
    };
    let doc = (harness.project.agent_docs())
        .find(|doc| doc.name == "band")
        .unwrap()
        .markdown
        .to_string();
    let start = doc.find("```json state/band/instance.json\n").unwrap();
    let body = &doc[start..];
    let example = &body[body.find('\n').unwrap() + 1..body.find("\n```\n").unwrap()];
    let folder = closed(harness);
    write(folder.path(), "state/band/instance.json", example);
    let (project, ..) = open_read_only(folder.path());
    assert_eq!(project.problems(), []);
    let band = InstanceId::new("band").unwrap();
    assert_eq!(project.tool_of(&band), Some("band"));
}

#[test]
fn an_owner_in_the_effects_of_a_track_plays_its_children_into_the_track() {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return;
    }
    let harness = Harness::new();
    harness.write("extensions/drone.ts", DRONE);
    harness.write("extensions/band.ts", &doc_owner());
    let track = r#"{"tool": "arrangement.track", "state": {"name": "pad", "order": 1, "effects": ["band"]}}"#;
    harness.write("state/arrangement/pad/instance.json", track);
    harness.write("state/arrangement/pad/band/instance.json", &record("{}"));
    harness.write(
        "state/arrangement/pad/band/drone.json",
        r#"{"tool": "drone", "state": {"hz": 330}}"#,
    );
    let harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let played = rendered(closed(harness).path());
    assert!(
        (rises(&played) as i64 - 330).abs() <= 2,
        "{}",
        rises(&played)
    );
}

/// The launcher of the doc for agents that write tools, as it is written there.
fn doc_launcher() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let mut blocks = doc.split("```ts\n");
    let block = blocks.find(|block| block.contains("players:")).unwrap();
    block.split("```").next().unwrap().to_string()
}

/// A project of the doc's launcher `loops` at the top, connected to the device, with a synth
/// that lets go at once and a utility as its children.
fn loops(state: &str) -> Option<Harness> {
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
    let record = format!(r#"{{"tool": "loops", "state": {state}}}"#);
    write(root, "state/loops/instance.json", &record);
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    write(root, "state/loops/synth.json", synth);
    write(
        root,
        "state/loops/fx.json",
        r#"{"tool": "utility", "state": {}}"#,
    );
    Some(Harness::open(folder))
}

/// The first frame from `from` on whose left channel sounds.
fn first_sound(samples: &[f32], from: usize) -> Option<usize> {
    let left = samples.iter().step_by(2).enumerate().skip(from);
    let mut sounding = left.filter(|(_, sample)| sample.abs() > 1e-4);
    sounding.next().map(|(frame, _)| frame)
}

#[test]
fn a_render_of_the_launcher_of_the_doc_at_its_defaults_plays_its_pattern_on_the_bars() {
    let Some(harness) = loops("{}") else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let folder = closed(harness);
    let played = rendered_frames(folder.path(), 2 * BAR);
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
fn an_edit_of_a_pattern_plays_once_bun_answers_and_a_note_outside_it_is_a_problem() {
    let record = |start: u64| {
        let note = format!(r#"{{"start": {start}, "length": 240, "pitch": 48, "velocity": 100}}"#);
        format!(r#"{{"tool": "loops", "state": {{"bass": [{{"notes": [{note}]}}]}}}}"#)
    };
    let Some(mut harness) = loops("{}") else {
        return;
    };
    harness.write_and_apply("state/loops/instance.json", &record(960));
    let played = harness.play_from_the_start(BAR);
    // 960 ticks are 24 000 frames.
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(500));

    harness.write_and_apply("state/loops/instance.json", &record(2880));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(1500));
    assert_eq!(harness.project.problems(), []);

    // A bar is 3840 ticks: this note starts after the pattern.
    harness.write_and_apply("state/loops/instance.json", &record(4000));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0), None);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "state/loops/instance.json");
    let message = "players.bass.patterns[0].notes[0].start must be less than the pattern length 3840, not 4000";
    assert!(problem.message.contains(message), "{problem:?}");
}

/// Holds one note for most of a bar on the child `to` names, or on none.
const KEYS: &str = r#"import { text, tool } from "./sdk";

tool({
  name: "keys",
  title: "Keys",
  when: "You want a note held on a child",
  doc: "Holds one note on the child `to` names.",
  state: { to: text({ default: "synth" }) },
  behaviour: ({ state: { to } }) => ({
    players: to === "none" ? {} : {
      held: { patterns: [{ length: 3840, notes: [{ start: 0, length: 3800, pitch: 60, velocity: 100 }] }], plays: 0 },
    },
    connections: [
      ...(to === "none" ? [] : [{ from: { player: "held" }, to: { child: to } }]),
      { from: { child: "synth" }, to: "sound" },
      { from: { child: "other" }, to: "sound" },
    ],
  }),
});
"#;

#[test]
fn a_note_a_player_holds_ends_when_the_player_goes_away_or_plays_into_another_child() {
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
        // `other` gets the note of the player at the next bar, not before.
        assert_eq!(first_sound(&after, 1_200), None, "{to}");
    }
}
