//! `--screenshot`: the window of a project as it opens, drawn offscreen to a PNG, the project's
//! own TypeScript tools included, so an agent sees the cards and pages it builds. Each `--click`
//! is a click before the picture, so it also sees what a click opens: a panel, a menu. `--play`
//! then plays the piece for some seconds, so it sees what moves: meters, scopes, the step that
//! plays.
//!
//! It opens a copy of the folder, because opening a project writes into it. It plays no sound,
//! since the engine runs offline, and writes nothing to the app's support folder: no agent
//! panel, no plugin scan, no sampler library.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use gpui::{
    AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, point, px, size,
};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use sound_core::Engine;
use sound_ui::{Assets, POLL_INTERVAL, Session};

use crate::window::{Shell, WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::{OFFLINE, open_with_extensions, views};

/// How long the window runs before the picture: Bun draws a card within milliseconds, and a
/// page with a control loop moves for a while, as it does when the app opens.
const SETTLE: Duration = Duration::from_millis(500);

/// What happens between the window opening and the picture.
#[derive(Debug, Default, PartialEq)]
pub struct Before {
    /// The places of `--click x,y`, in points from the top left of the window, in order.
    clicks: Vec<Point<Pixels>>,
    /// How long the piece plays after the clicks, from `--play <seconds>`.
    play: Option<Duration>,
}

impl Before {
    pub fn parse(flags: &[&str]) -> Result<Self> {
        let mut before = Self::default();
        for flag in flags.chunks(2) {
            match flag {
                ["--click", at] => before.clicks.push(
                    at.split_once(',')
                        .and_then(|(x, y)| Some(point(px(x.parse().ok()?), px(y.parse().ok()?))))
                        .with_context(|| {
                            format!("--click takes a place as x,y in points, not {at}")
                        })?,
                ),
                ["--play", seconds] => {
                    let played = seconds.parse().ok().filter(|seconds| *seconds > 0.0);
                    let seconds: f32 = played.with_context(|| {
                        format!("--play takes seconds, such as 2, not {seconds}")
                    })?;
                    before.play = Some(Duration::try_from_secs_f32(seconds)?);
                }
                _ => bail!(
                    "--screenshot takes a file, then any number of --click x,y and a --play <seconds>"
                ),
            }
        }
        Ok(before)
    }
}

pub fn screenshot(project: &Path, png: &Path, before: &Before) -> Result<()> {
    if !project.join(crate::PROJECT_FILE).is_file() {
        bail!("{} is not a Sound Tools project", project.display());
    }
    // The folder name is the project name in the window. `.` has none until it is the full path.
    let project = &project
        .canonicalize()
        .with_context(|| format!("could not find {}", project.display()))?;
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

    let (control, mut engine) = Engine::new(OFFLINE);
    // A host with no folder to look in scans nothing and keeps no cache. A plugin of the
    // project shows as missing.
    let plugins = Plugins::listing(Vec::new(), ScanCommand::this_program()?, ScanCache::none());
    let (project, extensions) = open_with_extensions(&copy, control, plugins.clone())?;
    let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
    let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
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

    let mut run =
        |cx: &mut HeadlessAppContext, time| run(cx, &mut engine, &session, &plugins, time);
    run(&mut cx, Time::Real(SETTLE));
    for at in &before.clicks {
        click(&mut cx, window.into(), *at)?;
        run(&mut cx, Time::Real(SETTLE));
    }
    if let Some(time) = before.play {
        cx.update(|cx| session.update(cx, |session, _| session.engine().play()));
        run(&mut cx, Time::Played(time));
    }
    // A capture is of the last frame. Some views place themselves by what the frame before
    // measured, such as the add track row, and the app always draws another frame.
    cx.update_window(window.into(), |_, window, _| window.refresh())?;
    cx.run_until_parked();
    cx.capture_screenshot(window.into())?
        .save(png)
        .with_context(|| format!("could not write {}", png.display()))?;
    println!("wrote {}", png.display());
    println!(
        "{}",
        cx.update(|cx| crate::problems(session.read(cx).project()))
    );
    Ok(())
}

/// One click of the left button at `at`, as the pointer does it: it moves there first, so what
/// shows on hover is there to be clicked.
fn click(cx: &mut HeadlessAppContext, window: AnyWindowHandle, at: Point<Pixels>) -> Result<()> {
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    let events = [
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: None,
            modifiers,
        }),
        PlatformInput::MouseDown(MouseDownEvent {
            position: at,
            modifiers,
            button,
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            position: at,
            modifiers,
            button,
            click_count: 1,
        }),
    ];
    for event in events {
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(event, cx);
        })?;
        cx.run_until_parked();
    }
    Ok(())
}

/// How long the window runs: in real time, which Bun answers in, or until the engine played
/// past that much, which a poll on a busy machine takes longer for. Past it, because a picture
/// on a bar line would show the last step of the bar before under the first beat of the next:
/// the playhead is where the next block starts, and a sound has only said what it played.
#[derive(Clone, Copy)]
enum Time {
    Real(Duration),
    Played(Duration),
}

/// Runs the window for `time` as the app does, in polls of real time: the engine offline, the
/// background work of the extensions, the timers, and Bun.
fn run(
    cx: &mut HeadlessAppContext,
    engine: &mut Engine,
    session: &Entity<Session>,
    plugins: &Plugins,
    time: Time,
) {
    let frames = (OFFLINE.sample_rate as f32 * POLL_INTERVAL.as_secs_f32()) as usize;
    let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
    let mut scanned = plugins.scan_generation();
    let started = Instant::now();
    let mut played = Duration::ZERO;
    while match time {
        Time::Real(time) => started.elapsed() < time,
        Time::Played(time) => played <= time,
    } {
        played += POLL_INTERVAL;
        drum_pad::wait_for_sounds();
        cx.update(|cx| crate::window::tick(session, plugins, &mut scanned, cx));
        engine.process_block(&mut buffer);
        cx.advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
        std::thread::sleep(POLL_INTERVAL);
    }
    cx.run_until_parked();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_and_a_time_to_play_combine_and_what_is_no_place_or_time_is_refused() {
        let before = Before::parse(&["--click", "10,20.5", "--play", "1.5", "--click", "3,4"]);
        let expected = Before {
            clicks: vec![point(px(10.), px(20.5)), point(px(3.), px(4.))],
            play: Some(Duration::from_millis(1500)),
        };
        assert_eq!(before.unwrap(), expected);
        let refused: [&[&str]; 5] = [
            &["--play", "0"],
            &["--play", "soon"],
            &["--play"],
            &["--click", "3"],
            &["--wait", "2"],
        ];
        for flags in refused {
            assert!(Before::parse(flags).is_err(), "{flags:?}");
        }
    }
}
