//! The plugins' own windows, and where they are kept on this machine.
//!
//! The table is never borrowed while GPUI runs: opening or closing a window draws, and a card
//! being drawn asks this host about its plugin.

use gpui::{Keystroke, WindowHandle, WindowId};
use sound_core::{InstanceId, Project};

use super::{Hosted, PluginProblem, Plugins, Table};
use crate::PluginRecord;
use crate::backend::KeyDirection;
use crate::placements::PlacementStore;
use crate::window::{
    InTable, Placement, PluginFrame, Prepared, WindowOwner, WindowRequest, WindowSize,
};

impl Table {
    /// Whether the window of `id` is to be open the next time it can be. Only the composer
    /// decides this: a window the host takes down because its plugin reloads, or because the
    /// project closes, stays open in here and comes back.
    pub(super) fn keep_open(&mut self, id: &InstanceId, open: bool) {
        if let Some(placement) = self.placements.get_mut(id)
            && placement.open != open
        {
            placement.open = open;
            self.placements_changed = true;
        }
    }

    pub(super) fn place(&mut self, id: &InstanceId, placement: Placement) {
        if self.placements.get(id) != Some(&placement) {
            self.placements.insert(id.clone(), placement);
            self.placements_changed = true;
        }
    }
}

impl Plugins {
    /// `None` when no plugin is loaded for the record. A card asks on every frame, so this
    /// calls into no plugin, and gives up on a table that a plugin's own call has borrowed.
    pub fn window_offered(&self, id: &InstanceId) -> Option<bool> {
        let table = self.0.table.try_borrow().ok()?;
        table.loaded.get(id).map(|hosted| hosted.has_window)
    }

    /// Read while drawing a card, as [`Self::window_offered`].
    pub fn window_is_open(&self, id: &InstanceId) -> bool {
        let Ok(table) = self.0.table.try_borrow() else {
            return false;
        };
        table
            .loaded
            .get(id)
            .is_some_and(|hosted| hosted.window.is_open())
    }

    /// Opens the plugin's own window where it was last, or brings it forward.
    pub fn open_window(&self, id: &InstanceId, cx: &mut gpui::App) -> Result<(), PluginProblem> {
        let opened = self.show_window(id, true, cx);
        let mut table = self.0.table.borrow_mut();
        table.keep_open(id, opened.is_ok());
        opened
    }

    fn show_window(
        &self,
        id: &InstanceId,
        focus: bool,
        cx: &mut gpui::App,
    ) -> Result<(), PluginProblem> {
        // One: what the plugin says, with the table borrowed.
        let prepared = {
            let mut table = self.0.table.borrow_mut();
            let placement = table.placements.get(id).copied();
            let Some(hosted) = table.loaded.get_mut(id) else {
                // The plugin did not load. That is reported already.
                return Ok(());
            };
            let plugin_id = hosted.plugin_id.clone();
            let title = self.title(hosted);
            let no_window = || PluginProblem::NoWindow {
                plugin_id: plugin_id.clone(),
            };
            let Hosted {
                window,
                plugin,
                has_window,
                ..
            } = hosted;
            // A plugin in its own process opens its window there, or brings it forward.
            if let Some(remote) = plugin.remote_window() {
                let opened = remote.open(&title, placement, focus);
                match &opened {
                    Ok(_) => window.opened_there(),
                    Err(PluginProblem::NoWindow { .. }) => *has_window = false,
                    Err(_) => {}
                }
                table.card_changed = true;
                if let Ok(Some(placed)) = opened {
                    table.place(id, placed);
                }
                return opened.map(|_| ());
            }
            let prepared = match plugin.gui() {
                Some(gui) => window.prepare(gui),
                None => Err(no_window()),
            };
            // A plugin that turns out to have no window says so once, and the card stops
            // offering one for the rest of this session.
            if matches!(prepared, Err(PluginProblem::NoWindow { .. })) {
                *has_window = false;
            }
            let resizable = window.resizable();
            table.card_changed = true;
            prepared.map(|prepared| (prepared, plugin_id, title, resizable, placement))?
        };
        // Two: the window itself, with nothing borrowed.
        let (prepared, plugin_id, title, resizable, placement) = prepared;
        let size = match prepared {
            Prepared::AlreadyOpen(handle) => {
                return handle
                    .update(cx, |_, window, _| window.activate_window())
                    .map_err(|error| PluginProblem::WindowDidNotOpen {
                        plugin_id,
                        message: error.to_string(),
                    });
            }
            Prepared::Wanted(size) => size,
        };
        let owner: WindowOwner = std::rc::Rc::new(InTable {
            instance: id.clone(),
            plugins: self.downgrade(),
        });
        let request = WindowRequest {
            title: &title,
            size,
            resizable,
            placement,
            focus,
            own_process: false,
        };
        let opened = crate::window::open_window(&owner, request, cx);
        let (handle, parent, closed) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                // The plugin already holds what it needs for a window. Give it back.
                let _window = self.give_up_window(id);
                return Err(PluginProblem::WindowDidNotOpen {
                    plugin_id,
                    message: error.to_string(),
                });
            }
        };
        let placed = handle
            .update(cx, |_, window, cx| crate::window::placement_of(window, cx))
            .ok();
        // Three: the plugin fills it. A window whose plugin went while it opened, or that the
        // plugin refused, waits for the next poll to be taken down.
        let mut table = self.0.table.borrow_mut();
        let Some(hosted) = table.loaded.get_mut(id) else {
            table.finished_windows.push(handle);
            return Ok(());
        };
        let Hosted { window, plugin, .. } = hosted;
        let attached = match plugin.gui() {
            Some(gui) => window.attach(gui, handle, parent, closed),
            None => Err((PluginProblem::NoWindow { plugin_id }, handle)),
        };
        match attached {
            Ok(()) => {
                if let Some(placed) = placed {
                    table.place(id, placed);
                }
                Ok(())
            }
            Err((problem, handle)) => {
                table.finished_windows.push(handle);
                Err(problem)
            }
        }
    }

    /// The plugin and the piece it plays in.
    fn title(&self, hosted: &Hosted) -> String {
        let name = self
            .installed_name(hosted.format, &hosted.plugin_id)
            .unwrap_or_else(|| hosted.plugin_id.clone());
        let root = self.0.root.borrow();
        let folder = root.as_deref().and_then(std::path::Path::file_name);
        match folder {
            Some(folder) => format!("{name} — {}", folder.to_string_lossy()),
            None => name,
        }
    }

    pub fn close_window(&self, id: &InstanceId, cx: &mut gpui::App) {
        self.0.table.borrow_mut().keep_open(id, false);
        self.take_down(id, cx);
    }

    /// Leaves whether the window is to be open as it is.
    fn take_down(&self, id: &InstanceId, cx: &mut gpui::App) {
        if let Some(handle) = self.give_up_window(id) {
            crate::window::remove(handle, cx);
        }
    }

    /// A key the window got while the plugin's own view did not have the keyboard. `true` says
    /// the plugin used it. The window is checked: one that is going may still hand on a key.
    pub(crate) fn key(
        &self,
        id: &InstanceId,
        window: WindowId,
        keystroke: &Keystroke,
        direction: KeyDirection,
    ) -> bool {
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return false;
        };
        let Some(hosted) = table.loaded.get_mut(id) else {
            return false;
        };
        if !hosted.window.is(window) {
            return false;
        }
        hosted
            .plugin
            .gui()
            .is_some_and(|gui| gui.key(keystroke, direction))
    }

    /// The window is checked: one that went may still report a move on its way out.
    pub(crate) fn window_bounds_changed(
        &self,
        id: &InstanceId,
        window: WindowId,
        placement: Placement,
        content: WindowSize,
    ) {
        // A move GPUI reports from inside a call of ours that has the table is noted by that
        // call.
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return;
        };
        let Some(hosted) = table.loaded.get_mut(id) else {
            return;
        };
        if !hosted.window.is(window) {
            return;
        }
        let Hosted { window, plugin, .. } = hosted;
        if let Some(gui) = plugin.gui() {
            window.resized(gui, content);
        }
        table.place(id, placement);
    }

    /// Frees what the plugin holds for its window, and gives the window back to take down.
    #[must_use]
    fn give_up_window(&self, id: &InstanceId) -> Option<WindowHandle<PluginFrame>> {
        let mut table = self.0.table.borrow_mut();
        let hosted = table.loaded.get_mut(id)?;
        let finished = hosted.give_up_window();
        table.card_changed = true;
        finished
    }

    /// The window is going, whatever took it down. GPUI tells its observers while it still
    /// holds the window, so the plugin lets go of its view before that view is released. A
    /// window that still has its view here was closed by the composer.
    pub(crate) fn window_was_closed(&self, id: &InstanceId) {
        let mut table = self.0.table.borrow_mut();
        if let Some(hosted) = table.loaded.get_mut(id)
            && hosted.give_up_window().is_some()
        {
            table.card_changed = true;
            table.keep_open(id, false);
        }
    }

    /// The window work that needs the application, which a poll does not have: taking down
    /// windows of plugins that went, resizing, and opening the windows that are to be open.
    /// Call it after [`Self::poll`].
    pub fn settle_windows(&self, cx: &mut gpui::App) {
        let (finished, resize, reopen) = {
            let mut table = self.0.table.borrow_mut();
            let Table {
                loaded,
                retired,
                finished_windows,
                placements,
                ..
            } = &mut *table;
            let reopen: Vec<InstanceId> = loaded
                .iter()
                .filter(|(id, hosted)| {
                    hosted.has_window
                        && !hosted.window.is_open()
                        && placements.get(*id).is_some_and(|placement| placement.open)
                })
                .map(|(id, _)| id.clone())
                .collect();
            let resize: Vec<_> = loaded
                .values_mut()
                .chain(retired)
                .filter_map(|hosted| hosted.window.take_wanted_size())
                .collect();
            (std::mem::take(finished_windows), resize, reopen)
        };
        for (handle, wanted) in resize {
            crate::window::resize(handle, wanted, cx);
        }
        // The keys windows in plugins' own processes did not use play the computer keys here.
        {
            let mut table = self.0.table.borrow_mut();
            for hosted in table.loaded.values_mut() {
                if let Some(remote) = hosted.plugin.remote_window() {
                    remote.pass_spare_keys(cx);
                }
            }
        }
        for handle in finished {
            crate::window::remove(handle, cx);
        }
        // Wait while the application is becoming active, at the first polls: AppKit then gives
        // the keyboard to the front window, which a floating plugin window would be. One that
        // cannot come back is forgotten as open, so it is not tried at every poll.
        let active = cx.active_window();
        if reopen.is_empty() || (active.is_none() && !cx.windows().is_empty()) {
            return;
        }
        for id in reopen {
            if let Err(problem) = self.show_window(&id, false, cx) {
                let mut table = self.0.table.borrow_mut();
                table.keep_open(&id, false);
                table.window_problems.push(problem);
            }
        }
        // A plugin may take the keyboard as it is shown, which Six Sines does. It goes back to
        // the window that had it.
        if let Some(active) = active
            && cx.active_window() != Some(active)
        {
            active
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
    }

    /// For the application quitting, before anything of it is torn down, so no plugin is left
    /// holding a view of a window that is going.
    pub fn close_all_windows(&self, cx: &mut gpui::App) {
        let open: Vec<InstanceId> = {
            let table = self.0.table.borrow();
            let open = table.loaded.iter();
            open.filter(|(_, hosted)| hosted.window.is_open())
                .map(|(id, _)| id.clone())
                .collect()
        };
        // Still to be open, so they come back when the project opens again.
        for id in open {
            self.take_down(&id, cx);
        }
    }

    /// The first poll of a project reads where its plugin windows were.
    pub(super) fn remember_the_windows(
        &self,
        project: &Project,
        problems: &mut Vec<PluginProblem>,
    ) {
        if self.0.root.borrow().is_some() {
            return;
        }
        let root = project.root().to_path_buf();
        match self.0.placement_store.read(&root) {
            Ok(placements) => self.0.table.borrow_mut().placements = placements,
            Err(message) => problems.push(PluginProblem::WindowPlaces { message }),
        }
        *self.0.root.borrow_mut() = Some(root);
    }
}

/// Writes the placements when they changed. A window whose record is gone is forgotten on the
/// way, when the project is at hand.
pub(super) fn write_placements(
    table: &mut Table,
    store: &PlacementStore,
    root: &std::path::Path,
    project: Option<&Project>,
) -> Result<(), PluginProblem> {
    if !std::mem::take(&mut table.placements_changed) {
        return Ok(());
    }
    if let Some(project) = project {
        table
            .placements
            .retain(|id, _| project.resolve::<PluginRecord>(id).is_some());
    }
    store
        .write(root, &table.placements)
        .map_err(|message| PluginProblem::WindowPlaces { message })
}
