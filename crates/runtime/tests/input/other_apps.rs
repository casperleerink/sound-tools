//! `project.json` hears another app: it is tapped while a connection starts at it.

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use runtime::input::{AppSounds, Devices, OtherApps};
use sound_core::{AppSound, Changes, LiveWriter, SavedConnection, SavedDestination, SavedSource};

use crate::support::{Closes, Opened, open, write};

/// The sound of one app that runs or not, with no tap: two channels at 48 kHz, into which the
/// test writes what the app would play.
#[derive(Clone, Default)]
struct SimulatedApp {
    /// Its processes; none while it is not running.
    processes: Arc<Mutex<Vec<u32>>>,
    live: Arc<Mutex<Option<LiveWriter>>>,
    /// For each tap that closed, the thread it closed on.
    closed: Arc<Mutex<Vec<ThreadId>>>,
}

impl SimulatedApp {
    fn sounds(&self) -> AppSounds {
        let (app, processes) = (self.clone(), self.processes.clone());
        AppSounds {
            open: Arc::new(move |name| {
                if app.processes.lock().unwrap().is_empty() {
                    return Err(sound_core::DeviceError::NoApp(String::from(name.clone())));
                }
                let (writer, live) = sound_core::live_input(48_000, 2);
                *app.live.lock().unwrap() = Some(writer);
                Ok((Box::new(Closes(app.closed.clone())), live))
            }),
            processes: Arc::new(move |_| processes.lock().unwrap().clone()),
        }
    }

    fn set_processes(&self, processes: Vec<u32>) {
        *self.processes.lock().unwrap() = processes;
    }

    /// Plays one stereo frame, `frames` times.
    fn play(&self, frame: [f32; 2], frames: usize) {
        if let Some(writer) = self.live.lock().unwrap().as_mut() {
            writer.write(&frame.repeat(frames));
        }
    }

    /// Whether the engine has the live input of its tap.
    fn is_heard(&self) -> bool {
        (self.live.lock().unwrap().as_ref()).is_some_and(|writer| !writer.is_abandoned())
    }

    fn closed_on(&self) -> Vec<ThreadId> {
        self.closed.lock().unwrap().clone()
    }
}

fn hear_music() -> SavedConnection {
    SavedConnection {
        from: SavedSource::App(AppSound::Named("Music".to_string())),
        to: SavedDestination::DeviceOutput(0),
    }
}

/// An empty project that hears the simulated app.
fn open_with_app() -> (Opened, SimulatedApp) {
    let folder = tempfile::tempdir().unwrap();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []}"#;
    write(folder.path(), "project.json", project);
    write(folder.path(), "extensions/theme.json", "{}");
    let mut opened = open(folder, Devices::default());
    let app = SimulatedApp::default();
    opened.apps = Some(OtherApps::new(app.sounds(), Arc::new(|| {})));
    (opened, app)
}

impl Opened {
    fn connect(&mut self, connection: SavedConnection, on: bool) {
        let mut changes = Changes::new();
        match on {
            true => changes.connect(connection),
            false => changes.disconnect(connection),
        }
        self.project.commit("Hear Music", changes).unwrap();
    }

    /// The problems of `project.json`.
    fn problems(&self) -> Vec<String> {
        let problems = self.project.problems().into_iter();
        let connections = problems.filter(|problem| problem.path == "project.json");
        connections.map(|problem| problem.message).collect()
    }
}

/// An app that is not running is a problem until it starts, then the engine plays it. Without
/// the connection its tap closes.
#[test]
fn a_connection_from_an_app_hears_it_once_it_runs() {
    let (mut opened, music) = open_with_app();
    opened.connect(hear_music(), true);
    if cfg!(target_os = "macos") {
        let expected = r#"connections[0]: not heard, because "Music" is not running, or has played no sound yet"#;
        opened.until("the problem", |opened| {
            opened.problems() == [expected]
        });
    }

    // The processes of the app are looked at again every two seconds.
    music.set_processes(vec![71]);
    opened.until("the engine plays the app", |opened| {
        music.play([0.5, 0.25], 512);
        let output = opened.render(512);
        output[output.len() - 2..] == [0.5, 0.25]
    });
    if cfg!(target_os = "macos") {
        opened.until("no problem", |opened| opened.problems().is_empty());
    }

    opened.connect(hear_music(), false);
    opened.until("the tap closed", |opened| {
        opened.render(512);
        !music.is_heard()
    });
}

/// Core Audio can take a while to close a tap, so a tap never closes on the thread that polls:
/// not the one a new tap replaces when the processes of the app change, nor the last one when
/// no connection hears the app anymore.
#[test]
fn a_tap_closes_off_the_thread_that_polls() {
    let (mut opened, music) = open_with_app();
    music.set_processes(vec![71]);
    opened.connect(hear_music(), true);
    opened.until("the tap", |_| music.is_heard());

    // A helper starts to play its sound.
    music.set_processes(vec![71, 72]);
    opened.until("the tap again", |_| music.closed_on().len() == 1);
    assert!(music.is_heard());

    opened.connect(hear_music(), false);
    opened.until("the tap closed", |_| music.closed_on().len() == 2);
    let here = std::thread::current().id();
    assert!(music.closed_on().iter().all(|thread| *thread != here));
}
