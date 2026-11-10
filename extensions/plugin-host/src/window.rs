//! The plugin's own window: one GPUI window per open plugin, with an empty root view. The
//! plugin's view is a child of its `NSView` on macOS, of its `HWND` on Windows, and draws over
//! it. Nothing here knows a plugin format; that is [`crate::backend::PluginGui`].
//!
//! A plugin in a Win32 window counts its size in physical pixels, as both formats say for
//! Win32; Cocoa and GPUI count logical ones. A [`WindowSize`] is in the plugin's pixels.
//!
//! Both formats call a plugin's window on the main thread only, except CLAP's
//! `clap_host_gui.closed`, which may come from any thread and only sets a flag the next poll
//! reads.

use std::ffi::c_void;
use std::ptr::NonNull;

use gpui::{
    App, Bounds, Context, DisplayId, FocusHandle, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke,
    ModifiersChangedEvent, Pixels, Render, Size, Subscription, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId, WindowKind, WindowOptions,
    div, point, prelude::*, px, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use sound_core::InstanceId;
use sound_ui::{SpareKey, SpareKeys};

use crate::PluginProblem;
use crate::backend::{KeyDirection, PluginGui};
use crate::host::WeakPlugins;

/// In the plugin's own pixels: logical on macOS, physical on Windows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WindowSize {
    pub width: u32,
    pub height: u32,
}

/// Only so that a window is never zero-sized: every plugin with a window says its size.
const DEFAULT_SIZE: WindowSize = WindowSize {
    width: 600,
    height: 400,
};

/// Where a plugin's window was and whether it was open, as this machine keeps it.
///
/// `x` and `y` are the top left corner, title bar included, in logical pixels on its display.
/// No size: a plugin keeps its own in its state, and a second one here could only disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Placement {
    pub open: bool,
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<u64>,
}

/// How much of a window has to be on its display for it to come back there: enough to take it
/// by its title bar.
const VISIBLE: i32 = 40;

fn reachable(x: i32, y: i32, width: f32, height: f32) -> bool {
    let (width, height) = (width as i32, height as i32);
    (0..=width - VISIBLE).contains(&x) && (0..=height - VISIBLE).contains(&y)
}

/// The root view of a plugin's window. It draws nothing, and hears the window move and resize
/// and the keys the plugin's own view does not take.
pub(crate) struct PluginFrame {
    owner: WindowOwner,
    /// GPUI gives keys only to what has the focus and what is around it.
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl PluginFrame {
    fn new(owner: WindowOwner, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let bounds = cx.observe_window_bounds(window, |frame, window, cx| {
            let Some(plugins) = frame.owner.plugins.upgrade() else {
                return;
            };
            let (placement, content) = (placement_of(window, cx), content_of(window));
            // GPUI's own handle, not the one `HasWindowHandle` gives.
            let id = Window::window_handle(window).window_id();
            plugins.window_bounds_changed(&frame.owner.instance, id, placement, content);
        });
        // A key held here comes up in whatever has the keyboard now.
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                SpareKeys::pass(SpareKey::LetGo, cx);
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            owner,
            focus,
            _subscriptions: [bounds, activation],
        }
    }

    /// `true` when the plugin used the key.
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
        // A key the plugin has no use for plays the computer keys, as in the main window.
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|frame, event: &KeyDownEvent, window, cx| {
                match frame.key(window, &event.keystroke, KeyDirection::Down) {
                    true => cx.stop_propagation(),
                    false => SpareKeys::pass(SpareKey::Down(event), cx),
                }
            }))
            // Every key up, also of one the plugin took: the computer keys end only a note
            // they started.
            .on_key_up(cx.listener(|frame, event: &KeyUpEvent, window, cx| {
                SpareKeys::pass(SpareKey::Up(event), cx);
                if frame.key(window, &event.keystroke, KeyDirection::Up) {
                    cx.stop_propagation();
                }
            }))
            // macOS sends no key up while cmd is held.
            .on_modifiers_changed(|event: &ModifiersChangedEvent, _, cx| {
                if event.modifiers.platform {
                    SpareKeys::pass(SpareKey::LetGo, cx);
                }
            })
    }
}

#[derive(Clone)]
pub(crate) struct WindowOwner {
    pub instance: InstanceId,
    pub plugins: WeakPlugins,
}

pub(crate) enum Prepared {
    AlreadyOpen(WindowHandle<PluginFrame>),
    Wanted(WindowSize),
}

pub(crate) struct WindowRequest<'a> {
    pub title: &'a str,
    pub size: WindowSize,
    pub resizable: bool,
    pub placement: Option<Placement>,
    /// A window that comes back by itself, as the project opens or its plugin reloads, leaves
    /// the keyboard where it was.
    pub focus: bool,
}

/// The plugin's window, from the host's side. What the plugin makes and frees for it is the
/// backend's.
#[derive(Default)]
pub(crate) struct PluginWindow {
    open: Option<WindowHandle<PluginFrame>>,
    /// Frees the plugin's view when the window goes, whatever took it down. See [`open_window`].
    closed: Option<Subscription>,
    /// A size the plugin asked for, until whoever polls gives the window it.
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

    /// Whether `id` is this window. A window that went may still report a move on its way.
    pub(crate) fn is(&self, id: WindowId) -> bool {
        self.open.is_some_and(|handle| handle.window_id() == id)
    }

    /// The plugin asked its window to be this big, as it opens or when its interface changes.
    pub(crate) fn wants_size(&mut self, wanted: WindowSize) {
        self.wanted_size = Some(wanted);
        self.size = Some(wanted);
    }

    /// The window's content is `content` now. In a resizable window that is the composer
    /// dragging its edge: the plugin settles on a size near it, which the window takes at the
    /// next poll. Anything else is the window taking a size this host gave it.
    pub(crate) fn resized(&mut self, gui: &mut dyn PluginGui, content: WindowSize) {
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
    pub(crate) fn take_wanted_size(&mut self) -> Option<(WindowHandle<PluginFrame>, WindowSize)> {
        let handle = self.open?;
        Some((handle, self.wanted_size.take()?))
    }

    /// The first step of opening a window: everything the plugin has to say, before GPUI opens
    /// the window. [`Self::attach`] is the last.
    ///
    /// Both formats ask this order for an embedded window: create, ask how big, set the parent,
    /// show. The scale is not set here: both formats say not to on Cocoa, where sizes are
    /// already logical, and on Windows it is not known until the window is on its display.
    pub(crate) fn prepare(&mut self, gui: &mut dyn PluginGui) -> Result<Prepared, PluginProblem> {
        if let Some(handle) = self.open {
            return Ok(Prepared::AlreadyOpen(handle));
        }
        // From here the plugin holds resources for a window, until `give_up` frees them.
        gui.create()?;
        let size = gui.size().unwrap_or(DEFAULT_SIZE);
        self.size = Some(size);
        self.resizable = gui.can_resize();
        Ok(Prepared::Wanted(size))
    }

    pub(crate) fn resizable(&self) -> bool {
        self.resizable
    }

    /// The plugin fills the window that was made for it. On a failure the window comes back,
    /// for the caller to take down once nothing is borrowed.
    ///
    /// On Windows the plugin is told the window's scale first, as both formats ask there. Its
    /// sizes stay physical pixels whether it takes the scale or reads it from the system.
    pub(crate) fn attach(
        &mut self,
        gui: &mut dyn PluginGui,
        handle: WindowHandle<PluginFrame>,
        parent: Option<ParentView>,
        closed: Subscription,
    ) -> Result<(), (PluginProblem, WindowHandle<PluginFrame>)> {
        self.open = Some(handle);
        self.closed = Some(closed);
        // No parent is GPUI's test platform: the plugin is shown with nothing to draw in, so
        // tests can check the rest of its life without a display.
        let attached = match parent {
            Some(parent) => {
                if let Some(scale) = parent.physical_scale {
                    gui.set_scale(f64::from(scale));
                    if let Some(scaled) = gui.size()
                        && self.size != Some(scaled)
                    {
                        self.size = Some(scaled);
                        self.wanted_size = Some(scaled);
                    }
                }
                // SAFETY: the view belongs to the window just opened. Every way that window
                // goes frees the plugin's resources for it first: `give_up`, or the `closed`
                // subscription, which runs while the window still holds the view.
                unsafe { gui.set_parent(parent.view) }
            }
            None => Ok(()),
        };
        match attached.and_then(|()| gui.show()) {
            Ok(()) => Ok(()),
            Err(problem) => Err((problem, self.give_up(Some(gui)).unwrap_or(handle))),
        }
    }

    /// Frees whatever the plugin holds for a window and gives back the window, for the caller
    /// to take down once nothing is borrowed. The plugin goes on playing.
    #[must_use]
    pub(crate) fn give_up(
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

/// An error says only that the window was already gone.
pub(crate) fn remove(handle: WindowHandle<PluginFrame>, cx: &mut App) {
    handle
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

pub(crate) fn placement_of(window: &Window, cx: &App) -> Placement {
    let corner = window.bounds().origin;
    Placement {
        open: true,
        x: f32::from(corner.x).round() as i32,
        y: f32::from(corner.y).round() as i32,
        display: window.display(cx).map(|display| u64::from(display.id())),
    }
}

/// The size of the plugin's view, in the plugin's pixels.
fn content_of(window: &Window) -> WindowSize {
    let scale = physical_scale(window).unwrap_or(1.0);
    let content = window.viewport_size();
    WindowSize {
        width: (f32::from(content.width) * scale).round().max(1.0) as u32,
        height: (f32::from(content.height) * scale).round().max(1.0) as u32,
    }
}

/// A size in the plugin's pixels as GPUI counts it in `window`.
fn points(plugin_size: WindowSize, window: &Window) -> Size<Pixels> {
    let scale = physical_scale(window).unwrap_or(1.0);
    size(
        px(plugin_size.width as f32 / scale),
        px(plugin_size.height as f32 / scale),
    )
}

/// How many physical pixels make one of GPUI's, for a plugin in a Win32 window. `None` where
/// the plugin counts as GPUI does.
fn physical_scale(window: &Window) -> Option<f32> {
    // The trait's method, not `Window::window_handle`, which is the GPUI handle of the window.
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(_) => Some(window.scale_factor()),
        _ => None,
    }
}

/// An error says only that the window was already gone.
pub(crate) fn resize(handle: WindowHandle<PluginFrame>, wanted: WindowSize, cx: &mut App) {
    handle
        .update(cx, |_, window, _| window.resize(points(wanted, window)))
        .ok();
}

pub(crate) struct ParentView {
    view: NonNull<c_void>,
    /// See [`physical_scale`].
    physical_scale: Option<f32>,
}

/// Opens one window for a plugin and gives back its handle, the view the plugin fills and the
/// subscription that frees that view when the window goes.
///
/// The subscription keeps the plugin's view inside the life of its parent. GPUI's
/// `App::update_window` tells the `on_window_closed` observers before it drops the `Window`,
/// which releases the native view, whatever took the window down. `SubscriberSet::retain`
/// takes a subscriber out before calling it, so it may drop its own subscription.
///
/// The window floats above the main one and never above another application's windows. On
/// macOS GPUI's `Floating` kind is an `NSPanel`, which hides while another application is
/// active (`hidesOnDeactivate`). On Windows the main window becomes its owner, which does the
/// same; the owner also takes it down, through the observer above. GPUI draws through
/// DirectComposition, over any child window, so there the window is transparent.
pub(crate) fn open_window(
    owner: &WindowOwner,
    request: WindowRequest<'_>,
    cx: &mut App,
) -> anyhow::Result<(WindowHandle<PluginFrame>, Option<ParentView>, Subscription)> {
    let content = size(
        px(request.size.width as f32),
        px(request.size.height as f32),
    );
    let (bounds, display_id) = where_to_open(request.placement, content, cx);
    // Before the new window opens, because opening it makes it the active one.
    #[cfg(target_os = "windows")]
    let main_window = win32::main_window();
    let window_background = match cfg!(target_os = "windows") {
        true => WindowBackgroundAppearance::Transparent,
        false => WindowBackgroundAppearance::Opaque,
    };
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
        window_background,
        ..Default::default()
    };
    let mut view = None;
    let frame_owner = owner.clone();
    let handle = cx.open_window(options, |window, cx| {
        let physical_scale = physical_scale(window);
        view = native_view(window).map(|view| ParentView {
            view,
            physical_scale,
        });
        // Only now that it is on its display can it turn physical pixels into GPUI's.
        if physical_scale.is_some() {
            window.resize(points(request.size, window));
        }
        cx.new(|cx| PluginFrame::new(frame_owner, window, cx))
    })?;
    #[cfg(target_os = "windows")]
    if let (Some(parent), Some(main_window)) = (&view, main_window) {
        win32::float_above(parent.view, main_window);
    }
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

/// Where it was, when that display is still there and the window can be reached on it, else
/// in the middle of the main display.
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

fn native_view(window: &Window) -> Option<NonNull<c_void>> {
    // The trait's method, not `Window::window_handle`, which is the GPUI handle of the window.
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
        RawWindowHandle::Win32(handle) => NonNull::new(handle.hwnd.get() as *mut c_void),
        _ => None,
    }
}

/// Declared here instead of taking a dependency.
#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    /// The root owner of the active window: the main window, also while a plugin window is
    /// active. `None` while the application is not active.
    pub(super) fn main_window() -> Option<NonNull<c_void>> {
        // SAFETY: both take and give window handles and nothing else; a null one is no window.
        unsafe { NonNull::new(GetAncestor(GetActiveWindow(), GA_ROOTOWNER)) }
    }

    pub(super) fn float_above(window: NonNull<c_void>, owner: NonNull<c_void>) {
        // SAFETY: both are open windows of this thread. For a window that is not a child,
        // `GWLP_HWNDPARENT` sets its owner; a failure leaves a plain window beside the main one.
        unsafe { SetWindowLongPtrW(window.as_ptr(), GWLP_HWNDPARENT, owner.as_ptr() as isize) };
    }

    const GA_ROOTOWNER: u32 = 3;
    const GWLP_HWNDPARENT: i32 = -8;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetActiveWindow() -> *mut c_void;
        fn GetAncestor(window: *mut c_void, flags: u32) -> *mut c_void;
        fn SetWindowLongPtrW(window: *mut c_void, index: i32, value: isize) -> isize;
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
}
