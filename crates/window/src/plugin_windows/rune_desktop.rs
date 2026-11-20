//! The plugins' windows on Rune. Each is a window of its own state, which holds nothing but
//! the way back to the host and to the main window.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

use plugin_host::{Display, KeyDirection, ParentView, Plugins, WindowId};
use rune::{App, AppSpec, Cx, El, KeyEvent, Mods, Tx, WindowHandle, WindowOptions, WindowSpec, el};

use super::{Content, Desktop, Events, Opening, Spare, closed_by_composer, key, native, place_of};

pub(crate) struct RuneDesktop {
    plugins: Plugins,
    events: Rc<Events>,
    handles: HashMap<WindowId, WindowHandle>,
}

impl RuneDesktop {
    pub(crate) fn new(plugins: Plugins, events: Rc<Events>) -> Self {
        Self {
            plugins,
            events,
            handles: HashMap::new(),
        }
    }
}

/// A plugin's window: its title until it is set, and what it tells.
struct Frame {
    window: WindowId,
    plugins: Plugins,
    events: Rc<Events>,
    handle: Rc<OnceCell<WindowHandle>>,
    title: Option<String>,
}

impl Desktop for RuneDesktop {
    fn displays(&self) -> Vec<Display> {
        native::displays()
    }

    fn is_active(&self) -> bool {
        rune::is_headless() || native::app_is_active()
    }

    fn open(&mut self, opening: Opening) {
        let Opening {
            window,
            title,
            content,
            resizable,
            position,
            focus,
        } = opening;
        let (size, physical_size) = match content {
            Content::Points(width, height) => ((width, height), None),
            Content::Physical(width, height) => {
                ((width as f32, height as f32), Some((width, height)))
            }
        };
        let handle = Rc::new(OnceCell::new());
        let frame = Frame {
            window,
            plugins: self.plugins.clone(),
            events: self.events.clone(),
            handle: handle.clone(),
            title: Some(title),
        };
        let spec = AppSpec {
            poll: Some(poll),
            on_key: Some(on_key),
            ..AppSpec::new(
                "Sound Tools",
                size,
                || unreachable!("the state is made before"),
                view,
            )
        };
        let app = App::new(spec)
            .on_close(on_close)
            .on_focus(on_focus)
            .on_modifiers(on_modifiers);
        let options = WindowOptions {
            min_size: Some((0.0, 0.0)),
            resizable,
            // macOS places the content's corner there, with the main display's scale: set below.
            position: position.filter(|_| cfg!(not(target_os = "macos"))),
            physical_size,
            ..WindowOptions::default()
        };
        let plugins = self.plugins.clone();
        let owner = native::owner();
        let spec = WindowSpec::from_app(format!("plugin-{}", window.number()), app)
            // Else the plugin's view would be under the title bar's buttons.
            .chrome(rune::Chrome {
                full_size_content: false,
                ..rune::Chrome::DEFAULT
            })
            .options(options)
            .state(frame)
            .on_created(move |handle| created(&plugins, window, &handle, position, focus, owner));
        let spec = if focus { spec } else { spec.inactive() };
        let opened = rune::open_window(spec);
        let _ = handle.set(opened.clone());
        self.handles.insert(window, opened);
    }

    fn resize(&mut self, window: WindowId, points: (f32, f32)) {
        if let Some(handle) = self.handles.get(&window) {
            handle.resize_instant(points);
        }
    }

    fn focus(&mut self, window: WindowId) {
        if let Some(handle) = self.handles.get(&window) {
            handle.focus();
        }
    }

    fn close(&mut self, window: WindowId) {
        if let Some(handle) = self.handles.remove(&window) {
            handle.close();
        }
    }

    fn placement(&self, window: WindowId) -> Option<rune::WindowPlacement> {
        self.handles.get(&window)?.placement()
    }
}

/// The window is made: the plugin's view goes in it.
fn created(
    plugins: &Plugins,
    window: WindowId,
    handle: &WindowHandle,
    position: Option<(i32, i32)>,
    focus: bool,
    owner: native::Owner,
) {
    let scale = handle.placement().map_or(1.0, |at| at.scale);
    if cfg!(target_os = "macos")
        && let Some(position) = position
    {
        handle.set_position(super::frame_at(position, &native::displays(), scale));
    }
    let keyboard = native::keyboard();
    let raw = handle.with_raw_handle(|raw| raw.as_raw()).ok();
    let parent = raw.map(|handle| ParentView { handle, scale });
    // SAFETY: the view is the window's, which goes only after the host let go of it: it says
    // `Close` first, or hears `window_closed` from `on_close`, or frees every view on quit.
    unsafe { plugins.window_created(window, parent) };
    if let Some(raw) = &raw {
        native::float(raw, owner);
    }
    // A plugin may take the keyboard as it is shown, which Six Sines does.
    if !focus
        && let Some(keyboard) = keyboard
        && native::keyboard() != Some(keyboard)
    {
        native::give_keyboard_to(keyboard);
    }
}

fn view(_: &Frame, _: &mut Cx<Frame>) -> El {
    el().w_full()
        .h_full()
        .on_key_up(|frame: &mut Frame, event: &KeyEvent, _: &mut Tx| {
            key(
                &frame.plugins,
                &frame.events,
                frame.window,
                event,
                KeyDirection::Up,
            );
            true
        })
}

fn poll(frame: &mut Frame, _: f64, tx: &mut Tx) -> bool {
    if let Some(title) = frame.title.take() {
        tx.set_title(title);
    }
    false
}

fn on_key(frame: &mut Frame, event: &KeyEvent, _: &mut Tx) -> bool {
    key(
        &frame.plugins,
        &frame.events,
        frame.window,
        event,
        KeyDirection::Down,
    );
    true
}

fn on_focus(frame: &mut Frame, focused: bool, _: &mut Tx) {
    if !focused {
        frame.events.spare(Spare::LetGo);
    }
}

fn on_modifiers(frame: &mut Frame, mods: Mods, _: &mut Tx) {
    if cfg!(target_os = "macos") && mods.cmd {
        frame.events.spare(Spare::LetGo);
    }
}

/// The composer closed it. Its place still reads here.
fn on_close(frame: &mut Frame, _: &mut Tx) {
    let placement = frame.handle.get().and_then(WindowHandle::placement);
    let place = placement.and_then(|at| place_of(&at, &native::displays()));
    closed_by_composer(&frame.plugins, &frame.events, frame.window, place);
}
