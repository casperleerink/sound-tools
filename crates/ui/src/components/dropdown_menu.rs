//! Dropdown menu: labelled groups, radio items with an optional second line,
//! separators and a group that scrolls. Ported from the source design system's
//! `dropdown-menu.tsx` and the desktop app's model picker.
//!
//! [`Trigger::Select`] makes it a select: a 24 pt trigger that says what is picked, for a list
//! that does not fit as segments, such as the shape of an EQ band. When the picked item has an
//! icon, the trigger shows the icon and not the words, so that it fits in a cell of a card.
//!
//! [`Select`](super::select::Select) is the same list as a control on saved state: the owner
//! gives the value on every render and keeps no menu of its own.
//!
//! [`Trigger::Chevron`] is the menu half of a [`crate::components::split_button::SplitButton`].

use std::rc::Rc;

use gpui::{
    App, Context, Div, ElementId, EventEmitter, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    MouseDownEvent, Render, RenderOnce, SharedString, Stateful, StyleRefinement, Styled, Window,
    div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::components::kbd::Kbd;
use crate::components::popover::{Align, Side, TRIGGER_HEIGHT, anchor, surface, trigger};
use crate::components::tooltip::Tooltip;
use crate::theme::ActiveTheme;

const ROW_HEIGHT: f32 = 32.;
/// The rounded square an item icon sits on.
const ICON_TILE: f32 = 24.;

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub value: SharedString,
    label: SharedString,
    /// The muted second line, which says why a row cannot be picked when it cannot.
    pub description: Option<SharedString>,
    icon: Option<SharedString>,
    shortcut: Option<SharedString>,
    disabled: bool,
    selectable: bool,
    checked: bool,
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
            checked: false,
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

    /// Shows the check whatever the menu's selected value is, for a menu with more than one
    /// select in it, such as the agent's approvals and model. The owner says what is picked in
    /// each and makes the items not selectable.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
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
}

impl MenuGroup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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
    /// A quiet line that is not an item: what a source of items is still doing, or what it has
    /// to say about them. It cannot be picked and the keyboard skips it. Notes sit under the
    /// groups, and stay in view when the groups scroll.
    Note(SharedString),
}

/// Every item in render order, with its keyboard index.
pub(crate) fn flat(entries: &[MenuEntry]) -> Vec<&MenuItem> {
    entries
        .iter()
        .flat_map(|entry| match entry {
            MenuEntry::Group(group) => group.items.iter(),
            MenuEntry::Separator | MenuEntry::Note(_) => [].as_slice().iter(),
        })
        .collect()
}

/// The row the highlight moves to from `highlighted`, `delta` rows on, skipping disabled rows
/// and going round. From no highlight, `usize::MAX`, down is the first row and up the last.
/// `None` for a menu with no rows.
pub(crate) fn highlight_step(
    entries: &[MenuEntry],
    highlighted: usize,
    delta: isize,
) -> Option<usize> {
    let items = flat(entries);
    let count = items.len();
    if count == 0 {
        return None;
    }
    let mut next = if highlighted == usize::MAX {
        if delta > 0 { 0 } else { count - 1 }
    } else {
        (highlighted as isize + delta).rem_euclid(count as isize) as usize
    };
    for _ in 0..count {
        if !items[next].disabled {
            break;
        }
        next = (next as isize + delta).rem_euclid(count as isize) as usize;
    }
    Some(next)
}

type SelectFn = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

/// The menu body: groups, separators, rows.
#[derive(IntoElement)]
pub struct MenuList {
    base: Div,
    entries: Vec<MenuEntry>,
    selected: Option<SharedString>,
    highlighted: usize,
    max_height: Option<f32>,
    on_select: Option<SelectFn>,
}

impl MenuList {
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self {
            base: div(),
            entries,
            selected: None,
            highlighted: usize::MAX,
            max_height: None,
            on_select: None,
        }
    }

    /// Scroll the groups past this height.
    pub fn max_height(mut self, height: Option<f32>) -> Self {
        self.max_height = height;
        self
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
        let (muted, hover, line, text, tile) = (
            theme.gray_700,
            theme.alpha_at(0.10),
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.alpha_at(0.06),
        );
        let on_select = self.on_select;
        let selected = self.selected;
        let highlighted = self.highlighted;
        let mut index = 0;
        let mut notes = Vec::new();

        let groups = self
            .entries
            .into_iter()
            .filter_map(|entry| match entry {
                MenuEntry::Separator => {
                    Some(div().h(px(1.)).mx(px(8.)).bg(line).into_any_element())
                }
                MenuEntry::Note(note) => {
                    notes.push(note);
                    None
                }
                MenuEntry::Group(group) => {
                    let rows: Vec<_> = group
                        .items
                        .into_iter()
                        .map(|item| {
                            let row_ix = index;
                            index += 1;
                            let is_selected =
                                item.checked || selected.as_ref() == Some(&item.value);
                            let on_select = on_select.clone();
                            let value = item.value.clone();
                            let selector = item.value.clone();
                            div()
                                .id(("menu-row", row_ix))
                                .debug_selector(move || format!("menu-{selector}"))
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(10.))
                                .min_h(px(ROW_HEIGHT))
                                .py(px(4.))
                                .pl(px(if item.icon.is_some() { 6. } else { 10. }))
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
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .items_center()
                                            .justify_center()
                                            .size(px(ICON_TILE))
                                            .rounded(px(6.))
                                            .bg(tile)
                                            .child(Icon::new(name).size(16.).color(text)),
                                    )
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w_0()
                                        .flex_1()
                                        .child(div().truncate().child(item.label.clone()))
                                        .when_some(item.description.clone(), |d, description| {
                                            // The label truncates and this wraps: a second
                                            // line is there to be read, and a row is as tall
                                            // as it needs.
                                            d.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
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

                    let element = div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .px(px(8.))
                        .py(px(4.))
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
                        .children(rows)
                        .into_any_element();
                    Some(element)
                }
            })
            .collect::<Vec<_>>();
        let notes = notes.into_iter().enumerate().map(|(note_ix, note)| {
            div()
                .id(("menu-note", note_ix))
                // What a test looks a quiet line up by, as a row is looked up by its value.
                .debug_selector(move || format!("menu-note-{note_ix}"))
                .px(px(12.))
                .py(px(6.))
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(muted)
                .child(note)
        });

        // The groups are 4 pt apart and 8 pt from the edge of the menu.
        let groups = div().flex().flex_col().py(px(4.)).children(groups);
        let max_height = self.max_height;
        self.base
            .flex()
            .flex_col()
            .map(|d| match max_height {
                Some(height) => d.child(
                    groups
                        .id("menu-scroll")
                        .overflow_y_scroll()
                        .max_h(px(height)),
                ),
                None => d.child(groups),
            })
            .children(notes)
    }
}

/// What opens the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    /// The shared 32 pt trigger with a border.
    #[default]
    Outline,
    /// No border or fill, just a hover wash. For the project name.
    Ghost,
    /// The title of a device card: a ghost the height of the icons at the other end of the
    /// header, so the hover wash has the same air above and below as theirs.
    Title,
    /// A 24 pt select on `alpha/5` that says what is picked, or the label while nothing is.
    Select,
    /// Only a chevron, 24 pt high and rounded on the right: the menu half of a split button.
    /// The label is its tooltip, since it shows no words.
    Chevron,
}

/// A select: 24 pt, 12 pt medium type, 6 pt corners, as a toggle or a segmented control.
pub(crate) fn select_trigger(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (background, hover, text) = (theme.alpha_at(0.05), theme.alpha_at(0.10), theme.gray_950);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(24.))
        .pl(px(8.))
        .pr(px(6.))
        .rounded(px(6.))
        .bg(background)
        .text_size(px(12.))
        .line_height(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// The title of a card: 24 pt, 6 pt corners and `alpha/8` under the pointer, as the icons of
/// the header. 14 pt medium, as a card title is.
fn title_trigger(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, text) = (theme.alpha_at(0.08), theme.gray_950);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(24.))
        .pl(px(8.))
        .pr(px(6.))
        .rounded(px(6.))
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// The chevron at the right of a split button, with the hover wash of its main half.
fn chevron_trigger(id: &'static str, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().alpha_at(0.05);
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.))
        .w(px(24.))
        .rounded_r(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// Quiet version of the shared trigger: no border or fill, just a hover wash.
fn ghost_trigger(id: &'static str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, text) = (theme.alpha_at(0.05), theme.gray_950);
    div()
        .id(id)
        .flex()
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
    /// Scroll the groups of the open menu past this height.
    max_height: Option<f32>,
    trigger: Trigger,
    /// A trigger this wide, its label at the left and its chevron at the right. `None`: as
    /// wide as what it says.
    trigger_width: Option<f32>,
    /// What a test looks the trigger up by, see `VisualTestContext::debug_bounds`.
    debug_name: Option<SharedString>,
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
            max_height: None,
            trigger: Trigger::Outline,
            trigger_width: None,
            debug_name: None,
        }
    }

    /// Names the trigger for tests, so a simulated mouse can find it.
    pub fn debug_name(mut self, name: impl Into<SharedString>) -> Self {
        self.debug_name = Some(name.into());
        self
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

    /// Scrolls the groups past this height, for a list that may be long, such as the plugins
    /// of this machine. The notes under the groups stay in view.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    /// Makes the trigger this wide, such as a select that spans two cells of a card.
    pub fn trigger_width(mut self, width: f32) -> Self {
        self.trigger_width = Some(width);
        self
    }

    pub fn trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// The row with this value, wherever it is in the groups.
    pub fn item(&self, value: &str) -> Option<&MenuItem> {
        flat(&self.entries)
            .into_iter()
            .find(|item| item.value.as_ref() == value)
    }

    pub fn value(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    /// What the trigger says.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// Replaces the items, for a menu whose labels follow the application, such as
    /// `Undo Move clip`. The keyboard highlight stays when the items are the same ones, so a
    /// change from outside while the menu is open does not take it away.
    pub fn set_entries(&mut self, entries: Vec<MenuEntry>, cx: &mut Context<Self>) {
        let values = |entries: &[MenuEntry]| -> Vec<SharedString> {
            flat(entries)
                .iter()
                .map(|item| item.value.clone())
                .collect()
        };
        if values(&entries) != values(&self.entries) {
            self.highlighted = usize::MAX;
        }
        self.entries = entries;
        cx.notify();
    }

    /// Changes what the trigger says, for a menu whose label is what it last picked, such as
    /// the instrument of a track.
    pub fn set_label(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) {
        let label = label.into();
        if self.label != label {
            self.label = label;
            cx.notify();
        }
    }

    pub fn set_selected(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.selected = Some(value.into());
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the trigger has the focus, as tab gives it.
    pub fn trigger_is_focused(&self, window: &Window) -> bool {
        self.trigger_focus.is_focused(window)
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
        if let Some(next) = highlight_step(&self.entries, self.highlighted, delta) {
            self.highlighted = next;
            cx.notify();
        }
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
            _ => return,
        }
        // An open menu keeps the key it used. Else escape would also close whatever holds the
        // menu, such as the track panel behind an instrument picker.
        cx.stop_propagation();
    }
}

impl Render for DropdownMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (side, align, width) = (self.side, self.align, self.width);
        let trigger_width = self.trigger_width;
        let trigger_element = match self.trigger {
            Trigger::Outline => trigger("dropdown-trigger", cx),
            Trigger::Ghost => ghost_trigger("dropdown-trigger", cx),
            Trigger::Title => title_trigger("dropdown-trigger", cx),
            Trigger::Select => select_trigger("dropdown-trigger", cx),
            Trigger::Chevron => chevron_trigger("dropdown-trigger", cx),
        };
        let (label, icon, chevron) = match self.trigger {
            Trigger::Select => {
                let picked = self.selected.as_ref().and_then(|value| self.item(value));
                (
                    picked.map_or_else(|| self.label.clone(), MenuItem::label),
                    picked.and_then(|item| item.icon.clone()),
                    12.,
                )
            }
            Trigger::Outline | Trigger::Ghost => (self.label.clone(), None, 14.),
            // The glyph of the icons beside it.
            Trigger::Title => (self.label.clone(), None, 12.),
            Trigger::Chevron => (self.label.clone(), None, 12.),
        };
        let words = self.trigger != Trigger::Chevron;
        let text = cx.theme().gray_950;
        let (muted, ring) = (cx.theme().gray_700, cx.theme().lavender);

        // A trigger in less room than its label, such as the title of a narrow card, gives way
        // and ends its label in an ellipsis. The chevron stays.
        div()
            .relative()
            .flex()
            .min_w_0()
            .child(
                trigger_element
                    .min_w_0()
                    .when_some(self.debug_name.clone(), |element, name| {
                        element.debug_selector(move || name.to_string())
                    })
                    .when_some(trigger_width, |trigger, width| trigger.w(px(width)))
                    .track_focus(&self.trigger_focus)
                    .border_1()
                    .focus_visible(move |s| s.border_color(ring))
                    .map(|trigger| match icon {
                        Some(icon) => trigger.child(Icon::new(icon).size(14.).color(text)),
                        None if !words => {
                            trigger.tooltip(move |_, cx| Tooltip::new(label.clone()).view(cx))
                        }
                        None => trigger.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .when(trigger_width.is_some(), |label| label.flex_1())
                                .child(label),
                        ),
                    })
                    .child(
                        div()
                            .flex_none()
                            .child(Icon::new("chevron-down").size(chevron).color(muted)),
                    )
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
                                .max_height(self.max_height)
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
