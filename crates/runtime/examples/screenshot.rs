//! Renders the window of a project folder to PNGs with no visible window, the project's own
//! TypeScript tools included, to see how the cards and pages an agent builds turn out:
//!
//! ```sh
//! cargo run -p runtime --example screenshot -- <project> <out-folder> [--size WxH]
//! ```
//!
//! - `window.png`: the window as the project opens, which is its page when it has one.
//! - `track-<name>.png`: each track of the arrangement selected and its panel open, so its
//!   cards show. `<name>` is the folder of the track.
//!
//! It opens a copy of the folder, because opening a project writes into it. It plays no sound,
//! since the engine runs offline, and writes nothing to the app's support folder: no agent
//! panel, no plugin scan, no sampler library. Bun runs the tools as in the app. The window is
//! 1470 x 920 points at scale 2 unless `--size` says otherwise. macOS only, as the window
//! snapshots: the offscreen renderer is Metal's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use arrangement::view::ArrangementView;
use gpui::{AppContext as _, Entity, HeadlessAppContext, px, size};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use runtime::window::Shell;
use runtime::{OFFLINE, main_arrangement, open_with_extensions, views};
use sound_core::Engine;
use sound_ui::{Assets, POLL_INTERVAL, Session};

const USAGE: &str = "usage: screenshot <project> <out-folder> [--size WxH]";

/// How long the window runs before each picture: Bun draws a card within milliseconds, and a
/// page with a control loop moves for a while, as it does when the app opens.
const SETTLE: Duration = Duration::from_millis(500);

fn main() -> Result<()> {
    let started = Instant::now();
    let (project, out, (width, height)) = arguments()?;
    if cfg!(not(target_os = "macos")) {
        bail!("the window renders offscreen with Metal, so this runs on macOS only");
    }
    if !project.join("project.json").is_file() {
        bail!("{} is not a Sound Tools project", project.display());
    }
    std::fs::create_dir_all(&out)?;
    // The folder name is the project name in the window.
    let name = project
        .file_name()
        .context("the project folder has no name")?;
    let scratch = tempfile::tempdir()?;
    let copy = scratch.path().join(name);
    copy_folder(&project, &copy)?;

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    // Bun answers on a thread of its own, which wakes the window.
    cx.allow_parking();
    cx.update(sound_ui::init);
    cx.update(runtime::window::bind_keys);

    let (control, mut engine) = Engine::new(OFFLINE);
    // A host with no folder to look in scans nothing and keeps no cache. A plugin of the
    // project shows as missing.
    let plugins = Plugins::listing(Vec::new(), ScanCommand::this_program()?, ScanCache::none());
    let (project, extensions) = open_with_extensions(&copy, control, plugins.clone())?;
    let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
    let window = cx.open_window(size(px(width), px(height)), |window, cx| {
        cx.new(|cx| {
            let (mut views, mut devices) = views(plugins.downgrade());
            if let Some(extensions) = extensions {
                sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
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
    let save = |cx: &mut HeadlessAppContext, name: &str| -> Result<()> {
        // A capture is of the last frame. Some views place themselves by what the frame before
        // measured, such as the add track row, and the app always draws another frame.
        cx.update_window(window.into(), |_, window, _| window.refresh())?;
        cx.run_until_parked();
        let path = out.join(format!("{name}.png"));
        cx.capture_screenshot(window.into())?.save(&path)?;
        println!("wrote {}", path.display());
        Ok(())
    };

    settle(&mut cx, &mut engine, &session, &plugins);
    save(&mut cx, "window")?;

    let main = cx.update(|cx| anyhow::Ok(window.read(cx)?.main_view().cloned()))?;
    if let Some(view) = main.and_then(|view| view.downcast::<ArrangementView>().ok()) {
        let tracks = cx.update(|cx| {
            let project = session.read(cx).project();
            let arrangement = main_arrangement(project);
            let tracks =
                arrangement.map(|arrangement| arrangement::tracks(project, arrangement.id()));
            let tracks = tracks.into_iter().flatten();
            tracks.map(|(track, _)| track).collect::<Vec<_>>()
        });
        let timeline = cx.update(|cx| view.read(cx).timeline().clone());
        for track in tracks {
            cx.update_window(window.into(), |_, window, cx| {
                timeline.update(cx, |timeline, cx| {
                    timeline.select_track(Some(track.id().clone()), cx)
                });
                view.update(cx, |view, cx| {
                    view.open_track_panel(track.clone(), window, cx)
                });
            })?;
            settle(&mut cx, &mut engine, &session, &plugins);
            save(&mut cx, &format!("track-{}", track.id().name()))?;
        }
    }

    println!(
        "{}",
        cx.update(|cx| runtime::problems(session.read(cx).project()))
    );
    println!("took {:.1} s", started.elapsed().as_secs_f32());
    Ok(())
}

/// Runs the window for [`SETTLE`] as the app does, in polls of real time: the engine offline,
/// the background work of the extensions, the timers, and Bun.
fn settle(
    cx: &mut HeadlessAppContext,
    engine: &mut Engine,
    session: &Entity<Session>,
    plugins: &Plugins,
) {
    let frames = (OFFLINE.sample_rate as f32 * POLL_INTERVAL.as_secs_f32()) as usize;
    let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
    let mut scanned = plugins.scan_generation();
    let started = Instant::now();
    while started.elapsed() < SETTLE {
        drum_pad::wait_for_sounds();
        cx.update(|cx| runtime::window::tick(session, plugins, &mut scanned, cx));
        engine.process_block(&mut buffer);
        cx.advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
        std::thread::sleep(POLL_INTERVAL);
    }
    cx.run_until_parked();
}

/// The project folder, the folder for the pictures, and the size of the window in points.
fn arguments() -> Result<(PathBuf, PathBuf, (f32, f32))> {
    let mut folders = Vec::new();
    let mut window = (1470., 920.);
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument != "--size" {
            folders.push(PathBuf::from(argument));
            continue;
        }
        let value = arguments.next().context(USAGE)?;
        let (width, height) = value
            .to_str()
            .and_then(|value| value.split_once('x'))
            .context(USAGE)?;
        window = (
            width.parse().context(USAGE)?,
            height.parse().context(USAGE)?,
        );
    }
    let [project, out] = <[PathBuf; 2]>::try_from(folders).ok().context(USAGE)?;
    Ok((project, out, window))
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
