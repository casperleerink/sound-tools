//! Runs without a window, as an agent runs them.
#![allow(clippy::unwrap_used)]

use std::process::Command;

use serde_json::Value;

/// A `--tree` of a new project in a window `size` wide, with no other flag.
fn tree(size: &str) -> Value {
    let scratch = tempfile::tempdir().unwrap();
    let tree = scratch.path().join("tree.json");
    let ran = Command::new(env!("CARGO_BIN_EXE_sound-tools"))
        .arg(scratch.path().join("song"))
        .args(["--size", size, "--tree"])
        .arg(&tree)
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    serde_json::from_slice(&std::fs::read(tree).unwrap()).unwrap()
}

#[test]
fn a_tree_shows_the_window_laid_out_and_built_for_it() {
    let tree = tree("2560x400");
    let planes = tree["planes"].as_array().unwrap();
    let song = planes.iter().find(|plane| plane["key"] == "song").unwrap();
    // The timeline is the window less its heads: far more than a plane is taken to be before
    // it is laid out.
    let shown = song["right"].as_f64().unwrap() - song["left"].as_f64().unwrap();
    let points = shown * 0.025;
    assert!(points > 2000.0, "{song}");
}
