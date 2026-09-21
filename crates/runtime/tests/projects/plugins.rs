//! A whole project with a CLAP plugin as the instrument of a track: what it plays, what it
//! saves, and what happens when this machine does not have the plugin.
//!
//! The plugin is the repository's own test instrument, so CI needs no third-party plugin. Its
//! left channel is a cosine per key from the frame the note arrived on, its right channel is
//! the sustain pedal as a number, and its saved state is a transpose that a pedal of 64 or
//! more sets. See `tooling/test-clap-plugin`.

use crate::support::{Harness, TRACK, clip, difference, test_plugin, test_plugin_host};

/// Frames per tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

/// A clip of one bar with one note at tick 480, and a pedal move at tick 0 when `pedal` says
/// so. The pedal value is what the plugin reads and also what it saves as its transpose.
fn clip_with_pedal(pedal: Option<u8>) -> String {
    let pedal = match pedal {
        Some(value) => format!(r#", "pedal": [{{"start": 0, "value": {value}}}]"#),
        None => String::new(),
    };
    format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": 0, "length": 3840, "notes": [{{"start": 480, "length": 960, "pitch": 60, "velocity": 100}}]{pedal}}}}}"#
    )
}

/// Writes a track whose instrument is the test plugin, with one clip.
fn write_plugin_track(harness: &mut Harness, name: &str, clip: &str) {
    let folder = format!("state/arrangement/{name}");
    let track = TRACK.replace("NAME", name).replace("ORDER", "1");
    harness.write(&format!("{folder}/instance.json"), &track);
    harness.write(&format!("{folder}/instrument.json"), &test_plugin(name));
    harness.write(&format!("{folder}/take.json"), clip);
    let folder = harness.path(&folder);
    assert_eq!(harness.apply(&[folder]), 3);
}

fn left(render: &[f32]) -> Vec<f32> {
    render.iter().step_by(2).copied().collect()
}

fn right(render: &[f32]) -> Vec<f32> {
    render.iter().skip(1).step_by(2).copied().collect()
}

#[test]
fn a_plugin_track_plays_the_notes_of_its_clip_and_the_pedal_with_its_value() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    write_plugin_track(&mut harness, "piano", &clip_with_pedal(Some(100)));
    assert_eq!(harness.project.problems(), []);

    let render = harness.play(16_000);
    let (left, right) = (left(&render), right(&render));

    // The note at tick 480 sounds from its own frame and not one frame earlier.
    let note = 480 * TICK;
    assert_eq!(left[..note], vec![0.0; note]);
    assert_ne!(left[note], 0.0);

    // The pedal at tick 0 arrives with its value, not as on or off.
    assert_eq!(right[0], 100.0 / 127.0);
    assert_eq!(right[note], 100.0 / 127.0);
}

/// The plugin's own state is an asset of the project. Nothing else keeps it.
#[test]
fn plugin_state_is_saved_comes_back_on_reopen_and_the_render_is_the_same() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, plugins) = Harness::with_test_plugin(folder);
    write_plugin_track(&mut harness, "piano", &clip_with_pedal(Some(100)));
    let before = harness.play(16_000);
    plugins.poll(&harness.project);

    // A pedal of 100 makes the test plugin transpose by 36 and say its state changed.
    let asset = harness.path("assets/plugin-state/piano.bin");
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(&saved[..4], b"STT1");
    assert_eq!(
        i32::from_le_bytes([saved[4], saved[5], saved[6], saved[7]]),
        36
    );

    // The same project again: the same bytes out.
    let (mut harness, plugins) = harness.reopen_with_test_plugin();
    let after = harness.play(16_000);
    assert_eq!(difference(&before, &after), None);
    plugins.poll(&harness.project);
    assert_eq!(std::fs::read(&asset).unwrap(), saved);

    // Now take the pedal out of the clip and open it again. The notes are still transposed,
    // which can only come from the state the plugin loaded out of the project.
    harness.write("state/arrangement/piano/take.json", &clip_with_pedal(None));
    let (mut harness, _plugins) = harness.reopen_with_test_plugin();
    let without_pedal = harness.play(16_000);
    assert_eq!(left(&without_pedal), left(&before));
    assert!(right(&without_pedal).iter().all(|sample| *sample == 0.0));
}

/// The missing-plugin case: the record is left alone, the problem names it, that track is
/// silent and everything else plays as before.
#[test]
fn a_project_that_names_a_plugin_this_machine_does_not_have_opens_and_reports_it() {
    let pad = |harness: &mut Harness| {
        harness.write_track(
            "pad",
            2,
            0.1,
            &[("long", clip(0, 15360, &[(0, 15360, 72)]))],
        );
    };
    // The same project without the plugin track, for what the rest of it must sound like.
    let (mut without, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    pad(&mut without);
    let only_pad = without.play(16_000);

    let (mut harness, _plugins) = Harness::with_test_plugin(tempfile::tempdir().unwrap());
    pad(&mut harness);
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "piano"}}"#;
    let folder = "state/arrangement/piano";
    harness.write(
        &format!("{folder}/instance.json"),
        &TRACK.replace("NAME", "piano").replace("ORDER", "1"),
    );
    harness.write(&format!("{folder}/instrument.json"), missing);
    harness.write(&format!("{folder}/take.json"), &clip_with_pedal(Some(100)));
    let path = harness.path(folder);
    harness.apply(&[path]);

    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "state/arrangement/piano/instrument.json");
    assert!(
        problems[0].message.contains("com.example.nowhere"),
        "{problems:?}"
    );

    // The track is silent and the other one renders exactly as it does without it.
    let with_missing = harness.play(16_000);
    assert_eq!(difference(&only_pad, &with_missing), None);

    // Close and reopen: the record is byte for byte what the agent wrote, and no state asset
    // was made for a plugin that never loaded.
    let record = harness.path(&format!("{folder}/instrument.json"));
    let bytes = std::fs::read(&record).unwrap();
    let (harness, _plugins) = harness.reopen_with_test_plugin();
    assert_eq!(std::fs::read(&record).unwrap(), bytes);
    assert_eq!(String::from_utf8(bytes).unwrap(), missing);
    assert!(!harness.path("assets/plugin-state/piano.bin").exists());
    assert_eq!(harness.project.problems().len(), 1);
}

/// `--render` and `--inspect` open the project read-only. They load plugins too, so an offline
/// render of a plugin track is the sound the composer hears.
#[test]
fn an_offline_render_of_a_read_only_project_plays_the_plugin() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    write_plugin_track(&mut harness, "piano", &clip_with_pedal(Some(100)));
    let live = harness.play(16_000);

    let root = harness.project.root().to_path_buf();
    let plugins = test_plugin_host(&root, false);
    let (mut project, mut engine) = runtime::open_read_only_with(&root, plugins).unwrap();
    assert_eq!(project.problems(), []);
    project.engine().play();
    let rendered = runtime::render(&mut project, &mut engine, 16_000).unwrap();
    assert_eq!(difference(&live, &rendered), None);
}
