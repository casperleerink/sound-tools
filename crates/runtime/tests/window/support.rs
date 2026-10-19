//! A window on a temporary project, and the hands of a composer: a mouse that presses, moves
//! and releases, and the keys. A project's own tools run in Bun, which these tests need.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AppContext, BackgroundExecutor, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, point,
};
use plugin_host::{Plugins, ScanCache, WeakPlugins};
use runtime::window::audio_input::{OpenInput, OpenedInput};
use runtime::window::other_apps::AppSounds;
use runtime::window::{DeviceAccess, ListDevices, Shell, bind_keys};
use runtime::{OFFLINE, views};
use sound_core::{CaptureWriter, Engine, InstanceId, LiveWriter, Project};
use sound_ui::{POLL_INTERVAL, Session};
use tempfile::TempDir;

use crate::plugin_hosts::{scanner, test_plugin_host};
use crate::rack::rack_files;

pub(crate) const BAR: u64 = 3840;

pub(crate) struct Opened<'a> {
    pub folder: TempDir,
    pub engine: Engine,
    /// The plugin host of this project. Held weakly, as the runtime holds it: the project is
    /// what ends it, and that is what saves the state of every plugin.
    pub plugins: WeakPlugins,
    pub session: Entity<Session>,
    pub shell: Entity<Shell>,
    /// What the scan had found the last time a poll asked for a frame, as the window's own
    /// poll keeps it.
    scanned: u64,
    pub cx: &'a mut VisualTestContext,
}

pub(crate) fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// Opens the window on a new, empty project.
pub(crate) fn open_new(cx: &mut TestAppContext) -> Opened<'_> {
    open_files(cx, &[])
}

/// Opens the window on a project of these files, by their paths in its folder.
pub(crate) fn open_files<'a>(cx: &'a mut TestAppContext, files: &[(String, String)]) -> Opened<'a> {
    open_with(cx, files, no_plugins, DeviceAccess::default())
}

/// Opens the window on a `rack` of these children, see `crate::rack`, once Bun drew the card
/// of each.
pub(crate) fn open_rack<'a>(cx: &'a mut TestAppContext, children: &[(&str, &str)]) -> Opened<'a> {
    open_rack_with(cx, children, no_plugins)
}

/// [`open_rack`], after `prepare` adds to its folder, such as the audio file of a sampler.
pub(crate) fn open_rack_after<'a>(
    cx: &'a mut TestAppContext,
    children: &[(&str, &str)],
    prepare: impl FnOnce(&Path),
) -> Opened<'a> {
    let host = |folder: &Path| {
        prepare(folder);
        no_plugins(folder)
    };
    open_rack_with(cx, children, host)
}

/// The same, with a plugin host that looks only in `plugins/` inside the project folder, where
/// the repository's own test plugin is put. No plugin of this machine is ever used, so these
/// tests run the same everywhere.
pub(crate) fn open_rack_with_test_plugin<'a>(
    cx: &'a mut TestAppContext,
    children: &[(&str, &str)],
) -> Opened<'a> {
    open_rack_with(cx, children, test_plugin_host)
}

fn open_rack_with<'a>(
    cx: &'a mut TestAppContext,
    children: &[(&str, &str)],
    host: impl FnOnce(&Path) -> Plugins,
) -> Opened<'a> {
    let mut opened = open_with(cx, &rack_files(children), host, DeviceAccess::default());
    for (name, record) in children {
        if !crate::rack::plays_no_sound(record) {
            opened.control(&format!("div-card-{name}"));
        }
    }
    opened
}

fn no_plugins(_: &Path) -> Plugins {
    Plugins::new(Vec::new(), scanner(), ScanCache::none())
}

/// Opens the window on a project of `files` with the plugin host `host` makes for its folder,
/// and what the window may open of the machine.
pub(crate) fn open_with<'a>(
    cx: &'a mut TestAppContext,
    files: &[(String, String)],
    host: impl FnOnce(&Path) -> Plugins,
    device: DeviceAccess,
) -> Opened<'a> {
    let folder = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = folder.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    open_folder_with(cx, folder, host, device)
}

/// Opens the window on the project in `folder` again, as quitting and starting the app does.
pub(crate) fn open_folder(cx: &mut TestAppContext, folder: TempDir) -> Opened<'_> {
    open_folder_with(cx, folder, no_plugins, DeviceAccess::default())
}

pub(crate) fn open_folder_with<'a>(
    cx: &'a mut TestAppContext,
    folder: TempDir,
    host: impl FnOnce(&Path) -> Plugins,
    device: DeviceAccess,
) -> Opened<'a> {
    assert!(
        sound_typescript::has_bun(),
        "these tests run the tools of a project in Bun: install it, see tooling/fetch-bun.sh"
    );
    // Bun's answers wake the window's tasks from a thread of their own.
    cx.executor().allow_parking();
    let (control, engine) = Engine::new(OFFLINE);
    let plugins = host(folder.path());
    let (project, extensions) =
        runtime::open_with_extensions(folder.path(), control, plugins.clone()).unwrap();
    cx.update(sound_ui::init);
    let session = cx.new(|cx| Session::new(project, cx));
    cx.update(bind_keys);
    let plugins = plugins.downgrade();
    let (shell, cx) = cx.add_window_view({
        let (session, plugins) = (session.clone(), plugins.clone());
        move |window, cx| {
            let (mut views, mut devices) = views(plugins);
            if let Some(extensions) = extensions {
                sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
            }
            let name = "Test device".into();
            Shell::with_device(session, (views, devices), name, device, window, cx)
        }
    });
    cx.run_until_parked();
    Opened {
        folder,
        engine,
        plugins,
        session,
        shell,
        scanned: 0,
        cx,
    }
}

/// An audio input the window opens as its default input, with no device: two channels at
/// 48 kHz, into which the test writes what a device would have captured. With no device the
/// window has no timing either, so engine frame `n` sounds at `n / 48000` s, and an input frame
/// written at that moment was played while the composer heard that frame.
#[derive(Clone, Default)]
pub(crate) struct SimulatedInput {
    writer: Arc<Mutex<Option<CaptureWriter>>>,
    /// What the engine plays live, written with the capture.
    live: Arc<Mutex<Option<LiveWriter>>>,
    /// Input frames written so far.
    written: Arc<AtomicU64>,
    /// Times the window opened it.
    pub openings: Arc<AtomicU32>,
    /// Times the window tried to, the ones that failed too.
    attempts: Arc<AtomicU32>,
    /// Whether opening it fails, as a device that is in use or not allowed.
    pub fails: Arc<AtomicBool>,
    /// Its sample rate, when not 48 kHz.
    pub rate: Arc<AtomicU32>,
    /// Each time it closed, whether it closed on the thread that draws.
    closed: Arc<Mutex<Vec<bool>>>,
}

impl SimulatedInput {
    /// `executor` is what an input that closes asks whether it is on the thread that draws.
    /// How many times it closed.
    pub(crate) fn closings(&self) -> usize {
        self.closed.lock().unwrap().len()
    }

    pub(crate) fn opener(&self, executor: BackgroundExecutor) -> OpenInput {
        let input = self.clone();
        Arc::new(move || {
            input.attempts.fetch_add(1, Ordering::Relaxed);
            if input.fails.load(Ordering::Relaxed) {
                return Err(sound_core::DeviceError::NoInputDevice);
            }
            let rate = match input.rate.load(Ordering::Relaxed) {
                0 => 48_000,
                rate => rate,
            };
            let (writer, reader) = sound_core::capture(rate, 2);
            let (live_writer, live) = sound_core::live_input(rate, 2);
            *input.writer.lock().unwrap() = Some(writer);
            *input.live.lock().unwrap() = Some(live_writer);
            input.written.store(0, Ordering::Relaxed);
            input.openings.fetch_add(1, Ordering::Relaxed);
            let stream = SimulatedStream {
                closed: input.closed.clone(),
                executor: executor.clone(),
            };
            Ok(OpenedInput {
                stream: Some(Box::new(stream)),
                reader,
                live,
                notice: None,
            })
        })
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

/// The sound of one app that runs or not, with no tap: two channels at 48 kHz, into which the
/// test writes what the app would play.
#[derive(Clone, Default)]
pub(crate) struct SimulatedApp {
    /// Its processes; none while it is not running.
    processes: Arc<Mutex<Vec<u32>>>,
    live: Arc<Mutex<Option<LiveWriter>>>,
    /// For each tap that closed, whether it closed on the thread that draws.
    closed: Arc<Mutex<Vec<bool>>>,
}

impl SimulatedApp {
    /// `executor` is what a tap that closes asks whether it is on the thread that draws.
    pub(crate) fn sounds(&self, executor: BackgroundExecutor) -> AppSounds {
        let (app, processes) = (self.clone(), self.processes.clone());
        AppSounds {
            open: Arc::new(move |name| {
                if app.processes.lock().unwrap().is_empty() {
                    return Err(sound_core::DeviceError::NoApp(name.clone().into()));
                }
                let (writer, live) = sound_core::live_input(48_000, 2);
                *app.live.lock().unwrap() = Some(writer);
                let stream = SimulatedStream {
                    closed: app.closed.clone(),
                    executor: executor.clone(),
                };
                Ok((Box::new(stream), live))
            }),
            processes: Arc::new(move |_| processes.lock().unwrap().clone()),
        }
    }

    /// The app starts, or quits with no processes.
    pub(crate) fn set_processes(&self, processes: Vec<u32>) {
        *self.processes.lock().unwrap() = processes;
    }

    /// Plays one stereo frame, `frames` times.
    pub(crate) fn play(&self, frame: [f32; 2], frames: usize) {
        if let Some(writer) = self.live.lock().unwrap().as_mut() {
            writer.write(&frame.repeat(frames));
        }
    }

    /// Whether the engine has the live input of its tap.
    pub(crate) fn is_heard(&self) -> bool {
        (self.live.lock().unwrap().as_ref()).is_some_and(|writer| !writer.is_abandoned())
    }

    /// For each tap that closed, whether it closed on the thread that draws.
    pub(crate) fn closed_on_ui_thread(&self) -> Vec<bool> {
        self.closed.lock().unwrap().clone()
    }
}

/// The stream of a simulated tap or input, which tells where it was dropped.
struct SimulatedStream {
    closed: Arc<Mutex<Vec<bool>>>,
    executor: BackgroundExecutor,
}

impl Drop for SimulatedStream {
    fn drop(&mut self) {
        let on_ui_thread = self.executor.is_main_thread();
        self.closed.lock().unwrap().push(on_ui_thread);
    }
}

/// A new, empty project in a window that hears a simulated app.
pub(crate) fn open_with_app(cx: &mut TestAppContext) -> (Opened<'_>, SimulatedApp) {
    let app = SimulatedApp::default();
    let device = DeviceAccess {
        app_sounds: Some(app.sounds(cx.executor())),
        ..DeviceAccess::default()
    };
    (open_with(cx, &[], no_plugins, device), app)
}

/// A new, empty project in a window that lists the audio devices with `list`.
pub(crate) fn open_with_devices(cx: &mut TestAppContext, list: ListDevices) -> Opened<'_> {
    let device = DeviceAccess {
        list_devices: Some(list),
        ..DeviceAccess::default()
    };
    open_with(cx, &[], no_plugins, device)
}

impl Opened<'_> {
    /// Lets the engine take what was sent to it, the poll timer see the result, and what Bun
    /// answered land.
    pub(crate) fn settle(&mut self) {
        // The sounds of a Drum pad are made on a thread of their own; the window takes them
        // once per poll.
        drum_pad::wait_for_sounds();
        self.poll_plugins();
        self.render(64);
        self.cx.executor().advance_clock(POLL_INTERVAL);
        self.cx.run_until_parked();
        self.poll_plugins();
    }

    /// Settles until `done` holds: Bun answers on a thread of its own, in real time.
    pub(crate) fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            self.settle();
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{what} did not happen within 5 s");
    }

    /// One poll of the background work, which the runtime does on its own timer every 16 ms:
    /// what Bun said is heard, and a plugin whose record no longer names it is let go of.
    pub(crate) fn poll_plugins(&mut self) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let (session, scanned) = (self.session.clone(), &mut self.scanned);
        self.cx
            .update(|_, cx| runtime::window::tick(&session, &plugins, scanned, cx));
        self.cx.run_until_parked();
    }

    /// Runs the engine for `frames` and gives what it played, interleaved.
    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0_f32; frames * OFFLINE.channels];
        for buffer in output.chunks_mut(512 * OFFLINE.channels) {
            self.engine.process_block(buffer);
        }
        output
    }

    /// Puts `state` in place of the record of `instance`, as the project writes it, with no undo
    /// step: so an undo later gives back the same file.
    pub(crate) fn replace<S: sound_core::State>(&mut self, instance: &str, state: S) {
        let instance = id(instance);
        self.edit(|project| {
            let mut changes = sound_core::Changes::new();
            changes.delete(&instance);
            project.commit("Remove", changes)?;
            let mut changes = sound_core::Changes::new();
            changes.create(instance.clone(), state);
            project.commit("Add", changes)?;
            project.clear_history();
            Ok(())
        });
        self.settle();
    }

    /// The view of the card of `instance` that the main page shows.
    pub(crate) fn card<V: 'static>(&mut self, instance: &str) -> Entity<V> {
        let (shell, instance) = (self.shell.clone(), id(instance));
        self.cx.read(|cx| {
            let page = shell.read(cx).main_view().unwrap();
            let view = sound_typescript::embedded_view(page, &instance, cx).unwrap();
            view.downcast::<V>().ok().unwrap()
        })
    }

    pub(crate) fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.cx.run_until_parked();
    }

    pub(crate) fn project<R>(&mut self, read: impl FnOnce(&Project) -> R) -> R {
        let session = self.session.clone();
        self.cx.read(|cx| read(session.read(cx).project()))
    }

    /// Runs a project operation through the session, as the watcher or another view would.
    pub(crate) fn edit<R>(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<R, sound_core::ProjectError>,
    ) -> Option<R> {
        let session = self.session.clone();
        let result = self
            .cx
            .update(|_, cx| session.update(cx, |session, cx| session.edit(cx, operation)));
        self.cx.run_until_parked();
        result
    }

    pub(crate) fn undo_label(&mut self) -> Option<String> {
        self.project(|project| project.undo_label().map(str::to_string))
    }

    pub(crate) fn gesture_open(&mut self) -> bool {
        let session = self.session.clone();
        self.cx.read(|cx| session.read(cx).gesture_open())
    }

    pub(crate) fn notice(&mut self) -> Option<String> {
        let session = self.session.clone();
        self.cx
            .read(|cx| session.read(cx).notice().map(ToString::to_string))
    }

    /// A path in the project folder, canonical as the project knows it.
    pub(crate) fn path(&mut self, relative: &str) -> PathBuf {
        self.project(|project| project.root().join(relative))
    }

    /// The middle of a control that names itself for tests: `knob-<id>` or `segment-<value>`.
    /// GPUI knows the bounds of what the last frame painted, and a cached view paints
    /// nothing, so this asks for a whole frame first. Waits for Bun to draw it, up to 5 s.
    pub(crate) fn control(&mut self, selector: &str) -> Point<Pixels> {
        let start = Instant::now();
        loop {
            if let Some(found) = self.find(selector) {
                return found;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "nothing on screen is called {selector}"
            );
            self.settle();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Where a control is on screen, whole. `None` when it is not there.
    pub(crate) fn bounds(&mut self, selector: &str) -> Option<Bounds<Pixels>> {
        let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
        self.cx.debug_bounds(selector)
    }

    /// The same, `None` when that control is not on screen. A selector that is made while the
    /// test runs, such as a menu row of a plugin, is leaked: GPUI keeps them by `&'static str`
    /// and a test process is short.
    pub(crate) fn find(&mut self, selector: &str) -> Option<Point<Pixels>> {
        let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
        self.cx.debug_bounds(selector).map(|bounds| bounds.center())
    }

    pub(crate) fn press(&mut self, position: Point<Pixels>) {
        self.press_times(position, 1);
    }

    fn press_times(&mut self, position: Point<Pixels>, click_count: usize) {
        self.press_with(position, click_count, Modifiers::default());
    }

    pub(crate) fn press_with(
        &mut self,
        position: Point<Pixels>,
        click_count: usize,
        modifiers: Modifiers,
    ) {
        self.cx.simulate_mouse_move(position, None, modifiers);
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    /// A move with the left button held.
    pub(crate) fn drag_to(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
        self.cx.run_until_parked();
    }

    pub(crate) fn release(&mut self, position: Point<Pixels>) {
        self.release_times(position, 1);
    }

    fn release_times(&mut self, position: Point<Pixels>, click_count: usize) {
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
        self.cx.run_until_parked();
    }

    /// Press, move in two steps, release: the mouse never jumps in one event.
    pub(crate) fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.press(from);
        let half = point((from.x + to.x) / 2., (from.y + to.y) / 2.);
        self.drag_to(half);
        self.drag_to(to);
        self.release(to);
    }

    /// Closes the window and the project, and gives the folder back for a second opening.
    pub(crate) fn close(self) -> TempDir {
        let Self {
            folder,
            engine,
            plugins,
            session,
            shell,
            scanned: _,
            cx,
        } = self;
        drop(plugins);
        cx.update(|window, _| window.remove_window());
        drop((engine, shell));
        cx.run_until_parked();
        // The window held every view, and the views held the session. This was the last hold.
        let released = session.downgrade();
        drop(session);
        cx.cx.update(|_| {});
        cx.run_until_parked();
        let held = released.upgrade().is_some();
        assert!(
            !held,
            "something still holds the session and the project lock"
        );
        folder
    }

    pub(crate) fn click(&mut self, position: Point<Pixels>) {
        self.press(position);
        self.release(position);
    }

    pub(crate) fn double_click(&mut self, position: Point<Pixels>) {
        self.double_press(position);
        self.release_times(position, 2);
    }

    /// A click and a second press that stays down, as a double click that goes on into a
    /// drag: how a note is drawn in the note editor.
    pub(crate) fn double_press(&mut self, position: Point<Pixels>) {
        self.click(position);
        self.press_times(position, 2);
    }
}

/// The record of a plugin instrument that names the repository's test plugin.
pub(crate) fn test_plugin_record(format: plugin_host::PluginFormat, state_asset: &str) -> String {
    let id = match format {
        plugin_host::PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        plugin_host::PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    };
    format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{id}", "state_asset": "{state_asset}"}}}}"#,
        format.as_str()
    )
}

/// The loudest sample.
pub(crate) fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// Every record file under `state/` and `project.json`, by relative path, with what it holds. A
/// record written by hand comes back from an undo as the project writes it, so the files are
/// compared as JSON.
pub(crate) fn files(root: &Path) -> Vec<(PathBuf, serde_json::Value)> {
    let read = |path: &Path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    fn walk(
        folder: &Path,
        root: &Path,
        found: &mut Vec<(PathBuf, serde_json::Value)>,
        read: &dyn Fn(&Path) -> serde_json::Value,
    ) {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, found, read);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                found.push((relative, read(&path)));
            }
        }
    }
    let mut found = vec![(
        PathBuf::from("project.json"),
        read(&root.join("project.json")),
    )];
    walk(&root.join("state"), root, &mut found, &read);
    found
}

/// The files and the last undo step before an action.
pub(crate) struct Before {
    pub files: Vec<(PathBuf, serde_json::Value)>,
    pub undo_label: Option<String>,
}

/// Every record file and `project.json`, and the undo step on top.
pub(crate) fn mark(opened: &mut Opened<'_>) -> Before {
    Before {
        files: files(opened.folder.path()),
        undo_label: opened.undo_label(),
    }
}

/// The action was exactly one undo step with this label: one cmd-z gives the files of before
/// back and the step before on top, and shift-cmd-z does it again.
pub(crate) fn one_undo_step(opened: &mut Opened<'_>, label: &str, before: &Before) {
    assert_eq!(opened.undo_label().as_deref(), Some(label));
    let after = files(opened.folder.path());
    opened.keys("secondary-z");
    assert_eq!(opened.undo_label(), before.undo_label, "more than one step");
    let now = files(opened.folder.path());
    assert_eq!(now, before.files, "undo did not give the files back");
    opened.keys("shift-secondary-z");
    let now = files(opened.folder.path());
    assert_eq!(now, after, "redo did not do it again");
}

/// Writes a record from outside and applies it, as the watcher does.
pub(crate) fn write_outside(opened: &mut Opened<'_>, relative: &str, contents: &str) {
    let path = opened.path(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    opened.settle();
}
