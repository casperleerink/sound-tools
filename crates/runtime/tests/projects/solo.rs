//! `--solo` of a render and an analysis: the tracks it names play alone, in memory only.

use crate::mixer::files;
use crate::plugin_hosts::open_read_only;
use crate::support::{BAR, Harness, clip};

fn track(name: &str, order: u32, mute: bool) -> String {
    format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "{name}", "order": {order}, "mute": {mute}}}}}"#
    )
}

/// A bass that is muted in its file, and a lead.
fn bass_and_lead() -> Harness {
    let mut harness = Harness::new();
    harness.write_track("bass", 1, 0.3, &[("line", clip(0, 3840, &[(0, 3840, 40)]))]);
    harness.write_track("lead", 2, 0.3, &[("tune", clip(0, 3840, &[(0, 1920, 72)]))]);
    let path = harness.write(
        "state/arrangement/bass/instance.json",
        &track("bass", 1, true),
    );
    assert_eq!(harness.apply(&[path]), 1);
    harness
}

#[test]
fn a_soloed_track_plays_alone_unmuted_and_no_file_changes() {
    let harness = bass_and_lead();
    let before = files(harness.project.root());
    let (mut project, mut engine, plugins) = open_read_only(harness.project.root());
    let problems = runtime::solo(&mut project, &mut engine, &plugins, &["bass".to_string()]);
    assert!(problems.unwrap().is_empty());
    project.engine().play();
    let soloed = runtime::render(&mut project, &mut engine, &plugins, BAR).unwrap();
    assert_eq!(files(harness.project.root()), before);

    // The same as the bass alone, unmuted, from the files.
    harness.write(
        "state/arrangement/bass/instance.json",
        &track("bass", 1, false),
    );
    harness.write(
        "state/arrangement/lead/instance.json",
        &track("lead", 2, true),
    );
    let mut alone = harness.reopen();
    let expected = alone.play(BAR);
    assert!(expected.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(soloed, expected);
}

#[test]
fn a_track_that_is_not_there_names_the_ones_that_are() {
    let harness = bass_and_lead();
    let (mut project, mut engine, plugins) = open_read_only(harness.project.root());
    let mut solo =
        |name: &str| runtime::solo(&mut project, &mut engine, &plugins, &[name.to_string()]);
    let error = solo("drums").unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"no track is called "drums". The tracks: "bass", "lead""#
    );
    // By id too.
    solo("arrangement/lead").unwrap();
}
