//! The plugin's own window: a window of the application with the plugin's own view inside it.
//!
//! One GPUI window per open plugin, beside the main one, with an empty root view, as big as
//! the plugin asked for. The plugin's view is a child of that window's view and draws over it.
//! It floats: it stays above the main window, and it hides while another application is in
//! front, as a panel of this application. Where it sat and whether it was open are kept in
//! this machine's store, see `placements.rs`; neither is part of the piece.
//!
//! Nothing here knows a plugin format. What a plugin has to do for a window is
//! [`crate::backend::PluginGui`], which both backends fill in: `clap.rs` with the GUI
//! extension, `vst3/view.rs` with `IPlugView` and an `IPlugFrame`. Neither format needed
//! anything of this file.
//!
//! Every call of a plugin's window belongs to the main thread, which is where [`crate::Plugins`]
//! lives, and both formats say so. The one callback that does not is CLAP's
//! `clap_host_gui.closed`, which a plugin may make from any thread; it only sets a flag that
//! the next poll reads.

use std::ffi::c_void;
use std::ptr::NonNull;

use gpui::{
    App, Bounds, Context, DisplayId, FocusHandle, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke,
    Pixels, Render, Size, Subscription, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowId, WindowKind, WindowOptions, div, point, prelude::*, px, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use sound_core::InstanceId;

use crate::PluginProblem;
use crate::backend::{KeyDirection, PluginGui};
use crate::host::WeakPlugins;

/// How big a plugin's window is, in logical pixels. The formats each have a type of their own
/// for this and they say the same thing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// How big a plugin's window is when the plugin does not say. Every plugin with a window
/// answers when it is asked; this is only so that a window is never zero-sized.
const DEFAULT_SIZE: WindowSize = WindowSize {
    width: 600,
    height: 400,
};

/// Where a plugin's window was and whether it was open, as this machine keeps it.
///
/// `x` and `y` are the top left corner of the window, title bar included, in logical pixels
/// from the top left of the display it was on; `display` is that display's id, when it had one.
/// A window that comes back where it was is as big as its plugin says then: a plugin keeps its
/// own size in its own state, and a second size here could only disagree with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Placement {
    pub open: bool,
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<u64>,
}

/// How much of a window has to be on its display for it to come back there: enough to take it
/// by its title bar. A window saved on a display that is not there any more, or that is smaller
/// now, is put in the middle of the main one instead.
const VISIBLE: i32 = 40;

/// Whether a window whose corner is at `x`, `y` on a display of `width` by `height` can be
/// reached there.
fn reachable(x: i32, y: i32, width: f32, height: f32) -> bool {
    let (width, height) = (width as i32, height as i32);
    (0..=width - VISIBLE).contains(&x) && (0..=height - VISIBLE).contains(&y)
}

/// The root view of a plugin's window. It draws nothing: the plugin's own view is in the same
/// window and covers it. It is what hears the window move and resize, and what gets a key while
/// the plugin's own view does not have the keyboard.
pub struct PluginFrame {
    owner: WindowOwner,
    /// The window's own focus, which nothing else in it takes. GPUI gives a key to what has
    /// the focus and to what is around it, so this is what makes the frame hear keys.
    focus: FocusHandle,
    _bounds: Subscription,
}

impl PluginFrame {
    fn new(owner: WindowOwner, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A move and a resize both land here, from the composer or from us.
        let bounds = cx.observe_window_bounds(window, |frame, window, cx| {
            let Some(plugins) = frame.owner.plugins.upgrade() else {
                return;
            };
            let (placement, content) = (placement_of(window, cx), content_of(window));
            // GPUI's own handle, not the one `HasWindowHandle` gives.
            let id = Window::window_handle(window).window_id();
            plugins.window_bounds_changed(&frame.owner.instance, id, placement, content);
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            owner,
            focus,
            _bounds: bounds,
        }
    }

    /// Gives a key to the plugin. `true` says it used it, and then nothing else gets it.
    fn key(&self, window: &Window, keystroke: &Keystroke, direction: KeyDirection) -> bool {
        let Some(plugins) = self.owner.plugins.upgrade() else {
            return false;
        };
        // GPUI's own handle, not the one `HasWindowHandle` gives.
        let id = Window::window_handle(window).window_id();
        plugins.key(&self.owner.instance, id, keystroke, direction)
    }
}

impl Render for PluginFrame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|frame, event: &KeyDownEvent, window, cx| {
                if frame.key(window, &event.keystroke, KeyDirection::Down) {
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|frame, event: &KeyUpEvent, window, cx| {
                if frame.key(window, &event.keystroke, KeyDirection::Up) {
                    cx.stop_propagation();
                }
            }))
    }
}

/// Who a window belongs to: what the frame needs to tell the host about it.
#[derive(Clone)]
pub(crate) struct WindowOwner {
    pub instance: InstanceId,
    pub plugins: WeakPlugins,
}

/// What [`PluginWindow::prepare`] found: a window that is already open, or how big a new one
/// has to be.
pub(crate) enum Prepared {
    AlreadyOpen(WindowHandle<PluginFrame>),
    Wanted(WindowSize),
}

/// What a new window needs to be opened.
pub(crate) struct WindowRequest<'a> {
    pub title: &'a str,
    pub size: WindowSize,
    /// Whether the composer may drag its edge, which the plugin decides.
    pub resizable: bool,
    /// Where it was, when it was somewhere before.
    pub placement: Option<Placement>,
    /// Whether it takes the keyboard. A window the composer opens does; one that comes back by
    /// itself, as the project opens or its plugin reloads, leaves the keyboard where it was.
    pub focus: bool,
}

/// The plugin's window, from the host's side. The plugin's own part of it, what it makes and
/// frees, is the backend's; this is the window it goes in.
#[derive(Default)]
pub(crate) struct PluginWindow {
    open: Option<WindowHandle<PluginFrame>>,
    /// Frees the plugin's view when the window goes, whatever took it down. See
    /// [`open_window`] for why this is what keeps the plugin's view inside its parent's life.
    closed: Option<Subscription>,
    /// A size the plugin asked for, until whoever polls gives the window it.
    wanted_size: Option<WindowSize>,
    /// How big the plugin's view is, as far as the host knows: what it said as it opened, what
    /// it asked for since and what a drag of the edge left it at. A window whose content has
    /// this size has nothing to tell the plugin.
    size: Option<WindowSize>,
    /// Whether the composer may drag the window's edge, which the plugin said as it opened.
    resizable: bool,
}

impl PluginWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Whether `id` is this window. A window that went may still report a move on its way.
    pub fn is(&self, id: WindowId) -> bool {
        self.open.is_some_and(|handle| handle.window_id() == id)
    }

    /// The plugin asked its window to be this big. Plugins do it as they open, and again when
    /// their own interface changes. Nothing of ours has to follow: the window holds the plugin
    /// and nothing else.
    pub fn wants_size(&mut self, wanted: WindowSize) {
        self.wanted_size = Some(wanted);
        self.size = Some(wanted);
    }

    /// The window's content is `content` now. For a window the composer may resize that is a
    /// drag of its edge: the plugin makes a size it takes of it, takes it, and the window is
    /// given that size at the next poll. Anything else is the window taking a size this host
    /// gave it, and there is nothing to tell the plugin.
    pub fn resized(&mut self, gui: &mut dyn PluginGui, content: WindowSize) {
        if !self.resizable || self.size == Some(content) {
            return;
        }
        let settled = gui.resize(content).or(self.size).unwrap_or(content);
        self.size = Some(settled);
        if settled != content {
            self.wanted_size = Some(settled);
        }
    }

    /// The window and the size its plugin last asked for, once.
    #[must_use]
    pub fn take_wanted_size(&mut self) -> Option<(WindowHandle<PluginFrame>, WindowSize)> {
        let handle = self.open?;
        Some((handle, self.wanted_size.take()?))
    }

    /// The first half of opening a window: everything the plugin has to say, with nothing of
    /// GPUI running. See [`crate::Plugins::open_window`] for why the two halves are apart.
    ///
    /// The order for an embedded window: create, ask how big, put the view in a window, show.
    /// The scale is left alone, as both formats say for Cocoa, where sizes are already logical.
    pub fn prepare(&mut self, gui: &mut dyn PluginGui) -> Result<Prepared, PluginProblem> {
        if let Some(handle) = self.open {
            return Ok(Prepared::AlreadyOpen(handle));
        }
        // From here the plugin holds resources for a window, and `give_up` frees them. It is
        // never made twice: an attempt whose window did not open leaves them and comes back.
        gui.create()?;
        let size = gui.size().unwrap_or(DEFAULT_SIZE);
        self.size = Some(size);
        self.resizable = gui.can_resize();
        Ok(Prepared::Wanted(size))
    }

    /// Whether the composer may drag the edge of the window [`Self::prepare`] made ready.
    pub fn resizable(&self) -> bool {
        self.resizable
    }

    /// The second half: the plugin fills the window that was made for it. On a failure the
    /// window comes back, for the caller to take down once nothing is borrowed.
    pub fn attach(
        &mut self,
        gui: &mut dyn PluginGui,
        handle: WindowHandle<PluginFrame>,
        view: Option<NonNull<c_void>>,
        closed: Subscription,
    ) -> Result<(), (PluginProblem, WindowHandle<PluginFrame>)> {
        self.open = Some(handle);
        self.closed = Some(closed);
        // A window with no view of its own is the one GPUI makes without a platform behind it,
        // which is what a test has. The plugin is then shown with nothing to draw in, so the
        // rest of its life can be checked without a display. See `tests/plugin_host/window.rs`.
        let attached = match view {
            // SAFETY: the view belongs to the window that was just opened. Every way that
            // window can go frees the plugin's resources for it first: `give_up`, the window's
            // own close control, and the check in `Plugins::settle_windows` for a window that
            // went without saying so. The application ends before a window it still has.
            Some(view) => unsafe { gui.set_parent(view) },
            None => Ok(()),
        };
        match attached.and_then(|()| gui.show()) {
            Ok(()) => Ok(()),
            Err(problem) => match self.give_up(Some(gui)) {
                Some(handle) => Err((problem, handle)),
                None => Ok(()),
            },
        }
    }

    /// Frees whatever the plugin holds for a window and gives back the window it was in, for
    /// the caller to take down once nothing is borrowed. `None` says there is no window to
    /// take down, which includes an attempt whose window never opened. Nothing of the plugin's
    /// sound or state is touched: a plugin goes on playing with no window.
    #[must_use]
    pub fn give_up(
        &mut self,
        gui: Option<&mut dyn PluginGui>,
    ) -> Option<WindowHandle<PluginFrame>> {
        self.wanted_size = None;
        self.size = None;
        self.closed = None;
        if let Some(gui) = gui {
            gui.destroy();
        }
        self.open.take()
    }
}

/// Takes a window down. An error says only that the window was already gone.
pub(crate) fn remove(handle: WindowHandle<PluginFrame>, cx: &mut App) {
    handle
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

/// Where an open window is now.
pub(crate) fn placement_of(window: &Window, cx: &App) -> Placement {
    let corner = window.bounds().origin;
    Placement {
        open: true,
        x: f32::from(corner.x).round() as i32,
        y: f32::from(corner.y).round() as i32,
        display: window.display(cx).map(|display| u64::from(display.id())),
    }
}

/// How big the content of a window is, which is the plugin's view.
fn content_of(window: &Window) -> WindowSize {
    let content = window.viewport_size();
    WindowSize {
        width: f32::from(content.width).round().max(1.0) as u32,
        height: f32::from(content.height).round().max(1.0) as u32,
    }
}

/// Gives a window the size its plugin asked for. An error says only that the window was
/// already gone, and then there is nothing to size.
pub(crate) fn resize(handle: WindowHandle<PluginFrame>, wanted: WindowSize, cx: &mut App) {
    let wanted = size(px(wanted.width as f32), px(wanted.height as f32));
    handle.update(cx, |_, window, _| window.resize(wanted)).ok();
}

/// Opens one window for a plugin and gives back its handle, the view the plugin fills and the
/// subscription that frees that view when the window goes.
///
/// The subscription is what keeps the plugin's view inside the life of the view it is in.
/// GPUI takes a window down in `App::update_window`: it removes the window from the
/// application, tells the observers of `on_window_closed`, and only then drops the `Window`
/// it is still holding, which is what releases the `NSWindow` and its view. So an observer
/// runs while the parent is still there, whatever took the window down: the window's own
/// close control, [`remove`], or anything else. Read from the pinned GPUI, `App::update_window`
/// and `SubscriberSet::retain`, which takes its subscribers out before it calls one, so this
/// may drop its own subscription from inside the call.
///
/// The window is GPUI's `Floating` kind, which on macOS is an `NSPanel` at the floating window
/// level: above every normal window of this application, the main one included. A panel hides
/// while another application is active, which AppKit does by default for every `NSPanel`
/// (`hidesOnDeactivate`), so it is never above another application's windows.
pub(crate) fn open_window(
    owner: &WindowOwner,
    request: WindowRequest<'_>,
    cx: &mut App,
) -> anyhow::Result<(
    WindowHandle<PluginFrame>,
    Option<NonNull<c_void>>,
    Subscription,
)> {
    let content = size(
        px(request.size.width as f32),
        px(request.size.height as f32),
    );
    let (bounds, display_id) = where_to_open(request.placement, content, cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(request.title.to_string().into()),
            ..Default::default()
        }),
        kind: WindowKind::Floating,
        focus: request.focus,
        is_resizable: request.resizable,
        display_id,
        ..Default::default()
    };
    let mut view = None;
    let frame_owner = owner.clone();
    let handle = cx.open_window(options, |window, cx| {
        view = cocoa_view(window);
        cx.new(|cx| PluginFrame::new(frame_owner, window, cx))
    })?;
    let (instance, plugins, id) = (
        owner.instance.clone(),
        owner.plugins.clone(),
        handle.window_id(),
    );
    let closed = cx.on_window_closed(move |_, closed| {
        if closed == id
            && let Some(plugins) = plugins.upgrade()
        {
            plugins.window_was_closed(&instance);
        }
    });
    Ok((handle, view, closed))
}

/// Where a window opens: where it was, when that display is still there and the window can be
/// reached on it, and else in the middle of the main display.
fn where_to_open(
    placement: Option<Placement>,
    content: Size<Pixels>,
    cx: &App,
) -> (Bounds<Pixels>, Option<DisplayId>) {
    let restored = placement.and_then(|placement| {
        let display = match placement.display {
            Some(id) => cx.find_display(DisplayId::from(id)),
            None => cx.primary_display(),
        }?;
        let space = display.bounds().size;
        reachable(
            placement.x,
            placement.y,
            f32::from(space.width),
            f32::from(space.height),
        )
        .then(|| {
            let corner = point(px(placement.x as f32), px(placement.y as f32));
            (Bounds::new(corner, content), Some(display.id()))
        })
    });
    restored.unwrap_or_else(|| (Bounds::centered(None, content, cx), None))
}

/// The `NSView` of a window, which is what CLAP's Cocoa API takes as the parent.
fn cocoa_view(window: &Window) -> Option<NonNull<c_void>> {
    // The trait's method, not `Window::window_handle`, which is the GPUI handle of the window.
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
        _ => None,
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
        // Saved on a larger display than the one there is now.
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
}
