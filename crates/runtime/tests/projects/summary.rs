//! What `--inspect` prints for a small known project.

use crate::support::{
    Harness, TRACK, clip, test_plugin, test_plugin_host_that_only_lists, test_plugin_of,
};

#[test]
fn the_summary_tells_what_plays_where() {
    let mut harness = Harness::piece();
    harness.write_and_apply(
        "state/arrangement/pad/bars-5-8.json",
        &clip(15360, 15360, &[(960, 480, 70), (1440, 480, 74)]),
    );
    harness.write_and_apply(
        "state/drone.json",
        r#"{"tool": "tone", "state": {"frequency_hz": 110.0, "gain": 0.1}}"#,
    );
    harness.write_and_apply("state/arrangement/pad/broken.json", "{");
    let project_file = std::fs::read_to_string(harness.path("project.json")).unwrap();
    let project_file = project_file.replace(
        r#"[{"tick": 0, "bpm": 120.0}]"#,
        r#"[{"tick": 0, "bpm": 120.0}, {"tick": 15360, "bpm": 90.0}]"#,
    );
    harness.write_and_apply("project.json", &project_file);

    // A second, read-only open next to the live one sees the same: this is `--inspect`.
    let (inspected, _engine, _plugins) = runtime::open_read_only(harness.project.root()).unwrap();
    let expected = r#"extensions: arrangement, compressor, filter, fit-tempo, instrument, plugin-host, tone
time signature: 4/4, 3840 ticks per bar, 960 ticks per beat
tempo: 120 bpm from 1:1:000 (tick 0)
tempo: 90 bpm from 5:1:000 (tick 15360)
arrangement `arrangement`: 3 tracks. Positions are bar:beat:tick, a clip runs up to its end position
  track `arrangement/track-1` "Track 1": colour blue, order 0, instrument instrument.synth
    no clips
  track `arrangement/piano` "piano": colour blue, order 1, instrument instrument.synth
    clip `arrangement/piano/chords`: 1:1:000 to 5:1:000, ticks 0 to 15360, 5 notes, pitch 48 to 64
  track `arrangement/pad` "pad": colour blue, order 2, instrument instrument.synth
    clip `arrangement/pad/long`: 1:1:000 to 5:1:000, ticks 0 to 15360, 1 note, pitch 72 to 72
    clip `arrangement/pad/bars-5-8`: 5:1:000 to 9:1:000, ticks 15360 to 30720, 2 notes, pitch 70 to 74
instance `drone` [tone] {"frequency_hz":110.0,"gain":0.1}
connections: 0
problems: 1
  state/arrangement/pad/broken.json: ?: EOF while parsing an object at line 1 column 1"#;
    assert_eq!(runtime::summary(&inspected), expected);
    assert_eq!(runtime::summary(&harness.project), expected);
}

/// `--inspect` opens the project with a host that loads no plugin, because printing a project
/// needs none and a plugin that is loaded runs somebody else's code in this process. What it
/// prints must be the same all the same: an agent reads it to find out that this machine does
/// not have a plugin the project names.
#[test]
fn inspecting_prints_the_same_project_without_loading_a_plugin() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    for (name, order, record) in [
        ("piano", 1, test_plugin("piano")),
        (
            "missing",
            2,
            test_plugin_of(plugin_host::PluginFormat::Clap, "gone")
                .replace(test_clap_plugin::PLUGIN_ID, "com.example.nowhere"),
        ),
    ] {
        let track = format!("state/arrangement/{name}");
        harness.write(
            &format!("{track}/instance.json"),
            &TRACK
                .replace("NAME", name)
                .replace("ORDER", &order.to_string()),
        );
        harness.write(&format!("{track}/instrument.json"), &record);
        harness.write(
            &format!("{track}/take.json"),
            &clip(0, 3840, &[(480, 960, 60)]),
        );
        let path = harness.path(&track);
        harness.apply(&[path]);
    }
    let loading = runtime::summary(&harness.project);

    // The same folder, opened the way `--inspect` opens it.
    let listing = test_plugin_host_that_only_lists(harness.project.root());
    let (inspected, _engine) =
        runtime::open_read_only_with(harness.project.root(), listing).unwrap();
    assert_eq!(runtime::summary(&inspected), loading);
    assert!(loading.contains("com.example.nowhere"), "{loading}");
    assert!(loading.contains("problems: 1"), "{loading}");
}
