#![allow(clippy::unwrap_used)]

//! A script on the live project folder, rendered offline: new code fades in, wrong code keeps
//! the old one playing, and a new value keeps the memory of the code.

use std::path::{Path, PathBuf};

use script::ScriptState;
use sound_core::{
    Changes, Engine, EngineConfig, InstanceId, PortReference, Project, Registry, SavedConnection,
};
use sound_notes::AUDIO_OUTPUT;

const SAMPLE_RATE: u32 = 48_000;

fn state(code: &[&str]) -> ScriptState {
    ScriptState {
        code: code.iter().map(|line| line.to_string()).collect(),
        ..ScriptState::default()
    }
}

/// A project where the script `sound` plays on the device, with no input: what it makes, it
/// makes itself.
fn open(folder: &Path, code: &[&str]) -> (Project, Engine) {
    let mut registry = Registry::new();
    script::register(&mut registry).unwrap();
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut project = Project::open(folder, registry, control).unwrap();
    let mut changes = Changes::new();
    let sound = changes.create(InstanceId::new("sound").unwrap(), state(code));
    let output = PortReference::new(sound.id(), AUDIO_OUTPUT);
    changes.connect(SavedConnection::to_device(output, 0));
    project.commit("Add script", changes).unwrap();
    project.engine().play();
    (project, engine)
}

/// The left channel.
fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    for buffer in output.chunks_mut(480 * 2) {
        engine.process_block(buffer);
    }
    output.iter().step_by(2).copied().collect()
}

fn write(project: &Project, code: &[&str], values: &str) -> PathBuf {
    let code = serde_json::to_string(&code).unwrap();
    let record =
        format!(r#"{{"tool": "script", "state": {{"code": {code}, "values": {{{values}}}}}}}"#);
    let path = project.root().join("state/sound.json");
    std::fs::write(&path, record).unwrap();
    path
}

fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
}

const SINE: [&str; 1] = ["out = 0.5 * sin(phasor(220) * tau)"];

#[test]
fn new_code_fades_in_without_a_jump() {
    let folder = tempfile::tempdir().unwrap();
    let (mut project, mut engine) = open(folder.path(), &SINE);
    // Ends mid-cycle, where a cut to the new code would jump.
    let mut output = render(&mut engine, 12_345);
    assert!(output.last().unwrap().abs() > 0.1);

    // The other sine starts at phase 0, and on the other side.
    let path = write(&project, &["out = -0.5 * sin(phasor(330) * tau)"], "");
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 1);
    let after = render(&mut engine, 4_800);
    output.extend(&after);

    // A sine of 330 Hz at 0.5 steps at most 0.022 a frame; the fade adds a little.
    assert!(largest_step(&output) < 0.03, "{}", largest_step(&output));
    let crossings = after[480..]
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count();
    assert!((29..=30).contains(&crossings), "{crossings}");
}

#[test]
fn wrong_code_is_a_problem_and_the_old_code_plays_on() {
    let undisturbed = {
        let folder = tempfile::tempdir().unwrap();
        let (_project, mut engine) = open(folder.path(), &SINE);
        render(&mut engine, 20_000)
    };
    let folder = tempfile::tempdir().unwrap();
    let (mut project, mut engine) = open(folder.path(), &SINE);
    let mut output = render(&mut engine, 10_000);

    let path = write(&project, &["// louder", "out = sine(220)"], "");
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 0);
    output.extend(render(&mut engine, 10_000));

    let problems = project.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].path, "state/sound.json");
    assert!(
        problems[0]
            .message
            .contains("code[1]: unknown function `sine`"),
        "{}",
        problems[0].message
    );
    assert_eq!(output, undisturbed);
}

#[test]
fn a_new_value_keeps_the_memory_of_the_code() {
    // Counts its frames in a history, so code that started again would start from 0.
    let code = [
        "param level = 0.5 [0, 1]",
        "history frames",
        "frames = frames + 1",
        "out = level * frames / 96000",
    ];
    let folder = tempfile::tempdir().unwrap();
    let (mut project, mut engine) = open(folder.path(), &code);
    render(&mut engine, 24_000);

    let path = write(&project, &code, r#""level": 1"#);
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 1);
    let after = render(&mut engine, 24_000);

    // The level glides over 20 ms; from then on it is 1, and the count went on.
    let frame = 24_000 + 2_000;
    assert!(
        (after[2_000] - frame as f32 / 96_000.0).abs() < 1e-3,
        "{}",
        after[2_000]
    );
}

#[test]
fn a_value_must_name_a_param_of_the_code_and_be_in_its_range() {
    let wrong = |values: &[(&str, f32)]| {
        ScriptState {
            values: values
                .iter()
                .map(|(name, value)| (name.to_string(), *value))
                .collect(),
            ..state(&["param level = 0.5 [0, 1]"])
        }
        .compile()
        .unwrap_err()
    };
    assert_eq!(
        wrong(&[("gain", 0.5)]),
        "values.gain: the code has no `param gain`"
    );
    assert_eq!(
        wrong(&[("level", 2.0)]),
        "values.level: 2 is outside [0, 1]"
    );
}
