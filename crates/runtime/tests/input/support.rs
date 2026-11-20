use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use midi::Played;
use plugin_host::{Plugins, ScanCache, ScanCommand};
use runtime::OFFLINE;
use runtime::input::{Devices, Happened, Input, OpenInput, OpenedInput, OtherApps, Waker};
use serde_json::Value;
use sound_core::{CaptureWriter, Engine, InstanceId, LiveWriter, Project};
use sound_notes::{Pitch, Velocity};
use sound_typescript::next::{Drawn, Tools};

/// How long a test waits for what it waits for, Bun and the threads, at most.
const PATIENCE: Duration = Duration::from_secs(30);

pub(crate) fn write(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

pub(crate) fn id(path: &str) -> InstanceId {
    InstanceId::new(path).unwrap()
}

/// A project open on the offline engine with its tools in Bun and its live input, polled as
/// the window polls it.
pub(crate) struct Opened {
    pub project: Project,
    pub engine: Engine,
    pub plugins: Plugins,
    pub tools: Tools,
    pub input: Input,
    /// The taps of other apps, when the test hears them.
    pub apps: Option<OtherApps>,
    pub selected: Option<InstanceId>,
    pub notices: Vec<String>,
    /// What threads woke the owner for.
    pub wakes: Arc<AtomicU64>,
    pub folder: tempfile::TempDir,
}

/// The project in `folder`, with `devices`. Its files are written already.
pub(crate) fn open(folder: tempfile::TempDir, devices: Devices) -> Opened {
    assert!(
        sound_typescript::has_bun(),
        "the tools of the project run in Bun: install it, see tooling/fetch-bun.sh"
    );
    let (control, engine) = Engine::new(OFFLINE);
    let scanner = ScanCommand::new(
        env!("CARGO_BIN_EXE_test-runtime"),
        [std::ffi::OsString::from(plugin_host::SCAN_ARGUMENT)],
    );
    let plugins = Plugins::new(Vec::new(), scanner, ScanCache::none());
    let (mut project, extensions) =
        runtime::open_with_extensions(folder.path(), control, plugins.clone()).unwrap();
    let wakes = Arc::new(AtomicU64::new(0));
    let waker: Waker = {
        let wakes = wakes.clone();
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::Relaxed);
        })
    };
    let tools = Tools::start(extensions.unwrap(), &project, false, waker.clone());
    let input = Input::new(&mut project, devices, waker);
    let mut opened = Opened {
        project,
        engine,
        plugins,
        tools,
        input,
        apps: None,
        selected: None,
        notices: Vec::new(),
        wakes,
        folder,
    };
    opened.settle();
    opened
}

impl Opened {
    /// One poll of the window: the project, Bun and the live input, and a step of the control
    /// loops.
    pub(crate) fn poll(&mut self) {
        runtime::tick(&mut self.project, &self.plugins);
        let events = self.project.drain_events();
        self.tools.poll(&mut self.project, &events);
        if let Some(apps) = &mut self.apps {
            apps.poll(&mut self.project, &events);
        }
        let happened = (self.input).poll(&mut self.project, &events, self.selected.as_ref());
        for happened in happened {
            match happened {
                Happened::Notice(notice) => self.notices.push(notice),
                Happened::Take(take) => {
                    let notice = self.tools.take(&mut self.project, take);
                    self.notices.extend(notice);
                }
            }
        }
        let status = self.project.engine().poll().unwrap();
        let time = status.frames as f64 / f64::from(OFFLINE.sample_rate);
        (self.tools).run_loops(&self.project, status.playing, status.playhead_tick, time, 0.016);
    }

    /// Polls, and waits until Bun did all it was told. The engine plays a little first, as a
    /// device plays it all the time.
    pub(crate) fn settle(&mut self) {
        self.render(64);
        self.poll();
        self.tools.settle(&mut self.project).unwrap();
        self.poll();
    }

    /// Settles until `done` holds. Bun and the threads of the input answer in real time.
    pub(crate) fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < PATIENCE {
            self.settle();
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "{what} did not come within {PATIENCE:?}; notices: {:?}; problems: {:?}",
            self.notices,
            self.project.problems()
        );
    }

    /// The engine plays `frames` frames; gives what it played, left and right.
    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        runtime::render(&mut self.project, &mut self.engine, &self.plugins, frames).unwrap()
    }

    /// The loudest sample of the next `frames` frames.
    pub(crate) fn loudest(&mut self, frames: usize) -> f32 {
        let output = self.render(frames);
        output.iter().fold(0.0, |loudest, sample| sample.abs().max(loudest))
    }

    pub(crate) fn start_playing(&mut self) {
        self.project.engine().play();
        self.render(64);
        self.settle();
    }

    /// The record of `instance`.
    pub(crate) fn record(&self, instance: &str) -> Value {
        let state = self.project.state_json(&id(instance));
        state.map_or(Value::Null, |state| serde_json::from_str(&state).unwrap())
    }

    /// Presses the element `key` of the view of `instance`, as a pointer does, and waits until
    /// Bun ran it.
    pub(crate) fn press(&mut self, instance: &str, key: &str) {
        let instance = id(instance);
        let shown = BTreeSet::from([instance.clone()]);
        self.tools.show(shown, BTreeMap::new(), BTreeMap::new());
        self.until("the view", |opened| {
            matches!(opened.tools.drawn(&instance), Some(Drawn::View { .. }))
        });
        self.tools.act(&instance, key, "press", None).unwrap();
        self.settle();
        assert_eq!(self.tools.take_failed_actions(), [] as [String; 0]);
    }

    /// A key of the MIDI keyboard goes down.
    pub(crate) fn play_midi(&mut self, pitch: u8) {
        self.send_midi(Played::On {
            pitch: Pitch::new(pitch).unwrap(),
            velocity: Velocity::new(100).unwrap(),
        });
    }

    pub(crate) fn send_midi(&mut self, played: Played) {
        self.settle();
        assert!(self.input.midi_input().unwrap().send(played));
    }
}

/// An audio input with no device, into which the test writes what it would capture.
#[derive(Clone, Default)]
pub(crate) struct SimulatedInput {
    writer: Arc<Mutex<Option<CaptureWriter>>>,
    /// What the engine plays live, written with the capture.
    live: Arc<Mutex<Option<LiveWriter>>>,
    /// Input frames written so far.
    written: Arc<AtomicU64>,
    /// Times it opened.
    pub openings: Arc<AtomicU32>,
    /// Whether opening it fails, as a device that is in use or not allowed.
    pub fails: Arc<AtomicBool>,
    /// For each time it closed, the thread it closed on.
    closed: Arc<Mutex<Vec<ThreadId>>>,
}

impl SimulatedInput {
    pub(crate) fn devices(&self) -> Devices {
        Devices {
            open_input: Some(self.opener()),
            ..Devices::default()
        }
    }

    fn opener(&self) -> OpenInput {
        let input = self.clone();
        Arc::new(move || {
            if input.fails.load(Ordering::Relaxed) {
                return Err(sound_core::DeviceError::NoInputDevice);
            }
            let (writer, reader) = sound_core::capture(48_000, 2);
            let (live_writer, live) = sound_core::live_input(48_000, 2);
            *input.writer.lock().unwrap() = Some(writer);
            *input.live.lock().unwrap() = Some(live_writer);
            input.written.store(0, Ordering::Relaxed);
            input.openings.fetch_add(1, Ordering::Relaxed);
            let stream = Closes(input.closed.clone());
            Ok(OpenedInput {
                stream: Some(Box::new(stream)),
                reader,
                live,
                notice: None,
            })
        })
    }

    /// The threads it closed on, one for each time it closed.
    pub(crate) fn closed_on(&self) -> Vec<ThreadId> {
        self.closed.lock().unwrap().clone()
    }

    /// Writes the input up to engine frame `until`: each frame from `sample(frame)`, left and
    /// right, captured at the moment that engine frame sounds.
    pub(crate) fn write_until(&self, until: u64, sample: impl Fn(u64) -> [f32; 2]) {
        let mut writer = self.writer.lock().unwrap();
        let Some(writer) = writer.as_mut() else {
            return;
        };
        let first = self.written.load(Ordering::Relaxed);
        if until <= first {
            return;
        }
        let samples: Vec<f32> = (first..until).flat_map(sample).collect();
        let nanos = |frame: u64| frame * 1_000_000_000 / 48_000;
        writer.write(&samples, nanos(first), 0);
        if let Some(live) = self.live.lock().unwrap().as_mut() {
            live.write(&samples);
        }
        self.written.store(until, Ordering::Relaxed);
    }
}

/// The stream of a simulated device, which tells which thread dropped it.
pub(crate) struct Closes(pub Arc<Mutex<Vec<ThreadId>>>);

impl Drop for Closes {
    fn drop(&mut self) {
        self.0.lock().unwrap().push(std::thread::current().id());
    }
}
