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
    FORMATS, Harness, Played, id, peak, record, state_asset, tell_the_plugin, vst3_state,
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
        let isolation = Isolation::new(false)
            .with_timeouts(Duration::from_millis(300), Duration::from_secs(60));
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
        let isolation = Isolation::new(false)
            .with_timeouts(Duration::from_secs(10), Duration::from_millis(500));
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
        tell_the_plugin(None, None);
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
        let before = Instant::now();
        harness.project.rebind(&id).expect("the record runs again");
        assert!(
            before.elapsed() < Duration::from_millis(100),
            "{format:?}: no second load"
        );
        assert_eq!(instrument_problems(&harness).len(), 1, "{format:?}");
    }
}
