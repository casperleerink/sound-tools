//! Which thread each call of a plugin's life arrives on, and in which order.
//!
//! Both formats put starting and stopping the processing on the audio thread, and activating
//! and deactivating on the main thread while nothing is processing. A strict plugin asserts
//! exactly this, so the engine here runs on a thread of its own.

use std::sync::mpsc;

use plugin_host::PluginFormat;
use sound_core::Changes;

use crate::support::{
    FORMATS, Harness, LoggedCall, Played, id, lifecycle, plugin_folder_of, record, state_asset,
    tell_the_plugin,
};

/// The calls both formats write down.
const LIFECYCLE: [&str; 5] = [
    "activate",
    "start_processing",
    "process",
    "stop_processing",
    "deactivate",
];

fn played() -> Vec<Played> {
    (0..64)
        .flat_map(|index| {
            let frame = index * 512;
            [
                Played::On {
                    frame,
                    pitch: 60,
                    velocity: 100,
                },
                Played::Off {
                    frame: frame + 200,
                    pitch: 60,
                },
            ]
        })
        .collect()
}

struct Life {
    calls: Vec<LoggedCall>,
}

impl Life {
    /// The calls about the audio processors. Plugin 0 is the plugin itself, such as its window.
    fn names(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter(|call| call.plugin != 0)
            .map(|call| format!("{}({})", call.call, call.plugin))
            .collect()
    }

    fn lifecycle_names(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter(|call| call.plugin != 0 && LIFECYCLE.contains(&call.call.as_str()))
            .map(|call| format!("{}({})", call.call, call.plugin))
            .collect()
    }

    fn of(&self, name: &str, plugin: u64) -> &LoggedCall {
        let found = self
            .calls
            .iter()
            .find(|call| call.call == name && call.plugin == plugin);
        found.unwrap_or_else(|| panic!("no {name} of plugin {plugin} in {:?}", self.names()))
    }

    fn position(&self, name: &str, plugin: u64) -> usize {
        let found = self
            .calls
            .iter()
            .position(|call| call.call == name && call.plugin == plugin);
        found.unwrap_or_else(|| panic!("no {name} of plugin {plugin} in {:?}", self.names()))
    }

    /// The library counts them for the whole process, so a test reads its own by position.
    fn plugins(&self) -> Vec<u64> {
        let mut plugins: Vec<u64> = self
            .calls
            .iter()
            .map(|call| call.plugin)
            .filter(|plugin| *plugin != 0)
            .collect();
        plugins.sort_unstable();
        plugins.dedup();
        plugins
    }
}

struct Audio {
    work: mpsc::Sender<usize>,
    done: mpsc::Receiver<()>,
}

impl Audio {
    fn render(&self, blocks: usize) {
        self.work.send(blocks).expect("the audio thread takes work");
        self.done.recv().expect("the audio thread answers");
    }
}

/// Runs `steps` with a real audio thread beside it. The project and the engine go before the
/// log is read, so the log holds the whole life of every plugin.
fn run(format: PluginFormat, steps: impl FnOnce(&mut Parts<'_>, &Audio)) -> Life {
    run_with(format, false, played(), steps)
}

/// The same with a second plugin as the rack's effect.
fn run_with_an_effect(format: PluginFormat, steps: impl FnOnce(&mut Parts<'_>, &Audio)) -> Life {
    run_with(format, true, played(), steps)
}

fn run_with(
    format: PluginFormat,
    with_effect: bool,
    played: Vec<Played>,
    steps: impl FnOnce(&mut Parts<'_>, &Audio),
) -> Life {
    let folder = tempfile::tempdir().expect("a temporary folder");
    // Outside the project folder, which goes with the project.
    let log_folder = tempfile::tempdir().expect("a temporary folder");
    let log = log_folder.path().join("lifecycle.log");
    tell_the_plugin(Some(&log), None);
    let search = vec![plugin_folder_of(folder.path(), format)];
    let mut harness = Harness::open_in_process_with_paths(folder, search);
    harness.add_track(record(format, "piano"), played);
    if with_effect {
        harness.add_effect(record(format, "trim"));
    }
    harness.project.engine().play();

    {
        let Harness {
            project,
            engine,
            plugins,
            ..
        } = &mut harness;
        let (work, work_in) = mpsc::channel::<usize>();
        let (done_out, done) = mpsc::channel::<()>();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut buffer = vec![0.0_f32; 512 * 2];
                while let Ok(blocks) = work_in.recv() {
                    for _ in 0..blocks {
                        engine.process_block(&mut buffer);
                    }
                    if done_out.send(()).is_err() {
                        break;
                    }
                }
            });
            let audio = Audio { work, done };
            audio.render(2);
            project.engine().poll().expect("the engine polls");
            plugins.poll(project);
            let mut parts = Parts { project, plugins };
            steps(&mut parts, &audio);
            audio.render(2);
            plugins.poll(project);
        });
    }

    drop(harness);
    tell_the_plugin(None, None);
    Life {
        calls: lifecycle(&log),
    }
}

struct Parts<'a> {
    project: &'a mut sound_core::Project,
    plugins: &'a mut plugin_host::Plugins,
}

#[test]
fn a_swap_stops_the_plugin_that_goes_on_the_audio_thread_before_the_new_one_plays() {
    for format in FORMATS {
        a_swap_stops_the_one_that_goes(format);
    }
}

fn a_swap_stops_the_one_that_goes(format: PluginFormat) {
    let life = run(format, |harness, audio| {
        let mut changes = Changes::new();
        changes.create(id("track/instrument"), record(format, "other"));
        harness
            .project
            .commit("Another state asset", changes)
            .expect("the change applies");
        audio.render(2);
        harness.plugins.poll(harness.project);
    });

    let plugins = life.plugins();
    assert_eq!(plugins.len(), 2, "{:?}", life.names());
    let (first, second) = (plugins[0], plugins[1]);

    let audio_thread = &life.of("process", first).thread;
    assert_eq!(
        &life.of("stop_processing", first).thread,
        audio_thread,
        "{:?}",
        life.names()
    );
    assert_eq!(
        life.of("deactivate", first).processed,
        life.of("stop_processing", first).processed,
        "{:?}",
        life.names()
    );
    assert!(
        life.position("stop_processing", first) < life.position("start_processing", second),
        "{:?}",
        life.names()
    );
    assert!(
        life.position("stop_processing", first) < life.position("deactivate", first),
        "{:?}",
        life.names()
    );
    assert_ne!(
        &life.of("deactivate", first).thread,
        audio_thread,
        "deactivate arrived on the audio thread: {:?}",
        life.names()
    );
}

#[test]
fn a_delete_stops_the_plugin_on_the_audio_thread_and_deactivates_it_on_the_main_thread() {
    for format in FORMATS {
        a_delete_stops_it_on_the_audio_thread(format);
    }
}

fn a_delete_stops_it_on_the_audio_thread(format: PluginFormat) {
    let life = run(format, |harness, audio| {
        let mut changes = Changes::new();
        changes.delete(&id("track/instrument"));
        harness
            .project
            .commit("Delete the instrument", changes)
            .expect("the delete applies");
        audio.render(2);
        harness.plugins.poll(harness.project);
    });

    let plugin = life.plugins()[0];
    let names = life.lifecycle_names();
    assert_eq!(
        names,
        [
            format!("activate({plugin})"),
            format!("start_processing({plugin})"),
            format!("process({plugin})"),
            format!("stop_processing({plugin})"),
            format!("deactivate({plugin})"),
        ],
        "{format:?} {:?}",
        life.names()
    );
    let audio_thread = &life.of("process", plugin).thread;
    assert_eq!(&life.of("stop_processing", plugin).thread, audio_thread);
    assert_ne!(&life.of("deactivate", plugin).thread, audio_thread);
    assert_eq!(
        life.of("deactivate", plugin).processed,
        life.of("stop_processing", plugin).processed,
        "the plugin processed after it was stopped: {names:?}"
    );
}

/// The engine is gone by then, so the wrapper stops the plugin as it is dropped.
#[test]
fn closing_the_project_stops_every_plugin_before_it_is_deactivated() {
    for format in FORMATS {
        closing_stops_before_deactivating(format);
    }
}

fn closing_stops_before_deactivating(format: PluginFormat) {
    let life = run(format, |harness, audio| {
        audio.render(2);
        harness.plugins.close(harness.project);
    });

    let plugin = life.plugins()[0];
    let names = life.names();
    assert!(
        life.position("stop_processing", plugin) < life.position("deactivate", plugin),
        "{names:?}"
    );
    assert_eq!(
        life.of("deactivate", plugin).processed,
        life.of("stop_processing", plugin).processed,
        "the plugin processed after it was stopped: {names:?}"
    );
}

/// VST 3 asks a host to terminate a plugin it is done with: a plugin that is only released holds
/// the host objects it was given and may reach for them as it goes.
#[test]
fn a_load_that_fails_after_the_plugin_started_still_terminates_it() {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let log_folder = tempfile::tempdir().expect("a temporary folder");
    let log = log_folder.path().join("lifecycle.log");
    tell_the_plugin(Some(&log), None);
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Vst3)];
    let mut harness = Harness::open_in_process_with_paths(folder, search);

    // A state file the plugin refuses, so the load fails after `initialize`.
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::create_dir_all(asset.parent().expect("the folder")).expect("the folder");
    let vst3_state = b"SVT3\x08\x00\x00\x00not-mine\x00\x00\x00\x00";
    std::fs::write(&asset, vst3_state).expect("the state file");
    harness.add_track(record(PluginFormat::Vst3, "piano"), played());

    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("could not be read"), "{problems:?}");

    drop(harness);
    tell_the_plugin(None, None);
    let life = Life {
        calls: lifecycle(&log),
    };
    // `setActive(0)` on a plugin that is not active is harmless.
    let names: Vec<String> = life.calls.iter().map(|call| call.call.clone()).collect();
    assert_eq!(
        names,
        ["initialize", "deactivate", "terminate"],
        "{names:?}"
    );
}

#[test]
fn deleting_an_effect_stops_its_plugin_on_the_audio_thread_and_leaves_the_instrument_playing() {
    for format in FORMATS {
        deleting_an_effect_keeps_the_thread_rules(format);
    }
}

fn deleting_an_effect_keeps_the_thread_rules(format: PluginFormat) {
    let life = run_with_an_effect(format, |harness, audio| {
        let mut changes = Changes::new();
        changes.delete(&id("track/effect"));
        harness
            .project
            .commit("Delete the effect", changes)
            .expect("the delete applies");
        audio.render(2);
        harness.plugins.poll(harness.project);
    });

    let plugins = life.plugins();
    assert_eq!(plugins.len(), 2, "{:?}", life.names());
    let (instrument, effect) = (plugins[0], plugins[1]);
    let names = life.names();

    let audio_thread = &life.of("process", effect).thread;
    assert_eq!(
        &life.of("stop_processing", effect).thread,
        audio_thread,
        "{names:?}"
    );
    assert_ne!(
        &life.of("deactivate", effect).thread,
        audio_thread,
        "{names:?}"
    );
    assert!(
        life.position("stop_processing", effect) < life.position("deactivate", effect),
        "{names:?}"
    );
    assert_eq!(
        life.of("deactivate", effect).processed,
        life.of("stop_processing", effect).processed,
        "the effect processed after it was stopped: {names:?}"
    );
    // The instrument is stopped by the project closing, not by the delete.
    assert!(
        life.position("stop_processing", instrument) > life.position("stop_processing", effect),
        "the instrument stopped before the effect: {names:?}"
    );
    assert!(
        life.of("stop_processing", instrument).processed
            > life.of("stop_processing", effect).processed,
        "the instrument played no block after the effect went: {names:?}"
    );
}

#[test]
fn a_latency_change_starts_the_plugin_again_by_the_thread_rules() {
    for format in FORMATS {
        a_latency_change_keeps_the_thread_rules(format);
    }
}

fn a_latency_change_keeps_the_thread_rules(format: PluginFormat) {
    let mut played = played();
    played.push(Played::On {
        frame: 1_000,
        pitch: test_plugin_support::LATENCY_KEY,
        velocity: 40,
    });
    played.sort_by_key(|played| played.frame());
    let life = run_with(format, false, played, |harness, audio| {
        for _ in 0..4 {
            audio.render(2);
            harness.project.engine().poll().expect("the engine polls");
            let problems = harness.plugins.poll(harness.project);
            assert_eq!(problems, [], "{format:?}");
            let problems = harness.plugins.send_restarts(harness.project);
            assert_eq!(problems, [], "{format:?}");
        }
    });

    let first = life.plugins()[0];
    let audio_thread = life.of("process", first).thread.clone();
    let asked = life.position("latency_asked", first);
    let after: Vec<&crate::support::LoggedCall> = life.calls[asked..]
        .iter()
        .filter(|call| LIFECYCLE.contains(&call.call.as_str()))
        .collect();
    let names: Vec<&str> = after.iter().map(|call| call.call.as_str()).collect();
    // Then the project closing stops and deactivates it once more.
    assert_eq!(
        names[..4],
        [
            "stop_processing",
            "deactivate",
            "activate",
            "start_processing"
        ],
        "{format:?} {:?}",
        life.names()
    );
    assert_eq!(after[0].thread, audio_thread, "{format:?}");
    assert_ne!(after[1].thread, audio_thread, "{format:?}");
    assert_ne!(after[2].thread, audio_thread, "{format:?}");
    assert_eq!(after[3].thread, audio_thread, "{format:?}");
    assert_eq!(after[1].processed, after[0].processed, "{format:?}");
}
