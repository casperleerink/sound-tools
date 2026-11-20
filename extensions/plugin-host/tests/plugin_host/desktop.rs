//! The app's side of the plugins' windows, as a test plays it: a desktop of one display that
//! makes no real window. It does what the host's commands say, as the app does, and tells the
//! host what a composer does to a window.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::ptr::NonNull;

use plugin_host::{
    Display, KeyDirection, Keystroke, ParentView, Plugins, WindowCommand, WindowId, WindowPlace,
    WindowSize,
};
use raw_window_handle::{AppKitWindowHandle, RawWindowHandle, Win32WindowHandle};

/// What every window gives a plugin as its view: a byte that lives as long as the process. The
/// test plugins only check that it is there.
static VIEW: u8 = 0;

/// A window the desktop shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    pub title: String,
    /// The frame's top left, in physical desktop pixels.
    pub position: (i32, i32),
    /// The content, in physical pixels.
    pub content: (u32, u32),
    pub resizable: bool,
    pub focus: bool,
}

pub(crate) struct Desktop {
    display: Display,
    /// A Win32 view, which counts physical pixels, instead of an `NSView`.
    win32: bool,
    pub shown: BTreeMap<WindowId, Shown>,
    /// Every command the host gave, in order.
    pub commands: Vec<WindowCommand>,
}

impl Desktop {
    /// One display of 1920 by 1080 points at `scale`.
    pub(crate) fn new(scale: f64) -> Self {
        let pixels = |points: u32| (f64::from(points) * scale) as u32;
        Self {
            display: Display {
                id: Some(1),
                position: (0, 0),
                size: (pixels(1920), pixels(1080)),
                scale,
            },
            win32: false,
            shown: BTreeMap::new(),
            commands: Vec::new(),
        }
    }

    pub(crate) fn with_win32_views(mut self) -> Self {
        self.win32 = true;
        self
    }

    /// The one window shown.
    pub(crate) fn only(&self) -> WindowId {
        assert_eq!(self.shown.len(), 1, "{:?}", self.shown);
        *self.shown.keys().next().unwrap()
    }

    pub(crate) fn window(&self) -> &Shown {
        &self.shown[&self.only()]
    }

    /// In the plugin's pixels.
    pub(crate) fn content(&self) -> (u32, u32) {
        let (width, height) = self.window().content;
        match self.win32 {
            true => (width, height),
            false => (self.points(width), self.points(height)),
        }
    }

    /// What the app does after every poll, with the app active.
    pub(crate) fn settle(&mut self, plugins: &Plugins) {
        for command in plugins.settle_windows(true) {
            self.commands.push(command.clone());
            self.obey(plugins, command);
        }
    }

    /// The composer opens the window from the card.
    pub(crate) fn open(&mut self, plugins: &Plugins, id: &sound_core::InstanceId) {
        plugins.open_window(id).expect("the window opens");
        self.settle(plugins);
    }

    fn obey(&mut self, plugins: &Plugins, command: WindowCommand) {
        match command {
            WindowCommand::Open {
                window,
                title,
                size,
                resizable,
                placement,
                focus,
            } => {
                let content = self.physical(size);
                let displays = [self.display];
                let position = placement
                    .and_then(|placement| placement.position_on(&displays))
                    .unwrap_or_else(|| self.middle(content));
                let shown = Shown {
                    title,
                    position,
                    content,
                    resizable,
                    focus,
                };
                self.shown.insert(window, shown);
                let parent = ParentView {
                    handle: self.view(),
                    scale: self.display.scale,
                };
                // SAFETY: the view is a static, alive for as long as any plugin may hold it.
                unsafe { plugins.window_created(window, Some(parent)) };
                self.tell(plugins, window);
            }
            WindowCommand::Resize { window, size } => {
                let content = self.physical(size);
                if let Some(shown) = self.shown.get_mut(&window) {
                    shown.content = content;
                    self.tell(plugins, window);
                }
            }
            WindowCommand::Focus { window } => {
                if let Some(shown) = self.shown.get_mut(&window) {
                    shown.focus = true;
                }
            }
            WindowCommand::Close { window } => {
                self.shown.remove(&window);
            }
        }
    }

    /// The composer drags the window's edge until its content is `points`.
    pub(crate) fn drag_edge(&mut self, plugins: &Plugins, points: (u32, u32)) {
        let window = self.only();
        let (width, height) = points;
        let content = self.physical(WindowSize { width, height });
        self.shown.get_mut(&window).unwrap().content = content;
        self.tell(plugins, window);
    }

    /// The composer closes the window by its title bar.
    pub(crate) fn close_by_its_title_bar(&mut self, plugins: &Plugins) {
        let window = self.only();
        plugins.window_closed(window);
        self.shown.remove(&window);
    }

    pub(crate) fn type_key(
        &self,
        plugins: &Plugins,
        keystroke: &Keystroke,
        direction: KeyDirection,
    ) -> bool {
        plugins.window_key(self.only(), keystroke, direction)
    }

    /// The app quitting.
    pub(crate) fn quit(&mut self, plugins: &Plugins) {
        for window in plugins.close_all_windows() {
            self.shown.remove(&window);
        }
    }

    fn tell(&self, plugins: &Plugins, window: WindowId) {
        let shown = &self.shown[&window];
        let place = WindowPlace {
            position: shown.position,
            content_size: shown.content,
            display: self.display,
        };
        plugins.window_moved(window, place);
    }

    fn view(&self) -> RawWindowHandle {
        let view = NonNull::from(&VIEW).cast();
        match self.win32 {
            true => {
                let hwnd = std::num::NonZeroIsize::new(view.as_ptr() as isize).unwrap();
                RawWindowHandle::Win32(Win32WindowHandle::new(hwnd))
            }
            false => RawWindowHandle::AppKit(AppKitWindowHandle::new(view)),
        }
    }

    fn physical(&self, size: WindowSize) -> (u32, u32) {
        let pixels = |side: u32| match self.win32 {
            true => side,
            false => (f64::from(side) * self.display.scale).round() as u32,
        };
        (pixels(size.width), pixels(size.height))
    }

    fn points(&self, pixels: u32) -> u32 {
        (f64::from(pixels) / self.display.scale).round() as u32
    }

    fn middle(&self, content: (u32, u32)) -> (i32, i32) {
        let (width, height) = self.display.size;
        (
            ((width - content.0) / 2) as i32,
            ((height - content.1) / 2) as i32,
        )
    }
}
