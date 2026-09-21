//! What the host tells a plugin about the kind of run it is in: a device playing in real time,
//! or a render.
//!
//! Both formats have a way to say it and both mean the same thing by it: a plugin that streams
//! its samples from disk may wait for them when nothing is waiting for the block. VST 3 puts
//! the mode in `setupProcessing` and in every `ProcessData`; CLAP has the `render` extension,
//! which the host sets on the main thread before the plugin is activated.
//!
//! The test plugin of each format writes down what it was told, so a test reads exactly what
//! the host said.

use plugin_host::PluginFormat;

use crate::support::{FORMATS, Harness, Played, lifecycle, record, tell_the_plugin};

fn played() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }]
}

/// The window's engine plays on a device, and every plugin is told so.
#[test]
fn a_run_on_a_device_tells_every_plugin_it_is_playing_in_real_time() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        tell_the_plugin(Some(&log), None);
        let mut harness = Harness::new();
        harness.add_track(record(format, "piano"), played());
        harness.play(1024);

        let said = modes(&log);
        assert_eq!(said, ["mode[realtime]"], "{format:?}: {said:?}");
        if format == PluginFormat::Vst3 {
            assert_eq!(block_modes(&log), ["process_mode[realtime]"], "{format:?}");
        }
    }
}

/// `--render` opens the project with an engine that renders, and every plugin is told that
/// instead. Without it a sampled instrument renders the silence of samples it never waited for.
#[test]
fn a_render_tells_every_plugin_that_nothing_is_waiting_for_the_block() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        tell_the_plugin(Some(&log), None);
        let mut harness = Harness::rendering_offline();
        harness.add_track(record(format, "piano"), played());
        let render = harness.play(1024);
        assert!(render.first_sound().is_some(), "{format:?}: no sound");

        let said = modes(&log);
        assert_eq!(said, ["mode[offline]"], "{format:?}: {said:?}");
        // VST 3 says it twice: once in the setup, and then in every block, which must be the
        // mode of the setup it belongs to.
        if format == PluginFormat::Vst3 {
            assert_eq!(block_modes(&log), ["process_mode[offline]"], "{format:?}");
        }
    }
}

/// What the plugin was told about the kind of run, in order.
fn modes(log: &std::path::Path) -> Vec<String> {
    named(log, "mode[")
}

/// What every first block of a VST 3 plugin said it was.
fn block_modes(log: &std::path::Path) -> Vec<String> {
    named(log, "process_mode[")
}

fn named(log: &std::path::Path, prefix: &str) -> Vec<String> {
    lifecycle(log)
        .into_iter()
        .map(|call| call.call)
        .filter(|call| call.starts_with(prefix))
        .collect()
}
