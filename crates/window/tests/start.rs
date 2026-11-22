//! The start state without a window, as an agent drives it: `--tree` lists its controls and
//! `--act` presses them.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

/// The app with no folder and nothing remembered in `home`, `steps` by `--act`.
fn start(home: &Path, steps: &[Value]) -> (Output, Option<Value>) {
    let steps_file = home.join("steps.json");
    std::fs::write(&steps_file, serde_json::to_string(steps).unwrap()).unwrap();
    let tree = home.join("tree.json");
    let ran = Command::new(env!("CARGO_BIN_EXE_sound-tools"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("LOCALAPPDATA", home)
        .arg("--tree")
        .arg(&tree)
        .arg("--act")
        .arg(&steps_file)
        .output()
        .unwrap();
    let report = std::fs::read(tree).ok();
    (
        ran,
        report.map(|report| serde_json::from_slice(&report).unwrap()),
    )
}

fn keys(report: &Value) -> Vec<&str> {
    let controls = report["controls"].as_array().unwrap().iter();
    controls
        .map(|control| control["key"].as_str().unwrap())
        .collect()
}

#[test]
fn an_agent_sees_the_start_and_makes_a_new_project_by_label() {
    let scratch = tempfile::tempdir().unwrap();
    let home = dunce::canonicalize(scratch.path()).unwrap();
    let (ran, report) = start(&home, &[]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(keys(&report.unwrap()), ["choose-folder", "new-project"]);

    let steps = [
        json!({ "label": "New project…", "action": "press" }),
        json!({ "key": "project-name", "action": "commit", "value": "Sketch" }),
        json!({ "label": "Create", "action": "press" }),
    ];
    let (ran, report) = start(&home, &steps);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let sketch = home.join("Sketch");
    assert_eq!(report.unwrap()["opened"], json!(sketch));
    assert!(sketch.join("project.json").is_file());
}

#[test]
fn a_step_that_names_nothing_here_fails_the_run() {
    let home = tempfile::tempdir().unwrap();
    let (ran, _) = start(
        home.path(),
        &[json!({ "label": "Mute Track 2", "action": "press" })],
    );
    assert!(!ran.status.success());
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(said.contains("there is no Mute Track 2 here"), "{said}");
}
