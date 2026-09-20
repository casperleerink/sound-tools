//! The project menu: the project name top-left as a quiet menu. Add track, undo and redo with
//! what they would do, the output device by name, and the project folder.

use gpui::{Context, Entity, IntoElement, Render, SharedString, Window, prelude::*};
use sound_core::Project;
use sound_ui::Session;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked,
};

use crate::{add_track, main_arrangement};

const ADD_TRACK: &str = "add-track";
const UNDO: &str = "undo";
const REDO: &str = "redo";
const DEVICE: &str = "device";
const REVEAL: &str = "reveal";

pub struct ProjectMenu {
    session: Entity<Session>,
    device_name: SharedString,
    menu: Entity<DropdownMenu>,
}

impl ProjectMenu {
    pub fn new(
        session: Entity<Session>,
        device_name: SharedString,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = session.read(cx).project();
        let name = project.root().file_name().unwrap_or_default();
        let name = name.to_string_lossy().into_owned();
        let items = entries(project, &device_name);
        let menu = cx.new(|cx| {
            DropdownMenu::new(name, items, cx)
                .selected(DEVICE)
                .ghost(true)
                .width(280.)
        });
        // Undo and redo say what they would do, so the items follow every change.
        cx.observe(&session, |this, session, cx| {
            let items = entries(session.read(cx).project(), &this.device_name);
            this.menu.update(cx, |menu, cx| menu.set_entries(items, cx));
        })
        .detach();
        cx.subscribe(&menu, Self::on_picked).detach();
        Self {
            session,
            device_name,
            menu,
        }
    }

    pub fn menu(&self) -> &Entity<DropdownMenu> {
        &self.menu
    }

    fn on_picked(&mut self, _: Entity<DropdownMenu>, picked: &MenuPicked, cx: &mut Context<Self>) {
        // An error from any of these shows as the notice of the session.
        self.session
            .update(cx, |session, cx| match picked.0.as_ref() {
                ADD_TRACK => {
                    if let Some(arrangement) = main_arrangement(session.project()) {
                        session.edit(cx, |project| add_track(project, &arrangement));
                    }
                }
                UNDO => {
                    session.edit(cx, Project::undo);
                }
                REDO => {
                    session.edit(cx, Project::redo);
                }
                REVEAL => cx.reveal_path(session.project().root()),
                // Switching the device is not built yet. The menu only names it.
                _ => {}
            });
    }
}

fn entries(project: &Project, device_name: &SharedString) -> Vec<MenuEntry> {
    let command =
        |value: &'static str, label: String| MenuItem::new(value, label).selectable(false);
    let history = |value, verb: &str, label: Option<&str>, shortcut: &'static str| {
        let text = label.map_or(verb.to_string(), |label| format!("{verb} {label}"));
        command(value, text)
            .shortcut(shortcut)
            .disabled(label.is_none())
    };
    vec![
        MenuEntry::Group(
            MenuGroup::new().item(
                command(ADD_TRACK, "Add track".to_string())
                    .disabled(main_arrangement(project).is_none()),
            ),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().items([
            history(UNDO, "Undo", project.undo_label(), "mod+z"),
            history(REDO, "Redo", project.redo_label(), "shift+mod+z"),
        ])),
        MenuEntry::Separator,
        MenuEntry::Group(
            MenuGroup::new()
                .label("Output device")
                .item(MenuItem::new(DEVICE, device_name.clone())),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(
            MenuGroup::new().item(command(REVEAL, "Reveal project folder".to_string())),
        ),
    ]
}

impl Render for ProjectMenu {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.menu.clone()
    }
}
