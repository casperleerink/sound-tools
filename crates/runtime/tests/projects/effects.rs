//! Effect plugins in the chain of a rack, for both formats: what a chain plays, what its order
//! does to the samples, what a missing one costs, and what a plugin's own state does.
//!
//! The effect is the repository's own test plugin, which is an instrument and an effect in one
//! (see `tooling/test-plugin-support`). In the chain it hears the notes of the rack too, as the
//! instrument does, so what comes out of it is `input * 0.5 + offset` plus those notes,
//! and the offset is its saved state. Chaining two of them with different offsets is therefore
//! an order a test reads straight out of the samples.

use plugin_host::PluginFormat;

use plugin_host::Plugins;

use crate::rack::racks_files;
use crate::support::{
    Harness, difference, plugin_state, read_only_test_plugin_host, saved_offset, test_plugin_of,
    write,
};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// The frame the one note of these tests sounds from.
const NOTE: usize = 480 * TICK;

/// A timeline of one bar with one note at tick 480, played at `velocity`.
fn clip_at(velocity: u8) -> String {
    format!(
        r#"{{"tool": "timeline", "state": {{"start": 0, "length": 3840, "notes": [{{"start": 480, "length": 960, "pitch": 60, "velocity": {velocity}}}]}}}}"#
    )
}

/// The record of the rack with these effects in this order.
fn rack_record(effects: &[&str]) -> String {
    serde_json::json!({ "tool": "rack", "state": { "order": effects } }).to_string()
}

/// A rack whose instrument is the test plugin and whose effects are more records of it, each with
/// a state asset of its own holding the offset it is given, in hundredths.
fn rack(format: PluginFormat, velocity: u8, effects: &[(&str, i32)]) -> (Harness, Plugins) {
    let folder = tempfile::tempdir().unwrap();
    let (piano, take) = (test_plugin_of(format, "piano"), clip_at(velocity));
    let records: Vec<(String, String)> = (effects.iter())
        .map(|(name, _)| (name.to_string(), test_plugin_of(format, name)))
        .collect();
    let mut children = vec![("instrument", piano.as_str()), ("take", take.as_str())];
    children.extend(
        records
            .iter()
            .map(|(name, record)| (name.as_str(), record.as_str())),
    );
    for (path, contents) in racks_files(&[("rack", &children)]) {
        write(folder.path(), &path, &contents);
    }
    for (name, offset) in effects {
        let asset = folder
            .path()
            .join(format!("assets/plugin-state/{name}.bin"));
        std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
        std::fs::write(&asset, plugin_state(format, *offset)).unwrap();
    }
    Harness::with_test_plugin(folder)
}

/// What every frame of `plain`, the instrument alone, becomes after an effect chain of these
/// offsets, in hundredths. Each effect plays the notes again, as the instrument does.
fn through(plain: &[f32], offsets: &[i32]) -> Vec<f32> {
    plain
        .iter()
        .map(|notes| {
            let mut sample = *notes;
            for offset in offsets {
                sample = sample * 0.5 + *offset as f32 / 100.0 + notes;
            }
            sample
        })
        .collect()
}

/// A rack of the plugin with no effects, and what it plays.
fn without_effects(format: PluginFormat, velocity: u8) -> Vec<f32> {
    let (mut harness, _plugins) = rack(format, velocity, &[]);
    assert_eq!(harness.project.problems(), []);
    harness.play_from_the_start(16_000)
}

/// The heart of step 6: the order of the list is the order of the sound.
#[test]
fn two_effects_in_one_order_and_in_the_other_differ_by_what_each_adds() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let plain = without_effects(format, 40);
        let (mut harness, _plugins) = rack(format, 40, &[("a", 10), ("b", 40)]);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let a_then_b = harness.play_from_the_start(16_000);
        assert_eq!(a_then_b, through(&plain, &[10, 40]), "{format:?}");

        // The other way round is one record and one undo step: only the list changed.
        let file = "state/rack/instance.json";
        assert_eq!(
            harness.write_and_apply(file, &rack_record(&["b", "a"])),
            1,
            "{format:?}"
        );
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let b_then_a = harness.play_from_the_start(16_000);
        assert_eq!(b_then_a, through(&plain, &[40, 10]), "{format:?}");
        assert!(difference(&a_then_b, &b_then_a).is_some(), "{format:?}");

        // One undo of that reorder, and the rack sounds as it did.
        assert!(harness.project.undo().unwrap().is_some(), "{format:?}");
        assert_eq!(
            difference(&a_then_b, &harness.play_from_the_start(16_000)),
            None,
            "{format:?}"
        );
    }
}

/// A missing effect: the record is untouched, the problem names it, and the rack plays on
/// through the rest of its chain. A missing effect never silences it.
#[test]
fn a_missing_effect_is_reported_and_the_rack_plays_through_the_rest_of_the_chain() {
    let format = PluginFormat::Clap;
    let plain = without_effects(format, 40);
    let (mut harness, _plugins) = rack(format, 40, &[("a", 10), ("b", 40)]);

    // The first of the two names a plugin this machine does not have.
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "a"}}"#;
    let file = "state/rack/a.json";
    harness.write_and_apply(file, missing);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].message.contains("com.example.nowhere"),
        "{problems:?}"
    );

    // The sound goes through the slot unchanged and through the effect that is there.
    let played = harness.play_from_the_start(16_000);
    assert_eq!(played, through(&plain, &[40]));

    // Close and reopen: the record is byte for byte what the agent wrote.
    let bytes = std::fs::read(harness.path(file)).unwrap();
    let (harness, _plugins) = harness.reopen_with_test_plugin();
    assert_eq!(std::fs::read(harness.path(file)).unwrap(), bytes);
    assert_eq!(String::from_utf8(bytes).unwrap(), missing);
    assert_eq!(harness.project.problems().len(), 1);
}

/// The state of an effect is the plugin's own, saved as an asset. It is never an undo step,
/// and it comes back as it was after a close and a reopen.
#[test]
fn the_state_of_an_effect_survives_close_and_reopen_and_is_not_an_undo_step() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let (mut harness, plugins) = rack(format, 127, &[("trim", 0)]);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let steps = harness.project.undo_label().map(str::to_string);

        let before = harness.play_from_the_start(16_000);
        assert_eq!(
            before[0], 0.0,
            "{format:?}: the effect started with an offset"
        );
        plugins.poll(&harness.project);

        // What the plugin learned is in the project, and no undo step was made for it.
        let asset = harness.path("assets/plugin-state/trim.bin");
        let saved = std::fs::read(&asset).unwrap();
        assert_eq!(saved_offset(format, &saved), 100, "{format:?}");
        assert_eq!(
            harness.project.undo_label().map(str::to_string),
            steps,
            "{format:?}: saving plugin state was an undo step"
        );

        // Close and reopen: the effect comes up with the offset it learned, so it adds 1.0
        // from the first frame, before any note.
        let (mut harness, _plugins) = harness.reopen_with_test_plugin();
        let after = harness.play_from_the_start(16_000);
        assert_eq!(after[0], 1.0, "{format:?}");
        assert_eq!(after[1], 1.0, "{format:?}");
        assert_eq!(
            after[NOTE * 2],
            before[NOTE * 2] + 1.0,
            "{format:?}: the note is not what it was plus the offset"
        );
    }
}

/// An offline render of a read-only project plays the chain, so what `--render` writes is what
/// the composer hears.
#[test]
fn an_offline_render_of_a_read_only_project_plays_the_whole_chain() {
    let format = PluginFormat::Vst3;
    let (mut harness, _plugins) = rack(format, 40, &[("a", 10), ("b", 40)]);
    let live = harness.play_from_the_start(16_000);

    let root = harness.project.root().to_path_buf();
    let plugins = read_only_test_plugin_host(&root);
    let (mut project, mut engine) = runtime::open_read_only_with(&root, plugins.clone()).unwrap();
    assert_eq!(project.problems(), []);
    project.engine().play();
    let rendered = runtime::render(&mut project, &mut engine, &plugins, 16_000).unwrap();
    assert_eq!(difference(&live, &rendered), None);
}
