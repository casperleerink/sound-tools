//! Tools of the project that own children: a behaviour in TypeScript says how the children
//! connect, Bun works it out while the sound plays, and a render waits for it. Bun is not part
//! of the build, so where it is not installed, as in CI, these tests say so and pass.

use std::path::Path;

use sound_core::InstanceId;

use crate::plugin_hosts::open_read_only;
use crate::support::{Harness, write};

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
    let (mut project, mut engine, plugins) = open_read_only(folder);
    project.engine().play();
    runtime::render(&mut project, &mut engine, &plugins, 48_000).unwrap()
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
