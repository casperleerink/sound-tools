//! Tools a project writes in TypeScript in its `extensions/` folder, run by Bun: the first
//! example of their agent doc loads as it is written and plays, and a bank of combs whose shape
//! is a choice plays with every choice. Bun is not part of the build, so where it is not
//! installed, as in CI, these tests say so and pass.

use std::path::Path;

use crate::support::{BAR, Harness, clip, synth};

fn bun_is_installed() -> bool {
    let home = std::env::var_os("HOME").map(|home| Path::new(&home).join(".bun/bin/bun"));
    let on_path = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|folder| folder.join("bun").is_file())
    });
    on_path || home.is_some_and(|path| path.is_file())
}

/// The first TypeScript example of the doc for agents that write tools: a whole tool.
fn doc_example() -> String {
    let doc = sound_typescript::AGENT_DOC.markdown;
    let block = doc.split("```ts\n").nth(1).unwrap();
    block.split("```").next().unwrap().to_string()
}

/// Combs whose number is a choice: `sound` builds a different Hum for each.
const COMBS: &str = r#"import { choice, delay, feedback, input, knob, lowpass, mix, tool } from "./sdk";

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
    const rings = TIMES.slice(0, combs).map((ms) => {
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
    if !bun_is_installed() {
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
