//! Tone on the live project folder, rendered offline: files written during playback are heard,
//! and a Tone that stays keeps its phase.

use std::path::{Path, PathBuf};

use sound_core::{
    Changes, Engine, EngineConfig, InstanceId, PortReference, Project, ProjectError, Registry,
    SavedConnection,
};
use tone::{AUDIO_OUTPUT, ToneState};

use crate::support::{
    SAMPLE_RATE, assert_continuous, channel, largest_step, peak, render, rising_zero_crossings,
};

const FIRST: ToneState = ToneState {
    frequency_hz: 220.0,
    gain: 0.5,
};

/// A stereo project with one Tone on the left channel, playing.
fn open(folder: &Path) -> (Project, Engine) {
    let mut registry = Registry::new();
    tone::register(&mut registry).unwrap();
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut project = Project::open(folder, registry, control).unwrap();
    if project.instances().count() == 0 {
        let mut changes = Changes::new();
        let first = changes.create(InstanceId::new("first").unwrap(), FIRST);
        let output = PortReference::new(first.id(), AUDIO_OUTPUT);
        changes.connect(SavedConnection::to_device(output, 0));
        project.commit("Add tone", changes).unwrap();
    }
    project.engine().play();
    (project, engine)
}

fn write(project: &Project, relative: &str, contents: &str) -> PathBuf {
    let path = project.root().join(relative);
    std::fs::write(&path, contents).unwrap();
    path
}

const PROJECT_WITH_SECOND: &str = r#"{
  "format": 1,
  "extensions": ["tone"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [
    {"from": {"instance": "first", "port": "audio"}, "to": {"device_output": 0}},
    {"from": {"instance": "second", "port": "audio"}, "to": {"device_output": 1}}
  ]
}"#;

#[test]
fn a_tone_written_during_playback_sounds_and_the_other_keeps_its_phase() {
    let undisturbed = {
        let folder = tempfile::tempdir().unwrap();
        let (_project, mut engine) = open(folder.path());
        channel(&render(&mut engine, 30_000), 0, 2)
    };

    let folder = tempfile::tempdir().unwrap();
    let (mut project, mut engine) = open(folder.path());
    let mut output = render(&mut engine, 10_001);
    let batches = project.engine().poll().unwrap().batches_applied;

    // An agent adds a Tone: one record file and its connection.
    let second = r#"{"tool": "tone", "state": {"frequency_hz": 330.0, "gain": 0.25}}"#;
    let paths = [
        write(&project, "state/second.json", second),
        write(&project, "project.json", PROJECT_WITH_SECOND),
    ];
    assert_eq!(project.apply_outside_changes(&paths).unwrap(), 2);
    output.extend(render(&mut engine, 9_999));
    let status = project.engine().poll().unwrap();
    assert_eq!(status.batches_applied, batches + 1);
    assert!(status.playing);

    // The agent deletes it again. The connection goes with it.
    std::fs::remove_file(&paths[0]).unwrap();
    assert_eq!(project.apply_outside_changes(&paths[..1]).unwrap(), 2);
    output.extend(render(&mut engine, 10_000));
    assert_eq!(
        project.engine().poll().unwrap().batches_applied,
        batches + 2
    );

    // The first Tone never restarted or stopped: bit for bit the same as with no edits.
    assert_eq!(channel(&output, 0, 2), undisturbed);
    let right = channel(&output, 1, 2);
    assert_eq!(peak(&right[..10_001]), 0.0);
    assert!(peak(&right[10_001..20_000]) > 0.2499);
    assert_eq!(peak(&right[20_000..]), 0.0);

    // Undo brings the Tone and its connection back, as one step.
    project.undo().unwrap();
    let again = channel(&render(&mut engine, 4_800), 1, 2);
    assert!(peak(&again) > 0.2499);
    assert!((32..=34).contains(&rising_zero_crossings(&again)));
}

#[test]
fn an_outside_frequency_edit_keeps_the_phase() {
    let folder = tempfile::tempdir().unwrap();
    let (mut project, mut engine) = open(folder.path());
    // 12 345 frames ends mid-cycle, where a phase reset would show as a jump.
    let mut samples = channel(&render(&mut engine, 12_345), 0, 2);
    assert!(samples.last().unwrap().abs() > 0.1);

    let edited = r#"{"tool": "tone", "state": {"frequency_hz": 330.0, "gain": 0.5}}"#;
    let path = write(&project, "state/first.json", edited);
    project.apply_outside_changes(&[path]).unwrap();
    let changed = channel(&render(&mut engine, SAMPLE_RATE as usize), 0, 2);
    samples.extend(&changed);
    assert_continuous(&samples, largest_step(330.0, 0.5));
    assert!((329..=331).contains(&rising_zero_crossings(&changed)));
}

#[test]
fn values_out_of_range_are_rejected_from_files_and_from_the_interface() {
    let folder = tempfile::tempdir().unwrap();
    let (mut project, _engine) = open(folder.path());
    let loud = r#"{"tool": "tone", "state": {"frequency_hz": 330.0, "gain": 1.5}}"#;
    let path = write(&project, "state/first.json", loud);
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 0);
    let problems = project.problems();
    assert_eq!(problems[0].path, "state/first.json");
    assert_eq!(
        problems[0].message,
        "state: gain must be from 0 to 1, not 1.5"
    );

    let first = project
        .resolve::<ToneState>(&InstanceId::new("first").unwrap())
        .unwrap();
    assert_eq!(project.state(&first), Some(&FIRST));
    let mut edit = project.begin("Too high");
    let error = project
        .update(&mut edit, &first, |state| state.frequency_hz = 96_000.0)
        .unwrap_err();
    assert!(
        matches!(error, ProjectError::InvalidState { .. }),
        "{error}"
    );
    project.cancel(edit).unwrap();
}
