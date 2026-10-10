//! A `project.json` connection from the device input or from the sound of other apps: it loads,
//! binds and plays the live input into the port it names, and one that cannot play is a
//! problem.

use sound_core::{AppSound, Engine, EngineConfig, Project, ProjectError, live_input};

use crate::tools::{Harness, SAMPLE_RATE, project_file, registry, write};

const AMPLIFIER: &str = r#"{"tool": "test.amplifier", "state": {"gain": 0.5}}"#;
const AMPLIFIER_TO_DEVICE: &str =
    r#"{"from": {"instance": "amp", "port": "out"}, "to": {"device_output": 0}}"#;

fn from_device_input(channel: usize, port: &str) -> String {
    format!(
        r#"{{"from": {{"device_input": {channel}}}, "to": {{"input": {{"instance": "amp", "port": "{port}"}}}}}}"#
    )
}

fn from_app(app: &str, port: &str) -> String {
    format!(
        r#"{{"from": {{"app": "{app}"}}, "to": {{"input": {{"instance": "amp", "port": "{port}"}}}}}}"#
    )
}

fn music() -> AppSound {
    AppSound::Named("Music".to_string())
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
fn an_app_connection_plays_the_sound_of_the_app_beside_the_device_input() {
    let connections = [from_device_input(0, "in"), from_app("Music", "in")].join(", ");
    let mut harness = amplifier_hearing(&connections);
    let (mut device, input) = live_input(SAMPLE_RATE, 1);
    harness.project.set_live_input(Some(input));
    let (mut music_writer, sound) = live_input(SAMPLE_RATE, 2);
    harness.project.set_app_sound(&music(), Some(Ok(sound)));
    harness.level();
    device.write(&[0.2; 480]);
    music_writer.write(&[0.4; 960]);
    // Both reach the input of the amplifier and are summed there.
    assert!((harness.level() - 0.3).abs() < 1e-6);

    // Without the sound of the app, the device input plays on.
    harness.project.set_app_sound(&music(), None);
    harness.level();
    device.write(&[0.2; 480]);
    assert!((harness.level() - 0.1).abs() < 1e-6);
    let problems: &[&str] = if cfg!(target_os = "macos") {
        &[]
    } else {
        &["connections[1]: not heard, because hearing other apps works only on macOS"]
    };
    assert_eq!(messages(&harness.project), problems);
}

#[cfg(target_os = "macos")]
#[test]
fn an_app_connection_that_is_not_heard_says_why() {
    let mut harness = amplifier_hearing(&from_app("Music", "in"));
    // Until the window tries, and in a render, it is silence and no problem.
    assert_eq!(messages(&harness.project), [] as [String; 0]);

    let why = r#""Music" is not running, or has played no sound yet"#;
    harness
        .project
        .set_app_sound(&music(), Some(Err(why.to_string())));
    assert_eq!(
        messages(&harness.project),
        [format!("connections[0]: not heard, because {why}")]
    );
    let (_writer, sound) = live_input(44_100, 2);
    harness.project.set_app_sound(&music(), Some(Ok(sound)));
    let problems = messages(&harness.project);
    let expected = "connections[0]: not heard, because the sound of other apps comes at 44100 Hz and the output runs at 48000 Hz";
    assert!(problems[0].starts_with(expected), "{problems:?}");
    harness.project.set_app_sound(&music(), None);
    assert_eq!(messages(&harness.project), [] as [String; 0]);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn an_app_connection_is_heard_only_on_macos() {
    let harness = amplifier_hearing(&from_app("all", "in"));
    assert_eq!(
        messages(&harness.project),
        ["connections[0]: not heard, because hearing other apps works only on macOS"]
    );
}

/// The message of a `project.json` with this one connection, which does not load.
fn refused(connection: &str) -> String {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "state/amp.json", AMPLIFIER);
    write(folder.path(), "project.json", &project_file(connection));
    let (control, _engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let Err(ProjectError::InvalidProjectFile { message, .. }) =
        Project::open(folder.path(), registry(), control)
    else {
        panic!("the project opened");
    };
    message
}

#[test]
fn a_connection_that_mixes_the_starts_or_names_no_app_does_not_load() {
    let mixed = r#"{"from": {"instance": "amp", "port": "out", "device_input": 0}, "to": {"device_output": 0}}"#;
    let message = refused(mixed);
    let expected = r#"connections[0].from: a connection starts at an output, {"instance": ..., "port": ...}, at the device input, {"device_input": 0}, or at the sound of other apps, {"app": "Music"} or {"app": "all"}"#;
    assert!(message.starts_with(expected), "{message}");

    let message = refused(r#"{"from": {"app": " "}, "to": {"device_output": 0}}"#);
    let expected = r#"name an app, such as "Music", or "all" for every app"#;
    assert!(message.contains(expected), "{message}");
}
