//! The plugin's own window in the plugin's own process, on macOS, where a view of one process
//! cannot sit in a window of another.
//!
//! The process is an application with no Dock icon or menu bar, and its window a panel that takes
//! the keyboard without making this process the active app, so Sound Tools keeps its menu bar.
//! The panel floats over Sound Tools and hides with it: this process watches which app is in
//! front. Everything the window hears that the host keeps, where it is and the keys the plugin
//! did not use, waits here for the host's next poll.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{ActivationPolicy, App, Keystroke, WindowId};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use sound_ui::{SpareKey as Spare, SpareKeys};

use super::child::{Received, Reply, Server, end_now};
use super::protocol::{Answer, Request, SpareKey};
use crate::PluginProblem;
use crate::backend::KeyDirection;
use crate::window::{
    FrameOwner, ParentView, Placement, PluginWindow, Prepared, WindowOwner, WindowRequest,
    WindowSize,
};

/// How often the main thread shows it is alive.
const HEARTBEAT: Duration = Duration::from_millis(100);

/// How often the window looks which app is in front.
const FOLLOW: Duration = Duration::from_millis(250);

/// What the window heard that waits for the next poll.
#[derive(Default)]
struct Heard {
    placement: Option<Placement>,
    closed: bool,
    spare_keys: Vec<SpareKey>,
}

/// The server with the plugin's window, on the main thread of the application.
struct Windowed {
    server: Server,
    window: PluginWindow,
    heard: Rc<RefCell<Heard>>,
    owner: Option<WindowOwner>,
}

/// Serves the host in `application`, which opens the plugin's window when the host asks.
/// Never returns: the process ends with the host, or with the host's quit.
pub(super) fn serve(
    server: Server,
    requests: smol::channel::Receiver<Received>,
    application: gpui::Application,
    parent: u32,
) -> ! {
    application.run(move |cx| {
        cx.set_activation_policy(ActivationPolicy::Accessory);
        let heard = Rc::new(RefCell::new(Heard::default()));
        SpareKeys::install(
            {
                let heard = heard.clone();
                move |key, _| heard.borrow_mut().spare_keys.push(spare_key(key))
            },
            cx,
        );
        let mapping = server.mapping.clone();
        let windowed = Rc::new(RefCell::new(Windowed {
            server,
            window: PluginWindow::default(),
            heard: heard.clone(),
            owner: None,
        }));
        let owner: WindowOwner = Rc::new(InThisProcess {
            windowed: Rc::downgrade(&windowed),
            heard,
        });
        windowed.borrow_mut().owner = Some(owner);
        cx.spawn(async move |cx| {
            loop {
                mapping.shared().heartbeat.fetch_add(1, Ordering::Relaxed);
                cx.background_executor().timer(HEARTBEAT).await;
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            loop {
                // Not in `cx.update`: a request the plugin answers with a dialog holds the app.
                follow(parent);
                cx.background_executor().timer(FOLLOW).await;
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            while let Ok((message, bytes)) = requests.recv().await {
                cx.update(|cx| serve_one(&windowed, message, bytes, cx));
            }
            end_now();
        })
        .detach();
    });
    end_now()
}

fn serve_one(
    windowed: &Rc<RefCell<Windowed>>,
    message: crate::bridge::protocol::Message,
    bytes: Option<Vec<u8>>,
    cx: &mut App,
) {
    let mut windowed = windowed.borrow_mut();
    let reply = match message.request {
        Request::OpenWindow {
            title,
            placement,
            focus,
        } => Reply::Answer(Answer::WindowOpened(
            windowed.open(&title, placement, focus, cx),
        )),
        Request::CloseWindow => {
            windowed.close(cx);
            Reply::Answer(Answer::WindowClosed)
        }
        Request::Quit => {
            windowed.close(cx);
            windowed.server.quit()
        }
        request => {
            let is_poll = matches!(request, Request::Poll);
            let message = crate::bridge::protocol::Message {
                sends: message.sends,
                request,
            };
            let reply = windowed.server.serve(message, bytes);
            match (is_poll, reply) {
                (true, Reply::Answer(Answer::Polled(requests, _))) => {
                    Reply::Answer(windowed.polled(requests, cx))
                }
                (_, reply) => reply,
            }
        }
    };
    windowed.server.send(reply);
}

impl Windowed {
    fn open(
        &mut self,
        title: &str,
        placement: Option<Placement>,
        focus: bool,
        cx: &mut App,
    ) -> Result<Option<Placement>, PluginProblem> {
        let Self {
            server,
            window,
            owner,
            ..
        } = self;
        let no_window = |plugin_id: String| PluginProblem::NoWindow { plugin_id };
        let (Some(plugin), Some(owner)) = (server.plugin.as_mut(), owner.as_ref()) else {
            return Err(no_window(String::new()));
        };
        let Some(gui) = plugin.gui() else {
            return Err(no_window(String::new()));
        };
        let size = match window.prepare(gui)? {
            Prepared::AlreadyOpen(handle) => {
                handle
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
                return Ok(None);
            }
            Prepared::Wanted(size) => size,
        };
        let request = WindowRequest {
            title,
            size,
            resizable: window.resizable(),
            placement,
            focus,
            own_process: true,
        };
        let did_not_open = |message: String| PluginProblem::WindowDidNotOpen {
            plugin_id: String::new(),
            message,
        };
        let (handle, parent, closed) = match crate::window::open_window(owner, request, cx) {
            Ok(opened) => opened,
            Err(error) => {
                if let Some(gui) = plugin.gui() {
                    let _window = window.give_up(Some(gui));
                }
                return Err(did_not_open(error.to_string()));
            }
        };
        if let Some(parent) = &parent {
            float(parent);
        }
        let placed = handle
            .update(cx, |_, window, cx| crate::window::placement_of(window, cx))
            .ok();
        let Some(gui) = plugin.gui() else {
            crate::window::remove(handle, cx);
            return Err(no_window(String::new()));
        };
        match window.attach(gui, handle, parent, closed) {
            Ok(()) => Ok(placed),
            Err((problem, handle)) => {
                crate::window::remove(handle, cx);
                Err(problem)
            }
        }
    }

    fn close(&mut self, cx: &mut App) {
        let gui = self.server.plugin.as_mut().and_then(|plugin| plugin.gui());
        if let Some(handle) = self.window.give_up(gui) {
            crate::window::remove(handle, cx);
        }
    }

    /// The answer of a poll, with what the window heard since the last one. The window takes
    /// a size the plugin asked for here, and goes when the plugin closed it.
    fn polled(&mut self, mut requests: crate::backend::Requests, cx: &mut App) -> Answer {
        if let Some(wanted) = requests.window_size.take() {
            self.window.wants_size(wanted);
        }
        if let Some((handle, wanted)) = self.window.take_wanted_size() {
            crate::window::resize(handle, wanted, cx);
        }
        if requests.window_closed {
            self.close(cx);
        }
        let mut heard = self.heard.borrow_mut();
        requests.window_closed |= std::mem::take(&mut heard.closed);
        requests.placement = heard.placement.take();
        Answer::Polled(requests, std::mem::take(&mut heard.spare_keys))
    }
}

/// What a window in this process tells: the window is the plugin's, so its moves and keys go to
/// the plugin here, and what the host keeps waits for its poll.
struct InThisProcess {
    windowed: std::rc::Weak<RefCell<Windowed>>,
    heard: Rc<RefCell<Heard>>,
}

impl FrameOwner for InThisProcess {
    fn bounds_changed(&self, window: WindowId, placement: Placement, content: WindowSize) {
        self.heard.borrow_mut().placement = Some(placement);
        // A move from inside a call of ours, which has the server, is that call's.
        let Some(windowed) = self.windowed.upgrade() else {
            return;
        };
        let Ok(mut windowed) = windowed.try_borrow_mut() else {
            return;
        };
        let Windowed {
            server,
            window: plugin_window,
            ..
        } = &mut *windowed;
        if !plugin_window.is(window) {
            return;
        }
        if let Some(gui) = server.plugin.as_mut().and_then(|plugin| plugin.gui()) {
            plugin_window.resized(gui, content);
        }
    }

    fn key(&self, window: WindowId, keystroke: &Keystroke, direction: KeyDirection) -> bool {
        let Some(windowed) = self.windowed.upgrade() else {
            return false;
        };
        let Ok(mut windowed) = windowed.try_borrow_mut() else {
            return false;
        };
        let Windowed {
            server,
            window: plugin_window,
            ..
        } = &mut *windowed;
        plugin_window.is(window)
            && server
                .plugin
                .as_mut()
                .and_then(|plugin| plugin.gui())
                .is_some_and(|gui| gui.key(keystroke, direction))
    }

    /// The composer closed it, by its title bar: the plugin lets go of its view while the
    /// window still holds it, and the host hears it at its next poll.
    fn closed(&self, _window: WindowId) {
        let Some(windowed) = self.windowed.upgrade() else {
            return;
        };
        let Ok(mut windowed) = windowed.try_borrow_mut() else {
            return;
        };
        let Windowed { server, window, .. } = &mut *windowed;
        let gui = server.plugin.as_mut().and_then(|plugin| plugin.gui());
        if window.give_up(gui).is_some() {
            self.heard.borrow_mut().closed = true;
        }
    }
}

fn spare_key(key: Spare<'_>) -> SpareKey {
    match key {
        Spare::Down(event) => SpareKey::Down {
            keystroke: event.keystroke.unparse(),
            held: event.is_held,
        },
        Spare::Up(event) => SpareKey::Up {
            keystroke: event.keystroke.unparse(),
        },
        Spare::LetGo => SpareKey::LetGo,
    }
}

/// The window floats over Sound Tools' own windows, on the Space it opened on and next to a main
/// window in full screen. GPUI's panel that does not activate sits above every app and on every
/// Space, which is for a menu, not a window.
fn float(parent: &ParentView) {
    const FLOATING_LEVEL: isize = 3;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    let view = parent.view.as_ptr().cast::<AnyObject>();
    // SAFETY: an `NSView` of a window this process just opened, on its main thread.
    unsafe {
        let window: *mut AnyObject = msg_send![view, window];
        if window.is_null() {
            return;
        }
        let _: () = msg_send![window, setLevel: FLOATING_LEVEL];
        let _: () = msg_send![window, setCollectionBehavior: FULL_SCREEN_AUXILIARY];
        let _: () = msg_send![window, setHidesOnDeactivate: Bool::NO];
    }
}

/// Shows this process's windows while Sound Tools or this process is the app in front, and
/// hides them behind any other, as Sound Tools' own floating windows do.
fn follow(parent: u32) {
    let own = std::process::id();
    // SAFETY: AppKit's shared objects, on the main thread; a missing one is nil.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if front.is_null() {
            return;
        }
        let front: i32 = msg_send![front, processIdentifier];
        let ours = u32::try_from(front).is_ok_and(|front| front == parent || front == own);
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let hidden: Bool = msg_send![app, isHidden];
        match (ours, hidden.as_bool()) {
            (true, true) => {
                let _: () = msg_send![app, unhideWithoutActivation];
            }
            (false, false) => {
                let _: () = msg_send![app, hide: std::ptr::null_mut::<AnyObject>()];
            }
            _ => {}
        }
    }
}
