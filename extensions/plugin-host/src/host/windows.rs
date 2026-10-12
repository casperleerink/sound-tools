//! The plugins' own windows: opening, closing and settling them, the keys and moves they
//! hand on, and where they are kept on this machine.

use gpui::{Keystroke, WindowHandle, WindowId};
use sound_core::{InstanceId, Project};

use super::{Hosted, PluginProblem, Plugins, Table};
use crate::PluginRecord;
use crate::backend::KeyDirection;
use crate::placements::PlacementStore;
use crate::window::{Placement, PluginFrame, Prepared, WindowOwner, WindowRequest, WindowSize};

impl Table {
    /// Says whether the window of `id` is to be open, the next time it can be. Only the
    /// composer decides this: opening a window, closing one, or a plugin's own window that
    /// the composer closed. A window the host takes down because its plugin reloads, or because
    /// the project closes, stays open in here, and comes back.
    pub(super) fn keep_open(&mut self, id: &InstanceId, open: bool) {
        if let Some(placement) = self.placements.get_mut(id)
            && placement.open != open
        {
            placement.open = open;
            self.placements_changed = true;
        }
    }

    /// Notes where the window of `id` is now.
    pub(super) fn place(&mut self, id: &InstanceId, placement: Placement) {
        if self.placements.get(id) != Some(&placement) {
            self.placements.insert(id.clone(), placement);
            self.placements_changed = true;
        }
    }
}

impl Plugins {
    /// Whether the plugin of this record has a window of its own to open. `None` says the
    /// record has no plugin loaded at all, which is a plugin that did not load and is already
    /// reported; its card says that instead of offering a window.
    ///
    /// A card asks on every frame it draws. The answer is the one the plugin gave while it
    /// loaded, so this calls into no plugin, and it gives up rather than wait for a table that
    /// a plugin's own call has borrowed.
    pub fn window_offered(&self, id: &InstanceId) -> Option<bool> {
        let table = self.0.table.try_borrow().ok()?;
        table.loaded.get(id).map(|hosted| hosted.has_window)
    }

    /// Whether the window of this record's plugin is open. Read while drawing a card, so it
    /// gives up on a table that a plugin's own call has borrowed, as [`Self::window_offered`].
    pub fn window_is_open(&self, id: &InstanceId) -> bool {
        let Ok(table) = self.0.table.try_borrow() else {
            return false;
        };
        table
            .loaded
            .get(id)
            .is_some_and(|hosted| hosted.window.is_open())
    }

    /// Opens the plugin's own window, or brings the one that is open forward, for the composer.
    /// It opens where it was the last time, and the project remembers that it is open.
    pub fn open_window(&self, id: &InstanceId, cx: &mut gpui::App) -> Result<(), PluginProblem> {
        let opened = self.show_window(id, true, cx);
        let mut table = self.0.table.borrow_mut();
        table.keep_open(id, opened.is_ok());
        opened
    }

    /// Opens the window of `id`, taking the keyboard or not. See [`Self::open_window`].
    ///
    /// It is in three steps because the table may not be borrowed while GPUI runs: opening a
    /// window draws, and a card being drawn asks this host what its plugin has.
    fn show_window(
        &self,
        id: &InstanceId,
        focus: bool,
        cx: &mut gpui::App,
    ) -> Result<(), PluginProblem> {
        // One: what the plugin says, with the table borrowed and no GPUI in sight.
        let prepared = {
            let mut table = self.0.table.borrow_mut();
            let placement = table.placements.get(id).copied();
            let Some(hosted) = table.loaded.get_mut(id) else {
                // The record names a plugin this machine does not have, or the load failed.
                // That is reported, and the card shows it instead of offering a window.
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
            let prepared = match plugin.gui() {
                Some(gui) => window.prepare(gui),
                None => Err(no_window()),
            };
            // A plugin that turns out to have no window says so once. The card stops offering
            // one for the rest of this session, so the composer is not asked to find out again.
            // A VST 3 plugin is offered a window without being asked, because asking means
            // building its whole interface; this is where the answer arrives instead.
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
        let owner = WindowOwner {
            instance: id.clone(),
            plugins: self.downgrade(),
        };
        let request = WindowRequest {
            title: &title,
            size,
            resizable,
            placement,
            focus,
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

    /// What a plugin's window is called: the plugin and the piece it plays in.
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

    /// Closes the plugin's own window, for the composer. Its sound and its state are untouched,
    /// and the project remembers that it is closed.
    pub fn close_window(&self, id: &InstanceId, cx: &mut gpui::App) {
        self.0.table.borrow_mut().keep_open(id, false);
        self.take_down(id, cx);
    }

    /// Takes the window of `id` down, and leaves what the project remembers of it as it is.
    fn take_down(&self, id: &InstanceId, cx: &mut gpui::App) {
        // Outside the borrow: taking a window down runs GPUI.
        if let Some(handle) = self.give_up_window(id) {
            crate::window::remove(handle, cx);
        }
    }

    /// A key the window of `id` got while the plugin's own view did not have the keyboard.
    /// `true` says the plugin used it. See `backend::PluginGui::key`. The window is checked, as
    /// for a move: a window that is going may still hand on a key.
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

    /// The window of `id` moved or was resized. Where it is now is noted for the project, and
    /// a drag of its edge is given to the plugin. The window this says is checked, because one
    /// that went may still report a move on its way out.
    pub(crate) fn window_bounds_changed(
        &self,
        id: &InstanceId,
        window: WindowId,
        placement: Placement,
        content: WindowSize,
    ) {
        // A move that GPUI reports from inside a call of ours that has the table is noted by
        // that call itself.
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

    /// Frees whatever the plugin of `id` holds for a window and gives back the window it was
    /// in, for the caller to take down.
    #[must_use]
    fn give_up_window(&self, id: &InstanceId) -> Option<WindowHandle<PluginFrame>> {
        let mut table = self.0.table.borrow_mut();
        let hosted = table.loaded.get_mut(id)?;
        let finished = hosted.window.give_up(hosted.plugin.gui());
        table.card_changed = true;
        finished
    }

    /// The window is going, whatever took it down. GPUI tells its observers while it still
    /// holds the window, so this is the moment the plugin lets go of the view it is in, before
    /// that view is released. See `window::open_window`.
    ///
    /// A window this host took down has let go of its view already. One that still has it was
    /// closed by the composer, with its close control, and the project remembers that.
    pub(crate) fn window_was_closed(&self, id: &InstanceId) {
        let mut table = self.0.table.borrow_mut();
        if let Some(hosted) = table.loaded.get_mut(id)
            && hosted.window.give_up(hosted.plugin.gui()).is_some()
        {
            table.card_changed = true;
            table.keep_open(id, false);
        }
    }

    /// The window work that needs the application: taking down the windows of plugins that
    /// have gone, giving a window the size its plugin asked for, and opening the windows the
    /// project remembers as open whose plugin is loaded, which is how they come back when the
    /// project opens and when their plugin reloads. Whoever polls the host calls it after
    /// [`Self::poll`]; the moments that find such a plugin, a record that was deleted, an undo
    /// or a reload, have no application at hand.
    pub fn settle_windows(&self, cx: &mut gpui::App) {
        // Everything is read out first: running GPUI while the table is borrowed would let a
        // card that is drawn ask this host about its plugin.
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
        for handle in finished {
            crate::window::remove(handle, cx);
        }
        // A window that comes back by itself leaves the keyboard where it is. The application
        // is waited for while it is becoming active, which it is at the first polls: AppKit
        // gives the keyboard to the front window once it is, and a plugin's window, which
        // floats, would be that window. One that cannot come back is forgotten as open, so it
        // is not tried again at every poll.
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

    /// Frees the view of every plugin window and takes the windows down. The application
    /// calls it as it quits, before anything of it is torn down, so that no plugin is left
    /// holding a view of a window that is going.
    pub fn close_all_windows(&self, cx: &mut gpui::App) {
        let open: Vec<InstanceId> = {
            let table = self.0.table.borrow();
            let open = table.loaded.iter();
            open.filter(|(_, hosted)| hosted.window.is_open())
                .map(|(id, _)| id.clone())
                .collect()
        };
        // The project remembers them as open: they come back when it opens again.
        for id in open {
            self.take_down(&id, cx);
        }
    }

    /// The first poll of a project reads where its plugin windows were. Every host reads it,
    /// and only the one that has windows opens any, see [`Self::settle_windows`].
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

/// Keeps where the plugin windows are in this machine's store, when they changed since the
/// last write. A window whose record is gone is forgotten on the way, when the project is at
/// hand. A write that fails is said once and not tried again until a window changes again.
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
