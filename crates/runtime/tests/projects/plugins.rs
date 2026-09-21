//! A whole project with a hosted plugin as the instrument of a track: what it plays, what it
//! saves, and what happens when this machine does not have the plugin.
//!
//! The plugin is the repository's own test instrument, one per format, so CI needs no
//! third-party plugin. Its left channel is a cosine per key from the frame the note arrived
//! on, its right channel is the sustain pedal as a number, and its saved state is a transpose
//! that a pedal of 64 or more sets. See `tooling/test-plugin-support`.

use plugin_host::PluginFormat;

use crate::support::{
    Harness, TRACK, clip, difference, test_plugin, test_plugin_host, test_plugin_of,
};

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
    write_plugin_track_of(harness, PluginFormat::Clap, name, 1, clip);
}

/// The same, for either format and at a chosen place in the arrangement.
fn write_plugin_track_of(
    harness: &mut Harness,
    format: PluginFormat,
    name: &str,
    order: u32,
    clip: &str,
) {
    let folder = format!("state/arrangement/{name}");
    let track = TRACK
        .replace("NAME", name)
        .replace("ORDER", &order.to_string());
    harness.write(&format!("{folder}/instance.json"), &track);
    harness.write(
        &format!("{folder}/instrument.json"),
        &test_plugin_of(format, name),
    );
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

/// What an agent reads. `problems.txt` follows a plugin, as it follows a record that does not
/// load, so correcting the id clears the line with no restart.
#[test]
fn the_problems_file_follows_a_plugin_that_is_corrected() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "piano"}}"#;
    harness.write(
        "state/arrangement/piano/instance.json",
        &TRACK.replace("NAME", "piano").replace("ORDER", "1"),
    );
    harness.write("state/arrangement/piano/instrument.json", missing);
    let path = harness.path("state/arrangement/piano");
    harness.apply(&[path]);

    harness.project.poll().unwrap();
    let text = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert!(text.contains("com.example.nowhere"), "{text}");

    harness.write_and_apply(
        "state/arrangement/piano/instrument.json",
        &test_plugin("piano"),
    );
    harness.project.poll().unwrap();
    assert_eq!(harness.project.problems(), []);
    let text = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert!(text.contains("No problems"), "{text}");
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
    let (mut project, mut engine) = runtime::open_read_only_with(&root, plugins.clone()).unwrap();
    assert_eq!(project.problems(), []);
    project.engine().play();
    let rendered = runtime::render(&mut project, &mut engine, &plugins, 16_000).unwrap();
    assert_eq!(difference(&live, &rendered), None);
}

/// A render does the main-thread work of the plugin host for every buffer, as a live session
/// does. A plugin may be silent until the host answers it: a sampler that streams from disk
/// waits that way, and a render that only ran the engine would write its silence to the file
/// and call it the piece.
#[test]
fn a_render_answers_a_plugin_that_is_waiting_for_its_host() {
    tell_the_plugin_to_wait_for_its_host();
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let folder = tempfile::tempdir().unwrap();
        let (mut harness, _plugins) = Harness::with_test_plugin(folder);
        write_plugin_track_of(&mut harness, format, "piano", 1, &clip_with_pedal(None));
        assert_eq!(harness.project.problems(), []);

        let root = harness.project.root().to_path_buf();
        let plugins = test_plugin_host(&root, false);
        let (mut project, mut engine) =
            runtime::open_read_only_with(&root, plugins.clone()).unwrap();
        project.engine().play();
        let rendered = runtime::render(&mut project, &mut engine, &plugins, 16_000).unwrap();

        let note = 480 * TICK;
        let sounded = left(&rendered)[note..].iter().any(|sample| *sample != 0.0);
        assert!(sounded, "{format:?}: the render is silent");
    }
}

/// Makes both test plugins wait for the main-thread work of their host before they sound. The
/// plugin runs in this process, and nextest gives every test a process of its own.
fn tell_the_plugin_to_wait_for_its_host() {
    // SAFETY: nothing but this thread exists yet, so no other thread reads the environment.
    unsafe { std::env::set_var(test_plugin_support::NEEDS_HOST_VARIABLE, "1") };
}

/// Both formats in one piece. The two test plugins are the same instrument, so the two tracks
/// make the same sound and swapping one for the other is a change a render can be read for.
#[test]
fn one_project_plays_a_clap_track_and_a_vst3_track_and_a_swap_is_undone_exactly() {
    let folder = tempfile::tempdir().unwrap();
    let (mut harness, _plugins) = Harness::with_test_plugin(folder);
    write_plugin_track_of(
        &mut harness,
        PluginFormat::Clap,
        "clap-piano",
        1,
        &clip_with_pedal(None),
    );
    write_plugin_track_of(
        &mut harness,
        PluginFormat::Vst3,
        "vst3-piano",
        2,
        &clip_with_pedal(None),
    );
    assert_eq!(harness.project.problems(), []);

    // Both play. The two tracks are the same instrument in the two formats, so the piece is
    // twice one track: a render of it is the sum, and the note is where the clip says.
    let before = harness.play_from_the_start(16_000);
    let note = 480 * TICK;
    assert_eq!(
        before[..note * 2].iter().find(|sample| **sample != 0.0),
        None,
        "something sounded before the note"
    );
    assert!(before[note * 2] != 0.0, "the plugins made no sound");

    // Swapping the VST 3 track to CLAP, as an agent editing the file would. It names a state
    // file of its own, as the window does when a plugin is picked: the state of a VST 3 plugin
    // is not one a CLAP plugin can read, and a record that took one over would be reported.
    harness.write(
        "state/arrangement/vst3-piano/instrument.json",
        &test_plugin_of(PluginFormat::Clap, "swapped"),
    );
    let path = harness.path("state/arrangement/vst3-piano/instrument.json");
    assert_eq!(harness.apply(&[path]), 1);
    assert_eq!(harness.project.problems(), []);
    let swapped = harness.play_from_the_start(16_000);
    assert_eq!(difference(&before, &swapped), None, "the formats differ");

    // And back by undo. The record is the one it was, and so is the render.
    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(harness.project.problems(), []);
    let undone = harness.play_from_the_start(16_000);
    assert_eq!(difference(&before, &undone), None);
    let record =
        std::fs::read_to_string(harness.path("state/arrangement/vst3-piano/instrument.json"))
            .unwrap();
    assert!(record.contains("vst3"), "{record}");
}
