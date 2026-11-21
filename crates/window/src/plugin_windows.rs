//! The plugins' own windows: a Rune window for each, with the plugin's view a child of its
//! native view. The plugin host says what it wants as commands and hears what happened; this
//! turns those into calls on a [`Desktop`], Rune's in the app and a recording one in tests,
//! because a run without a window opens none.

mod native;
mod rune_desktop;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use plugin_host::{
    Display, Key, KeyDirection, Keystroke, Modifiers, PluginProblem, PluginRecord, Plugins,
    WindowCommand, WindowId, WindowPlace, WindowSize,
};
use rune::{KeyEvent, WindowPlacement};
use sound_core::{InstanceId, Project, State as _};
use sound_typescript::window::WindowOpener;

pub(crate) use rune_desktop::RuneDesktop;

/// Rune has no moved callback: where each window is gets read this often, and as it closes.
const PLACE_EVERY: Duration = Duration::from_millis(500);

/// Why a plugin's window does not open, in the composer's words.
const NO_WINDOW: &str = "it has no window of its own";
const NOT_HERE: &str = "plugin windows do not open on Linux yet; the plugin plays";

/// A plugin's view goes in an `NSView` or an `HWND`, and Linux has neither.
const OPENS: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// The plugin host as its windows use it.
pub(crate) trait Host {
    fn settle(&self, app_is_active: bool) -> Vec<WindowCommand>;
    fn moved(&self, window: WindowId, place: WindowPlace);
    fn closed(&self, window: WindowId);
    fn key(&self, window: WindowId, keystroke: &Keystroke, direction: KeyDirection) -> bool;
    fn close_all(&self) -> Vec<WindowId>;
}

impl Host for Plugins {
    fn settle(&self, app_is_active: bool) -> Vec<WindowCommand> {
        self.settle_windows(app_is_active)
    }

    fn moved(&self, window: WindowId, place: WindowPlace) {
        self.window_moved(window, place);
    }

    fn closed(&self, window: WindowId) {
        self.window_closed(window);
    }

    fn key(&self, window: WindowId, keystroke: &Keystroke, direction: KeyDirection) -> bool {
        self.window_key(window, keystroke, direction)
    }

    fn close_all(&self) -> Vec<WindowId> {
        self.close_all_windows()
    }
}

/// What Rune does for these windows. One call is one call of Rune's.
pub(crate) trait Desktop {
    /// The first is the main one.
    fn displays(&self) -> Vec<Display>;
    /// Whether one of the app's windows has the keyboard.
    fn is_active(&self) -> bool;
    /// `rune::open_window`.
    fn open(&mut self, opening: Opening);
    /// `WindowHandle::resize_instant`, in points.
    fn resize(&mut self, window: WindowId, points: (f32, f32));
    fn focus(&mut self, window: WindowId);
    fn close(&mut self, window: WindowId);
    fn placement(&self, window: WindowId) -> Option<WindowPlacement>;
}

/// A window to open, in Rune's units.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Opening {
    pub(crate) window: WindowId,
    pub(crate) title: String,
    pub(crate) content: Content,
    pub(crate) resizable: bool,
    /// The frame's top left in physical desktop pixels; `None` in the middle.
    pub(crate) position: Option<(i32, i32)>,
    /// Without it, the window opens inactive and the keyboard stays where it was.
    pub(crate) focus: bool,
}

/// The content's size as Rune takes it: points, or physical pixels on Windows, where a plugin
/// counts those.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Content {
    Points(f32, f32),
    Physical(u32, u32),
}

/// What a plugin's window passes to the main one: the keys its plugin did not use. Every key up
/// passes, so a note the computer keys started ends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Spare {
    /// A key went down, or the OS sent it again as it is held: `repeat`.
    Down {
        key: KeyEvent,
        repeat: bool,
    },
    Up(KeyEvent),
    /// The keys that are down will not come up here: the window lost the keyboard, or ⌘ went
    /// down, under which macOS sends no key up.
    LetGo,
}

/// What the plugins' windows tell the main window between its frames.
#[derive(Default)]
pub(crate) struct Events {
    closed: RefCell<Vec<WindowId>>,
    spare: RefCell<Vec<Spare>>,
    wake: Arc<Mutex<Option<rune::Waker>>>,
}

impl Events {
    pub(crate) fn waking(wake: Arc<Mutex<Option<rune::Waker>>>) -> Self {
        Self {
            wake,
            ..Self::default()
        }
    }

    fn spare(&self, key: Spare) {
        self.spare.borrow_mut().push(key);
        self.wake();
    }

    fn wake(&self) {
        if let Some(wake) = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            wake();
        }
    }
}

/// A key typed in the plugin's `window` while its own view did not take it: the plugin may use
/// it, else it goes to the main window. `repeat`: the OS sent it again as it is held.
pub(crate) fn key(
    host: &impl Host,
    events: &Events,
    window: WindowId,
    event: &KeyEvent,
    (direction, repeat): (KeyDirection, bool),
) {
    let used = keystroke(event).is_some_and(|keystroke| host.key(window, &keystroke, direction));
    match direction {
        KeyDirection::Down if used => {}
        KeyDirection::Down => events.spare(Spare::Down {
            key: event.clone(),
            repeat,
        }),
        KeyDirection::Up => events.spare(Spare::Up(event.clone())),
    }
}

/// The composer closed the plugin's `window`, which was last at `place`. Before its native
/// view goes, so the plugin lets go of its view first.
pub(crate) fn closed_by_composer(
    host: &impl Host,
    events: &Events,
    window: WindowId,
    place: Option<WindowPlace>,
) {
    if let Some(place) = place {
        host.moved(window, place);
    }
    host.closed(window);
    events.closed.borrow_mut().push(window);
    events.wake();
}

/// The host's key of a key Rune heard; `None` for one no plugin takes, such as a modifier.
pub(crate) fn keystroke(event: &KeyEvent) -> Option<Keystroke> {
    use rune::Key as R;
    let key = match &event.key {
        R::Text(text) if text == " " => Key::Space,
        R::Text(text) => Key::Text(text.clone()),
        R::Command(' ') => Key::Space,
        R::Command(character) => Key::Text(character.to_string()),
        R::Enter => Key::Enter,
        R::Tab => Key::Tab,
        R::Backspace => Key::Backspace,
        R::Delete => Key::Delete,
        R::Escape => Key::Escape,
        R::Insert => Key::Insert,
        R::Home => Key::Home,
        R::End => Key::End,
        R::PageUp => Key::PageUp,
        R::PageDown => Key::PageDown,
        R::Left => Key::Left,
        R::Right => Key::Right,
        R::Up => Key::Up,
        R::Down => Key::Down,
        R::Function(number) => Key::Function(*number),
        _ => return None,
    };
    let mods = event.mods;
    let modifiers = Modifiers {
        shift: mods.shift,
        alt: mods.alt,
        command: mods.cmd,
        // Rune sets `ctrl` with `cmd` off macOS, where the host's `control` is the Windows key.
        control: if cfg!(target_os = "macos") {
            mods.ctrl
        } else {
            mods.logo
        },
    };
    Some(Keystroke { key, modifiers })
}

/// Where a window is, as the host takes it. `None` until Rune knows its display.
fn place_of(placement: &WindowPlacement, displays: &[Display]) -> Option<WindowPlace> {
    let (position, size) = placement.display?;
    let known =
        (displays.iter()).find(|display| display.position == position && display.size == size);
    let display = known.copied().unwrap_or(Display {
        id: None,
        position,
        size,
        scale: placement.scale,
    });
    Some(WindowPlace {
        position: placement.position,
        content_size: placement.content_size,
        display,
    })
}

/// Where Rune's `set_position` puts a window at `position`, which `displays` count, while the
/// window is at `scale`. macOS counts a place as points times the scale of the display it is
/// on, and turns one it is given into points with the window's own.
fn frame_at(position: (i32, i32), displays: &[Display], scale: f64) -> (i32, i32) {
    let inside = |display: &&Display| {
        let (x, y) = (
            position.0 - display.position.0,
            position.1 - display.position.1,
        );
        (0..display.size.0 as i32).contains(&x) && (0..display.size.1 as i32).contains(&y)
    };
    let on = displays
        .iter()
        .find(inside)
        .map_or(1.0, |display| display.scale);
    let side = |pixels: i32| (f64::from(pixels) / on * scale).round() as i32;
    (side(position.0), side(position.1))
}

/// The plugin's size as Rune opens a window of it.
fn content(size: WindowSize, physical: bool) -> Content {
    match physical {
        true => Content::Physical(size.width, size.height),
        false => Content::Points(size.width as f32, size.height as f32),
    }
}

/// The plugin's size in the points Rune resizes by, in a window of `scale`.
fn points(size: WindowSize, physical: bool, scale: f64) -> (f32, f32) {
    let scale = if physical && scale > 0.0 { scale } else { 1.0 };
    let side = |pixels: u32| (f64::from(pixels) / scale) as f32;
    (side(size.width), side(size.height))
}

/// The plugins' windows open now, as the main window keeps them.
pub(crate) struct PluginWindows<D> {
    desktop: D,
    events: Rc<Events>,
    /// A plugin counts physical pixels: Windows.
    physical: bool,
    open: BTreeMap<WindowId, Shown>,
    /// When the places were last read, in seconds of the window's clock.
    placed_at: Option<f64>,
}

struct Shown {
    title: String,
    /// As the host was last told.
    place: Option<WindowPlace>,
}

impl<D: Desktop> PluginWindows<D> {
    pub(crate) fn new(desktop: D, events: Rc<Events>, physical: bool) -> Self {
        Self {
            desktop,
            events,
            physical,
            open: BTreeMap::new(),
            placed_at: None,
        }
    }

    /// After every poll of the host: what its plugins' windows need, and where they went.
    pub(crate) fn settle(&mut self, host: &impl Host, seconds: f64) {
        for window in self.events.closed.take() {
            self.open.remove(&window);
        }
        for command in host.settle(self.desktop.is_active()) {
            self.obey(command);
        }
        let due = self
            .placed_at
            .is_none_or(|at| seconds - at >= PLACE_EVERY.as_secs_f64());
        if due && !self.open.is_empty() {
            self.placed_at = Some(seconds);
            self.tell_places(host);
        }
    }

    fn obey(&mut self, command: WindowCommand) {
        match command {
            WindowCommand::Open {
                window,
                title,
                size,
                resizable,
                placement,
                focus,
            } => {
                let displays = self.desktop.displays();
                self.desktop.open(Opening {
                    window,
                    title: title.clone(),
                    content: content(size, self.physical),
                    resizable,
                    position: placement.and_then(|placement| placement.position_on(&displays)),
                    focus,
                });
                self.open.insert(window, Shown { title, place: None });
            }
            WindowCommand::Resize { window, size } => {
                let scale = self.desktop.placement(window).map_or(1.0, |at| at.scale);
                self.desktop
                    .resize(window, points(size, self.physical, scale));
            }
            WindowCommand::Focus { window } => self.desktop.focus(window),
            WindowCommand::Close { window } => {
                self.open.remove(&window);
                self.desktop.close(window);
            }
        }
    }

    /// Tells the host where each window is, when that changed.
    fn tell_places(&mut self, host: &impl Host) {
        let displays = self.desktop.displays();
        for (window, shown) in &mut self.open {
            let place = (self.desktop.placement(*window)).and_then(|at| place_of(&at, &displays));
            if let Some(place) = place
                && shown.place != Some(place)
            {
                shown.place = Some(place);
                host.moved(*window, place);
            }
        }
    }

    /// For the app quitting or its main window closing: every plugin lets go of its view before
    /// its window goes, and they stay open for next time.
    pub(crate) fn close_all(&mut self, host: &impl Host) {
        for window in host.close_all() {
            self.open.remove(&window);
            self.desktop.close(window);
        }
    }

    pub(crate) fn take_spare_keys(&self) -> Vec<Spare> {
        self.events.spare.take()
    }

    /// The titles of the windows open now, for `--tree`.
    pub(crate) fn titles(&self) -> Vec<String> {
        self.open
            .values()
            .map(|shown| shown.title.clone())
            .collect()
    }
}

/// Opens the window of plugin `id`, or brings it forward: for code's `openWindow` and the card.
pub(crate) fn open(plugins: &Plugins, project: &Project, id: &InstanceId) -> Result<(), String> {
    if project.tool_of(id) != Some(PluginRecord::TOOL) {
        return Err(NO_WINDOW.to_string());
    }
    if !OPENS {
        return Err(NOT_HERE.to_string());
    }
    plugins.open_window(id).map_err(|problem| match problem {
        PluginProblem::NoWindow { .. } => NO_WINDOW.to_string(),
        other => other.to_string(),
    })
}

/// Where code's `openWindow` goes.
pub(crate) fn opener(plugins: Plugins) -> WindowOpener {
    Box::new(move |project, id| open(&plugins, project, id))
}
