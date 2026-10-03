//! The Sampler as the instrument of a real track: an outside agent changes it by file while it
//! plays and the change is heard with its glide and undone in one step, it comes back as it was
//! after close and reopen and renders the same to the byte, and a missing file is listed in
//! `problems.txt` while the rest plays.

use sampler::SamplerState;
use sound_core::{Changes, InstanceId};
use sound_media::AudioAsset;
use sound_notes::Pitch;

use crate::support::{BAR, Harness, TRACK, clip, difference, write_samples};

const FOLDER: &str = "state/arrangement/keys";
const SAMPLER_FILE: &str = "state/arrangement/keys/instrument.json";

fn record(state: &str) -> String {
    format!(r#"{{"tool": "sampler", "state": {state}}}"#)
}

/// A track `keys` whose Sampler holds one C4 for two bars, with this record.
fn keys_with(harness: &mut Harness, sampler: &str) {
    let track = TRACK.replace("NAME", "keys").replace("ORDER", "1");
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), &track),
        harness.write(SAMPLER_FILE, sampler),
        harness.write(
            &format!("{FOLDER}/held.json"),
            &clip(0, 7680, &[(0, 7680, 60)]),
        ),
    ];
    assert_eq!(harness.apply(&paths), 3);
}

/// The left channel of an interleaved render.
fn left(samples: &[f32]) -> Vec<f32> {
    samples.iter().step_by(2).copied().collect()
}

#[test]
fn an_outside_edit_of_the_sampler_while_it_plays_is_heard_with_its_glide_and_undone_in_one_step() {
    let mut harness = Harness::new();
    // A steady level at 44.1 kHz, which the engine plays at 48 kHz.
    write_samples(&harness, "steady.wav", 44_100, 5.0, |_| 0.25);
    keys_with(&mut harness, &record(r#"{"sample": "steady.wav"}"#));
    assert_eq!(harness.project.problems(), []);

    let mut played = left(&harness.play_from_the_start(BAR / 2));
    let before = played[BAR / 2 - 1];
    assert!(before > 0.1, "{before}");
    // An agent turns it down by 12 dB while it plays. The edit arrives at the next block.
    let quieter = record(r#"{"sample": "steady.wav", "gain_db": -12.0}"#);
    assert_eq!(harness.write_and_apply(SAMPLER_FILE, &quieter), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(left(&harness.play(BAR / 2)));

    let ratio = 10_f32.powf(-12.0 / 20.0);
    let glide = &played[BAR / 2..BAR / 2 + 960];
    let steps = glide.windows(2).map(|pair| pair[0] - pair[1]);
    let largest = steps.fold(0.0_f32, f32::max);
    let after = played[BAR / 2 + 960];
    println!(
        "gain 0 to -12 dB by file while it plays: {before:.5} to {after:.5} over 960 frames (20 ms), largest step {largest:.6}, a jump {:.5}",
        before - after
    );
    assert!(glide.windows(2).all(|pair| pair[1] <= pair[0]));
    assert!(largest <= (before - after) / 960.0 * 1.01, "{largest}");
    assert!((after / before - ratio).abs() < 1e-4, "{}", after / before);
    assert!((played[BAR - 1] / before - ratio).abs() < 1e-4);

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(SAMPLER_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["gain_db"], 0.0);
    let back = left(&harness.play(BAR / 4));
    assert!((back[BAR / 8] - before).abs() < 1e-5, "{}", back[BAR / 8]);
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte.
#[test]
fn the_sampler_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = Harness::new();
    write_samples(&harness, "kalimba.wav", 48_000, 2.0, |time| {
        (std::f64::consts::TAU * 523.25 * time).sin() as f32 * (-3.0 * time).exp() as f32 * 0.5
    });
    keys_with(&mut harness, &record(r#"{"sample": "kalimba.wav"}"#));
    let id = InstanceId::new("arrangement/keys/instrument").unwrap();
    let sampler = harness.project.resolve::<SamplerState>(&id).unwrap();
    let sound = SamplerState {
        sample: Some(AudioAsset::new("kalimba.wav").unwrap()),
        sfz: None,
        root: Pitch::new(72).unwrap(),
        start_seconds: 0.012,
        end_seconds: Some(1.18),
        attack_seconds: 0.002,
        decay_seconds: 0.4,
        sustain: 0.55,
        release_seconds: 0.3,
        velocity_to_volume: 0.5,
        gain_db: -3.0,
    };
    let mut changes = Changes::new();
    changes.set(&sampler, sound.clone());
    harness.project.commit("Change sampler", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(SAMPLER_FILE)).unwrap();
    assert_eq!(
        file,
        r#"{
  "tool": "sampler",
  "state": {
    "sample": "kalimba.wav",
    "root": 72,
    "start_seconds": 0.012,
    "end_seconds": 1.18,
    "attack_seconds": 0.002,
    "decay_seconds": 0.4,
    "sustain": 0.55,
    "release_seconds": 0.3,
    "velocity_to_volume": 0.5,
    "gain_db": -3.0
  }
}
"#
    );

    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let sampler = harness.project.resolve::<SamplerState>(&id).unwrap();
    assert_eq!(harness.project.state(&sampler), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(SAMPLER_FILE)).unwrap(),
        file
    );
    let second = harness.play(2 * BAR);
    let bytes = |samples: &[f32]| -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    };
    assert_eq!(bytes(&first), bytes(&second));
    assert!(first.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn a_missing_sample_is_in_problems_txt_and_the_rest_plays() {
    let piano_and_keys = |sampler: &str| {
        let mut harness = Harness::new();
        let chord = clip(0, 7680, &[(0, 7680, 48)]);
        harness.write_track("piano", 2, 0.15, &[("chord", chord)]);
        keys_with(&mut harness, &record(sampler));
        harness
    };
    let mut harness = piano_and_keys(r#"{"sample": "gone.wav"}"#);
    harness.project.poll().unwrap();
    let problems = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert!(
        problems.contains(
            "state/arrangement/keys/instrument.json: the sample assets/audio/gone.wav is not there, so the Sampler is silent"
        ),
        "{problems}"
    );
    let missing = harness.play(BAR);
    assert!(missing.iter().any(|sample| sample.abs() > 0.01));

    // What plays is the piano alone: the same as with an empty Sampler.
    let mut empty = piano_and_keys("{}");
    assert_eq!(empty.project.problems(), []);
    assert_eq!(difference(&missing, &empty.play(BAR)), None);
}
