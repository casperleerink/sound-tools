//! Which thread each call of a plugin's life arrives on, and in which order.
//!
//! CLAP puts `start_processing` and `stop_processing` on the audio thread, and `activate` and
//! `deactivate` on the main thread while nothing is processing. The engine of these tests is
//! driven from a thread of its own here, so the log the test plugin writes says which calls
//! really arrived on the thread that processes. A strict plugin asserts exactly this.

use std::sync::mpsc;

use sound_core::Changes;

use crate::support::{Harness, LoggedCall, Played, id, lifecycle, record, tell_the_plugin};

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

/// Everything the test plugin wrote down, with the thread that processed it.
struct Life {
    calls: Vec<LoggedCall>,
}

impl Life {
    /// The calls about the audio processors, in order. The calls about the plugin itself,
    /// written down with plugin 0, are not part of what this file is about; `window.rs` reads
    /// those.
    fn names(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter(|call| call.plugin != 0)
            .map(|call| format!("{}({})", call.call, call.plugin))
            .collect()
    }

    /// The one call `name` of plugin `plugin`.
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

    /// Which audio processors appear. The library counts them for the whole process, so a test
    /// reads its own by position and not by a fixed number. Number 0 is not one: it is what a
    /// call about the plugin itself, such as one of the window, is written down with.
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

/// The audio thread of one test: it owns the engine and renders when it is asked to.
struct Audio {
    work: mpsc::Sender<usize>,
    done: mpsc::Receiver<()>,
}

impl Audio {
    /// Renders `blocks` blocks of 512 frames and waits for them, so the test knows what the
    /// plugin has been through before it looks.
    fn render(&self, blocks: usize) {
        self.work.send(blocks).expect("the audio thread takes work");
        self.done.recv().expect("the audio thread answers");
    }
}

/// Opens a project with one plugin, runs `steps` with a real audio thread beside it, and gives
/// back what the plugin wrote down. The project and the engine both go before the log is read,
/// so the log holds the whole life of every plugin.
fn run(steps: impl FnOnce(&mut Parts<'_>, &Audio)) -> Life {
    let folder = tempfile::tempdir().expect("a temporary folder");
    // Outside the project folder, which goes with the project at the end of this.
    let log_folder = tempfile::tempdir().expect("a temporary folder");
    let log = log_folder.path().join("lifecycle.log");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::open(folder, true);
    harness.add_track(record("piano"), played());
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
            // Whatever is waiting for the engine to give a processor back.
            audio.render(2);
            plugins.poll(project);
        });
    }

    // The engine goes first, as the runtime does when a stream closes, and then the project.
    drop(harness);
    tell_the_plugin(None, None);
    Life {
        calls: lifecycle(&log),
    }
}

/// What a step may touch while the audio thread has the engine.
struct Parts<'a> {
    project: &'a mut sound_core::Project,
    plugins: &'a mut plugin_host::Plugins,
}

/// Swapping the plugin of a record. The one that goes is stopped on the thread that processed
/// it, before the new one plays a block, and only then deactivated on the main thread.
#[test]
fn a_swap_stops_the_plugin_that_goes_on_the_audio_thread_before_the_new_one_plays() {
    let life = run(|harness, audio| {
        let mut changes = Changes::new();
        changes.create(id("track/instrument"), record("other"));
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

    // Stopped on the thread that processed it.
    let audio_thread = &life.of("process", first).thread;
    assert_eq!(
        &life.of("stop_processing", first).thread,
        audio_thread,
        "{:?}",
        life.names()
    );
    // And never processed after that.
    assert_eq!(
        life.of("deactivate", first).processed,
        life.of("stop_processing", first).processed,
        "{:?}",
        life.names()
    );
    // Before the new plugin played, and before the main thread deactivated the old one.
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

/// Deleting the record. The engine takes the processor out of its slot and the plugin is
/// stopped there, on the audio thread, before the main thread deactivates it.
#[test]
fn a_delete_stops_the_plugin_on_the_audio_thread_and_deactivates_it_on_the_main_thread() {
    let life = run(|harness, audio| {
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
    let names = life.names();
    assert_eq!(
        names,
        [
            format!("activate({plugin})"),
            format!("start_processing({plugin})"),
            format!("process({plugin})"),
            format!("stop_processing({plugin})"),
            format!("deactivate({plugin})"),
        ],
        "{names:?}"
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

/// Closing the project. The engine is gone by then, so there is no audio thread left to stop a
/// plugin on: the wrapper stops it as it is dropped, which is still before anyone deactivates
/// it and after its last block.
#[test]
fn closing_the_project_stops_every_plugin_before_it_is_deactivated() {
    let life = run(|harness, audio| {
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
