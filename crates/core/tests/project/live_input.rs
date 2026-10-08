//! A `project.json` connection from the device input: it loads, binds and plays the live input
//! into the port it names, and one that cannot play is a problem.

use sound_core::{Engine, EngineConfig, Project, ProjectError, live_input};

use crate::tools::{Harness, SAMPLE_RATE, project_file, registry, write};

const AMPLIFIER: &str = r#"{"tool": "test.amplifier", "state": {"gain": 0.5}}"#;
const AMPLIFIER_TO_DEVICE: &str =
    r#"{"from": {"instance": "amp", "port": "out"}, "to": {"device_output": 0}}"#;

fn from_device_input(channel: usize, port: &str) -> String {
    format!(
        r#"{{"from": {{"device_input": {channel}}}, "to": {{"input": {{"instance": "amp", "port": "{port}"}}}}}}"#
    )
}

/// A project of one `test.amplifier` at half gain, heard on the device, with these connections
/// before the one to the device.
fn amplifier_hearing(connections: &str) -> Harness {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "state/amp.json", AMPLIFIER);
    let connections = format!("{connections}, {AMPLIFIER_TO_DEVICE}");
    write(folder.path(), "project.json", &project_file(&connections));
    Harness::open(folder)
}

fn messages(project: &Project) -> Vec<String> {
    let problems = project.problems().into_iter();
    problems.map(|problem| problem.message).collect()
}

#[test]
fn a_device_input_connection_plays_the_input_into_the_port_it_names() {
    let mut harness = amplifier_hearing(&from_device_input(0, "in"));
    assert_eq!(messages(&harness.project), [] as [String; 0]);
    // Until the window gives an input, and in a render, it is silence.
    assert_eq!(harness.level(), 0.0);

    let (mut writer, input) = live_input(SAMPLE_RATE, 1);
    harness.project.set_live_input(Some(input));
    harness.level();
    writer.write(&[0.8; 480]);
    assert_eq!(harness.level(), 0.4);
    // The file says what it said: the input is not saved anywhere.
    let saved = harness.read("project.json");
    assert!(saved.contains(r#""device_input": 0"#), "{saved}");
}

#[test]
fn a_device_input_connection_that_cannot_play_says_why() {
    // A port the instance does not have.
    let harness = amplifier_hearing(&from_device_input(0, "side"));
    assert_eq!(
        messages(&harness.project),
        [r#"connections[0]: instance "amp" has no input "side""#]
    );

    // An input at another rate than the output, and a channel the input does not have.
    let connections = [from_device_input(0, "in"), from_device_input(1, "in")].join(", ");
    let mut harness = amplifier_hearing(&connections);
    let (mut writer, input) = live_input(44_100, 1);
    harness.project.set_live_input(Some(input));
    harness.level();
    writer.write(&[0.8; 480]);
    assert_eq!(harness.level(), 0.0);
    let problems = messages(&harness.project);
    assert_eq!(problems.len(), 2, "{problems:?}");
    for (index, problem) in problems.iter().enumerate() {
        let expected = format!(
            "connections[{index}]: not heard, because the audio input runs at 44100 Hz and the output at 48000 Hz"
        );
        assert!(problem.starts_with(&expected), "{problem}");
    }
    let (_writer, input) = live_input(SAMPLE_RATE, 1);
    harness.project.set_live_input(Some(input));
    assert_eq!(
        messages(&harness.project),
        [
            "connections[1]: not heard, because the audio input has one channel, counted from 0, and no channel 1"
        ]
    );
    harness.project.set_live_input(None);
    assert_eq!(messages(&harness.project), [] as [String; 0]);
}

#[test]
fn a_connection_that_mixes_the_two_starts_does_not_load() {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "state/amp.json", AMPLIFIER);
    let mixed = r#"{"from": {"instance": "amp", "port": "out", "device_input": 0}, "to": {"device_output": 0}}"#;
    write(folder.path(), "project.json", &project_file(mixed));
    let (control, _engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let Err(ProjectError::InvalidProjectFile { message, .. }) =
        Project::open(folder.path(), registry(), control)
    else {
        panic!("the project opened");
    };
    let expected = r#"connections[0].from: a connection starts at an output, {"instance": ..., "port": ...}, or at the device input, {"device_input": 0}"#;
    assert!(message.starts_with(expected), "{message}");
}
