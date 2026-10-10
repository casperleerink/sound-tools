//! Tools a project writes in TypeScript in its `extensions/` folder, run by Bun: the first
//! example of their agent doc loads as it is written and plays, and a bank of combs whose shape
//! is a choice plays with every choice. Bun is not part of the build, so where it is not
//! installed, as in CI, these tests say so and pass.

use std::path::Path;

use crate::support::{BAR, Harness, clip, synth};

/// The first TypeScript example of the doc for agents that write tools: a whole tool.
fn doc_example() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let block = doc.split("```ts\n").nth(1).unwrap();
    block.split("```").next().unwrap().to_string()
}

/// Combs whose number is a choice: `sound` builds a different graph for each.
const COMBS: &str = r#"import { choice, delay, feedback, input, knob, lowpass, mix, tool, type Signal } from "./sdk";

const TIMES = [29.7, 37.1, 41.1, 43.7, 31.3, 39.9, 45.1, 33.5];

tool({
  name: "combs",
  title: "Combs",
  when: "You want a ringing room",
  doc: "Feedback combs.",
  state: {
    size: knob({ min: 0, max: 0.95, default: 0.8 }),
    combs: choice({ options: [2, 4, 8], default: 4 }),
    blend: knob({ min: 0, max: 1, default: 0.3 }),
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

/// A project whose `extensions/` holds the first example of the doc, `wobble`, and `COMBS`,
/// with a pad that plays a held chord through `effects` that are written next to it.
fn pad_through(effects: &[(&str, &str)]) -> Option<Harness> {
    pad_through_with(effects, "")
}

/// [`pad_through`], with more fields of the track's record, such as automation lanes.
fn pad_through_with(effects: &[(&str, &str)], track_fields: &str) -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/wobble.ts", &doc_example());
    crate::support::write(folder.path(), "extensions/combs.ts", COMBS);
    let mut harness = Harness::open(folder);
    let names: Vec<String> = effects
        .iter()
        .map(|(name, _)| format!("{name:?}"))
        .collect();
    let track = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "Pad", "effects": [{}]{track_fields}}}}}"#,
        names.join(", ")
    );
    harness.write("state/arrangement/pad/instance.json", &track);
    harness.write("state/arrangement/pad/instrument.json", &synth(0.2));
    let chord = clip(0, 15360, &[(0, 15360, 60), (0, 15360, 64), (0, 15360, 67)]);
    harness.write("state/arrangement/pad/chord.json", &chord);
    for (name, state) in effects {
        let record = format!(r#"{{"tool": "{name}", "state": {state}}}"#);
        harness.write(&format!("state/arrangement/pad/{name}.json"), &record);
    }
    let folder = harness.path("state/arrangement/pad");
    harness.apply(&[folder]);
    assert_eq!(harness.project.problems(), []);
    Some(harness)
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
    let Some(mut harness) = pad_through(&[("wobble", r#"{"rate": 2, "depth": 1}"#)]) else {
        return;
    };
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
        let Some(mut harness) = pad_through(&[("combs", &state)]) else {
            return;
        };
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
    let Some(harness) = pad_through(&[("wobble", r#"{"rate": 2}"#)]) else {
        return;
    };
    let wobble = sound_core::InstanceId::new("arrangement/pad/wobble").unwrap();
    let depth = harness.project.automation(&wobble, "depth").unwrap();
    // A lane added for it starts where it plays: at the default of the doc's `depth`.
    assert_eq!(depth.record, Some(0.5));
}

#[test]
fn a_lane_of_the_track_moves_a_knob_of_the_tremolo() {
    // The record dips to silence; the lane holds the depth at 0 for the whole bar.
    let lane = r#", "automation": [{"device": "wobble", "parameter": "depth", "points": [{"tick": 0, "value": 0.0}, {"tick": 15360, "value": 0.0}]}]"#;
    let Some(mut harness) = pad_through_with(&[("wobble", r#"{"rate": 2, "depth": 1}"#)], lane)
    else {
        return;
    };
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

/// A player of a sample: its sound, from the start, at its own speed.
const PLAYER: &str = r#"import { phasor, sample, tool } from "./sdk";

tool({
  name: "player",
  title: "Player",
  when: "You want a sound file to play",
  doc: "Plays a sound file over and over.",
  kind: "source",
  state: { sound: sample() },
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
    if !sound_typescript::has_bun() {
        return eprintln!("skipped: Bun is not installed");
    }
    let folder = tempfile::tempdir().unwrap();
    crate::support::write(folder.path(), "extensions/player.ts", PLAYER);
    let mut harness = Harness::open(folder);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "Player"}}"#;
    harness.write("state/arrangement/player/instance.json", track);
    let player = r#"{"tool": "player", "state": {"sound": "sine.wav"}}"#;
    harness.write("state/arrangement/player/instrument.json", player);
    let folder = harness.path("state/arrangement/player");
    harness.apply(&[folder]);
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
