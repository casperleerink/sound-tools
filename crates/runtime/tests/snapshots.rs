//! Renders the application window to PNGs with no visible window. `cargo test -p runtime --test
//! snapshots` writes to `$WINDOW_SNAPSHOT_DIR`, default `<target>/window-snapshots`. The window
//! is 1470 x 920 points at scale 2, the screen of a 13 inch MacBook Air without the menu bar,
//! which is the laptop the design is for:
//!
//! - `default.png`: a new, empty project, with the ways to start one.
//! - `new-project.png`: the templates a new project starts from.
//! - `notices.png`: an error from an edit, a file that is not live and an update that is ready,
//!   top-right.
//! - `menu.png`, `menu-devices.png`: the project menu, open, after one edit.
//! - `cards-*.png`: the card of every built-in device, see `snapshots/cards.rs`.
//! - `wavetable*.png`: the Wavetable, collapsed and on each page, see `snapshots/wavetable.rs`.
//! - `agent-*.png`: the agent sidebar, open and closed, see `snapshots/agent.rs`.
//! - `typescript-*.png`: pages of tools of the project, see `snapshots/typescript.rs`.
//! - `look-*.png`: the DAW template with the content of the deleted arrangement's snapshots,
//!   see `snapshots/look.rs`.
//!
//! `WINDOW_SNAPSHOT_ONLY=cards`, `=wavetable`, `=agent` or `=typescript` renders the default
//! project and those alone.

// The shared helpers of the tests unwrap, as the other test crates allow.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui::{
    AnyWindowHandle, AppContext, Entity, HeadlessAppContext, PlatformInput, WindowHandle, px, size,
};
use plugin_host::{PluginFormat, WeakPlugins};
use runtime::app::ChosenDevices;
use runtime::window::new_project::{TemplatePicker, choose_template};
use runtime::window::{DeviceAccess, DeviceMenu, Shell};
use runtime::{OFFLINE, views};
use sound_core::{DeviceChoice, Engine, InstanceId, Ticks};
use sound_ui::{Assets, Session};
use tempfile::TempDir;

use plugin_hosts::test_plugin_host;

#[path = "snapshots/agent.rs"]
mod agent;
#[path = "snapshots/cards.rs"]
mod cards;
#[path = "snapshots/look.rs"]
mod look;
#[allow(dead_code)]
#[path = "shared/plugin_hosts.rs"]
mod plugin_hosts;
#[allow(dead_code)]
#[path = "shared/rack.rs"]
mod rack;
#[allow(dead_code)]
#[path = "shared/song.rs"]
mod song;
#[path = "snapshots/typescript.rs"]
mod typescript;
#[path = "snapshots/wavetable.rs"]
mod wavetable;

/// The window in points.
const WINDOW_WIDTH: f32 = 1470.;
const WINDOW_HEIGHT: f32 = 920.;

struct Opened {
    _folder: TempDir,
    engine: Engine,
    plugins: WeakPlugins,
    session: Entity<Session>,
    window: WindowHandle<Shell>,
}

impl Opened {
    /// A window on a project of these files, by their paths in its folder. Its tools of the
    /// project have drawn when this returns.
    fn open(cx: &mut HeadlessAppContext, files: &[(String, String)]) -> Result<Self> {
        Self::open_with(cx, files, |_| Ok(()))
    }

    /// [`Self::open`], with `prepare` writing more into the project folder first.
    fn open_with(
        cx: &mut HeadlessAppContext,
        files: &[(String, String)],
        prepare: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<Self> {
        // The folder name is the project name in the window.
        let folder = tempfile::tempdir()?;
        let root = folder.path().join("Night Study");
        std::fs::create_dir_all(&root)?;
        for (relative, contents) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().context("a parent folder")?)?;
            std::fs::write(path, contents)?;
        }
        prepare(&root)?;
        cx.allow_parking();
        let (control, engine) = Engine::new(OFFLINE);
        // A plugin host that looks only in a folder of this project, with the repository's own
        // test plugin in it, so that the pickers hold the same plugins everywhere.
        let plugins = test_plugin_host(&root);
        let (project, extensions) = runtime::open_with_extensions(&root, control, plugins.clone())?;
        let plugins = plugins.downgrade();
        let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
        let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
            let session = session.clone();
            let (mut views, mut devices) = views(plugins.clone());
            if let Some(extensions) = extensions {
                sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
            }
            cx.new(|cx| {
                let name = "MacBook Pro Speakers".into();
                let device = DeviceAccess::default();
                Shell::with_device(session, (views, devices), name, device, window, cx)
            })
        })?;
        // Bun answers in real time, on a thread of its own.
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(Self {
            _folder: folder,
            engine,
            plugins,
            session,
            window,
        })
    }

    /// Lets the engine run for `frames` and the session see it, as the device and the poll
    /// timer do in the real window.
    fn advance(&mut self, frames: usize, cx: &mut HeadlessAppContext) {
        // What the poll of the window does for the background work of the extensions.
        drum_pad::wait_for_sounds();
        if let Some(plugins) = self.plugins.upgrade() {
            let mut scanned = plugins.scan_generation();
            cx.update(|cx| runtime::window::tick(&self.session, &plugins, &mut scanned, cx));
        }
        let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
        self.engine.process_block(&mut buffer);
        cx.update(|cx| self.session.update(cx, |session, cx| session.poll(cx)));
        // And what the timer of the tools of the project does, which a headless window runs not.
        cx.update(|cx| {
            let shell = self.window.read(cx)?;
            let page = shell.main_view().cloned();
            anyhow::Ok(page)
        })
        .ok()
        .flatten()
        .map(|page| cx.update(|cx| sound_typescript::run_frame(&page, cx)));
        cx.run_until_parked();
    }

    /// Seeks and plays, then plays half a second in frames of the window, so meters and what
    /// eases move as they do.
    fn play_from(&mut self, tick: Ticks, cx: &mut HeadlessAppContext) -> Result<()> {
        cx.update(|cx| {
            self.session.update(cx, |session, _| {
                session.engine().seek(tick);
                session.engine().play();
            })
        });
        for _ in 0..30 {
            self.advance(800, cx);
            std::thread::sleep(Duration::from_millis(16));
            cx.update_window(self.window.into(), |_, window, _| window.refresh())?;
            cx.run_until_parked();
        }
        Ok(())
    }

    /// One real event, and the frame it causes.
    fn event(&self, event: PlatformInput, cx: &mut HeadlessAppContext) -> Result<()> {
        cx.update_window(self.window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// One key, and a frame, as the screen draws one between two keys: a focus ring follows
    /// the focus that a frame sees.
    fn key(&self, keys: &str, cx: &mut HeadlessAppContext) -> Result<()> {
        let key_down = PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(keys)?,
            is_held: false,
            prefer_character_input: false,
        });
        self.event(key_down, cx)?;
        cx.capture_screenshot(self.window.into())?;
        Ok(())
    }

    /// The view of the card of `instance` that the main page shows.
    fn card<V: 'static>(&self, instance: &str, cx: &mut HeadlessAppContext) -> Result<Entity<V>> {
        let instance = InstanceId::new(instance)?;
        cx.update(|cx| {
            let shell = self.window.read(cx)?;
            let page = shell.main_view().context("no main view")?;
            let view = sound_typescript::embedded_view(page, &instance, cx);
            let view = view.with_context(|| format!("no card of {instance}"))?;
            view.downcast::<V>()
                .map_err(|_| anyhow::anyhow!("the card of {instance} is of another view"))
        })
    }
}

type Save<'a> = dyn Fn(&mut HeadlessAppContext, AnyWindowHandle, &str) -> Result<()> + 'a;

fn main() -> Result<()> {
    // The offscreen renderer of GPUI is Metal's. Other platforms have none.
    if cfg!(not(target_os = "macos")) {
        println!("The snapshots render with Metal, so they run on macOS only.");
        return Ok(());
    }
    let out_dir = std::env::var("WINDOW_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("../window-snapshots"));
    std::fs::create_dir_all(&out_dir)?;

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(sound_ui::init);
    cx.update(runtime::window::bind_keys);
    let save = |cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str| -> Result<()> {
        let path = out_dir.join(format!("{name}.png"));
        cx.capture_screenshot(window)?.save(&path)?;
        println!("wrote {}", path.display());
        Ok(())
    };

    let opened = Opened::open(&mut cx, &[])?;
    save(&mut cx, opened.window.into(), "default")?;

    let only = std::env::var("WINDOW_SNAPSHOT_ONLY").ok();
    let runs = |name: &str| only.as_deref().is_none_or(|only| only == name);
    if runs("cards") {
        cards::snapshots(&mut cx, &save)?;
    }
    if runs("wavetable") {
        wavetable::snapshots(&mut cx, &save)?;
    }
    if runs("agent") {
        agent::snapshots(&mut cx, &save)?;
    }
    if runs("typescript") {
        typescript::snapshots(&mut cx, &save)?;
    }
    if runs("look") {
        look::snapshots(&mut cx, &save)?;
    }
    if only.is_some() {
        return Ok(());
    }

    templates(&mut cx, &save)?;
    notices(&mut cx, &save)?;
    menu(&mut cx, opened, &save)?;
    Ok(())
}

/// The window of the templates, as "New project…" opens it.
fn templates(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    cx.update(|cx| choose_template(Rc::new(|_, _| {}), cx));
    cx.run_until_parked();
    let picker = cx.update(|cx| {
        (cx.windows().into_iter()).find_map(|window| window.downcast::<TemplatePicker>())
    });
    let picker = picker.context("the templates did not open")?;
    save(cx, picker.into(), "new-project")?;
    cx.update_window(picker.into(), |_, window, _| window.remove_window())?;
    cx.run_until_parked();
    Ok(())
}

/// The notices, top-right: an edit that failed, a file that is not live, here the record of a
/// plugin this machine does not have, and an update that is ready.
fn notices(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    let missing = r#"{"tool": "plugin", "state": {"format": "clap", "plugin_id": "com.example.nowhere", "state_asset": "bass"}}"#;
    let files = rack::rack_files(&[("instrument", missing)]);
    let notices = Opened::open(cx, &files)?;
    cx.update(|cx| {
        notices.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Wrong");
                let wrong = |state: &mut serde_json::Value| state["order"] = 3.into();
                project.update_json(&mut edit, &InstanceId::new("rack")?, wrong)?;
                project.finish(edit)
            })
        })
    });
    anyhow::ensure!(
        cx.update(|cx| notices.session.read(cx).notice().is_some()),
        "the edit did not fail"
    );
    // Set in an update of the window, which draws a frame after it.
    let version = runtime::update::Version::parse("0.2.0").context("no version")?;
    cx.update_window(notices.window.into(), |_, _, cx| {
        cx.set_global(runtime::update::Ready {
            version,
            action: runtime::update::Action::Restart,
        })
    })?;
    cx.run_until_parked();
    save(cx, notices.window.into(), "notices")?;
    drop(notices);
    cx.update(|cx| cx.remove_global::<runtime::update::Ready>());
    Ok(())
}

/// The project menu after an edit, so that undo has something to name, with recent projects and
/// the devices of a machine.
fn menu(cx: &mut HeadlessAppContext, opened: Opened, save: &Save) -> Result<()> {
    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = sound_core::Changes::new();
                changes.create(InstanceId::new("room")?, reverb::ReverbState::default());
                project.commit("Add reverb", changes)
            })
        })
    });
    let menu = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        let project_menu = shell.project_menu().clone();
        project_menu.update(cx, |project_menu, cx| {
            let recent = [
                "/Users/someone/Music/Night drive",
                "/Users/someone/Sketches/Piano",
            ];
            project_menu.set_recent_projects(recent.map(PathBuf::from).to_vec(), cx);
            let device = |id: &str, name: &str| DeviceChoice {
                id: id.to_string(),
                name: name.to_string(),
            };
            let devices = DeviceMenu {
                outputs: vec![
                    device("coreaudio:speakers", "MacBook Pro Speakers"),
                    device("coreaudio:blackhole", "BlackHole 2ch"),
                ],
                inputs: vec![
                    device("coreaudio:microphone", "MacBook Pro Microphone"),
                    device("coreaudio:blackhole", "BlackHole 2ch"),
                ],
                chosen: ChosenDevices {
                    output: Some("coreaudio:blackhole".to_string()),
                    input: None,
                },
            };
            project_menu.set_devices(devices, cx);
        });
        anyhow::Ok(project_menu.read(cx).menu().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    // Up from the bottom to "Open recent", and right opens its submenu.
    for key in ["up", "up", "up", "up", "right"] {
        opened.key(key, cx)?;
    }
    save(cx, opened.window.into(), "menu")?;
    // On up to the output device, which opens the devices of the machine.
    for key in ["left", "up", "up", "up", "up", "right"] {
        opened.key(key, cx)?;
    }
    save(cx, opened.window.into(), "menu-devices")?;
    Ok(())
}

/// The record of the repository's test plugin in `format`.
fn test_plugin(format: PluginFormat, state_asset: &str) -> String {
    let id = match format {
        PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    };
    format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{id}", "state_asset": "{state_asset}"}}}}"#,
        format.as_str()
    )
}
