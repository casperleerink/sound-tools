//! What `--inspect` prints for a small known project.

use crate::plugin_hosts::open_read_only;
use crate::rack::racks_files;
use crate::support::{
    Harness, clip, test_plugin, test_plugin_host_that_only_lists, test_plugin_of, write,
};

/// Edits `project.json` as an agent would, and applies it.
fn edit_project(harness: &mut Harness, edit: impl FnOnce(&mut serde_json::Value)) {
    let text = std::fs::read_to_string(harness.path("project.json")).unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    edit(&mut file);
    harness.write_and_apply("project.json", &file.to_string());
    assert_eq!(harness.project.problems(), [], "{file}");
}

/// The time signatures of a project, as `project.json` holds them.
fn signatures(signatures: &str) -> impl FnOnce(&mut serde_json::Value) {
    let signatures: serde_json::Value = serde_json::from_str(signatures).unwrap();
    move |file| {
        let tempo_map = file["tempo_map"].as_object_mut().unwrap();
        tempo_map.remove("time_signature");
        tempo_map.insert("time_signatures".into(), signatures);
    }
}

#[test]
fn the_summary_tells_what_plays_where() {
    let mut harness = Harness::piece();
    harness.write_and_apply(
        "state/drone.json",
        r#"{"tool": "tone", "state": {"frequency_hz": 110.0, "gain": 0.1}}"#,
    );
    edit_project(&mut harness, |file| {
        let changes = serde_json::json!([{"tick": 0, "bpm": 120.0}, {"tick": 15360, "bpm": 90.0}]);
        file["tempo_map"]["tempo_changes"] = changes;
    });
    harness.write_and_apply("state/pad/broken.json", "{");

    // A second, read-only open next to the live one sees the same: this is `--inspect`.
    let (inspected, _engine, _plugins) = open_read_only(harness.project.root());
    let summary = runtime::summary(&harness.project);
    assert_eq!(runtime::summary(&inspected), summary);
    let lines = [
        "time signature: 4/4 from bar 1 (tick 0), 3840 ticks per bar, 960 ticks per beat\n",
        "tempo: 90 bpm from 5:1:000 (tick 15360)\n",
        "instance `drone` [tone] {\"frequency_hz\":110.0,\"gain\":0.1}\n",
        "  instance `pad/instrument` [instrument.synth] ",
        "connections: 2\n",
        "  state/pad/broken.json: ?: EOF while parsing an object at line 1 column 1",
    ];
    for line in lines {
        assert!(summary.contains(line), "{line} in {summary}");
    }
}

/// A project whose time signature changes: the summary says where each one starts.
#[test]
fn the_summary_follows_a_changing_time_signature() {
    let mut harness = Harness::piece();
    edit_project(
        &mut harness,
        signatures(
            r#"[{"signature": "4/4", "bars": 2}, {"signature": "3/16", "bars": 1}, {"signature": "7/8", "bars": 1}]"#,
        ),
    );

    let summary = runtime::summary(&harness.project);
    let lines: Vec<&str> = summary.lines().collect();
    assert_eq!(
        lines[1..4],
        [
            "time signature: 4/4 from bar 1 (tick 0), 3840 ticks per bar, 960 ticks per beat",
            "time signature: 3/16 from bar 3 (tick 7680), 720 ticks per bar, 240 ticks per beat",
            "time signature: 7/8 from bar 4 (tick 8400), 3360 ticks per bar, 480 ticks per beat",
        ]
    );
}

/// `--inspect` opens the project with a host that loads no plugin, because printing a project
/// needs none and a plugin that is loaded runs somebody else's code in this process. What it
/// prints must be the same all the same: an agent reads it to find out that this machine does
/// not have a plugin the project names.
#[test]
fn inspecting_prints_the_same_project_without_loading_a_plugin() {
    let folder = tempfile::tempdir().unwrap();
    let missing = test_plugin_of(plugin_host::PluginFormat::Clap, "gone")
        .replace(test_clap_plugin::PLUGIN_ID, "com.example.nowhere");
    let (piano, take) = (test_plugin("piano"), clip(0, 3840, &[(480, 960, 60)]));
    let racks = [
        (
            "piano",
            [("instrument", piano.as_str()), ("take", take.as_str())],
        ),
        (
            "missing",
            [("instrument", missing.as_str()), ("take", take.as_str())],
        ),
    ];
    let racks: Vec<(&str, &[(&str, &str)])> = (racks.iter())
        .map(|(name, children)| (*name, children.as_slice()))
        .collect();
    for (path, contents) in racks_files(&racks) {
        write(folder.path(), &path, &contents);
    }
    let (harness, _plugins) = Harness::with_test_plugin(folder);
    let loading = runtime::summary(&harness.project);

    // The same folder, opened the way `--inspect` opens it.
    let listing = test_plugin_host_that_only_lists(harness.project.root());
    let (inspected, _engine) =
        runtime::open_read_only_with(harness.project.root(), listing).unwrap();
    assert_eq!(runtime::summary(&inspected), loading);
    assert!(loading.contains("com.example.nowhere"), "{loading}");
    assert!(loading.contains("problems: 1"), "{loading}");
}

/// Each section with its bars and ticks, so an agent can copy the music of "the chorus". The
/// last runs to the bar line after the last timeline, or to the end when it starts after it.
#[test]
fn the_summary_tells_the_bars_of_each_section() {
    let mut harness = Harness::piece();
    harness.write_and_apply(
        "state/pad/bars-5-8.json",
        &clip(15360, 15000, &[(960, 480, 70)]),
    );
    edit_project(&mut harness, |file| {
        file["sections"] =
            serde_json::json!([{"bar": 1, "name": "Intro"}, {"bar": 3, "name": "Chorus"}]);
    });
    let summary = runtime::summary(&harness.project);
    let lines = [
        "section \"Intro\": bars 1 to 2, ticks 0 to 7680\n",
        "section \"Chorus\": bars 3 to 8, ticks 7680 to 30720\n",
    ];
    for line in lines {
        assert!(summary.contains(line), "{line} in {summary}");
    }

    edit_project(&mut harness, |file| {
        let outro = serde_json::json!({"bar": 12, "name": "Outro"});
        file["sections"].as_array_mut().unwrap().push(outro);
    });
    let summary = runtime::summary(&harness.project);
    let lines = [
        "section \"Chorus\": bars 3 to 11, ticks 7680 to 42240\n",
        "section \"Outro\": from bar 12 (tick 42240) to the end\n",
    ];
    for line in lines {
        assert!(summary.contains(line), "{line} in {summary}");
    }

    // From bar 3 the bars are 7/8, 3360 ticks long.
    edit_project(&mut harness, |file| {
        signatures(r#"[{"signature": "4/4", "bars": 2}, {"signature": "7/8", "bars": 1}]"#)(file);
        file["sections"][1]["bar"] = 4.into();
    });
    let summary = runtime::summary(&harness.project);
    let lines = [
        "section \"Intro\": bars 1 to 3, ticks 0 to 11040\n",
        "section \"Chorus\": bars 4 to 11, ticks 11040 to 37920\n",
        "section \"Outro\": from bar 12 (tick 37920) to the end\n",
    ];
    for line in lines {
        assert!(summary.contains(line), "{line} in {summary}");
    }
}
