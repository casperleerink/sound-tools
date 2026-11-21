//! The plugins' own windows, and where they are kept on this machine. The app makes the windows,
//! see [`crate::window`].

use sound_core::{InstanceId, Project};

use super::{Hosted, PluginProblem, Plugins, Table};
use crate::PluginRecord;
use crate::placements::PlacementStore;
use crate::window::{
    KeyDirection, Keystroke, ParentView, Placement, Prepared, WindowCommand, WindowId, WindowPlace,
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

    fn place(&mut self, id: &InstanceId, placement: Placement) {
        if self.placements.get(id) != Some(&placement) {
            self.placements.insert(id.clone(), placement);
            self.placements_changed = true;
        }
    }

    /// The window is the app's to take down; the plugin has let go of its view.
    pub(super) fn close(&mut self, window: Option<WindowId>) {
        if let Some(window) = window {
            self.commands.push(WindowCommand::Close { window });
            self.card_changed = true;
        }
    }

    fn owner(&mut self, window: WindowId) -> Option<(InstanceId, &mut Hosted)> {
        let (id, hosted) = self
            .loaded
            .iter_mut()
            .find(|(_, hosted)| hosted.window.is(window))?;
        Some((id.clone(), hosted))
    }
}

impl Plugins {
    /// `None` when no plugin is loaded for the record. A card asks on every frame, so this
    /// calls into no plugin, and gives up on a table that a plugin's own call has borrowed.
    pub fn window_offered(&self, id: &InstanceId) -> Option<bool> {
        let table = self.0.table.try_borrow().ok()?;
        table.loaded.get(id).map(|hosted| hosted.has_window)
    }

    /// From the moment it is asked for. Read while drawing a card, as [`Self::window_offered`].
    pub fn window_is_open(&self, id: &InstanceId) -> bool {
        let Ok(table) = self.0.table.try_borrow() else {
            return false;
        };
        table
            .loaded
            .get(id)
            .is_some_and(|hosted| hosted.window.is_open())
    }

    /// Opens the plugin's own window where it was last, or brings it forward. Not an edit and
    /// no undo step.
    pub fn open_window(&self, id: &InstanceId) -> Result<(), PluginProblem> {
        let opened = self.show_window(id, true);
        let mut table = self.0.table.borrow_mut();
        table.keep_open(id, opened.is_ok());
        opened
    }

    fn show_window(&self, id: &InstanceId, focus: bool) -> Result<(), PluginProblem> {
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded,
            placements,
            commands,
            card_changed,
            windows_made,
            ..
        } = &mut *table;
        let Some(hosted) = loaded.get_mut(id) else {
            // The plugin did not load. That is reported already.
            return Ok(());
        };
        let title = self.title(hosted);
        let window = WindowId::next(windows_made);
        let Hosted {
            window: plugin_window,
            plugin,
            has_window,
            plugin_id,
            ..
        } = hosted;
        let prepared = match plugin.gui() {
            Some(gui) => plugin_window.prepare(gui, window),
            None => Err(PluginProblem::NoWindow {
                plugin_id: plugin_id.clone(),
            }),
        };
        // A plugin that turns out to have no window says so once, and the card stops offering
        // one for the rest of this session.
        if matches!(prepared, Err(PluginProblem::NoWindow { .. })) {
            *has_window = false;
        }
        *card_changed = true;
        commands.push(match prepared? {
            Prepared::AlreadyOpen(window) => WindowCommand::Focus { window },
            Prepared::Opening(size) => WindowCommand::Open {
                window,
                title,
                size,
                resizable: plugin_window.resizable(),
                placement: placements.get(id).copied(),
                focus,
            },
        });
        Ok(())
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

    pub fn close_window(&self, id: &InstanceId) {
        let mut table = self.0.table.borrow_mut();
        table.keep_open(id, false);
        let finished = (table.loaded.get_mut(id)).and_then(|hosted| hosted.give_up_window());
        table.close(finished);
    }

    /// The app made `window`, and the plugin's view goes in `parent`. `None` is a run that
    /// makes no windows: the plugin is shown with nothing to draw in. A plugin that refuses
    /// closes it again, and the next poll says why.
    ///
    /// # Safety
    ///
    /// `parent` is the view of `window`, alive until the host gives [`WindowCommand::Close`]
    /// for it or hears [`Self::window_closed`].
    pub unsafe fn window_created(&self, window: WindowId, parent: Option<ParentView>) {
        let mut table = self.0.table.borrow_mut();
        // A plugin that went meanwhile has given its `Close` already.
        let Some((id, hosted)) = table.owner(window) else {
            return;
        };
        let attached = match hosted.plugin.gui() {
            // SAFETY: the caller keeps `parent` alive until the host lets go of the window,
            // which frees the plugin's view first.
            Some(gui) => unsafe { hosted.window.attach(gui, parent) },
            None => Err(PluginProblem::NoWindow {
                plugin_id: hosted.plugin_id.clone(),
            }),
        };
        if let Err(problem) = attached {
            let finished = hosted.give_up_window();
            table.close(finished);
            table.keep_open(&id, false);
            table.window_problems.push(problem);
        }
    }

    /// The window moved or its content changed size: the composer dragged it, or it took a
    /// size this host gave. The app reads it after every change, and once it is made.
    pub fn window_moved(&self, window: WindowId, place: WindowPlace) {
        // A move from inside a call of ours, which a plugin can cause, is not heard; the next is.
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return;
        };
        let Some((id, hosted)) = table.owner(window) else {
            return;
        };
        if let Some(gui) = hosted.plugin.gui() {
            hosted.window.resized(gui, &place);
        }
        table.place(&id, Placement::at(place.position, &place.display));
    }

    /// The composer closed the window. Call it before the native view goes, so the plugin
    /// lets go of its view first.
    pub fn window_closed(&self, window: WindowId) {
        let mut table = self.0.table.borrow_mut();
        let Some((id, hosted)) = table.owner(window) else {
            return;
        };
        let _closing = hosted.give_up_window();
        table.card_changed = true;
        table.keep_open(&id, false);
    }

    /// A key the window got while the plugin's own view did not have the keyboard. `true` says
    /// the plugin used it; else it plays the computer keys, as in the main window, and so does
    /// every key up, so a note those started ends.
    pub fn window_key(
        &self,
        window: WindowId,
        keystroke: &Keystroke,
        direction: KeyDirection,
    ) -> bool {
        let Ok(mut table) = self.0.table.try_borrow_mut() else {
            return false;
        };
        let Some((_, hosted)) = table.owner(window) else {
            return false;
        };
        hosted
            .plugin
            .gui()
            .is_some_and(|gui| gui.key(keystroke, direction))
    }

    /// The window work since the last call, for the app to do, with the sizes plugins asked for
    /// and the windows that are to be open again. Call it after [`Self::poll`].
    ///
    /// Windows wait to open again while the app is not active, as at its first polls: AppKit
    /// would give the keyboard to the front window, which a floating plugin window would be. One
    /// that cannot come back is forgotten as open, so it is not tried every time.
    pub fn settle_windows(&self, app_is_active: bool) -> Vec<WindowCommand> {
        let reopen: Vec<InstanceId> = {
            let mut table = self.0.table.borrow_mut();
            let Table {
                loaded,
                retired,
                placements,
                commands,
                ..
            } = &mut *table;
            let resize = loaded.values_mut().chain(retired).filter_map(|hosted| {
                let (window, size) = hosted.window.take_wanted_size()?;
                Some(WindowCommand::Resize { window, size })
            });
            commands.extend(resize);
            let open_again = loaded.iter().filter(|(id, hosted)| {
                hosted.has_window
                    && !hosted.window.is_open()
                    && placements.get(*id).is_some_and(|placement| placement.open)
            });
            match app_is_active {
                true => open_again.map(|(id, _)| id.clone()).collect(),
                false => Vec::new(),
            }
        };
        for id in reopen {
            if let Err(problem) = self.show_window(&id, false) {
                let mut table = self.0.table.borrow_mut();
                table.keep_open(&id, false);
                table.window_problems.push(problem);
            }
        }
        std::mem::take(&mut self.0.table.borrow_mut().commands)
    }

    /// For the app quitting, before any window of it goes: every plugin lets go of its view, and
    /// gives the window to close. They stay open in the placements, so they come back when the
    /// project opens again.
    #[must_use]
    pub fn close_all_windows(&self) -> Vec<WindowId> {
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded, commands, ..
        } = &mut *table;
        let mut windows: Vec<WindowId> = std::mem::take(commands)
            .into_iter()
            .filter_map(|command| match command {
                WindowCommand::Close { window } => Some(window),
                _ => None,
            })
            .collect();
        windows.extend(loaded.values_mut().filter_map(Hosted::give_up_window));
        table.card_changed = true;
        windows
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

impl Hosted {
    /// Frees what the plugin holds for its window, and gives the window back to take down.
    #[must_use]
    pub(super) fn give_up_window(&mut self) -> Option<WindowId> {
        self.window.give_up(self.plugin.gui())
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
