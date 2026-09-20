//! Saved state edits: a file written during a held note, and states that are not valid.

use instrument::{SynthState, Waveform};
use sound_core::{ProjectError, State};

use crate::support::{Harness, id, largest_step, note, peak, rising_zero_crossings};

/// A cutoff below the note, so the output is close to a sine: it moves slowly, and a click
/// would stand out. A slow attack, so a restarted voice would show as a drop in level.
const BEFORE: SynthState = SynthState {
    waveform: Waveform::Saw,
    cutoff_hz: 200.0,
    resonance: 0.0,
    attack_seconds: 0.2,
    decay_seconds: 0.1,
    sustain: 0.8,
    release_seconds: 0.1,
    gain: 0.2,
};

const LOUDER_AND_BRIGHTER: &str = r#"{
  "tool": "instrument.synth",
  "state": {
    "waveform": "saw",
    "cutoff_hz": 300.0,
    "resonance": 0.3,
    "attack_seconds": 0.2,
    "decay_seconds": 0.1,
    "sustain": 0.9,
    "release_seconds": 0.1,
    "gain": 0.8
  }
}"#;

#[test]
fn a_file_edit_during_a_held_note_changes_the_sound_without_a_restart_or_a_click() {
    // Middle C is 261.63 Hz, so the edit lands mid-cycle, where a phase reset would show.
    let mut harness = Harness::with_track(vec![note(0, 96_000, 60, 100)], BEFORE);
    let edit = 48_123;
    let mut output = harness.play(edit);
    let changed = harness.write_and_apply("state/track/instrument.json", LOUDER_AND_BRIGHTER);
    assert_eq!(changed, 1);
    output.extend(harness.render(48_000));

    let level_before = peak(&output[edit - 480..edit]);
    let level_after = peak(&output[edit + 24_000..]);
    assert!(
        level_after > 4.0 * level_before,
        "{level_before} {level_after}"
    );

    // The level only rises. A restarted voice would drop to silence and attack again.
    for (index, window) in output[edit..].chunks(480).enumerate() {
        assert!(peak(window) > 0.95 * level_before, "window {index}");
    }

    // Every cycle is about as long as the 183.5 frames before the edit: the phase went on.
    // The moving filter shifts the crossings by a frame or two. A phase reset at this frame
    // would make one cycle 55 frames longer.
    let crossings = rising_zero_crossings(&output[24_000..]);
    for pair in crossings.windows(2) {
        let cycle = pair[1] - pair[0];
        assert!(
            (180..=186).contains(&cycle),
            "cycle of {cycle} frames at {}",
            24_000 + pair[0]
        );
    }

    // No step across the edit is larger than the steps of the louder sound it leads to.
    let around_the_edit = largest_step(&output[edit - 480..edit + 24_000]);
    let settled = largest_step(&output[edit + 24_000..]);
    assert!(
        around_the_edit <= 1.05 * settled,
        "{around_the_edit} {settled}"
    );
}

#[test]
fn a_gain_edit_to_zero_fades_out_and_a_waveform_edit_keeps_the_note() {
    let mut harness = Harness::with_track(vec![note(0, 96_000, 60, 100)], BEFORE);
    let synth = harness
        .project
        .resolve::<SynthState>(&id("track/instrument"))
        .unwrap();
    let mut output = harness.play(48_123);

    let mut edit = harness.project.begin("Square");
    let square = |state: &mut SynthState| state.waveform = Waveform::Square;
    harness.project.update(&mut edit, &synth, square).unwrap();
    harness.project.finish(edit).unwrap();
    let as_square = harness.render(24_000);
    assert!(peak(&as_square[12_000..]) > 0.05);

    let mut edit = harness.project.begin("Mute");
    let mute = |state: &mut SynthState| state.gain = 0.0;
    harness.project.update(&mut edit, &synth, mute).unwrap();
    harness.project.finish(edit).unwrap();
    let mute_frame = output.len() + as_square.len();
    output.extend(as_square);
    output.extend(harness.render(24_000));

    // The fade takes 0.05 s. Its steps are no larger than those of the square before it.
    let settled = largest_step(&output[mute_frame - 12_000..mute_frame]);
    let fade = largest_step(&output[mute_frame - 1..]);
    assert!(fade <= 1.05 * settled, "{fade} {settled}");
    assert!(peak(&output[mute_frame + 1_200..mute_frame + 1_680]) > 0.01);
    assert_eq!(peak(&output[mute_frame + 2_400 + 64..]), 0.0);
}

#[test]
fn a_file_with_a_value_out_of_range_is_rejected_and_names_the_field() {
    let mut harness = Harness::with_track(vec![note(0, 96_000, 60, 100)], BEFORE);
    let problem = |harness: &mut Harness, state: &str| {
        let record = format!(r#"{{"tool": "instrument.synth", "state": {state}}}"#);
        assert_eq!(
            harness.write_and_apply("state/track/instrument.json", &record),
            0
        );
        let problems = harness.project.problems();
        assert_eq!(problems[0].path, "state/track/instrument.json");
        problems[0].message.clone()
    };

    let low_cutoff = r#"{"waveform": "saw", "cutoff_hz": 5, "resonance": 0.2,
        "attack_seconds": 0.01, "decay_seconds": 0.2, "sustain": 0.7, "release_seconds": 0.3, "gain": 0.25}"#;
    assert_eq!(
        problem(&mut harness, low_cutoff),
        "state: cutoff_hz must be from 20 to 20000, not 5"
    );
    let sine = low_cutoff.replace("saw", "sine").replace(": 5,", ": 500,");
    assert_eq!(
        problem(&mut harness, &sine),
        "state.waveform: unknown variant `sine`, expected `saw` or `square`"
    );
    let misspelled = low_cutoff.replace("cutoff_hz", "cutoff");
    let message = problem(&mut harness, &misspelled);
    assert!(
        message.starts_with("state.cutoff: unknown field `cutoff`"),
        "{message}"
    );

    // The state from before still plays.
    let synth = harness
        .project
        .resolve::<SynthState>(&id("track/instrument"))
        .unwrap();
    assert_eq!(harness.project.state(&synth), Some(&BEFORE));
    assert!(peak(&harness.play(24_000)) > 0.01);

    let mut edit = harness.project.begin("Too loud");
    let too_loud = |state: &mut SynthState| state.gain = 1.5;
    let error = harness
        .project
        .update(&mut edit, &synth, too_loud)
        .unwrap_err();
    assert!(
        matches!(error, ProjectError::InvalidState { .. }),
        "{error}"
    );
    harness.project.cancel(edit).unwrap();
}

#[test]
fn every_field_has_its_range_and_its_message() {
    type Set = fn(&mut SynthState, f32);
    let fields: [(&str, Set, f32, f32); 7] = [
        (
            "cutoff_hz",
            |state, value| state.cutoff_hz = value,
            20.0,
            20_000.0,
        ),
        (
            "resonance",
            |state, value| state.resonance = value,
            0.0,
            1.0,
        ),
        (
            "attack_seconds",
            |state, value| state.attack_seconds = value,
            0.001,
            10.0,
        ),
        (
            "decay_seconds",
            |state, value| state.decay_seconds = value,
            0.001,
            10.0,
        ),
        ("sustain", |state, value| state.sustain = value, 0.0, 1.0),
        (
            "release_seconds",
            |state, value| state.release_seconds = value,
            0.001,
            10.0,
        ),
        ("gain", |state, value| state.gain = value, 0.0, 1.0),
    ];
    assert_eq!(SynthState::default().validate(), Ok(()));
    for (field, set, low, high) in fields {
        for (value, valid) in [
            (low, true),
            (high, true),
            (low - 0.0005, false),
            (high * 1.01, false),
            (f32::NAN, false),
        ] {
            let mut state = SynthState::default();
            set(&mut state, value);
            let expected = if valid {
                Ok(())
            } else {
                Err(format!("{field} must be from {low} to {high}, not {value}"))
            };
            assert_eq!(state.validate(), expected);
        }
    }
}

#[test]
fn the_default_state_saves_as_the_documented_record() {
    let mut harness = Harness::new();
    harness.add_track("track", Vec::new(), SynthState::default());
    let record =
        std::fs::read_to_string(harness.project.root().join("state/track/instrument.json"));
    let expected = r#"{
  "tool": "instrument.synth",
  "state": {
    "waveform": "saw",
    "cutoff_hz": 2000.0,
    "resonance": 0.2,
    "attack_seconds": 0.005,
    "decay_seconds": 0.2,
    "sustain": 0.7,
    "release_seconds": 0.3,
    "gain": 0.25
  }
}
"#;
    assert_eq!(record.unwrap(), expected);
}

#[test]
fn a_record_may_leave_out_fields_and_the_default_synth_has_a_sane_level() {
    let mut harness = Harness::with_track(vec![note(0, 960, 60, 127)], BEFORE);
    let record = r#"{"tool": "instrument.synth", "state": {}}"#;
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", record),
        1
    );
    let synth = harness
        .project
        .resolve::<SynthState>(&id("track/instrument"))
        .unwrap();
    assert_eq!(harness.project.state(&synth), Some(&SynthState::default()));

    let level = peak(&harness.play(24_000));
    // The README says 0.35 for velocity 127.
    assert!((0.33..0.37).contains(&level), "{level}");

    let record = r#"{"tool": "instrument.synth", "state": {"waveform": "square", "sustain": 0.5}}"#;
    harness.write_and_apply("state/track/instrument.json", record);
    let expected = SynthState {
        waveform: Waveform::Square,
        sustain: 0.5,
        ..SynthState::default()
    };
    assert_eq!(harness.project.state(&synth), Some(&expected));
}
