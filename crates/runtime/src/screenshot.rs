//! `--screenshot`: the window of a project as it opens, drawn offscreen to a PNG, the project's
//! own TypeScript tools included, so an agent sees the cards and pages it builds.
//!
//! It opens a copy of the folder, because opening a project writes into it. It plays no sound,
//! since the engine runs offline, and writes nothing to the app's support folder: no agent
//! panel, no plugin scan, no sampler library.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use gpui::{AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, px, size};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use sound_core::Engine;
use sound_ui::{Assets, POLL_INTERVAL, Session};

use crate::window::{Shell, WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::{OFFLINE, open_with_extensions, views};

/// How long the window runs before the picture when not told: Bun draws a card within
/// milliseconds, and a page with a control loop moves for a while, as it does when the app opens.
pub const SETTLE: Duration = Duration::from_millis(500);

/// Draws the window after it ran for `settle`, in real time: what moves, such as a canvas or
/// a meter, shows as it is by then.
pub fn screenshot(project: &Path, png: &Path, settle_for: Duration) -> Result<()> {
    let mut offline = Offline::open(project)?;
    offline.settle(settle_for)?;
    offline
        .capture()?
        .save(png)
        .with_context(|| format!("could not write {}", png.display()))?;
    println!("wrote {}", png.display());
    offline.print_problems();
    Ok(())
}

/// The window of a copy of a project, drawn offscreen, with the engine offline.
struct Offline {
    cx: HeadlessAppContext,
    engine: Engine,
    session: Entity<Session>,
    window: AnyWindowHandle,
    plugins: Plugins,
    /// The copy, deleted with it.
    _scratch: tempfile::TempDir,
}

impl Offline {
    fn open(project: &Path) -> Result<Self> {
        if !project.join(crate::PROJECT_FILE).is_file() {
            bail!("{} is not a Sound Tools project", project.display());
        }
        // The folder name is the project name in the window.
        let name = project
            .file_name()
            .context("the project folder has no name")?;
        let scratch = tempfile::tempdir()?;
        let copy = scratch.path().join(name);
        copy_folder(project, &copy)?;

        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(Assets),
            gpui_platform::current_headless_renderer,
        );
        // Bun answers on a thread of its own, which wakes the window.
        cx.allow_parking();
        cx.update(sound_ui::init);
        cx.update(crate::window::bind_keys);

        let (control, engine) = Engine::new(OFFLINE);
        // A host with no folder to look in scans nothing and keeps no cache. A plugin of the
        // project shows as missing.
        let plugins = Plugins::listing(Vec::new(), ScanCommand::this_program()?, ScanCache::none());
        let (project, extensions) = open_with_extensions(&copy, control, plugins.clone())?;
        let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
        let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
            cx.new(|cx| {
                let (mut views, mut devices) = views(plugins.downgrade());
                if let Some(extensions) = extensions {
                    sound_typescript::start_window(
                        extensions,
                        &session,
                        &mut views,
                        &mut devices,
                        cx,
                    );
                }
                Shell::new(
                    session.clone(),
                    (views, devices),
                    "Offline".into(),
                    window,
                    cx,
                )
            })
        })?;
        Ok(Self {
            cx,
            engine,
            session,
            window: window.into(),
            plugins,
            _scratch: scratch,
        })
    }

    /// Runs the window for `time` as the app does, in polls of real time: the engine offline,
    /// the background work of the extensions, the timers, and Bun. It draws a frame every poll,
    /// as a window on screen does, so what reads the frame before, such as a canvas's `last`,
    /// moves.
    fn settle(&mut self, time: Duration) -> Result<()> {
        let Self {
            cx,
            engine,
            session,
            window,
            plugins,
            ..
        } = self;
        let frames = (OFFLINE.sample_rate as f32 * POLL_INTERVAL.as_secs_f32()) as usize;
        let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
        let mut scanned = plugins.scan_generation();
        let started = Instant::now();
        while started.elapsed() < time {
            drum_pad::wait_for_sounds();
            cx.update(|cx| crate::window::tick(session, plugins, &mut scanned, cx));
            engine.process_block(&mut buffer);
            cx.advance_clock(POLL_INTERVAL);
            cx.run_until_parked();
            cx.update_window(*window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })?;
            std::thread::sleep(POLL_INTERVAL);
        }
        cx.run_until_parked();
        Ok(())
    }

    /// The window as drawn now. Some views place themselves by what the frame before measured,
    /// such as the add track row, and the app always draws another frame.
    fn capture(&mut self) -> Result<image::RgbaImage> {
        self.cx
            .update_window(self.window, |_, window, _| window.refresh())?;
        self.cx.run_until_parked();
        self.cx.capture_screenshot(self.window)
    }

    fn print_problems(&mut self) {
        let session = self.session.clone();
        println!(
            "{}",
            self.cx
                .update(|cx| crate::problems(session.read(cx).project()))
        );
    }
}

/// Copies the folder `from` to `to`. Hidden files, such as `.git` and the lock, are no part of
/// a project, and links are left out, so a link to a parent folder does not copy for ever.
fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("could not read {}", from.display()))?
    {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_folder(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)
                .with_context(|| format!("could not copy {}", source.display()))?;
        }
    }
    Ok(())
}
