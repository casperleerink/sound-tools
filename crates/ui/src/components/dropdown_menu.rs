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
//!
//! [`MenuItem::submenu`] makes a row that opens more rows at its side, as "Open recent" does in
//! the menus of other apps. The pointer opens it, and so do right and enter; left and escape
//! close it. A search does not look inside it.
//!
//! [`DropdownMenu::searchable`] puts a field above the rows, for a list too long to read
//! through, such as the parameters of a plugin. What is typed keeps the rows whose label or
//! value holds it, so a plugin parameter is found by its id too, and the first of them is highlighted, so enter picks it. Up and down move from there and
//! escape closes. It shows at most a hundred rows: drawing thousands of them on every frame
//! the menu is open would cost more than typing a few letters.

use std::rc::Rc;

use gpui::{
    App, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    IntoElement, KeyDownEvent, MouseDownEvent, Render, RenderOnce, SharedString, Stateful,
    StyleRefinement, Styled, Window, anchored, deferred, div, point, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::components::kbd::Kbd;
use crate::components::popover::{Align, Side, TRIGGER_HEIGHT, anchor, surface, trigger};
use crate::components::text_input::{InputSize, TextInput};
use crate::components::tooltip::Tooltip;
use crate::theme::ActiveTheme;

const ROW_HEIGHT: f32 = 32.;
/// The most rows a search shows. See the module doc.
const MAX_FOUND: usize = 100;
/// The rounded square an item icon sits on.
const ICON_TILE: f32 = 24.;
const SUBMENU_WIDTH: f32 = 280.;

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
    submenu: Option<Vec<MenuItem>>,
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
            submenu: None,
        }
    }

    /// Makes this row open `items` at its side instead of being picked. A row with no items
    /// to open is better disabled.
    pub fn submenu(mut self, items: impl IntoIterator<Item = MenuItem>) -> Self {
        self.submenu = Some(items.into_iter().collect());
        self
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

/// The item with this value, in a submenu too.
fn find<'a>(entries: &'a [MenuEntry], value: &str) -> Option<&'a MenuItem> {
    flat(entries).into_iter().find_map(|item| {
        if item.value.as_ref() == value {
            return Some(item);
        }
        let submenu = item.submenu.as_deref().unwrap_or_default();
        submenu.iter().find(|item| item.value.as_ref() == value)
    })
}

/// The rows of the submenu of the row `value`, as entries, for the keyboard.
fn submenu_entries(entries: &[MenuEntry], value: Option<&SharedString>) -> Option<Vec<MenuEntry>> {
    let items = find(entries, value?)?.submenu.clone()?;
    Some(vec![MenuEntry::Group(MenuGroup::new().items(items))])
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

/// The entries whose rows hold `query` in their label or value, whatever the case, at most
/// [`MAX_FOUND`] of them, and the notes. A quiet line says when nothing does, or how many more
/// there are. Separators go: the groups they kept apart may be gone.
fn found(entries: &[MenuEntry], query: &str) -> Vec<MenuEntry> {
    let query = query.trim().to_lowercase();
    let mut shown = 0;
    let mut more = 0;
    let mut found = Vec::new();
    let mut notes = Vec::new();
    for entry in entries {
        match entry {
            MenuEntry::Group(group) => {
                let mut items = Vec::new();
                for item in &group.items {
                    let holds = |text: &SharedString| text.to_lowercase().contains(&query);
                    if !holds(&item.label) && !holds(&item.value) {
                        continue;
                    }
                    if shown == MAX_FOUND {
                        more += 1;
                        continue;
                    }
                    shown += 1;
                    items.push(item.clone());
                }
                if !items.is_empty() {
                    let label = group.label.clone();
                    found.push(MenuEntry::Group(MenuGroup { label, items }));
                }
            }
            MenuEntry::Separator => {}
            MenuEntry::Note(note) => notes.push(MenuEntry::Note(note.clone())),
        }
    }
    if shown == 0 && !query.is_empty() {
        found.push(MenuEntry::Note("Nothing matches.".into()));
    } else if more > 0 {
        let note = format!("{more} more. Type a name or an id to find them.");
        found.push(MenuEntry::Note(note.into()));
    }
    found.extend(notes);
    found
}

type SelectFn = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type SubmenuFn = Rc<dyn Fn(Option<SharedString>, &mut Window, &mut App)>;
type HoverFn = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// The submenu that is open, see [`MenuItem::submenu`].
#[derive(Clone)]
pub(crate) struct OpenSubmenu {
    /// The value of the row it opens from, when one is open.
    value: Option<SharedString>,
    highlighted: usize,
    /// Tells the menu the row to open, or none when the pointer is on a row without one.
    on_open: SubmenuFn,
    /// Tells the menu whether the pointer is over the submenu, which is outside its bounds.
    on_hover: HoverFn,
}

/// The menu body: groups, separators, rows.
#[derive(IntoElement)]
pub struct MenuList {
    base: Div,
    entries: Vec<MenuEntry>,
    selected: Option<SharedString>,
    highlighted: usize,
    max_height: Option<f32>,
    on_select: Option<SelectFn>,
    submenu: Option<OpenSubmenu>,
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
            submenu: None,
        }
    }

    pub(crate) fn submenu(mut self, submenu: OpenSubmenu) -> Self {
        self.submenu = Some(submenu);
        self
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
        let (muted, line) = (theme.gray_700, theme.alpha_at(0.10));
        let on_select = self.on_select;
        let submenu = self.submenu;
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
                            let element = row(
                                &item,
                                ("menu-row", row_ix).into(),
                                is_selected,
                                row_ix == highlighted,
                                cx,
                            );
                            let value = item.value.clone();
                            let enabled = !item.disabled;
                            // Every row the pointer reaches says which submenu is open: its
                            // own, or none.
                            let element = match &submenu {
                                Some(submenu) if enabled => {
                                    let (on_open, opens) = (submenu.on_open.clone(), value.clone());
                                    let opens = item.submenu.is_some().then_some(opens);
                                    element.on_hover(move |hovered, window, cx| {
                                        if *hovered {
                                            on_open(opens.clone(), window, cx);
                                        }
                                    })
                                }
                                _ => element,
                            };
                            match (&item.submenu, &submenu) {
                                (Some(items), Some(submenu)) => element
                                    .when(enabled, |d| {
                                        let on_open = submenu.on_open.clone();
                                        d.on_click(move |_, window, cx| {
                                            on_open(Some(value.clone()), window, cx)
                                        })
                                    })
                                    .when(submenu.value.as_ref() == Some(&item.value), |d| {
                                        d.relative().child(submenu_panel(
                                            items,
                                            selected.as_ref(),
                                            submenu,
                                            on_select.clone(),
                                            cx,
                                        ))
                                    }),
                                _ => element.when_some(
                                    on_select.clone().filter(|_| enabled),
                                    |d, f| {
                                        d.on_click(move |_, window, cx| {
                                            f(value.clone(), window, cx)
                                        })
                                    },
                                ),
                            }
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

/// One row: its icon, its label and second line, and at the end its shortcut, a check, or the
/// chevron of a submenu.
fn row(
    item: &MenuItem,
    id: ElementId,
    selected: bool,
    highlighted: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover, text, tile) = (
        theme.gray_700,
        theme.alpha_at(0.10),
        theme.gray_950,
        theme.alpha_at(0.06),
    );
    let selector = item.value.clone();
    div()
        .id(id)
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
                .when(highlighted, |d| d.bg(hover))
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
                    // The label truncates and this wraps: a second line is there to be read,
                    // and a row is as tall as it needs.
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
        .when(selected, |d| {
            d.child(Icon::new("check").size(16.).color(text))
        })
        .when(item.submenu.is_some(), |d| {
            d.child(Icon::new("chevron-right").size(14.).color(muted))
        })
}

/// The rows of a submenu, beside the row it opens from, its first row level with that row.
fn submenu_panel(
    items: &[MenuItem],
    selected: Option<&SharedString>,
    submenu: &OpenSubmenu,
    on_select: Option<SelectFn>,
    cx: &App,
) -> impl IntoElement {
    let rows = items.iter().enumerate().map(|(row_ix, item)| {
        let is_selected = item.checked || selected == Some(&item.value);
        let value = item.value.clone();
        row(
            item,
            ("menu-submenu-row", row_ix).into(),
            is_selected,
            row_ix == submenu.highlighted,
            cx,
        )
        .when_some(on_select.clone().filter(|_| !item.disabled), |d, f| {
            d.on_click(move |_, window, cx| f(value.clone(), window, cx))
        })
    });
    let on_hover = submenu.on_hover.clone();
    // Up by the padding and the border of the panel, and right past the padding and the
    // border of the menu.
    div().absolute().top(px(-5.)).left_full().child(
        deferred(
            anchored()
                .offset(point(px(9.), px(0.)))
                .snap_to_window_with_margin(px(8.))
                .child(
                    surface(cx)
                        .id("menu-submenu")
                        .w(px(SUBMENU_WIDTH))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .p(px(4.))
                        .on_hover(move |hovered, window, cx| on_hover(*hovered, window, cx))
                        .children(rows),
                ),
        )
        .with_priority(2),
    )
}

/// What opens the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    /// The shared 32 pt trigger with a border.
    #[default]
    Outline,
    /// No border or fill, just a hover wash.
    Ghost,
    /// A ghost on `alpha/5`, with a stronger wash under the pointer. For the project name.
    Subtle,
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

/// Quiet version of the shared trigger: no border, `fill` (clear for a ghost) and a `hover`
/// wash.
fn ghost_trigger(id: &'static str, fill: Hsla, hover: Hsla, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(TRIGGER_HEIGHT))
        .px(px(8.))
        .rounded(px(8.))
        .bg(fill)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().gray_950)
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
    /// The row whose submenu is open, see [`MenuItem::submenu`].
    submenu: Option<SharedString>,
    submenu_highlighted: usize,
    /// A press there is not outside the menu, though it is outside its bounds.
    pointer_in_submenu: bool,
    side: Side,
    align: Align,
    width: f32,
    /// Scroll the groups of the open menu past this height.
    max_height: Option<f32>,
    trigger: Trigger,
    /// A trigger this wide, its label at the left and its chevron at the right. `None`: as
    /// wide as what it says.
    trigger_width: Option<f32>,
    /// A trigger this tall. `None`: the height of its kind.
    trigger_height: Option<f32>,
    /// An icon before the label of a trigger with words, in its own color or the label's.
    icon: Option<(SharedString, Option<Hsla>)>,
    /// What a test looks the trigger up by, see `VisualTestContext::debug_bounds`.
    debug_name: Option<SharedString>,
    /// The field above the rows of a menu that is searched, see [`Self::searchable`].
    search: Option<Search>,
}

struct Search {
    input: Entity<TextInput>,
    /// What the rows were last kept by. The field notifies at every blink of its caret too,
    /// and the rows and the highlight change only when the text does.
    query: String,
    /// The rows it keeps, worked out when the text or the rows change and not every frame.
    found: Vec<MenuEntry>,
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
            submenu: None,
            submenu_highlighted: usize::MAX,
            pointer_in_submenu: false,
            side: Side::default(),
            align: Align::default(),
            width: 320.,
            max_height: None,
            trigger: Trigger::Outline,
            trigger_width: None,
            trigger_height: None,
            icon: None,
            debug_name: None,
            search: None,
        }
    }

    /// Puts a field above the rows that keeps only the rows whose label or value holds what is
    /// typed.
    /// See the module doc.
    pub fn searchable(mut self, placeholder: &str, cx: &mut Context<Self>) -> Self {
        let menu = cx.weak_entity();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx)
                .placeholder(placeholder.to_string())
                .size(InputSize::Sm);
            // Enter and escape are the field's own keys, so they come from it.
            let on_enter = menu.clone();
            input.set_on_submit(move |_, window, cx| {
                // A menu that went with its view has nothing left to pick.
                on_enter
                    .update(cx, |menu, cx| menu.pick_highlighted(window, cx))
                    .ok();
            });
            input.set_on_cancel(move |_, window, cx| {
                menu.update(cx, |menu, cx| menu.close(window, cx)).ok();
            });
            input
        });
        cx.observe(&input, |menu, input, cx| {
            let text = input.read(cx).text().to_string();
            menu.search_changed(text, cx);
        })
        .detach();
        self.search = Some(Search {
            input,
            query: String::new(),
            found: found(&self.entries, ""),
        });
        self
    }

    /// The rows the open menu shows: every one, or what a search keeps.
    fn shown(&self) -> &[MenuEntry] {
        match &self.search {
            Some(search) => &search.found,
            None => &self.entries,
        }
    }

    /// The text of the search changed: the first row it keeps is highlighted, so enter picks
    /// it. With nothing typed nothing is highlighted, as in a menu with no search.
    fn search_changed(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.query == text {
            return;
        }
        search.found = found(&self.entries, &text);
        self.highlighted = match text.trim().is_empty() {
            true => usize::MAX,
            false => highlight_step(&search.found, usize::MAX, 1).unwrap_or(usize::MAX),
        };
        search.query = text;
        cx.notify();
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
        find(&self.entries, value)
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
        if let Some(search) = &mut self.search {
            search.found = found(&self.entries, &search.query);
        }
        cx.notify();
    }

    /// Makes the trigger this tall, such as the chevron of a split button that is a row.
    pub fn set_trigger_height(&mut self, height: f32, cx: &mut Context<Self>) {
        self.trigger_height = Some(height);
        cx.notify();
    }

    /// Changes what the trigger says, for a menu whose label is what it last picked, such as
    /// the instrument of a track.
    /// Puts `name` before the label, in `color` or the label's. A select shows the picked
    /// row's icon instead.
    pub fn set_icon(
        &mut self,
        name: impl Into<SharedString>,
        color: Option<Hsla>,
        cx: &mut Context<Self>,
    ) {
        self.icon = Some((name.into(), color));
        cx.notify();
    }

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
        match &self.search {
            // A search starts empty each time, with the keys in its field.
            Some(search) => {
                let input = search.input.clone();
                input.update(cx, |input, cx| input.set_text("", cx));
                window.focus(&input.focus_handle(cx), cx);
            }
            None => window.focus(&self.focus_handle, cx),
        }
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.highlighted = usize::MAX;
        self.set_submenu(None, cx);
        // The open menu held the focus. Without this it would be nowhere, and keys with it.
        window.focus(&self.trigger_focus, cx);
        cx.notify();
    }

    /// Opens the submenu of the row `value`, or closes the one that is open.
    fn set_submenu(&mut self, value: Option<SharedString>, cx: &mut Context<Self>) {
        if self.submenu != value {
            self.submenu = value;
            self.submenu_highlighted = usize::MAX;
            self.pointer_in_submenu = false;
            cx.notify();
        }
    }

    fn pick(&mut self, value: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let selectable = self.item(&value).is_none_or(|item| item.selectable);
        if selectable {
            self.selected = Some(value.clone());
        }
        self.close(window, cx);
        cx.emit(MenuPicked(value));
    }

    /// Move the highlight, skipping disabled rows.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(next) = highlight_step(self.shown(), self.highlighted, delta) {
            self.highlighted = next;
            cx.notify();
        }
    }

    /// Picks the highlighted row, when there is one that can be picked. A row with a submenu
    /// opens it, its first row highlighted.
    fn pick_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A row can turn disabled under the highlight, as Undo does with nothing to undo.
        let Some(item) = flat(self.shown())
            .get(self.highlighted)
            .filter(|item| !item.is_disabled())
            .map(|item| (item.value.clone(), item.submenu.is_some()))
        else {
            return;
        };
        match item {
            (value, true) => self.open_submenu_from_keys(value, cx),
            (value, false) => self.pick(value, window, cx),
        }
    }

    fn open_submenu_from_keys(&mut self, value: SharedString, cx: &mut Context<Self>) {
        self.set_submenu(Some(value), cx);
        let rows = submenu_entries(&self.entries, self.submenu.as_ref()).unwrap_or_default();
        self.submenu_highlighted = highlight_step(&rows, usize::MAX, 1).unwrap_or(usize::MAX);
    }

    /// The keys of an open submenu: the same as the menu's, and left closes it as escape does.
    fn on_submenu_key(
        &mut self,
        key: &str,
        rows: &[MenuEntry],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            "escape" | "left" => self.set_submenu(None, cx),
            "down" | "up" => {
                let delta = if key == "down" { 1 } else { -1 };
                if let Some(next) = highlight_step(rows, self.submenu_highlighted, delta) {
                    self.submenu_highlighted = next;
                    cx.notify();
                }
            }
            "enter" => {
                let value = flat(rows)
                    .get(self.submenu_highlighted)
                    .filter(|item| !item.is_disabled())
                    .map(|item| item.value.clone());
                if let Some(value) = value {
                    self.pick(value, window, cx);
                }
            }
            _ => return false,
        }
        true
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let used = match submenu_entries(&self.entries, self.submenu.as_ref()) {
            Some(rows) => self.on_submenu_key(key, &rows, window, cx),
            None => match key {
                "escape" => {
                    self.close(window, cx);
                    true
                }
                "down" => {
                    self.step(1, cx);
                    true
                }
                "up" => {
                    self.step(-1, cx);
                    true
                }
                "enter" => {
                    self.pick_highlighted(window, cx);
                    true
                }
                "right" => {
                    let opens = flat(self.shown())
                        .get(self.highlighted)
                        .filter(|item| item.submenu.is_some() && !item.is_disabled())
                        .map(|item| item.value.clone());
                    if let Some(value) = opens {
                        self.open_submenu_from_keys(value, cx);
                    }
                    true
                }
                _ => false,
            },
        };
        if !used {
            return;
        }
        // An open menu keeps the key it used. Else escape would also close whatever holds the
        // menu, such as the track panel behind an instrument picker.
        cx.stop_propagation();
    }
}

impl Render for DropdownMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (side, align, width) = (self.side, self.align, self.width);
        let (trigger_width, trigger_height) = (self.trigger_width, self.trigger_height);
        let trigger_element = match self.trigger {
            Trigger::Outline => trigger("dropdown-trigger", cx),
            Trigger::Ghost => {
                let hover = cx.theme().alpha_at(0.05);
                ghost_trigger("dropdown-trigger", Hsla::transparent_black(), hover, cx)
            }
            Trigger::Subtle => {
                let (fill, hover) = (cx.theme().alpha_at(0.05), cx.theme().alpha_at(0.10));
                ghost_trigger("dropdown-trigger", fill, hover, cx)
            }
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
            Trigger::Outline | Trigger::Ghost | Trigger::Subtle => (self.label.clone(), None, 14.),
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
                    .when_some(trigger_height, |trigger, height| trigger.h(px(height)))
                    .track_focus(&self.trigger_focus)
                    .border_1()
                    .focus_visible(move |s| s.border_color(ring))
                    .map(|trigger| match icon {
                        Some(icon) => trigger.child(Icon::new(icon).size(14.).color(text)),
                        None if !words => {
                            trigger.tooltip(move |_, cx| Tooltip::new(label.clone()).view(cx))
                        }
                        None => trigger
                            .when_some(self.icon.clone(), |trigger, (name, color)| {
                                trigger
                                    .child(Icon::new(name).size(14.).color(color.unwrap_or(text)))
                            })
                            .child(
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
                let search = self.search.as_ref().map(|search| {
                    div()
                        .px(px(8.))
                        .pt(px(8.))
                        .pb(px(4.))
                        .child(search.input.clone())
                });
                d.child(anchor(
                    side,
                    align,
                    surface(cx)
                        .track_focus(&self.focus_handle)
                        .w(px(width))
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            if !this.pointer_in_submenu {
                                this.close(window, cx)
                            }
                        }))
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                            this.on_key(ev, window, cx)
                        }))
                        .children(search)
                        .child(
                            MenuList::new(self.shown().to_vec())
                                .selected(self.selected.clone())
                                .highlighted(self.highlighted)
                                .max_height(self.max_height)
                                .submenu(OpenSubmenu {
                                    value: self.submenu.clone(),
                                    highlighted: self.submenu_highlighted,
                                    on_open: Rc::new(cx.processor(
                                        |this, value: Option<SharedString>, _, cx| {
                                            this.set_submenu(value, cx)
                                        },
                                    )),
                                    on_hover: Rc::new(cx.processor(|this, hovered: bool, _, _| {
                                        this.pointer_in_submenu = hovered
                                    })),
                                })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[MenuEntry]) -> Vec<String> {
        flat(entries)
            .iter()
            .map(|item| item.label().to_string())
            .collect()
    }

    fn notes(entries: &[MenuEntry]) -> Vec<String> {
        let notes = entries.iter().filter_map(|entry| match entry {
            MenuEntry::Note(note) => Some(note.to_string()),
            _ => None,
        });
        notes.collect()
    }

    /// A search keeps the rows whose label or value holds the text, whatever the case, and
    /// never more than the most it shows, also with nothing typed.
    #[test]
    fn a_search_keeps_at_most_a_hundred_rows_and_says_how_many_more_there_are() {
        let items = (0..250).map(|index| MenuItem::new(index.to_string(), format!("Knob {index}")));
        let entries = vec![MenuEntry::Group(MenuGroup::new().items(items))];

        let everything = found(&entries, "");
        assert_eq!(labels(&everything).len(), MAX_FOUND);
        assert_eq!(
            notes(&everything),
            ["150 more. Type a name or an id to find them."]
        );

        let some = found(&entries, "KNOB 24");
        let tens: Vec<String> = (240..250).map(|index| format!("Knob {index}")).collect();
        assert_eq!(labels(&some), [vec!["Knob 24".to_string()], tens].concat());
        assert!(notes(&some).is_empty());

        assert_eq!(notes(&found(&entries, "drive")), ["Nothing matches."]);

        // Rows past the most that share one label are still found, by their value.
        let same = (0..250).map(|index| MenuItem::new(index.to_string(), "Gain"));
        let same = vec![MenuEntry::Group(MenuGroup::new().items(same))];
        let last = flat(&found(&same, "249"))
            .iter()
            .map(|item| item.value.to_string())
            .collect::<Vec<_>>();
        assert_eq!(last, ["249"]);
    }
}
