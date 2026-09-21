//! The plugin's own window: a window of the application with the plugin's own view inside it.
//!
//! CLAP has two ways to show a plugin. The plugin makes a floating window of its own, or it
//! puts its view into a window the host provides. The specification calls the floating one a
//! fallback every plugin should support; in practice almost none do. Checked on this machine
//! on September 20, 2026: both real CLAP instruments here answer `is_api_supported` with
//! `false` for a floating window and `true` for an embedded one. So the host makes the window
//! and the plugin fills it.
//!
//! It is a window of its own beside the main one, never a panel inside it: one GPUI window per
//! open plugin, with an empty root view, as big as the plugin asked for. The plugin's view is
//! a child of that window's view and draws over it. Nothing about it is saved: where it sat and
//! whether it was open are not part of the piece.
//!
//! Every call of the GUI extension belongs to the main thread, which is where [`crate::Plugins`]
//! lives. The one callback that does not is `clap_host_gui.closed`, which a plugin may make
//! from any thread; it only sets a flag that the next poll reads.

use std::ffi::c_void;
use std::ptr::NonNull;

use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiError, GuiSize, PluginGui};
use clack_host::prelude::*;
use gpui::{
    App, Bounds, Context, IntoElement, Render, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowOptions, div, prelude::*, px, size,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use sound_core::InstanceId;

use crate::PluginProblem;
use crate::host::{SoundToolsHost, WeakPlugins};

/// How big a plugin's window is when the plugin does not say. Every plugin with a window
/// answers `get_size`; this is only so that a window is never zero-sized.
const DEFAULT_SIZE: GuiSize = GuiSize {
    width: 600,
    height: 400,
};

/// How to show a plugin on this machine: the platform's windowing API, in a window of ours.
fn configuration() -> Option<GuiConfiguration<'static>> {
    Some(GuiConfiguration {
        api_type: GuiApiType::default_for_current_platform()?,
        is_floating: false,
    })
}

/// The root view of a plugin's window. It draws nothing: the plugin's own view is in the same
/// window and covers it. It exists because a GPUI window needs a root.
pub struct PluginFrame;

impl Render for PluginFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// Who a window belongs to: what the frame needs to tell the host that it was closed.
pub(crate) struct WindowOwner {
    pub instance: InstanceId,
    pub plugins: WeakPlugins,
}

/// What [`PluginWindow::prepare`] found: a window that is already open, or how big a new one
/// has to be.
pub(crate) enum Prepared {
    AlreadyOpen(WindowHandle<PluginFrame>),
    Wanted(GuiSize),
}

/// The plugin's window, from the host's side.
///
/// `created` says the plugin holds resources for a window and `open` says which window they
/// are in. They are apart because the plugin is asked first and the window is made after, so
/// there is a moment with one and not the other, and an attempt that fails there must still
/// free what the plugin holds.
#[derive(Default)]
pub(crate) struct PluginWindow {
    created: bool,
    open: Option<WindowHandle<PluginFrame>>,
    /// Frees the plugin's view when the window goes, whatever took it down. See
    /// [`open_window`] for why this is what keeps the plugin's view inside its parent's life.
    closed: Option<Subscription>,
    /// A size the plugin asked for, until whoever polls gives the window it.
    wanted_size: Option<GuiSize>,
}

impl PluginWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The plugin asked its window to be this big. Plugins do it as they open, and again when
    /// their own interface changes. Nothing of ours has to follow: the window holds the plugin
    /// and nothing else.
    pub fn wants_size(&mut self, wanted: GuiSize) {
        self.wanted_size = Some(wanted);
    }

    /// The window and the size its plugin last asked for, once.
    #[must_use]
    pub fn take_wanted_size(&mut self) -> Option<(WindowHandle<PluginFrame>, GuiSize)> {
        let handle = self.open?;
        Some((handle, self.wanted_size.take()?))
    }

    /// Whether this plugin has a window at all. A plugin without one is ordinary: it has no
    /// interface of its own, and its card says so instead of offering to open one. The host
    /// asks while the plugin loads and keeps the answer, so that drawing a card calls into no
    /// plugin; [`Self::prepare`] asks again as the negotiation CLAP puts before `create`.
    pub fn is_offered(instance: &mut PluginInstance<SoundToolsHost>) -> bool {
        let Some(gui) = gui_of(instance) else {
            return false;
        };
        let Some(configuration) = configuration() else {
            return false;
        };
        gui.is_api_supported(&instance.plugin_handle(), configuration)
    }

    /// The first half of opening a window: everything the plugin has to say, with nothing of
    /// GPUI running. See [`crate::Plugins::open_window`] for why the two halves are apart.
    ///
    /// CLAP's order for an embedded window: create, ask how big, put the view in a window,
    /// show. The scale is left alone, as CLAP says for Cocoa, where sizes are already logical.
    pub fn prepare(
        &mut self,
        instance: &mut PluginInstance<SoundToolsHost>,
        plugin_id: &str,
    ) -> Result<Prepared, PluginProblem> {
        if let Some(handle) = self.open {
            return Ok(Prepared::AlreadyOpen(handle));
        }
        let no_window = || PluginProblem::NoWindow {
            plugin_id: plugin_id.to_string(),
        };
        let gui = gui_of(instance).ok_or_else(no_window)?;
        let configuration = configuration().ok_or_else(no_window)?;
        if !self.created {
            if !gui.is_api_supported(&instance.plugin_handle(), configuration) {
                return Err(no_window());
            }
            gui.create(&instance.plugin_handle(), configuration)
                .map_err(|error: GuiError| PluginProblem::WindowDidNotOpen {
                    plugin_id: plugin_id.to_string(),
                    message: error.to_string(),
                })?;
            // From here the plugin holds resources for a window, and `give_up` frees them.
            // `create` is never called twice, which CLAP forbids: an attempt whose window did
            // not open leaves `created` set and comes back here.
            self.created = true;
        }
        Ok(Prepared::Wanted(
            gui.get_size(&instance.plugin_handle())
                .unwrap_or(DEFAULT_SIZE),
        ))
    }

    /// The second half: the plugin fills the window that was made for it. On a failure the
    /// window comes back, for the caller to take down once nothing is borrowed.
    pub fn attach(
        &mut self,
        instance: &mut PluginInstance<SoundToolsHost>,
        plugin_id: &str,
        handle: WindowHandle<PluginFrame>,
        view: Option<NonNull<c_void>>,
        closed: Subscription,
    ) -> Result<(), (PluginProblem, WindowHandle<PluginFrame>)> {
        self.open = Some(handle);
        self.closed = Some(closed);
        let failed = |error: GuiError| PluginProblem::WindowDidNotOpen {
            plugin_id: plugin_id.to_string(),
            message: error.to_string(),
        };
        let Some(gui) = gui_of(instance) else {
            self.created = false;
            self.open = None;
            self.closed = None;
            return Err((
                PluginProblem::NoWindow {
                    plugin_id: plugin_id.to_string(),
                },
                handle,
            ));
        };
        // A window with no view of its own is the one GPUI makes without a platform behind it,
        // which is what a test has. The plugin is then shown with nothing to draw in, so the
        // rest of its life can be checked without a display. See `tests/plugin_host/window.rs`.
        let attached = match view {
            // SAFETY: the view belongs to the window that was just opened. Every way that
            // window can go frees the plugin's resources for it first: `give_up`, the window's
            // own close control, and the check in `Plugins::settle_windows` for a window that
            // went without saying so. The application ends before a window it still has.
            Some(view) => unsafe {
                let parent = clack_extensions::gui::Window::from_cocoa_nsview(view.as_ptr());
                gui.set_parent(&instance.plugin_handle(), parent)
            },
            None => Ok(()),
        };
        match attached.and_then(|()| gui.show(&instance.plugin_handle())) {
            Ok(()) => Ok(()),
            Err(error) => {
                let problem = failed(error);
                match self.give_up(instance) {
                    Some(handle) => Err((problem, handle)),
                    None => Ok(()),
                }
            }
        }
    }

    /// Frees whatever the plugin holds for a window and gives back the window it was in, for
    /// the caller to take down once nothing is borrowed. `None` says there is no window to
    /// take down, which includes an attempt whose window never opened. Nothing of the plugin's
    /// sound or state is touched: a plugin goes on playing with no window.
    #[must_use]
    pub fn give_up(
        &mut self,
        instance: &mut PluginInstance<SoundToolsHost>,
    ) -> Option<WindowHandle<PluginFrame>> {
        self.wanted_size = None;
        self.closed = None;
        if std::mem::take(&mut self.created)
            && let Some(gui) = gui_of(instance)
        {
            gui.destroy(&instance.plugin_handle());
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

/// Gives a window the size its plugin asked for. An error says only that the window was
/// already gone, and then there is nothing to size.
pub(crate) fn resize(handle: WindowHandle<PluginFrame>, wanted: GuiSize, cx: &mut App) {
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
pub(crate) fn open_window(
    owner: &WindowOwner,
    title: &str,
    wanted: GuiSize,
    cx: &mut App,
) -> anyhow::Result<(
    WindowHandle<PluginFrame>,
    Option<NonNull<c_void>>,
    Subscription,
)> {
    let bounds = size(px(wanted.width as f32), px(wanted.height as f32));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, bounds, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_string().into()),
            ..Default::default()
        }),
        // A window that follows a plugin that can be resized is later work, so it stays the
        // size the plugin asked for.
        is_resizable: false,
        ..Default::default()
    };
    let mut view = None;
    let handle = cx.open_window(options, |window, cx| {
        view = cocoa_view(window);
        cx.new(|_| PluginFrame)
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

/// The `NSView` of a window, which is what CLAP's Cocoa API takes as the parent.
fn cocoa_view(window: &Window) -> Option<NonNull<c_void>> {
    // The trait's method, not `Window::window_handle`, which is the GPUI handle of the window.
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view),
        _ => None,
    }
}

fn gui_of(instance: &mut PluginInstance<SoundToolsHost>) -> Option<PluginGui> {
    instance.plugin_shared_handle().get_extension::<PluginGui>()
}
