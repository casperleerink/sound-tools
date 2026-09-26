//! Effect plugins in the chain of a track, for both formats: what a chain plays, what its
//! order does to the samples, what a missing one costs, and what a plugin's own state does.
//!
//! The effect is the repository's own test plugin, which is an instrument and an effect in one
//! (see `tooling/test-plugin-support`). In an effect slot it gets no notes, so what comes out
//! of it is `input * 0.5 + offset`, and the offset is its saved state. Chaining two of them
//! with different offsets is therefore an order a test reads straight out of the samples.

use plugin_host::PluginFormat;

use crate::support::{
    Harness, difference, plugin_state, saved_offset, test_plugin_host, test_plugin_of,
};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// The frame the one note of these tests sounds from.
const NOTE: usize = 480 * TICK;

/// A clip of one bar with one note at tick 480, played at `velocity`.
fn clip_at(velocity: u8) -> String {
    format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": 0, "length": 3840, "notes": [{{"start": 480, "length": 960, "pitch": 60, "velocity": {velocity}}}]}}}}"#
    )
}

/// The track record with these effects named in this order.
fn track_record(effects: &[&str]) -> String {
    let names: Vec<String> = effects.iter().map(|name| format!("{name:?}")).collect();
    format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "effects": [{}]}}}}"#,
        names.join(", ")
    )
}

/// Writes a track whose instrument is the test plugin and whose effects are more records of it,
/// each with a state asset of its own holding the offset it is given, in hundredths.
fn write_track(harness: &mut Harness, format: PluginFormat, velocity: u8, effects: &[(&str, i32)]) {
    let folder = "state/arrangement/piano";
    let names: Vec<&str> = effects.iter().map(|(name, _)| *name).collect();
    harness.write(&format!("{folder}/instance.json"), &track_record(&names));
    harness.write(
        &format!("{folder}/instrument.json"),
        &test_plugin_of(format, "piano"),
    );
    harness.write(&format!("{folder}/take.json"), &clip_at(velocity));
    for (name, offset) in effects {
        harness.write(
            &format!("{folder}/{name}.json"),
            &test_plugin_of(format, name),
        );
        let asset = harness.path(&format!("assets/plugin-state/{name}.bin"));
        std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
        std::fs::write(&asset, plugin_state(format, *offset)).unwrap();
    }
    let path = harness.path(folder);
    harness.apply(&[path]);
}

/// What every frame of `plain` becomes after an effect chain of these offsets, in hundredths.
fn through(plain: &[f32], offsets: &[i32]) -> Vec<f32> {
    plain
        .iter()
        .map(|sample| {
            let mut sample = *sample;
            for offset in offsets {
                sample = sample * 0.5 + *offset as f32 / 100.0;
            }
            sample
        })
        .collect()
}

/// A project of one plugin track with no effects, and what it plays.
fn without_effects(format: PluginFormat, velocity: u8) -> Vec<f32> {
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    write_track(&mut harness, format, velocity, &[]);
    assert_eq!(harness.project.problems(), []);
    harness.play_from_the_start(16_000)
}

#[test]
fn one_effect_after_the_instrument_changes_the_render_by_exactly_what_it_does() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let plain = without_effects(format, 40);
        let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
        write_track(&mut harness, format, 40, &[("trim", 10)]);
        assert_eq!(harness.project.problems(), [], "{format:?}");

        let played = harness.play_from_the_start(16_000);
        assert_eq!(played, through(&plain, &[10]), "{format:?}");
        // And it really changed something: the note is there and it is not what it was.
        assert!(
            plain[NOTE * 2] != 0.0,
            "{format:?}: the instrument was silent"
        );
        assert!(difference(&plain, &played).is_some(), "{format:?}");
    }
}

/// The heart of step 6: the order of the list is the order of the sound.
#[test]
fn two_effects_in_one_order_and_in_the_other_differ_by_what_each_adds() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let plain = without_effects(format, 40);
        let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
        write_track(&mut harness, format, 40, &[("a", 10), ("b", 40)]);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let a_then_b = harness.play_from_the_start(16_000);
        assert_eq!(a_then_b, through(&plain, &[10, 40]), "{format:?}");

        // The other way round is one record and one undo step: only the list changed.
        let file = "state/arrangement/piano/instance.json";
        assert_eq!(
            harness.write_and_apply(file, &track_record(&["b", "a"])),
            1,
            "{format:?}"
        );
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let b_then_a = harness.play_from_the_start(16_000);
        assert_eq!(b_then_a, through(&plain, &[40, 10]), "{format:?}");
        assert!(difference(&a_then_b, &b_then_a).is_some(), "{format:?}");

        // One undo of that reorder, and the track sounds as it did.
        assert!(harness.project.undo().unwrap().is_some(), "{format:?}");
        assert_eq!(
            difference(&a_then_b, &harness.play_from_the_start(16_000)),
            None,
            "{format:?}"
        );
    }
}

/// A missing effect: the record is untouched, the problem names it, and the track plays on
/// through the rest of its chain. A missing effect never silences a track.
#[test]
fn a_missing_effect_is_reported_and_the_track_plays_through_the_rest_of_the_chain() {
    let format = PluginFormat::Clap;
    let plain = without_effects(format, 40);
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    write_track(&mut harness, format, 40, &[("a", 10), ("b", 40)]);

    // The first of the two names a plugin this machine does not have.
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "a"}}"#;
    let file = "state/arrangement/piano/a.json";
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

/// A name in the list with no record at all is the other half of the same rule.
#[test]
fn a_listed_effect_with_no_record_is_reported_and_says_what_to_write() {
    let format = PluginFormat::Clap;
    let plain = without_effects(format, 40);
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    write_track(&mut harness, format, 40, &[("b", 40)]);
    let file = "state/arrangement/piano/instance.json";
    harness.write_and_apply(file, &track_record(&["gone", "b"]));

    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/arrangement/piano/instance.json");
    assert!(problems[0].message.contains("no gone.json"), "{problems:?}");
    assert_eq!(harness.play_from_the_start(16_000), through(&plain, &[40]));
}

/// The state of an effect is the plugin's own, saved as an asset. It is never an undo step,
/// and it comes back as it was after a close and a reopen.
#[test]
fn the_state_of_an_effect_survives_close_and_reopen_and_is_not_an_undo_step() {
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let (mut harness, plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
        // A note at full velocity, which the effect hears at 1.0 and takes as its offset.
        write_track(&mut harness, format, 127, &[("trim", 0)]);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let steps = harness.project.undo_label().map(str::to_string);

        let before = harness.play_from_the_start(16_000);
        assert_eq!(
            before[0], 0.0,
            "{format:?}: the effect started with an offset"
        );
        plugins.poll(&mut harness.project);

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
    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    write_track(&mut harness, format, 40, &[("a", 10), ("b", 40)]);
    let live = harness.play_from_the_start(16_000);

    let root = harness.project.root().to_path_buf();
    let plugins = test_plugin_host(&root, false);
    let (mut project, mut engine) = runtime::open_read_only_with(&root, plugins.clone()).unwrap();
    assert_eq!(project.problems(), []);
    project.engine().play();
    let rendered = runtime::render(&mut project, &mut engine, &plugins, 16_000).unwrap();
    assert_eq!(difference(&live, &rendered), None);
}
