//! `--look`, `--screenshot`, `--click` and the other steps: the window of a project as it opens,
//! drawn offscreen with the project's own TypeScript tools, driven by name, so an agent sees
//! and uses the cards and pages it builds without a picture. Each step names a part as `--look`
//! lists it, such as `button-play` or `shape-clip-t01/clip`.
//!
//! It opens a copy of the folder, because opening a project writes into it; a folder of
//! `templates/` opens as a new project of it would. It plays no sound,
//! since the engine runs offline, and writes nothing to the app's support folder: no agent
//! panel, no plugin scan, no sampler library.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use gpui::{
    AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point,
    ScrollDelta, ScrollEvent, TouchPhase, WindowHandle, point, px, size,
};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use sound_core::Engine;
use sound_ui::on_screen::{self, Seen};
use sound_ui::{Assets, POLL_INTERVAL, Session};

use crate::window::{Shell, WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::{OFFLINE, open_with_extensions, views};

/// How long the window runs at most after it opens and after each step, for Bun to answer: a
/// page with a control loop moves for a while, as it does when the app opens.
const SETTLE: Duration = Duration::from_millis(500);
/// How long a hover rests the pointer: past the delay of a tooltip.
const REST: Duration = Duration::from_millis(600);

/// What happens to the window, in order.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Prints what the window shows, one part a line.
    Look,
    Screenshot(PathBuf),
    Click(Part),
    DoubleClick(Part),
    RightClick(Part),
    Hover(Part),
    /// A press on the part, a move in steps, and a release at the other part or that far.
    Drag(Part, To),
    /// A wheel or two fingers over the part, in points.
    Scroll(Part, Point<Pixels>),
    /// Keys such as `cmd-z` or `space`, several apart by spaces.
    Key(String),
    Type(String),
    Play(Duration),
}

/// A part by its name, at its middle or at a place in it.
#[derive(Debug, PartialEq)]
pub struct Part {
    name: String,
    /// `name@x,y`: in the units of a space, or of the space of a shape; in points from the top
    /// left of anything else.
    at: Option<(f64, f64)>,
}

#[derive(Debug, PartialEq)]
pub enum To {
    Part(Part),
    By(Point<Pixels>),
}

impl Step {
    /// Whether `flag` starts a step, so the arguments are steps.
    pub fn starts(flag: &str) -> bool {
        matches!(
            flag,
            "--look"
                | "--screenshot"
                | "--click"
                | "--double-click"
                | "--right-click"
                | "--hover"
                | "--drag"
                | "--scroll"
                | "--key"
                | "--type"
                | "--play"
        )
    }

    pub fn parse(flags: &[&str]) -> Result<Vec<Self>> {
        let mut steps = Vec::new();
        let mut rest = flags;
        while let [flag, after @ ..] = rest {
            let (step, taken) = match (*flag, after) {
                ("--look", _) => (Self::Look, 0),
                ("--screenshot", [png, ..]) => (Self::Screenshot(PathBuf::from(png)), 1),
                ("--click", [part, ..]) => (Self::Click(Part::parse(part)?), 1),
                ("--double-click", [part, ..]) => (Self::DoubleClick(Part::parse(part)?), 1),
                ("--right-click", [part, ..]) => (Self::RightClick(Part::parse(part)?), 1),
                ("--hover", [part, ..]) => (Self::Hover(Part::parse(part)?), 1),
                ("--drag", [from, to, ..]) => {
                    let to = match pair(to) {
                        Some((dx, dy)) => To::By(point(px(dx as f32), px(dy as f32))),
                        None => To::Part(Part::parse(to)?),
                    };
                    (Self::Drag(Part::parse(from)?, to), 2)
                }
                ("--scroll", [part, by, ..]) => {
                    let (dx, dy) = pair(by).with_context(|| {
                        format!("--scroll takes a part and points as dx,dy, not {by}")
                    })?;
                    let by = point(px(dx as f32), px(dy as f32));
                    (Self::Scroll(Part::parse(part)?, by), 2)
                }
                ("--key", [keys, ..]) => {
                    for key in keys.split(' ') {
                        Keystroke::parse(key).map_err(|_| {
                            anyhow::anyhow!("--key takes keys such as cmd-z, not {key}")
                        })?;
                    }
                    (Self::Key(keys.to_string()), 1)
                }
                ("--type", [text, ..]) => (Self::Type(text.to_string()), 1),
                ("--play", [seconds, ..]) => {
                    let played = seconds.parse().ok().filter(|seconds| *seconds > 0.0);
                    let seconds: f32 = played.with_context(|| {
                        format!("--play takes seconds, such as 2, not {seconds}")
                    })?;
                    (Self::Play(Duration::try_from_secs_f32(seconds)?), 1)
                }
                _ => bail!("{flag} is no step, or misses what it takes: see sound-tools --help"),
            };
            steps.push(step);
            rest = after.get(taken..).unwrap_or_default();
        }
        Ok(steps)
    }
}

impl Part {
    fn parse(text: &str) -> Result<Self> {
        let (name, at) = match text.rsplit_once('@') {
            Some((name, at)) => {
                let at = pair(at).with_context(|| format!("a place in {name} is x,y, not {at}"))?;
                (name, Some(at))
            }
            None => (text, None),
        };
        if name.is_empty() {
            bail!("a step names a part, as --look lists it, such as button-play");
        }
        Ok(Self {
            name: name.to_string(),
            at,
        })
    }

    /// Where it is in the window, as the window shows now.
    fn point(&self, seen: &[Seen]) -> Result<Point<Pixels>> {
        let Some(found) = on_screen::find(seen, &self.name) else {
            let shown = on_screen::listing(seen);
            bail!("the window shows no {}. It shows:\n{shown}", self.name);
        };
        if !found.shows() {
            bail!("{} is drawn, but scrolled out of view", self.name);
        }
        Ok(match (self.at, found.units) {
            (None, _) => found.middle(),
            (Some(at), Some(units)) => units.point(at),
            (Some((x, y)), None) => found.whole.origin + point(px(x as f32), px(y as f32)),
        })
    }
}

fn pair(text: &str) -> Option<(f64, f64)> {
    let (x, y) = text.split_once(',')?;
    let number = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
    };
    Some((number(x)?, number(y)?))
}

pub fn run(project: &Path, steps: &[Step]) -> Result<()> {
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
    crate::templates::open_as_project(&copy)?;

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
    let mut driven = Driven {
        cx,
        window,
        engine,
        session,
        plugins,
    };
    driven.run(Time::Settle);
    for step in steps {
        driven.step(step)?;
    }
    println!(
        "{}",
        (driven.cx).update(|cx| crate::problems(driven.session.read(cx).project()))
    );
    Ok(())
}

/// The window and what plays it.
struct Driven {
    cx: HeadlessAppContext,
    window: WindowHandle<Shell>,
    engine: Engine,
    session: Entity<Session>,
    plugins: Plugins,
}

impl Driven {
    fn step(&mut self, step: &Step) -> Result<()> {
        let (left, right) = (MouseButton::Left, MouseButton::Right);
        match step {
            Step::Look => print!("{}", on_screen::listing(&self.look()?)),
            Step::Screenshot(png) => self.screenshot(png)?,
            Step::Click(part) => self.click(part, left, 1)?,
            Step::DoubleClick(part) => {
                self.click(part, left, 1)?;
                self.click(part, left, 2)?;
            }
            Step::RightClick(part) => self.click(part, right, 1)?,
            Step::Hover(part) => {
                let at = self.point(part)?;
                self.move_to(at, None)?;
                self.cx.advance_clock(REST);
            }
            Step::Drag(from, to) => {
                let from = self.point(from)?;
                self.move_to(from, None)?;
                self.send(MouseDownEvent {
                    position: from,
                    modifiers: Modifiers::default(),
                    button: left,
                    click_count: 1,
                    first_mouse: false,
                })?;
                let to = match to {
                    To::Part(part) => self.point(part)?,
                    To::By(by) => from + *by,
                };
                // In steps, as a hand moves, each answered.
                for step in 1..=8 {
                    let at = from + (to - from) * (step as f32 / 8.);
                    self.move_to(at, Some(left))?;
                    self.run(Time::Settle);
                }
                self.send(MouseUpEvent {
                    position: to,
                    modifiers: Modifiers::default(),
                    button: left,
                    click_count: 1,
                })?;
            }
            Step::Scroll(part, by) => {
                let at = self.point(part)?;
                self.move_to(at, None)?;
                self.send(ScrollEvent {
                    position: at,
                    delta: ScrollDelta::Pixels(*by),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                    ..ScrollEvent::default()
                })?;
            }
            Step::Key(keys) => {
                for key in keys.split(' ') {
                    self.keystroke(Keystroke::parse(key)?)?;
                }
            }
            Step::Type(text) => {
                for character in text.chars() {
                    let mut keystroke = Keystroke::parse(&character.to_string())?;
                    keystroke.key_char = Some(character.to_string());
                    self.keystroke(keystroke)?;
                }
            }
            Step::Play(time) => {
                let session = self.session.clone();
                (self.cx).update(|cx| session.update(cx, |session, _| session.engine().play()));
                self.run(Time::Played(*time));
            }
        }
        self.run(Time::Settle);
        Ok(())
    }

    fn look(&mut self) -> Result<Vec<Seen>> {
        (self.cx).update_window(self.window.into(), |_, window, cx| {
            on_screen::look(window, cx)
        })
    }

    fn point(&mut self, part: &Part) -> Result<Point<Pixels>> {
        let seen = self.look()?;
        part.point(&seen)
    }

    fn screenshot(&mut self, png: &Path) -> Result<()> {
        // A capture is of the last frame. Some views place themselves by what the frame before
        // measured, such as the add track row, and the app always draws another frame.
        (self.cx).update_window(self.window.into(), |_, window, _| window.refresh())?;
        self.cx.run_until_parked();
        (self.cx.capture_screenshot(self.window.into())?)
            .save(png)
            .with_context(|| format!("could not write {}", png.display()))?;
        println!("wrote {}", png.display());
        Ok(())
    }

    /// A click as the pointer does it: it moves there first, so what shows on hover is there to
    /// be clicked.
    fn click(&mut self, part: &Part, button: MouseButton, click_count: usize) -> Result<()> {
        let at = self.point(part)?;
        self.move_to(at, None)?;
        let modifiers = Modifiers::default();
        self.send(MouseDownEvent {
            position: at,
            modifiers,
            button,
            click_count,
            first_mouse: false,
        })?;
        self.send(MouseUpEvent {
            position: at,
            modifiers,
            button,
            click_count,
        })
    }

    fn move_to(&mut self, at: Point<Pixels>, pressed: Option<MouseButton>) -> Result<()> {
        self.send(MouseMoveEvent {
            position: at,
            pressed_button: pressed,
            modifiers: Modifiers::default(),
        })
    }

    fn send(&mut self, event: impl gpui::InputEvent) -> Result<()> {
        let event: PlatformInput = event.to_platform_input();
        let window: AnyWindowHandle = self.window.into();
        (self.cx).update_window(window, |_, window, cx| {
            window.dispatch_event(event, cx);
        })?;
        self.cx.run_until_parked();
        Ok(())
    }

    fn keystroke(&mut self, keystroke: Keystroke) -> Result<()> {
        (self.cx).update_window(self.window.into(), |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        })?;
        self.cx.run_until_parked();
        Ok(())
    }

    /// Runs the window as the app does, in polls of real time: the engine offline, the
    /// background work of the extensions, the timers, and Bun.
    fn run(&mut self, time: Time) {
        let frames = (OFFLINE.sample_rate as f32 * POLL_INTERVAL.as_secs_f32()) as usize;
        let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
        let mut scanned = self.plugins.scan_generation();
        let started = Instant::now();
        let mut played = Duration::ZERO;
        let mut polls = 0;
        loop {
            let done = match time {
                Time::Played(time) => played > time,
                // Two polls at least: what a step changed reaches Bun in the first, and its
                // answer is drawn in a later one.
                Time::Settle => started.elapsed() >= SETTLE || (polls >= 2 && self.ready()),
            };
            if done {
                break;
            }
            played += POLL_INTERVAL;
            polls += 1;
            drum_pad::wait_for_sounds();
            let (session, plugins) = (&self.session, &self.plugins);
            (self.cx).update(|cx| crate::window::tick(session, plugins, &mut scanned, cx));
            self.engine.process_block(&mut buffer);
            self.cx.advance_clock(POLL_INTERVAL);
            self.cx.run_until_parked();
            std::thread::sleep(POLL_INTERVAL);
        }
        self.cx.run_until_parked();
    }

    /// Whether the page heard all Bun had to say, with nothing left for Bun to answer.
    fn ready(&mut self) -> bool {
        let shell = self.window;
        (self.cx)
            .update(|cx| {
                let page = shell.read(cx)?.main_view().cloned();
                anyhow::Ok(page.is_none_or(|page| sound_typescript::page_ready(&page, cx)))
            })
            .unwrap_or(true)
    }
}

/// How long the window runs: until Bun answered, or until the engine played past that much,
/// which a poll on a busy machine takes longer for. Past it, because a picture on a bar line
/// would show the last step of the bar before under the first beat of the next: the playhead
/// is where the next block starts, and a sound has only said what it played.
#[derive(Clone, Copy)]
enum Time {
    Settle,
    Played(Duration),
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
    fn steps_name_parts_and_what_is_no_step_or_no_part_is_refused() {
        let steps = Step::parse(&[
            "--click",
            "button-play",
            "--drag",
            "shape-clip-t00/c00",
            "40,0",
            "--drag",
            "space-timeline@3840,0.5",
            "button-play",
            "--look",
        ]);
        let part = |name: &str, at| Part {
            name: name.into(),
            at,
        };
        let expected = vec![
            Step::Click(part("button-play", None)),
            Step::Drag(
                part("shape-clip-t00/c00", None),
                To::By(point(px(40.), px(0.))),
            ),
            Step::Drag(
                part("space-timeline", Some((3840., 0.5))),
                To::Part(part("button-play", None)),
            ),
            Step::Look,
        ];
        assert_eq!(steps.unwrap(), expected);
        let refused: [&[&str]; 6] = [
            &["--play", "0"],
            &["--play"],
            &["--click"],
            &["--click", "@1,2"],
            &["--click", "space-roll@1"],
            &["--wait", "2"],
        ];
        for flags in refused {
            assert!(Step::parse(flags).is_err(), "{flags:?}");
        }
    }
}
