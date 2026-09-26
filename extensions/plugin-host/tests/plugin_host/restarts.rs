//! What a VST 3 plugin asks of its host with `restartComponent`, and what this host does about
//! it. ARCHITECTURE.md has the table of every flag; these are the flags that need the host to
//! do something, each made to happen by the repository's own VST 3 plugin. `kLatencyChanged`
//! is in `lifecycle.rs` and in the latency tests of the runtime.
//!
//! The plugin is told by a note on a key it does not play (`PRESET_KEY` and the keys after it
//! in `test_plugin_support`), as it is told to change its latency: its processor reports the
//! key and its controller acts on the main thread, where `restartComponent` belongs.

use plugin_host::PluginFormat;
use test_plugin_support::{
    DROP_PEDAL_KEY, LATENCY_KEY, LIST_LEVEL_KEY, MONO_KEY, MOVE_PEDAL_KEY, PRESET_KEY, RELOAD_KEY,
};

use crate::support::{
    Harness, Played, id, lifecycle, peak, record, saved_transpose, state_asset, tell_the_plugin,
    tell_the_plugin_to_ask_for_a_reload_as_it_loads, tell_the_plugin_to_edit_as_it_restarts,
    tell_the_plugin_to_list_its_level_late,
};

/// How many times the plugin's log has `call`.
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

/// `kParamValuesChanged`, "The host invalidates all caches of parameter values and asks the
/// edit controller for the current values." The plugin's controller loads a preset of its own
/// and shows `Level` at a half; only its processor plays the level, and the processor was told
/// nothing. The host compares what it last gave the processor with what the controller shows
/// and sends the difference, so the plugin plays at half its level from then on.
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

/// `kIoChanged`, "The host has to deactivate the plug-in, asks the plug-in for its wanted new
/// bus configurations, adapts its processing graph and reactivate the plug-in." The plugin's
/// main output becomes one channel. Its right channel was the pedal, silent here; once the host
/// has read the buses again, the one channel is heard on both sides.
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
    // Stereo at first: the tone on the left, the pedal, which is up, on the right.
    assert!(peak(&left[..2048]) > 0.0);
    assert_eq!(peak(&right[..2048]), 0.0);
    // Mono afterwards, on both sides.
    assert!(peak(&left[7168..]) > 0.0);
    assert_eq!(left[7168..], right[7168..]);
    assert_eq!(harness.problems(), Vec::<String>::new());

    // And it was the restart the header asks for: deactivated and activated again, after the
    // plugin said its output changed.
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

/// `kReloadComponent`, "The host has to unload completely the plug-in (controller/processor)
/// and reload it." The host gives the record to whoever polls to be run again, which is what a
/// record that changed gets: the plugin is saved on its way out and a new one loads from that
/// state. So the plugin comes back as it sounded.
#[test]
fn a_plugin_that_asks_to_be_loaded_again_comes_back_as_it_sounded() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            on(0, 60),
            // Down to 100 transposes the plugin by 36 semitones, a change of its own state.
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

    // A new plugin, and the old one let go of.
    let calls = lifecycle(log.path());
    let first = calls.iter().find(|call| call.call == "initialize").unwrap();
    let initialized = calls.iter().filter(|call| call.call == "initialize");
    assert_eq!(initialized.count(), 2, "{calls:?}");
    let ended = calls.iter().filter(|call| call.call == "terminate");
    assert!(
        ended.clone().any(|call| call.plugin == first.plugin),
        "{calls:?}"
    );

    // What the old one changed of its own state came back with the new one, and the new one
    // saves it again.
    harness.plugins.close(&harness.project);
    let bytes = harness
        .project
        .assets()
        .read(&state_asset("piano"))
        .unwrap()
        .unwrap();
    assert_eq!(saved_transpose(PluginFormat::Vst3, &bytes), 36);
}

/// A parameter the composer moves in the plugin's window while the plugin is started again is
/// not lost. The plugin asks for a new latency and its controller edits `Level` to a quarter in
/// the same moment, so the edit is on its way to an audio side that the engine is giving back.
/// That side hands what it never played back to the host, and the next one plays it.
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

/// `kMidiCCAssignmentChanged`, "The host has to rebuild the MIDI-CC => parameter mapping." The
/// plugin moves its sustain pedal to another parameter while the pedal is down, as a MIDI learn
/// or a loaded preset does. The host looks the mapping up again, so the next pedal move reaches
/// the plugin where it listens now, and it lets go of the old parameter, which would otherwise
/// stay down for good. Nothing is reported: nothing is wrong.
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
    // The plugin writes the pedal it hears into its right channel.
    assert_eq!(right[1024], 100.0 / 127.0);
    assert_eq!(right[8191], 90.0 / 127.0);
    assert_eq!(harness.problems(), Vec::<String>::new());
    // The old parameter was let go of, and the new pedal did not go there.
    assert_eq!(
        count(log.path(), "unmapped_pedal[0]"),
        1,
        "{:?}",
        lifecycle(log.path())
    );
    assert_eq!(count(log.path(), "unmapped_pedal[90]"), 0);
}

/// The same, when the plugin moves its pedal to no parameter at all. The pedal cannot reach it
/// any more, and the composer is told once, with the line a plugin gets that never mapped one.
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

/// A plugin that asks to be loaded again and started again while the host sets it up, from
/// inside `setComponentState`. The host reads everything after that anyway, so it does neither:
/// acting on it would set the plugin up again, which asks again, for ever.
#[test]
fn a_plugin_that_asks_for_a_reload_while_it_loads_is_loaded_once() {
    let log = tempfile::NamedTempFile::new().unwrap();
    tell_the_plugin(Some(log.path()), None);
    tell_the_plugin_to_ask_for_a_reload_as_it_loads();
    let mut harness = Harness::new();
    // A saved state, so that the host gives it to the plugin's controller as it loads.
    harness.write_offset(PluginFormat::Vst3, "piano", 0);
    harness.add_track(record(PluginFormat::Vst3, "piano"), vec![on(0, 60)]);
    // Three buffers, each with the poll of a live session.
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

/// `kParamIDMappingChanged`: the plugin has other parameters now. It lists `Level` only once a
/// key asks, and then loads a preset of its own that shows `Level` at a half. The host listed
/// the parameters again when it was told, so it compares `Level` too and the processor plays at
/// half its level.
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
