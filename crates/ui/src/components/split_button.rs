//! Split button: a main half that does the usual thing, and a chevron beside it that opens a
//! menu of every choice, the usual one included. Quiet like a ghost button: muted words, no
//! border and no fill until the pointer is on a half. Each half is a tab stop.
//!
//! Stateful, because the menu is. Both halves emit [`MenuPicked`]: the main half with the
//! value of the menu item it stands for, so whoever holds it handles one event for both.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    MouseButton, Render, SharedString, Window, div, prelude::*, px,
};

use crate::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuPicked, Trigger};
use crate::components::icon::Icon;
use crate::components::popover::Align;
use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

/// The height of both halves.
pub const HEIGHT: f32 = 24.;
/// The size of the icon before the words.
const ICON_SIZE: f32 = 14.;

/// A split button that is a row of a list, such as a header: as wide as it is given, its main
/// half taking the room the chevron leaves. Its icon and words start where the row says, from
/// its left edge, so they line up with what the rows around it show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub height: f32,
    pub icon_left: f32,
    pub words_left: f32,
}

/// What a split button says and offers.
pub struct SplitChoices {
    /// What the main half says.
    pub label: SharedString,
    /// The value of the item of `entries` that the main half picks.
    pub main_value: SharedString,
    /// The tooltip of the chevron, which shows no words.
    pub menu_label: SharedString,
    pub entries: Vec<MenuEntry>,
}

pub struct SplitButton {
    label: SharedString,
    icon: Option<SharedString>,
    main_value: SharedString,
    focus_handle: FocusHandle,
    main_focus: FocusHandle,
    keyboard_focus: KeyboardFocus,
    menu: Entity<DropdownMenu>,
    row: Option<Row>,
    /// What a test looks the main half up by. The chevron is `<name>-menu`.
    name: SharedString,
}

impl EventEmitter<MenuPicked> for SplitButton {}

impl SplitButton {
    /// `name` is what tests find the halves by: `<name>` and `<name>-menu`.
    pub fn new(
        name: impl Into<SharedString>,
        choices: SplitChoices,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = name.into();
        let menu = cx.new(|cx| {
            DropdownMenu::new(choices.menu_label, choices.entries, cx)
                .trigger(Trigger::Chevron)
                .debug_name(format!("{name}-menu"))
                .width(200.)
        });
        cx.subscribe(&menu, |_, _, picked: &MenuPicked, cx| {
            cx.emit(picked.clone())
        })
        .detach();
        Self {
            label: choices.label,
            icon: None,
            main_value: choices.main_value,
            focus_handle: cx.focus_handle(),
            main_focus: cx.focus_handle().tab_stop(true),
            keyboard_focus: KeyboardFocus::default(),
            menu,
            row: None,
            name,
        }
    }

    /// An icon before the words of the main half, such as `plus`.
    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// Opens the menu from the right edge of the chevron, for a button at the right of its view.
    pub fn align_end(self, cx: &mut Context<Self>) -> Self {
        self.menu
            .update(cx, |menu, cx| menu.set_align(Align::End, cx));
        self
    }

    /// Makes it a row of a list, see [`Row`].
    pub fn row(mut self, row: Row, cx: &mut Context<Self>) -> Self {
        self.row = Some(row);
        self.menu
            .update(cx, |menu, cx| menu.set_trigger_height(row.height, cx));
        self
    }

    pub fn menu(&self) -> &Entity<DropdownMenu> {
        &self.menu
    }
}

/// The whole button, not a tab stop: `on_focus_in` of it hears a focus on either half, or in
/// the open menu.
impl Focusable for SplitButton {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SplitButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (text, hover, line, ring) = (
            theme.gray_800,
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.lavender,
        );
        let ring_shows = self.keyboard_focus.shows_ring(&self.main_focus, window);
        let name = self.name.clone();
        // Inside the border of the focus ring.
        let (height, padding, gap) = match self.row {
            Some(row) if self.icon.is_some() => {
                let gap = row.words_left - row.icon_left - ICON_SIZE;
                (row.height, row.icon_left - 1., gap)
            }
            Some(row) => (row.height, row.words_left - 1., 8.),
            None => (HEIGHT, 8., 8.),
        };
        let main = div()
            .id("split-main")
            .debug_selector(move || name.to_string())
            .track_focus(&self.main_focus)
            .flex()
            .items_center()
            .gap(px(gap))
            .h(px(height))
            .pl(px(padding))
            .when(self.row.is_some(), |main| main.flex_1())
            .pr(px(10.))
            .rounded_l(px(6.))
            .border_1()
            .border_color(match ring_shows {
                true => ring,
                false => Hsla::transparent_black(),
            })
            .text_size(px(12.))
            .line_height(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.keyboard_focus.pressed(cx)),
            )
            // A click, or enter or space while it has the focus.
            .on_click(cx.listener(|this, _, _, cx| cx.emit(MenuPicked(this.main_value.clone()))))
            .when_some(self.icon.clone(), |main, icon| {
                main.child(Icon::new(icon).size(ICON_SIZE).color(text))
            })
            .child(self.label.clone());
        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_none()
            .when(self.row.is_some(), |button| button.w_full())
            .items_center()
            .child(main)
            .child(div().w(px(1.)).h(px(12.)).bg(line))
            .child(self.menu.clone())
    }
}
