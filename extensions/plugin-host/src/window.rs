//! A plugin's own window, from the host's side. The app makes and takes down the real window:
//! the host says what it wants as [`WindowCommand`]s and hears what happened through the
//! `Plugins::window_*` calls. So the host knows no toolkit, and never calls one while it holds
//! its table. Nothing here knows a plugin format; that is [`crate::backend::PluginGui`].
//!
//! A plugin in a Win32 window counts its size in physical pixels, as both formats say for
//! Win32; Cocoa counts points. A [`WindowSize`] is in the plugin's pixels.
//!
//! Both formats call a plugin's window on the main thread only, except CLAP's
//! `clap_host_gui.closed`, which may come from any thread and only sets a flag the next poll
//! reads.

use std::ffi::c_void;
use std::ptr::NonNull;

use raw_window_handle::RawWindowHandle;
use serde::{Deserialize, Serialize};

use crate::PluginProblem;
use crate::backend::PluginGui;

/// In the plugin's own pixels: physical on Windows, points elsewhere. The app opens a window
/// of this content size, with `physical_size` on Windows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// Only so that a window is never zero-sized: every plugin with a window says its size.
const DEFAULT_SIZE: WindowSize = WindowSize {
    width: 600,
    height: 400,
};

/// One window the app makes for a plugin. A new one each time a plugin's window opens, so an
/// event of a window that went can never reach the next one.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// One more than `made`, which counts them.
    pub(crate) fn next(made: &mut u64) -> Self {
        *made += 1;
        Self(*made)
    }

    /// Unique for the life of the host, for a key of the app's own.
    pub fn number(self) -> u64 {
        self.0
    }
}

/// What the host wants of the app's windows, from `Plugins::settle_windows`.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowCommand {
    /// A window for a plugin's view, without a minimum size of the app's own. `placement` is
    /// where it was, see [`Placement::position_on`]. Without `focus` it came back by itself (the
    /// project opened, its plugin reloaded): it opens without the keyboard, and the keyboard
    /// goes back to whatever had it if the plugin takes it as it is shown.
    Open {
        window: WindowId,
        title: String,
        size: WindowSize,
        resizable: bool,
        placement: Option<Placement>,
        focus: bool,
    },
    /// The content takes this size, without an animation.
    Resize { window: WindowId, size: WindowSize },
    /// Brings an open window forward, with the keyboard.
    Focus { window: WindowId },
    /// The plugin has let go of its view already.
    Close { window: WindowId },
}

/// Where a plugin's window was and whether it was open, as this machine keeps it.
///
/// `x` and `y` are the top left corner, title bar included, in points from the top left of its
/// display: the unit the files of earlier versions hold, so they read the same. [`Self::at`]
/// and [`Self::position_on`] convert from and to the app's physical desktop pixels. No size: a
/// plugin keeps its own in its state, and a second one here could only disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub(crate) open: bool,
    pub(crate) x: i32,
    pub(crate) y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) display: Option<u64>,
}

/// A display as the app sees it, in physical desktop pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// The system's own id, such as macOS's `CGDirectDisplayID`. `None` when the app cannot
    /// tell displays apart.
    pub id: Option<u64>,
    pub position: (i32, i32),
    pub size: (u32, u32),
    /// Physical pixels per point.
    pub scale: f64,
}

impl Display {
    fn scale(&self) -> f64 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }
}

/// Where a plugin's window is now, as the app reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlace {
    /// The frame's top left, title bar included, in physical desktop pixels.
    pub position: (i32, i32),
    /// Of the content, the plugin's view, in physical pixels.
    pub content_size: (u32, u32),
    /// The one with the most of the window.
    pub display: Display,
}

/// How much of a window has to be on its display for it to come back there: enough to take it
/// by its title bar.
const VISIBLE: i32 = 40;

fn reachable(x: i32, y: i32, width: f64, height: f64) -> bool {
    let (width, height) = (width as i32, height as i32);
    (0..=width - VISIBLE).contains(&x) && (0..=height - VISIBLE).contains(&y)
}

impl Placement {
    /// Of an open window whose frame's top left is at `position` on `display`.
    pub(crate) fn at(position: (i32, i32), display: &Display) -> Self {
        let points = |physical: i32, origin: i32| {
            (f64::from(physical - origin) / display.scale()).round() as i32
        };
        Self {
            open: true,
            x: points(position.0, display.position.0),
            y: points(position.1, display.position.1),
            display: display.id,
        }
    }

    /// Where to open the window, in physical desktop pixels: where it was, when that display is
    /// among `displays` and the window's title bar can be reached on it. `None` for the middle
    /// of the main display, which is the first of `displays`.
    pub fn position_on(&self, displays: &[Display]) -> Option<(i32, i32)> {
        let display = match self.display {
            Some(id) => displays.iter().find(|display| display.id == Some(id)),
            None => displays.first(),
        }?;
        let scale = display.scale();
        let (width, height) = (
            f64::from(display.size.0) / scale,
            f64::from(display.size.1) / scale,
        );
        let physical =
            |points: i32, origin: i32| origin + (f64::from(points) * scale).round() as i32;
        reachable(self.x, self.y, width, height).then(|| {
            (
                physical(self.x, display.position.0),
                physical(self.y, display.position.1),
            )
        })
    }
}

/// The native view of a window the app made for a plugin, which the plugin's view goes in.
#[derive(Clone, Copy, Debug)]
pub struct ParentView {
    /// An `AppKit` one or a `Win32` one; any other counts as none.
    pub handle: RawWindowHandle,
    /// Physical pixels per point on the window's display.
    pub scale: f64,
}

impl ParentView {
    fn view(&self) -> Option<NonNull<c_void>> {
        match self.handle {
            RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
            RawWindowHandle::Win32(handle) => NonNull::new(handle.hwnd.get() as *mut c_void),
            _ => None,
        }
    }
}

/// A key typed in a plugin's window while the plugin's own view did not have the keyboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keystroke {
    pub key: Key,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// What the key typed, shift included. With the command key held, which types nothing,
    /// the key's own character.
    Text(String),
    Space,
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    /// F1 is 1.
    Function(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    /// Command on macOS, Control elsewhere.
    pub command: bool,
    /// Control on macOS, the Windows key on Windows.
    pub control: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyDirection {
    Down,
    Up,
}

pub(crate) enum Prepared {
    AlreadyOpen(WindowId),
    /// The window to open, at the plugin's size.
    Opening(WindowSize),
}

/// The plugin's window, from the host's side. What the plugin makes and frees for it is the
/// backend's.
#[derive(Default)]
pub(crate) struct PluginWindow {
    /// From the moment it is asked for until the plugin lets go of its view.
    open: Option<WindowId>,
    /// The plugin's view counts physical pixels: it is in a Win32 window.
    physical: bool,
    /// A size the plugin asked for, until [`Self::take_wanted_size`] gives it to the app.
    wanted_size: Option<WindowSize>,
    /// How big the plugin's view is, as far as the host knows. A window whose content has this
    /// size has nothing to tell the plugin.
    size: Option<WindowSize>,
    resizable: bool,
}

impl PluginWindow {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub(crate) fn is(&self, window: WindowId) -> bool {
        self.open == Some(window)
    }

    /// The plugin asked its window to be this big, as it opens or when its interface changes.
    pub(crate) fn wants_size(&mut self, wanted: WindowSize) {
        self.wanted_size = Some(wanted);
        self.size = Some(wanted);
    }

    /// The window's content is `place` now. In a resizable window that is the composer
    /// dragging its edge: the plugin settles on a size near it, which the window takes at the
    /// next settle. Anything else is the window taking a size this host gave it.
    pub(crate) fn resized(&mut self, gui: &mut dyn PluginGui, place: &WindowPlace) {
        let content = self.plugin_size(place);
        if !self.resizable || self.size == Some(content) {
            return;
        }
        let settled = gui.resize(content).or(self.size).unwrap_or(content);
        self.size = Some(settled);
        if settled != content {
            self.wanted_size = Some(settled);
        }
    }

    fn plugin_size(&self, place: &WindowPlace) -> WindowSize {
        let scale = match self.physical {
            true => 1.0,
            false => place.display.scale(),
        };
        let side = |physical: u32| (f64::from(physical) / scale).round().max(1.0) as u32;
        WindowSize {
            width: side(place.content_size.0),
            height: side(place.content_size.1),
        }
    }

    /// The window and the size its plugin last asked for, once.
    #[must_use]
    pub(crate) fn take_wanted_size(&mut self) -> Option<(WindowId, WindowSize)> {
        let window = self.open?;
        Some((window, self.wanted_size.take()?))
    }

    /// The first step of opening a window: everything the plugin has to say before the app
    /// makes `window`. [`Self::attach`] is the last.
    ///
    /// Both formats ask this order for an embedded window: create, ask how big, set the parent,
    /// show. The scale is not set here: both formats say not to on Cocoa, where sizes are
    /// already points, and on Windows it is not known until the window is on its display.
    pub(crate) fn prepare(
        &mut self,
        gui: &mut dyn PluginGui,
        window: WindowId,
    ) -> Result<Prepared, PluginProblem> {
        if let Some(open) = self.open {
            return Ok(Prepared::AlreadyOpen(open));
        }
        // From here the plugin holds resources for a window, until `give_up` frees them.
        gui.create()?;
        let size = gui.size().unwrap_or(DEFAULT_SIZE);
        self.size = Some(size);
        self.resizable = gui.can_resize();
        self.open = Some(window);
        Ok(Prepared::Opening(size))
    }

    pub(crate) fn resizable(&self) -> bool {
        self.resizable
    }

    /// The plugin fills the window the app made for it. On a failure the caller gives up.
    ///
    /// On Windows the plugin is told the window's scale first, as both formats ask there. Its
    /// sizes stay physical pixels whether it takes the scale or reads it from the system.
    ///
    /// # Safety
    ///
    /// `parent` is the view of this window, alive until [`Self::give_up`] has run.
    pub(crate) unsafe fn attach(
        &mut self,
        gui: &mut dyn PluginGui,
        parent: Option<ParentView>,
    ) -> Result<(), PluginProblem> {
        // No parent is a run without windows: the plugin is shown with nothing to draw in, so
        // tests can check the rest of its life without a display.
        if let Some(parent) = parent
            && let Some(view) = parent.view()
        {
            if matches!(parent.handle, RawWindowHandle::Win32(_)) {
                self.physical = true;
                gui.set_scale(parent.scale);
                if let Some(scaled) = gui.size()
                    && self.size != Some(scaled)
                {
                    self.size = Some(scaled);
                    self.wanted_size = Some(scaled);
                }
            }
            // SAFETY: the caller keeps the view alive until `give_up`, which frees the plugin's
            // resources for it.
            unsafe { gui.set_parent(view) }?;
        }
        gui.show()
    }

    /// Frees whatever the plugin holds for a window and gives back the window, for the app to
    /// take down. The plugin goes on playing.
    #[must_use]
    pub(crate) fn give_up(&mut self, gui: Option<&mut dyn PluginGui>) -> Option<WindowId> {
        self.wanted_size = None;
        self.size = None;
        self.physical = false;
        if let Some(gui) = gui {
            gui.destroy();
        }
        self.open.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_comes_back_only_where_its_title_bar_can_be_reached() {
        assert!(reachable(0, 0, 1440.0, 900.0));
        assert!(reachable(1400, 860, 1440.0, 900.0));
        assert!(!reachable(1401, 100, 1440.0, 900.0));
        assert!(!reachable(100, 861, 1440.0, 900.0));
        assert!(!reachable(-1, 100, 1440.0, 900.0));
        assert!(!reachable(100, -1, 1440.0, 900.0));
        assert!(!reachable(2400, 100, 1440.0, 900.0));
    }

    #[test]
    fn a_placement_reads_as_it_is_written() {
        let placement = Placement {
            open: true,
            x: 120,
            y: 80,
            display: Some(1),
        };
        let text = serde_json::to_string(&placement).expect("it writes");
        assert_eq!(text, r#"{"open":true,"x":120,"y":80,"display":1}"#);
        assert_eq!(
            serde_json::from_str::<Placement>(r#"{"open":false,"x":1,"y":2}"#).expect("it reads"),
            Placement {
                open: false,
                x: 1,
                y: 2,
                display: None
            }
        );
    }

    /// A display of 1440 by 900 points at twice the pixels, right of a main one.
    const RETINA: Display = Display {
        id: Some(2),
        position: (3840, 0),
        size: (2880, 1800),
        scale: 2.0,
    };

    const MAIN: Display = Display {
        id: Some(1),
        position: (0, 0),
        size: (1920, 1080),
        scale: 1.0,
    };

    /// Files of earlier versions hold points from the display's corner, and the app counts
    /// physical pixels of the whole desktop.
    #[test]
    fn a_placement_is_kept_in_points_of_its_display_and_opens_in_pixels_of_the_desktop() {
        let kept = Placement::at((3840 + 200, 120), &RETINA);
        assert_eq!(
            kept,
            Placement {
                open: true,
                x: 100,
                y: 60,
                display: Some(2)
            }
        );
        assert_eq!(kept.position_on(&[MAIN, RETINA]), Some((4040, 120)));
        let older: Placement =
            serde_json::from_str(r#"{"open":true,"x":100,"y":60,"display":2}"#).expect("it reads");
        assert_eq!(older.position_on(&[MAIN, RETINA]), Some((4040, 120)));
        // Without a display it was on the main one.
        let anywhere = Placement {
            display: None,
            ..kept
        };
        assert_eq!(anywhere.position_on(&[MAIN, RETINA]), Some((100, 60)));
    }

    #[test]
    fn a_window_off_its_display_or_on_one_that_is_gone_opens_in_the_middle() {
        // 1400 points is reachable on a display 1440 wide, which is 2880 pixels.
        let edge = Placement {
            open: true,
            x: 1400,
            y: 0,
            display: Some(2),
        };
        assert_eq!(edge.position_on(&[MAIN, RETINA]), Some((3840 + 2800, 0)));
        let off = Placement { x: 1401, ..edge };
        assert_eq!(off.position_on(&[MAIN, RETINA]), None);
        let gone = Placement { x: 100, ..edge };
        assert_eq!(gone.position_on(&[MAIN]), None);
    }
}
