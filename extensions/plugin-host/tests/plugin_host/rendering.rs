//! What the host tells a plugin about the kind of run it is in: a device playing in real time,
//! or a render, where a plugin that streams its samples from disk may wait for them.

use plugin_host::PluginFormat;

use crate::support::{FORMATS, Harness, Played, lifecycle, record, tell_the_plugin};

fn played() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }]
}

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

/// Without it a sampled instrument renders the silence of samples it never waited for.
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
        // VST 3 says it in the setup and in every block, which must agree.
        if format == PluginFormat::Vst3 {
            assert_eq!(block_modes(&log), ["process_mode[offline]"], "{format:?}");
        }
    }
}

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
