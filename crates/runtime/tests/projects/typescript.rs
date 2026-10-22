//! Tools a project writes in TypeScript in its `extensions/` folder, run by Bun: the first
//! example of their agent doc loads as it is written and plays, and a bank of combs whose shape
//! is a choice plays with every choice. Bun is not part of the build, so where it is not
//! installed, these tests say so and pass.

use std::path::Path;

use crate::rack::racks_files;
use crate::support::{BAR, Harness, clip, synth, write};

/// The first TypeScript example of the doc for agents that write tools: a whole tool.
fn doc_example() -> String {
    let doc = sound_typescript::AGENT_DOCS[0].markdown;
    let block = doc.split("```ts\n").nth(1).unwrap();
    block.split("```").next().unwrap().to_string()
}

/// Combs whose number is a choice: `sound` builds a different graph for each.
const COMBS: &str = r#"import { choice, delay, feedback, input, number, lowpass, mix, tool, type Signal } from "./sdk";

const TIMES = [29.7, 37.1, 41.1, 43.7, 31.3, 39.9, 45.1, 33.5];

tool({
  name: "combs",
  title: "Combs",
  when: "You want a ringing room",
  doc: "Feedback combs.",
  state: {
    size: number({ min: 0, max: 0.95, default: 0.8 }),
    combs: choice({ options: [2, 4, 8], default: 4 }),
    blend: number({ min: 0, max: 1, default: 0.3 }),
  },
  sound: ({ size, combs, blend }) => {
    const rings: Signal[] = TIMES.slice(0, combs).map((ms) => {
      const comb = feedback();
      comb.set(lowpass(delay(input.plus(comb.times(size)), ms), 5000));
      return comb;
    });
    const wet = rings.reduce((sum, ring) => sum.plus(ring)).over(combs);
    return mix(input, wet, blend);
  },
});
"#;

/// A project whose `extensions/` holds the first example of the doc, `wobble`, `COMBS` and `LEVEL`,
/// with a rack `pad` that plays a held chord through `effects`.
fn pad_through(effects: &[(&str, &str)]) -> Harness {
    pad_through_with(effects, &[])
}

/// [`pad_through`], with lanes of the rack: each its name, the number it moves, such as
/// `wobble.depth`, and its points.
fn pad_through_with(effects: &[(&str, &str)], lanes: &[(&str, &str, &str)]) -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "extensions/wobble.ts", &doc_example());
    write(folder.path(), "extensions/combs.ts", COMBS);
    write(folder.path(), "extensions/level.ts", LEVEL);
    let (synth, chord) = (
        synth(0.2),
        clip(0, 15360, &[(0, 15360, 60), (0, 15360, 64), (0, 15360, 67)]),
    );
    let records: Vec<(&str, String)> = (effects.iter())
        .map(|(name, state)| (*name, format!(r#"{{"tool": "{name}", "state": {state}}}"#)))
        .chain(lanes.iter().map(|(name, _, points)| {
            (
                *name,
                format!(r#"{{"tool": "lane", "state": {{"points": {points}}}}}"#),
            )
        }))
        .collect();
    let mut children = vec![("instrument", synth.as_str()), ("chord", chord.as_str())];
    children.extend(
        records
            .iter()
            .map(|(name, record)| (*name, record.as_str())),
    );
    for (path, contents) in racks_files(&[("pad", &children)]) {
        write(folder.path(), &path, &contents);
    }
    let order: Vec<&str> = effects.iter().map(|(name, _)| *name).collect();
    let automation: Vec<_> = (lanes.iter())
        .map(|(lane, target, _)| serde_json::json!({ "lane": lane, "target": target }))
        .collect();
    let rack = serde_json::json!({ "tool": "rack", "state": { "order": order, "automation": automation } });
    write(folder.path(), "state/pad/instance.json", &rack.to_string());
    let harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness
}

/// The loudness of the left channel over 20 ms around `seconds`.
fn loudness_at(samples: &[f32], seconds: f32) -> f32 {
    let frames = 48_000.0 * 0.02;
    let start = ((seconds * 48_000.0 - frames / 2.0) as usize) * 2;
    let window = &samples[start..start + frames as usize * 2];
    let left = window.iter().step_by(2);
    (left.map(|sample| sample * sample).sum::<f32>() / frames).sqrt()
}

#[test]
fn the_tremolo_of_the_doc_dips_to_silence_at_its_rate() {
    let mut harness = pad_through(&[("wobble", r#"{"rate": 2, "depth": 1}"#)]);
    let output = harness.play_from_the_start(BAR);
    // At 2 Hz and full depth the wave is at its top, and the sound at silence, a quarter of
    // the way into each half second, and the sound is at its loudest three quarters in.
    for half_second in 1..4 {
        let start = half_second as f32 * 0.5;
        let (dip, top) = (
            loudness_at(&output, start + 0.125),
            loudness_at(&output, start + 0.375),
        );
        assert!(top > 0.01, "silent at {start} s");
        assert!(dip < top * 0.1, "{dip} is no dip under {top} at {start} s");
    }
}

#[test]
fn combs_play_with_every_choice_and_ring_on_after_the_sound() {
    for combs in [2, 4, 8] {
        let state = format!(r#"{{"combs": {combs}, "size": 0.9, "blend": 1}}"#);
        let mut harness = pad_through(&[("combs", &state)]);
        let output = harness.play_from_the_start(5 * BAR);
        let peak = output
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak > 0.01 && peak < 1.0, "{combs} combs: peak {peak}");
        // The chord ends at 8 s and the synth releases in 0.3 s; the combs still ring.
        assert!(
            loudness_at(&output, 8.5) > 0.001,
            "{combs} combs do not ring on"
        );
    }
}

#[test]
fn a_knob_the_record_leaves_out_is_automated_from_its_default() {
    let harness = pad_through(&[("wobble", r#"{"rate": 2}"#)]);
    let wobble = sound_core::InstanceId::new("pad/wobble").unwrap();
    let depth = harness.project.automation(&wobble, "depth").unwrap();
    // A lane added for it starts where it plays: at the default of the doc's `depth`.
    assert_eq!(depth.record, Some(0.5));
}

#[test]
fn a_lane_moves_a_knob_of_the_tremolo() {
    // The record dips to silence; the lane holds the depth at 0 for the whole bar.
    let points = r#"[{"tick": 0, "value": 0.0}, {"tick": 15360, "value": 0.0}]"#;
    let lane = [("depth", "wobble.depth", points)];
    let mut harness = pad_through_with(&[("wobble", r#"{"rate": 2, "depth": 1}"#)], &lane);
    let output = harness.play_from_the_start(BAR);
    for half_second in 1..4 {
        let start = half_second as f32 * 0.5;
        let (dip, top) = (
            loudness_at(&output, start + 0.125),
            loudness_at(&output, start + 0.375),
        );
        assert!(dip > top * 0.8, "{dip} dips under {top} at {start} s");
    }
}

/// A gain a lane can move.
const LEVEL: &str = r#"import { input, number, tool } from "./sdk";

tool({
  name: "level",
  title: "Level",
  when: "You want a gain",
  doc: "A gain.",
  state: { level: number({ min: 0, max: 1, default: 1 }) },
  sound: ({ level }) => input.times(level),
});
"#;

#[test]
fn a_file_that_hangs_as_it_loads_is_a_problem_and_the_other_tools_load() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/level.ts", LEVEL);
    crate::support::write(folder.path(), "extensions/stuck.ts", "while (true) {}\n");
    crate::support::write(
        folder.path(),
        "state/level.json",
        r#"{"tool": "level", "state": {}}"#,
    );
    let harness = Harness::open(folder);
    // The record of `level` loaded: its tool is there.
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/stuck.ts");
    assert!(
        problem.message.contains("did not finish loading"),
        "{problem:?}"
    );
}

#[test]
fn a_tool_whose_pattern_cannot_be_is_a_problem_before_anything_is_made_of_it() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let steps = r#"import { input, pattern, tool } from "./sdk";

tool({
  name: "steps",
  title: "Steps",
  when: "You want steps",
  doc: "Steps.",
  state: { steps: pattern({ length: 1000000000000, min: 0, max: 1, default: 0 }) },
  sound: () => input,
});
"#;
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/steps.ts", steps);
    let harness = Harness::open(folder);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/steps.ts");
    assert!(
        problem.message.contains("a pattern holds 1 to 1024 values"),
        "{problem:?}"
    );
}

#[test]
fn a_key_a_tool_does_not_have_is_a_problem_that_names_the_keys_it_has() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let old = LEVEL.replace("sound:", "onMidi: () => {},\n  sound:");
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/level.ts", &old);
    let harness = Harness::open(folder);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert!(
        problem
            .message
            .contains("onMidi: a tool has no onMidi; its keys are")
            && problem.message.contains("onNotes"),
        "{problem:?}"
    );
}

#[test]
fn a_key_the_app_keeps_is_a_problem_of_the_tool_that_takes_it() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let greedy = LEVEL.replace(
        "sound:",
        "globalKeys: [\"space\", \"cmd-z\"],\n  onKey: () => {},\n  sound:",
    );
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/level.ts", &greedy);
    let harness = Harness::open(folder);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert!(
        problem.message.contains("cmd-z is the app's"),
        "{problem:?}"
    );
}

/// Every function of the sound graph once, in mono and in stereo by its `out`, so a node the
/// SDK sends and the runtime does not read fails a test.
const EVERY_NODE: &str = r#"import * as s from "./sdk";

s.tool({
  name: "every",
  title: "Every node",
  when: "Never",
  doc: "Every function of the sound graph.",
  state: {
    out: s.choice({ options: ["mono", "stereo"], default: "mono" }),
    level: s.number({ min: 0, max: 1, default: 0.5 }),
    on: s.boolean({ default: true }),
    steps: s.pattern({ length: 4, min: 0, max: 1, default: 0.5 }),
  },
  inputs: { x: s.number({ min: 0, max: 1, default: 0 }), hit: s.bang() },
  sound: ({ out, level, on, steps, x, hit }) => {
    const back = s.feedback();
    back.set(s.input);
    const tape = s.buffer(0.1);
    tape.write(s.phasor(1).times(tape.length), s.input);
    s.output("level", level);
    const n = s.note;
    const all: s.Operand[] = [
      s.inputLeft, s.inputRight, s.channel, s.sampleRate, s.beat, s.bpm, s.playing,
      n.freq, n.pitch, n.gate, n.velocity, n.onset, on, x, hit, back,
      s.sin(1), s.cos(1), s.tan(1), s.tanh(1), s.abs(1), s.sqrt(1), s.exp(1), s.log(1),
      s.floor(1), s.wrap(1), s.db(1), s.saturate(1), level.negate(),
      level.minus(1), level.over(2), level.mod(2), level.lt(1), level.gt(1), level.le(1),
      level.ge(1), level.eq(1), level.ne(1), s.min(level, 1), s.max(level, 1), s.pow(level, 2),
      s.clamp(level, 0, 1), s.mix(level, 1, 0.5), s.noise(), s.delay(s.input, 10),
      s.delay(s.input, 10, 20), s.lowpass(s.input, 1000, 1), s.highpass(s.input, 1000, 1),
      s.bandpass(s.input, 1000, 1), s.smooth(level, 10), s.adsr(n.gate, 1, 1, 0.5, 1),
      s.hold(level, hit), s.rise(hit), s.change(level), steps.at(0), steps.length,
      s.lookup(steps, 0.5), tape.at(0), s.lookup(tape, 0.5),
    ];
    const sum = all.reduce<s.Signal>((total, each) => total.plus(each), s.input);
    return out === "mono" ? sum.times(0) : { left: sum.times(0), right: s.input };
  },
});
"#;

#[test]
fn every_node_of_the_sdk_builds_in_mono_and_in_stereo() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/every.ts", EVERY_NODE);
    let synth = synth(0.2);
    let (mono, stereo) = (
        r#"{"tool": "every", "state": {"out": "mono"}}"#,
        r#"{"tool": "every", "state": {"out": "stereo"}}"#,
    );
    let children = [
        ("instrument", synth.as_str()),
        ("mono", mono),
        ("stereo", stereo),
    ];
    for (path, contents) in racks_files(&[("pad", &children)]) {
        write(folder.path(), &path, &contents);
    }
    let harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
}

/// A player of a sample: its sound, from the start, at its own speed.
const PLAYER: &str = r#"import { phasor, file, tool } from "./sdk";

tool({
  name: "player",
  title: "Player",
  when: "You want a sound file to play",
  doc: "Plays a sound file over and over.",
  state: { sound: file() },
  sound: ({ sound }) => sound.at(phasor(1).times(sound.length)),
});
"#;

/// A second of a 441 Hz sine at 44.1 kHz.
fn write_sine(root: &Path) {
    let audio = root.join("assets/audio");
    std::fs::create_dir_all(&audio).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(audio.join("sine.wav"), spec).unwrap();
    for frame in 0..44_100 {
        let sample = (frame as f32 * 441.0 / 44_100.0 * std::f32::consts::TAU).sin() * 0.5;
        writer
            .write_sample((sample * f32::from(i16::MAX)) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

#[test]
fn a_sample_plays_once_its_file_is_there() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/player.ts", PLAYER);
    let player = r#"{"tool": "player", "state": {"sound": "sine.wav"}}"#;
    for (path, contents) in racks_files(&[("player", &[("instrument", player)])]) {
        write(folder.path(), &path, &contents);
    }
    let mut harness = Harness::open(folder);
    // The file is not there yet: a problem that names it, and silence.
    let problems = harness.project.problems();
    assert!(
        problems.iter().any(|p| p.message.contains("sine.wav")),
        "{problems:?}"
    );
    let silent = harness.play_from_the_start(4_800);
    assert!(silent.iter().all(|sample| *sample == 0.0));

    write_sine(harness.project.root());
    let audio = harness.path("assets/audio/sine.wav");
    harness.apply(&[audio]);
    assert_eq!(harness.project.problems(), []);
    let played = harness.play_from_the_start(48_000);
    let left: Vec<f32> = played.iter().step_by(2).copied().collect();
    let rises = left
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    // 441 Hz for a second, give or take the edges.
    assert!((rises as i64 - 441).abs() <= 2, "{rises} rises");
}
