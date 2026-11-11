//! A plugin that crashes or hangs, in a process of its own: it stops, its slot plays what an
//! empty one does, the problem says so, and the rest of the project plays on. Start again brings
//! it back as it was last saved.
//!
//! The track of these tests is the test plugin as its instrument and a second one as its
//! effect, which adds its saved offset to what reaches it. So after the instrument stops, the
//! offset alone shows the effect, another process, still playing.

use std::time::{Duration, Instant};

use plugin_host::{Isolation, PluginFormat};
use test_plugin_support::{CRASH_KEY, CRASH_STATE, HANG_KEY, HANG_SAVE_VARIABLE};

use crate::support::{
    FORMATS, Harness, Played, id, isolation, lifecycle, no_cache, peak, plugin_folder, record,
    state_asset, tell_the_plugin, vst3_state,
};

/// The offset of the effect, in hundredths, and as a sample.
const OFFSET: i32 = 25;
const OFFSET_SAMPLE: f32 = 0.25;

fn on(frame: u64, pitch: u8) -> Played {
    Played::On {
        frame,
        pitch,
        velocity: 100,
    }
}

/// A track that plays a note, then `breaking` at frame 4096, then another note at 12288.
fn track(harness: &mut Harness, format: PluginFormat, breaking: u8) {
    harness.write_offset(format, "effect", OFFSET);
    let played = vec![on(0, 60), on(4096, breaking), on(12288, 64)];
    harness.add_track(record(format, "piano"), played);
    harness.add_effect(record(format, "effect"));
}

/// What the problems of the project say of the instrument.
fn instrument_problems(harness: &Harness) -> Vec<String> {
    let problems = harness.problems().into_iter();
    problems
        .filter(|problem| problem.starts_with("state/track/instrument"))
        .collect()
}

/// Plays a crashing or hanging instrument through its stop and back.
fn stops_and_comes_back(harness: &mut Harness, format: PluginFormat) {
    let playing = harness.play(4096).left();
    assert!(
        peak(&playing) > OFFSET_SAMPLE + 0.1,
        "{format:?}: the instrument plays"
    );

    // The block of the bad note: the instrument stops, the effect plays on with nothing in.
    let after = harness.render(4096).left();
    harness.rebind_retries();
    let tail = &after[2048..];
    assert!(
        tail.iter()
            .all(|sample| (sample - OFFSET_SAMPLE).abs() < 1e-6),
        "{format:?}: only the effect plays once the instrument stopped, {}",
        peak(tail)
    );
    let problems = instrument_problems(harness);
    assert_eq!(
        problems.len(),
        1,
        "{format:?}: {problems:?} of {:?}",
        harness.problems()
    );
    assert!(problems[0].contains("stopped working"), "{problems:?}");
    let id = id("track/instrument");
    assert!(harness.plugins.has_stopped(&id), "{format:?}");

    // It stays stopped: another run of its behaviour does not load it again by itself.
    harness.project.rebind(&id).expect("the record runs again");
    harness.render(1024);
    assert!(harness.plugins.has_stopped(&id), "{format:?}");

    // Start again: the next note sounds through both.
    harness.plugins.start_again(&id);
    harness.rebind_retries();
    assert_eq!(
        instrument_problems(harness),
        Vec::<String>::new(),
        "{format:?}"
    );
    let back = harness.render(8192).left();
    assert!(
        peak(&back) > OFFSET_SAMPLE + 0.1,
        "{format:?}: it plays again"
    );
}

/// A plugin that crashes in its block costs its own slot. The process of the app lives, the
/// effect after it plays on, and Start again brings it back.
#[test]
fn a_plugin_that_crashes_as_it_plays_stops_and_the_rest_plays_on() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let mut harness = Harness::new();
        track(&mut harness, format, CRASH_KEY);
        stops_and_comes_back(&mut harness, format);
    }
}

/// A plugin that hangs in its block stops after the block timeout, as one that crashed.
#[test]
fn a_plugin_that_hangs_as_it_plays_stops_after_the_block_timeout() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        let isolation = Isolation::new(false).with_timeouts(
            Duration::from_millis(300),
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut harness = Harness::isolated_with(isolation);
        track(&mut harness, format, HANG_KEY);
        let started = Instant::now();
        stops_and_comes_back(&mut harness, format);
        assert!(started.elapsed() < Duration::from_secs(20), "{format:?}");
    }
}

/// A plugin whose main thread stops is hung once it has not turned for the time a host gives
/// it: the call that waited returns, and the plugin is stopped like one that crashed.
#[test]
fn a_plugin_whose_main_thread_hangs_is_stopped_after_its_time() {
    for format in FORMATS {
        tell_the_plugin(None, None);
        // SAFETY: as in `tell_the_plugin`.
        unsafe { std::env::set_var(HANG_SAVE_VARIABLE, "1") };
        let isolation = Isolation::new(false).with_timeouts(
            Duration::from_secs(10),
            Duration::from_millis(500),
            Duration::from_millis(500),
        );
        let mut harness = Harness::isolated_with(isolation);
        // A pedal that moves the plugin's state makes the host ask for it, and the plugin
        // hangs in its answer.
        let played = vec![
            on(0, 60),
            Played::Pedal {
                frame: 1024,
                value: 100,
            },
        ];
        harness.add_track(record(format, "piano"), played);
        let started = Instant::now();
        harness.play(4096);
        harness.rebind_retries();
        assert!(
            harness.plugins.has_stopped(&id("track/instrument")),
            "{format:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10), "{format:?}");
        // SAFETY: as above.
        unsafe { std::env::remove_var(HANG_SAVE_VARIABLE) };
    }
}

/// A plugin that crashes while it loads is one problem, and is not loaded again on every run of
/// its behaviour: only Start again tries.
#[test]
fn a_plugin_that_crashes_as_it_loads_is_one_problem_until_it_is_started_again() {
    for format in FORMATS {
        let log = tempfile::NamedTempFile::new().expect("a log");
        tell_the_plugin(Some(log.path()), None);
        let loads = || {
            let calls = lifecycle(log.path()).into_iter();
            calls
                .filter(|logged| logged.call == "crash_as_it_loads")
                .count()
        };
        let mut harness = Harness::new();
        let bytes = match format {
            PluginFormat::Clap => CRASH_STATE.to_vec(),
            PluginFormat::Vst3 => vst3_state(CRASH_STATE, b""),
        };
        let path = harness.project.assets().path(&state_asset("piano"));
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the folder");
        std::fs::write(path, bytes).expect("the state asset");
        harness.add_track(record(format, "piano"), vec![on(0, 60)]);
        let silent = harness.play(1024).left();
        assert_eq!(peak(&silent), 0.0, "{format:?}");
        let id = id("track/instrument");
        assert!(harness.plugins.has_stopped(&id), "{format:?}");
        let problems = instrument_problems(&harness);
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(problems[0].contains("stopped working"), "{problems:?}");
        assert_eq!(loads(), 1, "{format:?}");
        harness.project.rebind(&id).expect("the record runs again");
        assert_eq!(loads(), 1, "{format:?}: no second load by itself");
        assert_eq!(instrument_problems(&harness).len(), 1, "{format:?}");
        harness.plugins.start_again(&id);
        harness.rebind_retries();
        assert_eq!(loads(), 2, "{format:?}: Start again tries once more");
    }
}

/// The program the plugins play in was replaced while the host ran, as an update does: a plugin
/// that loads after that is a problem that asks for a restart, not a crash, and starts nothing.
#[test]
fn a_plugin_after_the_program_was_replaced_asks_for_a_restart() {
    tell_the_plugin(None, None);
    let programs = tempfile::tempdir().expect("a folder");
    let program = programs.path().join("plugin-scan");
    std::fs::copy(env!("CARGO_BIN_EXE_plugin-scan"), &program).expect("a copy of the program");
    let folder = tempfile::tempdir().expect("a temporary folder");
    let search = vec![plugin_folder(folder.path())];
    let scanner = plugin_host::ScanCommand::new(&program, []);
    let plugins = plugin_host::Plugins::isolated(search, scanner, no_cache(), isolation());
    let mut harness = Harness::with_plugins(folder, plugins);
    harness.add_track(record(PluginFormat::Clap, "piano"), vec![on(0, 60)]);
    assert!(peak(&harness.play(1024).left()) > 0.1, "it plays");

    let later = std::time::SystemTime::now() + Duration::from_secs(60);
    // Read only: Linux refuses to open a program that runs for writing.
    let file = std::fs::File::open(&program).expect("the program");
    file.set_modified(later).expect("a new time");
    harness.add_effect(record(PluginFormat::Clap, "effect"));
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("restarted"), "{problems:?}");
    assert!(!harness.plugins.has_stopped(&id("track/effect")));
}
