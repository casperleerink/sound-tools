//! What this host does about the `restartComponent` flags that need it to act. `kLatencyChanged`
//! is in `lifecycle.rs` and in the latency tests of the runtime.
//!
//! The plugin is told by a note on a key it does not play: its processor reports the key and its
//! controller acts on the main thread, where `restartComponent` belongs.

use plugin_host::PluginFormat;
use test_plugin_support::{
    DROP_PEDAL_KEY, LATENCY_KEY, LIST_LEVEL_KEY, MONO_KEY, MOVE_PEDAL_KEY, PRESET_KEY, RELOAD_KEY,
};

use crate::support::{
    Harness, Played, id, lifecycle, peak, record, saved_transpose, state_asset, tell_the_plugin,
    tell_the_plugin_to_ask_for_a_reload_as_it_loads, tell_the_plugin_to_edit_as_it_restarts,
    tell_the_plugin_to_list_its_level_late,
};

fn count(log: &std::path::Path, call: &str) -> usize {
    lifecycle(log)
        .iter()
        .filter(|line| line.call == call)
        .count()
}

fn on(frame: u64, pitch: u8) -> Played {
    Played::On {
        frame,
        pitch,
        velocity: 100,
    }
}

/// `kParamValuesChanged`: the controller loads a preset that shows `Level` at a half, and the
/// processor was told nothing. The host sends the difference.
#[test]
fn a_preset_the_controller_loads_by_itself_reaches_the_processor() {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![on(0, 60), on(2048, PRESET_KEY)],
    );
    let left = harness.play(8192).left();
    let before = peak(&left[..2048]);
    let after = peak(&left[4096..]);
    assert!(before > 0.0);
    assert!(
        (after - before / 2.0).abs() < before / 100.0,
        "the plugin plays at {after} of {before} after its preset, which is not the half the controller shows"
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// `kIoChanged`: the plugin's main output becomes one channel.
#[test]
fn a_plugin_whose_output_goes_mono_is_started_again_and_heard_on_both_sides() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    let mut harness = Harness::new();
    // Starting again ends the note that sounds, so another one follows it.
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![on(0, 60), on(2048, MONO_KEY), on(6144, 64)],
    );
    let render = harness.play(12288);
    let (left, right) = (render.left(), render.right());
    // Stereo at first: the tone on the left, the pedal, up, on the right.
    assert!(peak(&left[..2048]) > 0.0);
    assert_eq!(peak(&right[..2048]), 0.0);
    assert!(peak(&left[7168..]) > 0.0);
    assert_eq!(left[7168..], right[7168..]);
    assert_eq!(harness.problems(), Vec::<String>::new());

    let calls: Vec<String> = lifecycle(log.path())
        .into_iter()
        .map(|call| call.call)
        .collect();
    let went = calls.iter().position(|call| call == "went_mono").unwrap();
    let after: Vec<&str> = calls[went..]
        .iter()
        .map(String::as_str)
        .filter(|call| ["deactivate", "activate"].contains(call))
        .collect();
    assert_eq!(after[..2], ["deactivate", "activate"], "{calls:?}");
}

/// `kReloadComponent`: the record is run again, so the plugin is saved on its way out and a new
/// one loads from that state.
#[test]
fn a_plugin_that_asks_to_be_loaded_again_comes_back_as_it_sounded() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            on(0, 60),
            // Transposes the plugin by 36 semitones, a change of its own state.
            Played::Pedal {
                frame: 512,
                value: 100,
            },
            on(2048, RELOAD_KEY),
            on(6144, 60),
        ],
    );
    harness.play(4096);
    let retries = harness.plugins.take_retries();
    assert_eq!(retries, [id("track/instrument")]);
    for instance in &retries {
        assert!(harness.project.rebind(instance).unwrap());
    }
    let left = harness.render(4096).left();
    assert!(
        peak(&left[2048..]) > 0.0,
        "the plugin loaded again is silent"
    );
    assert_eq!(harness.problems(), Vec::<String>::new());

    let calls = lifecycle(log.path());
    let first = calls.iter().find(|call| call.call == "initialize").unwrap();
    let initialized = calls.iter().filter(|call| call.call == "initialize");
    assert_eq!(initialized.count(), 2, "{calls:?}");
    let ended = calls.iter().filter(|call| call.call == "terminate");
    assert!(
        ended.clone().any(|call| call.plugin == first.plugin),
        "{calls:?}"
    );

    harness.plugins.close(&mut harness.project);
    let bytes = harness
        .project
        .assets()
        .read(&state_asset("piano"))
        .unwrap()
        .unwrap();
    assert_eq!(saved_transpose(PluginFormat::Vst3, &bytes), 36);
}

/// The edit is on its way to an audio side the engine is giving back. That side hands what it
/// never played back to the host, and the next one plays it.
#[test]
fn an_edit_made_while_the_plugin_is_started_again_is_not_lost() {
    tell_the_plugin(None, None);
    tell_the_plugin_to_edit_as_it_restarts();
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            on(0, 60),
            Played::On {
                frame: 2048,
                pitch: LATENCY_KEY,
                velocity: 1,
            },
            on(6144, 64),
        ],
    );
    let left = harness.play(12288).left();
    let before = peak(&left[..2048]);
    let after = peak(&left[7168..]);
    assert!(before > 0.0);
    assert!(
        (after - before / 4.0).abs() < before / 100.0,
        "the plugin plays at {after} of {before} after it was started again, which is not the quarter the edit asked for"
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// `kMidiCCAssignmentChanged` while the pedal is down, as a MIDI learn or a loaded preset does.
/// The old parameter is let go of, else it would stay down for good.
#[test]
fn a_plugin_that_moves_its_pedal_gets_the_pedal_on_its_new_parameter() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            on(0, 60),
            Played::Pedal {
                frame: 1024,
                value: 100,
            },
            on(2048, MOVE_PEDAL_KEY),
            Played::Pedal {
                frame: 6144,
                value: 90,
            },
        ],
    );
    let right = harness.play(8192).right();
    assert_eq!(right[1024], 100.0 / 127.0);
    assert_eq!(right[8191], 90.0 / 127.0);
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(
        count(log.path(), "unmapped_pedal[0]"),
        1,
        "{:?}",
        lifecycle(log.path())
    );
    assert_eq!(count(log.path(), "unmapped_pedal[90]"), 0);
}

#[test]
fn a_plugin_that_moves_its_pedal_to_nothing_says_so_once() {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![on(0, 60), on(512, DROP_PEDAL_KEY)],
    );
    harness.project.engine().play();
    let mut reported = Vec::new();
    for _ in 0..4 {
        harness.render_without_polling(512);
        reported.extend(harness.plugins.poll(&harness.project));
    }
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert!(
        reported[0]
            .to_string()
            .contains("offers the host no way to send the sustain pedal"),
        "{reported:?}"
    );
}

/// From inside `setComponentState`. The host reads everything after that anyway; acting on it
/// would set the plugin up again, which asks again, for ever.
#[test]
fn a_plugin_that_asks_for_a_reload_while_it_loads_is_loaded_once() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    tell_the_plugin_to_ask_for_a_reload_as_it_loads();
    let mut harness = Harness::new();
    // A saved state, so the host gives it to the plugin's controller as it loads.
    harness.write_offset(PluginFormat::Vst3, "piano", 0);
    harness.add_track(record(PluginFormat::Vst3, "piano"), vec![on(0, 60)]);
    harness.play(1536);
    assert!(
        count(log.path(), "reload_asked") >= 1,
        "the plugin never asked"
    );
    assert_eq!(harness.plugins.take_retries(), []);
    assert_eq!(count(log.path(), "initialize"), 1);
    assert_eq!(count(log.path(), "activate"), 1);
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// `kParamIDMappingChanged`, then a preset that shows the new `Level` at a half.
#[test]
fn a_parameter_the_plugin_lists_later_follows_its_preset() {
    tell_the_plugin(None, None);
    tell_the_plugin_to_list_its_level_late();
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![on(0, 60), on(1024, LIST_LEVEL_KEY), on(3072, PRESET_KEY)],
    );
    let left = harness.play(8192).left();
    let before = peak(&left[..1024]);
    let after = peak(&left[5120..]);
    assert!(before > 0.0);
    assert!(
        (after - before / 2.0).abs() < before / 100.0,
        "the plugin plays at {after} of {before} after its preset, which is not the half the controller shows"
    );
}
