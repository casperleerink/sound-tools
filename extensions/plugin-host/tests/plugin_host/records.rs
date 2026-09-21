//! What a record that names a plugin this machine does not have does, and what fixing it does.

use crate::support::{Harness, Played, id, record};

/// Two notes, one in each half of the render. Engine time never goes back, so a test that
/// changes something between two renders needs a note on each side of the change.
fn played() -> Vec<Played> {
    vec![
        Played::On {
            frame: 100,
            pitch: 60,
            velocity: 100,
        },
        Played::Off {
            frame: 200,
            pitch: 60,
        },
        Played::On {
            frame: 600,
            pitch: 62,
            velocity: 100,
        },
        Played::Off {
            frame: 700,
            pitch: 62,
        },
    ]
}

fn plugin_json(plugin_id: &str) -> String {
    format!(
        r#"{{"tool": "plugin", "state": {{"format": "clap", "plugin_id": "{plugin_id}", "state_asset": "piano"}}}}"#
    )
}

#[test]
fn a_record_that_names_a_plugin_this_machine_does_not_have_is_a_problem_and_plays_nothing() {
    let mut harness = Harness::new();
    let mut missing = record("piano");
    missing.plugin_id = "com.example.not-installed".to_string();
    harness.add_track(missing.clone(), played());

    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("state/track/instrument.json: this machine has no CLAP plugin"),
        "{problems:?}"
    );
    assert!(
        problems[0].contains("com.example.not-installed"),
        "{problems:?}"
    );

    // The record is live and untouched, and the track is silent.
    let state = harness
        .project
        .resolve::<plugin_host::PluginRecord>(&id("track/instrument"))
        .expect("the record loaded");
    assert_eq!(harness.project.state(&state), Some(&missing));
    let render = harness.play(1024);
    assert_eq!(render.first_sound(), None);
}

/// No restart, no reinstall: the file is corrected and the plugin plays.
#[test]
fn correcting_the_id_in_the_file_makes_the_plugin_play_live() {
    let mut harness = Harness::new();
    let mut missing = record("piano");
    missing.plugin_id = "com.example.not-installed".to_string();
    harness.add_track(missing, played());
    assert_eq!(harness.play(512).first_sound(), None);

    // Nothing is installed and nothing restarts: only the file is corrected.
    let changed = harness.write_and_apply(
        "state/track/instrument.json",
        &plugin_json(test_clap_plugin::PLUGIN_ID),
    );
    assert_eq!(changed, 1);
    assert_eq!(harness.problems(), Vec::<String>::new());

    // The note at frame 600 of the same playback sounds, 88 frames into the next render.
    let render = harness.render(512);
    assert_eq!(render.first_sound(), Some(88));
}

#[test]
fn a_record_whose_plugin_id_becomes_unknown_goes_silent_instead_of_playing_the_old_one() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());
    assert_eq!(harness.play(512).first_sound(), Some(100));

    harness.write_and_apply(
        "state/track/instrument.json",
        &plugin_json("com.example.not-installed"),
    );
    assert_eq!(harness.problems().len(), 1, "{:?}", harness.problems());
    assert_eq!(harness.render(512).first_sound(), None);
}

#[test]
fn two_records_that_name_one_state_asset_are_reported() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());
    harness.write_and_apply(
        "state/track/second.json",
        &format!(
            r#"{{"tool": "plugin", "state": {{"format": "clap", "plugin_id": "{}", "state_asset": "piano"}}}}"#,
            test_clap_plugin::PLUGIN_ID
        ),
    );
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("already used by the instance"),
        "{problems:?}"
    );
    assert!(
        problems[0].contains("state/track/second.json"),
        "{problems:?}"
    );
}

#[test]
fn a_record_with_an_unknown_field_or_format_does_not_load() {
    let mut harness = Harness::new();
    harness.write_and_apply(
        "state/one.json",
        r#"{"tool": "plugin", "state": {"format": "vst3", "plugin_id": "a.b", "state_asset": "x"}}"#,
    );
    harness.write_and_apply(
        "state/two.json",
        r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "a.b", "state_asset": "x", "gain": 1}}"#,
    );
    let problems = harness.problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("state.format")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|problem| problem.contains("gain")),
        "{problems:?}"
    );
}
