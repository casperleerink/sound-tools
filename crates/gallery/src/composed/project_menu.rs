//! Project chrome: the project name top-left as a quiet menu holding add, undo/redo, the output
//! device and the project folder. Nothing else lives in the window chrome.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem, Trigger};

fn entries() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Group(
            MenuGroup::new().item(MenuItem::new("add", "Add tool…").selectable(false)),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(
            MenuGroup::new().items([
                MenuItem::new("undo", "Undo")
                    .shortcut("mod+z")
                    .selectable(false),
                MenuItem::new("redo", "Redo")
                    .shortcut("shift+mod+z")
                    .selectable(false),
            ]),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Output device").items([
            MenuItem::new("speakers", "MacBook Pro Speakers"),
            MenuItem::new("scarlett", "Scarlett 2i2"),
        ])),
        MenuEntry::Separator,
        MenuEntry::Group(
            MenuGroup::new()
                .item(MenuItem::new("reveal", "Reveal project folder").selectable(false)),
        ),
    ]
}

pub struct ProjectMenu {
    menu: Entity<DropdownMenu>,
}

impl ProjectMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let menu = cx.new(|cx| {
            DropdownMenu::new("Night Study", entries(), cx)
                .selected("speakers")
                .trigger(Trigger::Ghost)
                .width(260.)
        });
        Self { menu }
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.update(cx, |menu, cx| menu.open(window, cx));
    }
}

impl Render for ProjectMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (stage, border) = (theme.gray_100, theme.alpha_at(0.10));

        div()
            .flex_none()
            .w(px(900.))
            .h(px(200.))
            .rounded(px(12.))
            .bg(stage)
            .border_1()
            .border_color(border)
            .p(px(16.))
            .flex()
            .items_start()
            .child(self.menu.clone())
    }
}
