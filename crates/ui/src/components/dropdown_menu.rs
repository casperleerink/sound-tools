//! Dropdown menu: labelled groups, radio items with an optional second line,
//! separators and a group that scrolls. Ported from the source design system's
//! `dropdown-menu.tsx` and the desktop app's model picker. The list itself
//! (`MenuList`) is reused by `select.rs`.

use std::rc::Rc;

use gpui::{
    App, Context, Div, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    MouseDownEvent, Render, RenderOnce, SharedString, Stateful, StyleRefinement, Styled, Window,
    div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::components::kbd::Kbd;
use crate::components::popover::{Align, Side, TRIGGER_HEIGHT, anchor, surface, trigger};
use crate::theme::ActiveTheme;

const ROW_HEIGHT: f32 = 32.;

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub value: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    icon: Option<SharedString>,
    shortcut: Option<SharedString>,
    disabled: bool,
    selectable: bool,
}

impl MenuItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            icon: None,
            shortcut: None,
            disabled: false,
            selectable: true,
        }
    }

    /// Muted second line, as in the model picker.
    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Leading icon name from `crates/ui/assets/icons`.
    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// Keyboard hint drawn at the end of the row, e.g. `"mod+z"`.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Commands (`Undo`, `Reveal project folder`) run and close the menu without becoming the
    /// menu's selected value; only radio-style items do.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn label(&self) -> SharedString {
        self.label.clone()
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
}

#[derive(Clone, Debug, Default)]
pub struct MenuGroup {
    label: Option<SharedString>,
    items: Vec<MenuItem>,
    /// Scroll the items past this height, leaving the rest of the menu in place.
    max_height: Option<f32>,
}

impl MenuGroup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    pub fn item(mut self, item: MenuItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = MenuItem>) -> Self {
        self.items.extend(items);
        self
    }
}

#[derive(Clone, Debug)]
pub enum MenuEntry {
    Group(MenuGroup),
    Separator,
}

/// Every item in render order, with its keyboard index.
fn flat(entries: &[MenuEntry]) -> Vec<&MenuItem> {
    entries
        .iter()
        .flat_map(|entry| match entry {
            MenuEntry::Group(group) => group.items.iter(),
            MenuEntry::Separator => [].as_slice().iter(),
        })
        .collect()
}

type SelectFn = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

/// The menu body: groups, separators, rows. Shared by the dropdown and select.
#[derive(IntoElement)]
pub struct MenuList {
    base: Div,
    entries: Vec<MenuEntry>,
    selected: Option<SharedString>,
    highlighted: usize,
    on_select: Option<SelectFn>,
}

impl MenuList {
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self {
            base: div(),
            entries,
            selected: None,
            highlighted: usize::MAX,
            on_select: None,
        }
    }

    pub fn selected(mut self, value: Option<SharedString>) -> Self {
        self.selected = value;
        self
    }

    pub fn highlighted(mut self, index: usize) -> Self {
        self.highlighted = index;
        self
    }

    pub fn on_select(mut self, f: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl Styled for MenuList {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for MenuList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, hover, line, text) = (
            theme.gray_700,
            theme.alpha_at(0.10),
            theme.alpha_at(0.10),
            theme.gray_950,
        );
        let on_select = self.on_select;
        let selected = self.selected;
        let highlighted = self.highlighted;
        let mut index = 0;

        let groups = self
            .entries
            .into_iter()
            .enumerate()
            .map(|(group_ix, entry)| match entry {
                MenuEntry::Separator => div().h(px(1.)).mx(px(8.)).bg(line).into_any_element(),
                MenuEntry::Group(group) => {
                    let rows: Vec<_> = group
                        .items
                        .into_iter()
                        .map(|item| {
                            let row_ix = index;
                            index += 1;
                            let is_selected = selected.as_ref() == Some(&item.value);
                            let on_select = on_select.clone();
                            let value = item.value.clone();
                            div()
                                .id(("menu-row", row_ix))
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(12.))
                                .min_h(px(ROW_HEIGHT))
                                .py(px(4.))
                                .pl(px(10.))
                                .pr(px(8.))
                                .rounded(px(8.))
                                .text_size(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .when(item.disabled, |d| d.opacity(0.4))
                                .when(!item.disabled, |d| {
                                    d.cursor_pointer()
                                        .hover(|s| s.bg(hover))
                                        .when(row_ix == highlighted, |d| d.bg(hover))
                                })
                                .when_some(item.icon.clone(), |d, name| {
                                    d.child(Icon::new(name).size(16.).color(text))
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w_0()
                                        .flex_1()
                                        .child(div().truncate().child(item.label.clone()))
                                        .when_some(item.description.clone(), |d, description| {
                                            d.child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::NORMAL)
                                                    .text_color(muted)
                                                    .child(description),
                                            )
                                        }),
                                )
                                .when_some(item.shortcut.clone(), |d, shortcut| {
                                    d.child(Kbd::new(shortcut))
                                })
                                .when(is_selected, |d| {
                                    d.child(Icon::new("check").size(16.).color(text))
                                })
                                .when_some(on_select.filter(|_| !item.disabled), |d, f| {
                                    d.on_click(move |_, window, cx| f(value.clone(), window, cx))
                                })
                        })
                        .collect();

                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .p(px(8.))
                        .when_some(group.label, |d, label| {
                            d.child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(muted)
                                    .child(label),
                            )
                        })
                        .map(|d| match group.max_height {
                            Some(height) => d.child(
                                div()
                                    .id(("menu-scroll", group_ix))
                                    .overflow_y_scroll()
                                    .max_h(px(height))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .children(rows),
                            ),
                            None => d.children(rows),
                        })
                        .into_any_element()
                }
            })
            .collect::<Vec<_>>();

        self.base.flex().flex_col().children(groups)
    }
}

/// Quiet version of the shared trigger: no border or fill, just a hover wash.
fn ghost_trigger(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, text) = (theme.alpha_at(0.05), theme.gray_950);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(TRIGGER_HEIGHT))
        .px(px(8.))
        .rounded(px(8.))
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// Emitted with the value of the item that was picked, for commands and radio items alike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuPicked(pub SharedString);

impl EventEmitter<MenuPicked> for DropdownMenu {}

pub struct DropdownMenu {
    focus_handle: FocusHandle,
    /// The trigger is a tab stop. Enter opens the menu, and closing gives the focus back.
    trigger_focus: FocusHandle,
    label: SharedString,
    entries: Vec<MenuEntry>,
    selected: Option<SharedString>,
    highlighted: usize,
    open: bool,
    side: Side,
    align: Align,
    width: f32,
    ghost: bool,
}

impl DropdownMenu {
    pub fn new(
        label: impl Into<SharedString>,
        entries: Vec<MenuEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            trigger_focus: cx.focus_handle().tab_stop(true),
            label: label.into(),
            entries,
            selected: None,
            highlighted: usize::MAX,
            open: false,
            side: Side::default(),
            align: Align::default(),
            width: 320.,
            ghost: false,
        }
    }

    pub fn selected(mut self, value: impl Into<SharedString>) -> Self {
        self.selected = Some(value.into());
        self
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Quiet trigger: no border or fill, muted label. For the project name and the model picker.
    pub fn ghost(mut self, ghost: bool) -> Self {
        self.ghost = ghost;
        self
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    /// Replaces the items, for a menu whose labels follow the application, such as
    /// `Undo Move clip`.
    pub fn set_entries(&mut self, entries: Vec<MenuEntry>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.highlighted = usize::MAX;
        cx.notify();
    }

    pub fn set_selected(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.selected = Some(value.into());
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.highlighted = usize::MAX;
        // The open menu held the focus. Without this it would be nowhere, and keys with it.
        window.focus(&self.trigger_focus, cx);
        cx.notify();
    }

    fn pick(&mut self, value: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let selectable = flat(&self.entries)
            .iter()
            .find(|item| item.value == value)
            .is_none_or(|item| item.selectable);
        if selectable {
            self.selected = Some(value.clone());
        }
        self.close(window, cx);
        cx.emit(MenuPicked(value));
    }

    /// Move the highlight, skipping disabled rows.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let items = flat(&self.entries);
        let count = items.len();
        if count == 0 {
            return;
        }
        let mut next = if self.highlighted == usize::MAX {
            if delta > 0 { 0 } else { count - 1 }
        } else {
            (self.highlighted as isize + delta).rem_euclid(count as isize) as usize
        };
        for _ in 0..count {
            if !items[next].disabled {
                break;
            }
            next = (next as isize + delta).rem_euclid(count as isize) as usize;
        }
        self.highlighted = next;
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            "down" => self.step(1, cx),
            "up" => self.step(-1, cx),
            "enter" => {
                let value = flat(&self.entries)
                    .get(self.highlighted)
                    .map(|item| item.value.clone());
                if let Some(value) = value {
                    self.pick(value, window, cx);
                }
            }
            _ => {}
        }
    }
}

impl Render for DropdownMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (side, align, width) = (self.side, self.align, self.width);
        let trigger_element = if self.ghost {
            ghost_trigger("dropdown-trigger", cx)
        } else {
            trigger("dropdown-trigger", cx)
        };
        let (muted, ring) = (cx.theme().gray_700, cx.theme().lavender);

        div()
            .relative()
            .flex()
            .flex_none()
            .child(
                trigger_element
                    .track_focus(&self.trigger_focus)
                    .border_1()
                    .focus_visible(move |s| s.border_color(ring))
                    .child(self.label.clone())
                    .child(Icon::new("chevron-down").size(14.).color(muted))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.open {
                            this.close(window, cx);
                        } else {
                            this.open(window, cx);
                        }
                    })),
            )
            .when(self.open, |d| {
                d.child(anchor(
                    side,
                    align,
                    surface(cx)
                        .track_focus(&self.focus_handle)
                        .w(px(width))
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            this.close(window, cx)
                        }))
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                            this.on_key(ev, window, cx)
                        }))
                        .child(
                            MenuList::new(self.entries.clone())
                                .selected(self.selected.clone())
                                .highlighted(self.highlighted)
                                .on_select(cx.processor(
                                    |this, value: SharedString, window, cx| {
                                        this.pick(value, window, cx)
                                    },
                                )),
                        ),
                ))
            })
    }
}
