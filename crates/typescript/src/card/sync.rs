//! What the nodes of a card show that is an entity of the window: a text field, the card of
//! another instance, and a menu or a question its code opens.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyView, App, Bounds, Context, Entity, Focusable, Pixels, PromptLevel, SharedString,
    WeakEntity, Window, canvas, div, prelude::*, px,
};
use serde_json::json;
use sound_core::InstanceId;
use sound_notes::SIDECHAIN_INPUT;
use sound_ui::components::cell::{self, CELL_WIDTH};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MENU_WIDTH, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::icon::Icon;
use sound_ui::components::select::Select;
use sound_ui::components::text_input::TextInput;
use sound_ui::{ActiveTheme, Devices, Views};

use crate::bun::EventKind;
use crate::tree::{CardProps, CardSelect, InputProps, Props};
use crate::window::OpenedMenu;

use super::controls::shortcut_of;
use super::drawing::named_node;
use super::input::under;
use super::placed::Placing;
use super::{TitleBarChanged, TypeScriptCard};

/// The card of another instance, as a node shows it.
pub(super) struct Embedded {
    pub(super) instance: InstanceId,
    tool: String,
    /// Whether its power icon and its close icon show, whether its title is a menu, and
    /// whether it has a sidechain picker.
    icons: (bool, bool, bool, bool),
    on: Rc<Cell<bool>>,
    /// The selects of its sidechain picker as its code last said, which the picker reads.
    sidechain: Rc<RefCell<Vec<CardSelect>>>,
    pub(super) frame: CardFrame,
    pub(super) view: Option<AnyView>,
    /// On a page, which draws the card over itself.
    pub(super) placing: Option<Placing>,
}

/// The picker of what keys an embedded card, as the arrangement's racks had it: a select over
/// a select, two cells wide, each with its label under it. A pick tells the code of `card` which
/// select and which of its options.
fn sidechain_column(
    selects: &[CardSelect],
    card: &WeakEntity<TypeScriptCard>,
    node: u64,
) -> Column {
    const SPAN: usize = 2;
    let mut cells = selects.iter().enumerate().map(|(place, select)| {
        let rows = (select.options.iter().enumerate())
            .map(|(option, label)| MenuItem::new(option.to_string(), label.clone()));
        // The trigger says the value, and the list ticks the option that says it.
        let value = (select.options.iter())
            .position(|option| *option == select.value)
            .map_or_else(String::new, |option| option.to_string());
        let card = card.clone();
        let picker = Select::new(format!("sidechain-{place}"), value)
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(rows))])
            .placeholder(select.value.clone())
            .trigger_width(SPAN as f32 * CELL_WIDTH - 8.)
            .on_change(move |picked, _, cx| {
                let Ok(option) = picked.parse::<usize>() else {
                    return;
                };
                let picked = json!({ "select": place, "option": option });
                card.update(cx, |view, cx| {
                    view.event(node, EventKind::Sidechain, picked, cx);
                })
                .ok();
            });
        cell::Cell::new(picker)
            .span(SPAN)
            .label(select.label.clone())
    });
    let column = Column::new().span(SPAN);
    let column = match cells.next() {
        Some(cell) => column.top(cell),
        None => column,
    };
    match cells.next() {
        Some(cell) => column.bottom(cell),
        None => column,
    }
}

/// The title of an embedded card, as the window's cards have theirs: 14 pt medium, and with a
/// menu, a chevron after it and the wash of the pointer over it.
struct CardTitle {
    name: SharedString,
    /// For tests, which find it as `card-title-<instance>`.
    instance: String,
    on_press: Option<Rc<dyn Fn(Option<Bounds<Pixels>>, &mut App)>>,
    /// Where it was last laid out, so the menu of a press hangs under it.
    placed: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Render for CardTitle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (hover, muted) = (theme.alpha_at(0.08), theme.gray_700);
        let title = div()
            .id("card-title")
            .debug_selector({
                let instance = self.instance.clone();
                move || format!("card-title-{instance}")
            })
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(4.))
            .h(px(24.))
            .pl(px(8.))
            .pr(px(6.))
            .rounded(px(6.))
            .text_size(px(14.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.gray_950)
            .child(div().min_w_0().truncate().child(self.name.clone()));
        match self.on_press.clone() {
            Some(on_press) => {
                let placed = self.placed.clone();
                let mark = self.placed.clone();
                // The border a menu's trigger keeps for its focus ring, so the name sits where
                // the window's pickers put it.
                title
                    .relative()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(Icon::new("chevron-down").size(12.).color(muted))
                    .child(
                        canvas(move |bounds, _, _| mark.set(Some(bounds)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_click(move |_, _, cx| on_press(placed.get(), cx))
            }
            None => title,
        }
    }
}

impl TypeScriptCard {
    /// Makes what its nodes show that is an entity of the window: a text field, the card of
    /// another instance, and a menu its code opened. Outside a draw, which may not make them.
    pub(super) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let card = self.card;
        if let Some(menu) = self.live.update(cx, |live, _| live.take_menu(card)) {
            self.open_menu(menu, window, cx);
        }
        if let Some(asked) = self.live.update(cx, |live, _| live.take_asked()) {
            // The question takes the keys, and the key that asked comes up there.
            self.let_go_of_keys(cx);
            let buttons = [asked.confirm.as_str(), "Cancel"];
            let detail = asked.detail.as_deref();
            let answer = window.prompt(PromptLevel::Warning, &asked.message, detail, &buttons, cx);
            let live = self.live.downgrade();
            cx.spawn(async move |_, cx| {
                let said = answer.await == Ok(0);
                live.update(cx, |live, _| live.confirmed(asked.question, said))
                    .ok();
            })
            .detach();
        }
        let Some(Ok(table)) = self.live.read(cx).tree(card).cloned() else {
            return;
        };
        // What a node that has no key kept goes with it, and with the Bun that made it.
        let generation = self.live.read(cx).generation();
        let kept = |name: &String| {
            named_node(name).is_none_or(|(made, node)| made == generation && table.has(node))
        };
        self.spaces.borrow_mut().retain(|name, _| kept(name));
        self.indexes.borrow_mut().retain(|name, _| kept(name));
        self.scrolls.borrow_mut().retain(|name, _| kept(name));
        let title_bar = table.title_bar();
        if self.title_bar != title_bar {
            self.title_bar = title_bar;
            cx.emit(TitleBarChanged);
        }
        let open: Vec<u64> = (table.entries())
            .filter(|entry| matches!(entry.props, Props::Overlay(_)))
            .map(|entry| entry.id)
            .collect();
        let had_overlays = !self.overlays.is_empty();
        self.overlays.retain(|node| open.contains(node));
        if let Some(owner) = self.menu_overlay
            && !self.overlays.contains(&owner)
        {
            self.menu_overlay = None;
            if let Some((menu, _)) = &self.menu {
                menu.update(cx, |menu, cx| {
                    if menu.is_open() {
                        menu.close(window, cx);
                    }
                });
            }
        }
        let opened: Vec<u64> = (open.into_iter())
            .filter(|node| !self.overlays.contains(node))
            .collect();
        if !opened.is_empty() {
            if !had_overlays {
                self.focus_before_overlays = window.focused(cx).map(|focus| focus.downgrade());
            }
            // The card takes the keys, for escape to close it, before a text field in it that
            // asks for them.
            window.focus(&self.focus, cx);
            self.overlays.extend(opened);
        }
        // Not when the keys went elsewhere, such as to another card a press outside it went to.
        let give_back = had_overlays
            && self.overlays.is_empty()
            && (window.focused(cx).is_none() || self.focus.contains_focused(window, cx));
        let mut inputs = HashMap::new();
        let mut embedded = HashMap::new();
        for entry in table.entries() {
            match &entry.props {
                Props::Input(props) => {
                    let input = self.input(entry.id, props, window, cx);
                    inputs.insert(entry.id, input);
                }
                Props::Card(props) => {
                    if let Some(card) = self.embed(entry.id, props, window, cx) {
                        embedded.insert(entry.id, card);
                    }
                }
                _ => {}
            }
        }
        self.inputs = inputs;
        (self.scrolled_cards.borrow_mut()).retain(|node, _| embedded.contains_key(node));
        self.embedded = embedded;
        // After the fields, so one that closed with an overlay is dropped and not given the keys.
        if give_back {
            let back = (self.focus_before_overlays.take()).and_then(|focus| focus.upgrade());
            window.focus(&back.unwrap_or_else(|| self.focus.clone()), cx);
        }
    }

    /// The text field of the node `node`: the one it had, showing `value` when that changed and
    /// it is not typed in, or a new one.
    fn input(
        &mut self,
        node: u64,
        props: &InputProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<TextInput>, String) {
        let value = props.value.clone().unwrap_or_default();
        let disabled = props.disabled == Some(true);
        if let Some((input, shown)) = self.inputs.remove(&node) {
            input.update(cx, |input, cx| input.set_disabled(disabled, window, cx));
            if shown != value && !input.focus_handle(cx).is_focused(window) {
                input.update(cx, |input, cx| input.set_text(value.clone(), cx));
                return (input, value);
            }
            return (input, shown);
        }
        let placeholder = props.placeholder.clone().unwrap_or_default();
        let input = cx.new(|cx| {
            let input = TextInput::new(cx)
                .placeholder(placeholder)
                .disabled(disabled);
            match props.rows {
                Some(rows) => input.multi_line(rows).enter_adds_line(),
                None => input,
            }
        });
        let card = cx.weak_entity();
        input.update(cx, |input, cx| {
            input.set_text(value.clone(), cx);
            if props.on_change == Some(true) {
                let card = card.clone();
                input.set_on_change(move |text, _, cx| {
                    (card.update(cx, |view, cx| {
                        view.event(node, EventKind::Change, json!(text), cx)
                    }))
                    .ok();
                });
            }
            input.set_on_submit({
                let card = card.clone();
                move |text, _, cx| {
                    card.update(cx, |view, cx| view.commit(node, text, true, cx))
                        .ok();
                }
            });
            // Escape shows the text it had again, once the field's own update is over.
            input.set_on_cancel(move |_, window, cx| {
                let card = card.clone();
                window.defer(cx, move |window, cx| {
                    card.update(cx, |view, cx| view.revert(node, window, cx))
                        .ok();
                });
            });
        });
        let focus = input.focus_handle(cx);
        // What was typed counts when the field loses the keys too.
        cx.on_blur(&focus, window, move |view, _, cx| {
            if let Some(text) = view
                .inputs
                .get(&node)
                .map(|(input, _)| input.read(cx).text().to_string())
            {
                view.commit(node, &text, false, cx);
            }
        })
        .detach();
        if props.focus == Some(true) && !disabled {
            window.focus(&focus, cx);
            input.update(cx, TextInput::select_all_text);
        }
        (input, value)
    }

    /// Tells the code of the field `node` its text: on enter always, so a field the code shows
    /// to type in closes with its text as it was; on blur unless it shows what it last told.
    fn commit(&mut self, node: u64, text: &str, entered: bool, cx: &mut Context<Self>) {
        let Some((_, shown)) = self.inputs.get_mut(&node) else {
            return;
        };
        if shown == text && !entered {
            return;
        }
        *shown = text.to_string();
        self.event(node, EventKind::Commit, json!({ "text": text }), cx);
    }

    /// Shows in the field `node` what it last showed or told, gives the keys back and tells
    /// its code.
    fn revert(&mut self, node: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, shown)) = self.inputs.get(&node) else {
            return;
        };
        let shown = shown.clone();
        input.update(cx, |input, cx| input.set_text(shown, cx));
        window.focus(&self.focus, cx);
        self.event(node, EventKind::Cancel, json!({}), cx);
    }

    /// The card of the instance a node shows: the one it had, or a new one when the instance,
    /// its tool or the icons changed.
    fn embed(
        &mut self,
        node: u64,
        props: &CardProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Embedded> {
        let path = props.instance.as_deref()?;
        let instance = (path.split('/')).try_fold(self.id.clone(), |id, name| id.child(name));
        let instance = instance.ok()?;
        let tool = self
            .session
            .read(cx)
            .project()
            .tool_of(&instance)?
            .to_string();
        let on = props.on.unwrap_or(true);
        let selects = props.sidechain.clone().unwrap_or_default();
        // Only a device that a sidechain keys has the picker, as the arrangement's racks had it.
        let keyed = (self.session.read(cx).project())
            .input_port(&instance, SIDECHAIN_INPUT)
            .is_some();
        let icons = (
            props.on_power == Some(true),
            props.on_close == Some(true),
            props.on_title == Some(true),
            keyed && props.on_sidechain == Some(true) && !selects.is_empty(),
        );
        if let Some(old) = self.embedded.remove(&node)
            && old.instance == instance
            && old.tool == tool
            && old.icons == icons
        {
            old.on.set(on);
            if *old.sidechain.borrow() != selects {
                old.sidechain.replace(selects);
                if let Some(view) = &old.view {
                    App::notify(cx, view.entity_id());
                }
            }
            return Some(old);
        }
        let name = (Devices::label_of(&self.session, &instance, cx))
            .map_or_else(|| SharedString::from(tool.clone()), |label| label.name);
        let on_press = icons.2.then(|| {
            let card = cx.weak_entity();
            Rc::new(move |bounds: Option<Bounds<Pixels>>, cx: &mut App| {
                card.update(cx, |view, cx| {
                    view.anchor.set(bounds.map(under));
                    view.event(node, EventKind::Title, json!({}), cx)
                })
                .ok();
            }) as Rc<dyn Fn(Option<Bounds<Pixels>>, &mut App)>
        });
        let instance_name = instance.as_str().to_string();
        let title = cx.new(|_| CardTitle {
            name,
            instance: instance_name,
            on_press,
            placed: Rc::default(),
        });
        let mut frame = CardFrame::new(&instance, title);
        let shown = Rc::new(Cell::new(on));
        if icons.0 {
            let (card, shown) = (cx.weak_entity(), shown.clone());
            frame = frame.power(
                move |_| shown.get(),
                move |_, cx| {
                    card.update(cx, |view, cx| {
                        view.event(node, EventKind::Power, json!({}), cx);
                    })
                    .ok();
                },
            );
        }
        if icons.1 {
            let card = cx.weak_entity();
            frame = frame.close(move |_, cx| {
                card.update(cx, |view, cx| {
                    view.event(node, EventKind::Close, json!({}), cx);
                })
                .ok();
            });
        }
        let sidechain = Rc::new(RefCell::new(selects));
        if icons.3 {
            let (card, selects) = (cx.weak_entity(), sidechain.clone());
            frame = frame.sidechain(move |_| sidechain_column(&selects.borrow(), &card, node));
        }
        let view = Views::card_of(&self.session, &instance, frame.clone(), window, cx);
        let placing = (view.clone())
            .filter(|_| self.frame.is_none())
            .map(|view| Placing::new(view, self.origin.clone(), window, cx));
        Some(Embedded {
            instance,
            tool,
            icons,
            on: shown,
            sidechain,
            frame,
            view,
            placing,
        })
    }

    /// Opens a menu its code asked for: under the div, button or title pressed last, or where
    /// the pointer is. A menu still open answers that nothing was picked.
    fn open_menu(&mut self, menu: OpenedMenu, window: &mut Window, cx: &mut Context<Self>) {
        let OpenedMenu {
            question,
            items,
            notes,
            width,
            max_height,
            search,
        } = menu;
        // A group a run of items of one `group`, under its heading; `None` a separator.
        let mut entries: Vec<(Option<String>, Option<MenuGroup>)> = Vec::new();
        for (index, spec) in items.into_iter().enumerate() {
            let mut item = MenuItem::new(index.to_string(), spec.label)
                .disabled(spec.disabled)
                .checked(spec.checked)
                .selectable(false);
            if let Some(icon) = spec.icon {
                item = item.icon(icon);
            }
            if let Some(detail) = spec.detail {
                item = item.description(detail);
            }
            if let Some(keys) = spec.shortcut {
                item = item.shortcut(shortcut_of(&keys));
            }
            if spec.separator {
                entries.push((None, None));
            }
            match entries.last_mut() {
                Some((group, Some(items))) if *group == spec.group => {
                    *items = std::mem::take(items).item(item);
                }
                _ => {
                    let heading = spec.group.clone();
                    let group = MenuGroup::new().item(item);
                    let group = match &heading {
                        Some(heading) => group.label(heading.clone()),
                        None => group,
                    };
                    entries.push((heading, Some(group)));
                }
            }
        }
        let entries: Vec<MenuEntry> = (entries.into_iter())
            .map(|(_, group)| group.map_or(MenuEntry::Separator, MenuEntry::Group))
            .chain(notes.into_iter().map(|note| MenuEntry::Note(note.into())))
            .collect();
        self.answer_menu(None, cx);
        // A menu with a search field is another menu than one without.
        let cached = (self.menu.take())
            .map(|(menu, _)| menu)
            .filter(|menu| menu.read(cx).is_searchable() == search);
        let menu = match cached {
            Some(menu) => menu,
            None => {
                let menu = cx.new(|cx| {
                    let menu = DropdownMenu::new("", Vec::new(), cx)
                        .trigger(Trigger::Pointer)
                        .debug_name("menu");
                    match search {
                        true => menu.searchable("Search", cx),
                        false => menu,
                    }
                });
                cx.subscribe(&menu, |view, _, picked: &MenuPicked, cx| {
                    let item = picked.0.parse().ok();
                    view.answer_menu(item, cx);
                })
                .detach();
                // Closed with nothing picked: after a pick's event, which comes after its close.
                cx.observe(&menu, |_, menu, cx| {
                    if !menu.read(cx).is_open() {
                        let card = cx.entity().downgrade();
                        cx.defer(move |cx| {
                            card.update(cx, |view, cx| view.answer_menu(None, cx)).ok();
                        });
                    }
                })
                .detach();
                menu
            }
        };
        let under = self.anchor.take();
        menu.update(cx, |menu, cx| {
            menu.set_entries(entries, cx);
            menu.set_size(width.unwrap_or(MENU_WIDTH), max_height);
            match under {
                Some(position) => menu.open_under(position, window, cx),
                None => menu.open_at(window.mouse_position(), window, cx),
            }
        });
        self.menu = Some((menu, Some(question)));
        self.menu_overlay = self.overlays.last().copied();
    }

    /// Answers the open menu's question, once.
    fn answer_menu(&mut self, item: Option<usize>, cx: &mut Context<Self>) {
        let Some(question) = self.menu.as_mut().and_then(|(_, waiting)| waiting.take()) else {
            return;
        };
        self.live
            .update(cx, |live, _| live.menu_picked(question, item));
    }
}
