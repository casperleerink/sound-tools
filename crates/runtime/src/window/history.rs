//! The History tab of the left panel: the versions of the project as cards. The card of the
//! current point is open on its latest changes, with where other versions start between them;
//! the other versions follow, newest first, each saying what it started from. A click on a
//! change goes to it, on a card to its latest point, and a double click renames; its menu
//! renames, keeps, merges and folds away. The panel is one tab stop: up and down move a highlight, enter goes, the menu key or
//! shift-F10 opens its menu.

use std::collections::{HashMap, HashSet};
use std::io;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Entity, FocusHandle, Focusable, FontWeight,
    Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point as Position, ScrollHandle,
    SharedString, Stateful, Subscription, Task, Window, div, point, prelude::*, px,
};
use sound_core::{History, Line, Point, PointId};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::{self, TextInput};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::focus::KeyboardFocus;
use sound_ui::{ActiveTheme, Session, Theme};

/// How many changes the open card shows at first, and how many more each "Show more" adds.
const SHOWN: usize = 6;
const MORE: usize = 50;
const MENU_WIDTH: f32 = 280.;

/// What the panel shows, read once per change of the history.
struct Shown {
    open: Option<Open>,
    /// The other versions, newest first.
    others: Vec<Card>,
    folded_away: Vec<Card>,
}

struct Card {
    line: Line,
    color: usize,
    /// Of its latest point.
    time: SystemTime,
    from: Option<From>,
}

/// The line a line started from.
struct From {
    start: PointId,
    title: String,
    color: usize,
}

/// The line of the current point.
struct Open {
    card: Card,
    /// Newest first.
    entries: Vec<Entry>,
    /// Whether older changes are left to show.
    more: bool,
}

enum Entry {
    Change(Point),
    /// A version that starts after the change below it.
    Split {
        start: PointId,
        title: String,
        color: usize,
    },
    /// Work taken back with undo after the change below it.
    Undone {
        start: PointId,
        count: usize,
    },
}

/// What the keys can highlight, in the order shown.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Change(PointId),
    /// A line, by its start.
    Card(PointId),
    Split(PointId),
    Undone(PointId),
    More,
    FoldedAway,
}

impl Item {
    /// The line whose menu the item opens.
    fn line(self, open: Option<PointId>) -> Option<PointId> {
        match self {
            Item::Change(_) => open,
            Item::Card(start) | Item::Split(start) | Item::Undone(start) => Some(start),
            Item::More | Item::FoldedAway => None,
        }
    }
}

pub(super) struct HistoryPanel {
    session: Entity<Session>,
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// The item the keys moved to. Until they move, the current point.
    highlight: Option<Item>,
    /// More changes asked for, of the line that starts here.
    wanted: Option<(PointId, usize)>,
    /// What was read, at which generation of the history and how many points were wanted.
    read: Option<(u64, Option<(PointId, usize)>)>,
    shown: Result<Option<Rc<Shown>>, SharedString>,
    show_folded_away: bool,
    /// What the keys move through, with the child of the list each one is.
    items: Vec<(Item, usize)>,
    scroll: ScrollHandle,
    menu: Entity<DropdownMenu>,
    /// The line of the item whose menu is open.
    menu_line: Option<PointId>,
    /// The line being renamed, in a field in place of its title.
    renaming: Option<(PointId, Entity<TextInput>)>,
    _subscriptions: Vec<Subscription>,
    /// Draws the times again as they age.
    _clock: Task<()>,
}

impl HistoryPanel {
    pub(super) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let menu = cx.new(|cx| {
            DropdownMenu::new("", Vec::new(), cx)
                .debug_name("history-menu")
                .trigger(Trigger::Pointer)
                .width(MENU_WIDTH)
        });
        let subscriptions = vec![
            // Read again only after a change of the history, and only once it shows.
            cx.observe(&session, |panel, session, cx| {
                let generation = session
                    .read(cx)
                    .project()
                    .history()
                    .map(History::generation);
                if generation != panel.read.map(|(generation, _)| generation) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&menu, window, Self::picked),
        ];
        let focus_handle = cx.focus_handle().tab_stop(true);
        cx.on_focus_out(&focus_handle, window, |panel, _, _, cx| {
            panel.highlight = None;
            cx.notify();
        })
        .detach();
        let clock = cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if panel.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            session,
            focus_handle,
            keyboard_focus: KeyboardFocus::default(),
            highlight: None,
            wanted: None,
            read: None,
            shown: Ok(None),
            show_folded_away: false,
            items: Vec::new(),
            scroll: ScrollHandle::new(),
            menu,
            menu_line: None,
            renaming: None,
            _subscriptions: subscriptions,
            _clock: clock,
        }
    }

    /// Reads the history again when it changed or more of it is wanted.
    fn read_if_changed(&mut self, cx: &App) {
        let Some(history) = self.session.read(cx).project().history() else {
            self.shown = Ok(None);
            return;
        };
        let now = (history.generation(), self.wanted);
        if self.read == Some(now) {
            return;
        }
        self.read = Some(now);
        self.shown = read(history, self.wanted)
            .map(|shown| Some(Rc::new(shown)))
            .map_err(|error| format!("The history could not be read: {error}").into());
    }

    fn open_line(&self) -> Option<PointId> {
        let shown = self.shown.as_ref().ok()?.as_ref()?;
        Some(shown.open.as_ref()?.card.line.start)
    }

    fn show_more(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open_line() else {
            return;
        };
        let count = match self.wanted {
            Some((line, count)) if line == open => count,
            _ => SHOWN,
        };
        self.wanted = Some((open, count + MORE));
        cx.notify();
    }

    fn toggle_folded_away(&mut self, cx: &mut Context<Self>) {
        self.show_folded_away = !self.show_folded_away;
        cx.notify();
    }

    fn go(&mut self, item: Item, cx: &mut Context<Self>) {
        let point = match item {
            Item::Change(point) => Some(point),
            Item::Card(start) | Item::Split(start) | Item::Undone(start) => {
                (self.session.read(cx).project().history())
                    .and_then(|history| history.line(start))
                    .map(|line| line.tip)
            }
            Item::More => return self.show_more(cx),
            Item::FoldedAway => return self.toggle_folded_away(cx),
        };
        if let Some(point) = point {
            self.session
                .update(cx, |session, cx| session.go_to(point, cx));
        }
    }

    fn open_menu(
        &mut self,
        line: PointId,
        at: Position<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entries) = self.menu_entries(line, cx) else {
            return;
        };
        self.menu_line = Some(line);
        self.menu.update(cx, |menu, cx| {
            menu.set_entries(entries, cx);
            menu.open_at(at, window, cx);
        });
    }

    fn menu_entries(&self, start: PointId, cx: &App) -> Option<Vec<MenuEntry>> {
        let history = self.session.read(cx).project().history()?;
        let line = history.line(start)?;
        let current = (history.line_of(history.current()).ok().flatten())
            .filter(|current| current.start != start);
        let command =
            |value: &'static str, label: String| MenuItem::new(value, label).selectable(false);
        let mut items = Vec::new();
        if line.is_version() {
            items.push(command("rename", "Rename".into()));
        } else {
            items.push(command("keep", "Keep as version".into()));
        }
        if let Some(current) = current {
            let title = current.title();
            items.push(command(
                "merge",
                format!("Merge into \u{201c}{title}\u{201d}"),
            ));
            items.push(command(
                "merge-mine",
                format!("Merge into \u{201c}{title}\u{201d}, keep mine"),
            ));
            if line.is_version() {
                let label = if line.set_aside {
                    "Bring back"
                } else {
                    "Fold away"
                };
                items.push(command("aside", label.into()));
            }
        }
        Some(vec![MenuEntry::Group(MenuGroup::new().items(items))])
    }

    fn picked(
        &mut self,
        _: &Entity<DropdownMenu>,
        picked: &MenuPicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(start) = self.menu_line.take() else {
            return;
        };
        match picked.0.as_ref() {
            "rename" => self.start_renaming(start, window, cx),
            "keep" => self.edit(cx, |project| project.keep_as_version(start)),
            "aside" => {
                let set_aside = (self.session.read(cx).project().history())
                    .and_then(|history| history.line(start))
                    .is_some_and(|line| line.set_aside);
                self.edit(cx, |project| project.set_aside(start, !set_aside));
            }
            merge @ ("merge" | "merge-mine") => {
                let keep_mine = merge == "merge-mine";
                self.session
                    .update(cx, |session, cx| session.merge(start, keep_mine, cx));
            }
            _ => {}
        }
    }

    fn edit(
        &self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut sound_core::Project) -> Result<(), sound_core::ProjectError>,
    ) {
        self.session
            .update(cx, |session, cx| session.edit(cx, change));
    }

    fn start_renaming(&mut self, start: PointId, window: &mut Window, cx: &mut Context<Self>) {
        let line =
            (self.session.read(cx).project().history()).and_then(|history| history.line(start));
        let Some(line) = line.filter(|line| line.is_version()) else {
            return;
        };
        let name = line.name.clone().unwrap_or_default();
        let input = cx.new(|cx| TextInput::new(cx).placeholder("Name"));
        let panel = cx.weak_entity();
        input.update(cx, |input, cx| {
            input.set_text(name, cx);
            input.select_all_text(cx);
            let submit = panel.clone();
            input.set_on_submit(move |name, window, cx| {
                let name = Some(name.to_string());
                (submit.update(cx, |panel, cx| panel.finish_renaming(name, window, cx))).ok();
            });
            input.set_on_cancel(move |_, window, cx| {
                (panel.update(cx, |panel, cx| panel.finish_renaming(None, window, cx))).ok();
            });
        });
        let focus = input.focus_handle(cx);
        cx.on_blur(&focus, window, |panel, window, cx| {
            let name =
                (panel.renaming.as_ref()).map(|(_, input)| input.read(cx).text().to_string());
            panel.finish_renaming(name, window, cx)
        })
        .detach();
        window.focus(&focus, cx);
        self.renaming = Some((start, input));
        cx.notify();
    }

    /// Keeps the name typed, or with `None` lets it go. An empty name gives naming back.
    fn finish_renaming(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start, _)) = self.renaming.take() else {
            return;
        };
        if let Some(name) = name {
            self.edit(cx, |project| project.rename_version(start, &name));
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The index in [`Self::items`] of the highlight, or of the current point.
    fn highlighted(&self, cx: &App) -> Option<usize> {
        let current = self.session.read(cx).project().history()?.current();
        let find = |item: Item| self.items.iter().position(|(each, _)| *each == item);
        (self.highlight.and_then(find)).or_else(|| find(Item::Change(current)))
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Keys of the rename field and the menu, which are inside, are theirs.
        if !self.focus_handle.is_focused(window) {
            return;
        }
        let keys = &event.keystroke;
        let opens_menu = keys.key == "menu" || (keys.key == "f10" && keys.modifiers.shift);
        if keys.modifiers.modified() && !opens_menu {
            return;
        }
        if keys.key == "escape" {
            window.dispatch_action(Box::new(text_input::Cancel), cx);
            cx.stop_propagation();
            return;
        }
        let Some(index) = self.highlighted(cx) else {
            return;
        };
        let (item, child) = self.items[index];
        match keys.key.as_str() {
            "up" => self.highlight_item(index.saturating_sub(1), cx),
            "down" => self.highlight_item(index + 1, cx),
            "enter" => self.go(item, cx),
            _ if opens_menu => {
                let Some(line) = item.line(self.open_line()) else {
                    return;
                };
                let at = (self.scroll.bounds_for_item(child))
                    .map_or(self.scroll.bounds().origin, |bounds| {
                        bounds.bottom_left() + point(px(28.), px(0.))
                    });
                self.open_menu(line, at, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Moves the highlight to an item and scrolls it into view.
    fn highlight_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((item, child)) = self.items.get(index).copied() else {
            return;
        };
        self.highlight = Some(item);
        self.scroll.scroll_to_item(child);
        cx.notify();
    }

    /// A row the keys reach: a click goes, a right click opens the menu of its line.
    fn item(
        &self,
        item: Item,
        selector: String,
        highlighted: Option<Item>,
        colors: &Colors,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let line = item.line(self.open_line());
        let ring = colors.ring;
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .group(SharedString::from(format!("{item:?}")))
            .cursor_pointer()
            .when(highlighted == Some(item), |row| {
                row.shadow(vec![BoxShadow {
                    color: ring,
                    offset: Position::default(),
                    blur_radius: px(0.),
                    spread_radius: px(1.5),
                    inset: true,
                }])
            })
            .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                // Clicks in the rename field are the field's.
                let renaming = panel.renaming.as_ref().map(|(start, _)| Item::Card(*start));
                if renaming == Some(item) {
                    return;
                }
                match (item, event.click_count()) {
                    (Item::Card(start), 2) => panel.start_renaming(start, window, cx),
                    _ => panel.go(item, cx),
                }
            }))
            .when_some(line, |row, line| {
                row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        panel.open_menu(line, event.position, window, cx);
                    }),
                )
            })
    }

    /// The dot, the title or its rename field, and the time.
    fn title(&self, card: &Card, colors: &Colors) -> impl IntoElement {
        let title: AnyElement = match &self.renaming {
            Some((line, input)) if *line == card.line.start => {
                div().flex_1().child(input.clone()).into_any_element()
            }
            _ => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.text)
                .child(card.line.title())
                .into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(dot(colors.lane(card.color), 8.))
            .child(title)
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .child(ago(card.time, SystemTime::now())),
            )
    }

    /// "from ● Longer tail": a click on the name goes to that version.
    fn from(&self, card: &Card, colors: &Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let from = card.from.as_ref()?;
        let (start, bright) = (from.start, colors.text);
        let line = div()
            .pl(px(16.))
            .pt(px(2.))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .text_color(colors.muted)
            .child("from")
            .child(dot(colors.lane(from.color), 6.))
            .child(
                div()
                    .id(SharedString::from(format!("from-{}", card.line.start)))
                    .debug_selector({
                        let start = card.line.start.short();
                        move || format!("history-from-{start}")
                    })
                    .min_w_0()
                    .truncate()
                    .text_color(colors.body)
                    .hover(move |style| style.text_color(bright))
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        cx.stop_propagation();
                        panel.go(Item::Card(start), cx);
                    }))
                    .child(from.title.clone()),
            );
        Some(line.into_any_element())
    }

    /// The cards, each child of the list a row, so the keys can scroll one into view.
    fn rows(
        &mut self,
        shown: &Shown,
        highlighted: Option<Item>,
        colors: &Colors,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut items = Vec::new();
        let mut keyed = |item: Item, row: Stateful<gpui::Div>, rows: &mut Vec<AnyElement>| {
            items.push((item, rows.len()));
            rows.push(row.into_any_element());
        };
        if let Some(open) = &shown.open {
            let color = colors.lane(open.card.color);
            let card = |row: Stateful<gpui::Div>| row.mx(px(8.)).px(px(12.)).bg(colors.card);
            let header = self
                .item(
                    Item::Card(open.card.line.start),
                    format!("history-card-{}", open.card.line.start.short()),
                    highlighted,
                    colors,
                    cx,
                )
                .rounded_t(px(8.))
                .pt(px(12.))
                .pb(px(8.))
                .child(self.title(&open.card, colors))
                .children(self.from(&open.card, colors, cx));
            keyed(
                Item::Card(open.card.line.start),
                card(header).mt(px(8.)),
                &mut rows,
            );
            let current = self
                .session
                .read(cx)
                .project()
                .history()
                .map(History::current);
            for entry in &open.entries {
                let (item, selector, content): (Item, String, AnyElement) = match entry {
                    Entry::Change(change) => {
                        let here = Some(change.id) == current;
                        let group = SharedString::from(format!("{:?}", Item::Change(change.id)));
                        let right: AnyElement = if here {
                            div()
                                .text_color(colors.body)
                                .child("You are here")
                                .into_any_element()
                        } else {
                            div()
                                .opacity(0.)
                                .group_hover(group, |style| style.opacity(1.))
                                .child(ago(change.time, SystemTime::now()))
                                .into_any_element()
                        };
                        (
                            Item::Change(change.id),
                            format!("history-row-{}", change.id.short()),
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .h(px(28.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .when(here, |row| row.bg(colors.current_row))
                                .hover(|style| style.bg(colors.hover_row))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(13.))
                                        .text_color(if here { colors.text } else { colors.body })
                                        .child(change.label.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .text_color(colors.muted)
                                        .child(right),
                                )
                                .into_any_element(),
                        )
                    }
                    Entry::Split {
                        start,
                        title,
                        color,
                    } => (
                        Item::Split(*start),
                        format!("history-split-{}", start.short()),
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(28.))
                            .px(px(8.))
                            .rounded(px(4.))
                            .hover(|style| style.bg(colors.hover_row))
                            .text_size(px(12.))
                            .text_color(colors.muted)
                            .child(dot(colors.lane(*color), 6.))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.body)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title.clone()),
                            )
                            .child("started here")
                            .into_any_element(),
                    ),
                    Entry::Undone { start, count } => {
                        let tooltip = Tooltip::new(
                            "Taken back with undo. Click to go there. This undo keeps work as a version:",
                        )
                        .shortcut("alt+mod+z");
                        let words = match count {
                            1 => "1 change taken back".to_string(),
                            _ => format!("{count} changes taken back"),
                        };
                        (
                            Item::Undone(*start),
                            format!("history-undone-{}", start.short()),
                            div()
                                .id(SharedString::from(format!("undone-tip-{start}")))
                                .flex()
                                .items_center()
                                .h(px(28.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .hover(|style| style.bg(colors.hover_row))
                                .tooltip(move |_, cx| tooltip.clone().view(cx))
                                .text_size(px(12.))
                                .text_color(colors.muted)
                                .child(words)
                                .into_any_element(),
                        )
                    }
                };
                let row = self.item(item, selector, highlighted, colors, cx).child(
                    // The bar of the line's colour down the left of its changes.
                    div()
                        .ml(px(3.))
                        .pl(px(6.))
                        .border_l_2()
                        .border_color(color.opacity(0.45))
                        .child(content),
                );
                keyed(item, card(row), &mut rows);
            }
            let footer = if open.more {
                self.item(Item::More, "history-more".into(), highlighted, colors, cx)
                    .pt(px(4.))
                    .pb(px(10.))
                    .pl(px(32.))
                    .text_size(px(12.))
                    .text_color(colors.muted)
                    .hover(|style| style.text_color(colors.text))
                    .child("Show more changes")
            } else {
                div().id("history-open-end").h(px(8.))
            };
            let footer = card(footer).rounded_b(px(8.));
            if open.more {
                keyed(Item::More, footer, &mut rows);
            } else {
                rows.push(footer.into_any_element());
            }
        }
        if !shown.others.is_empty() {
            rows.push(label("Other versions", colors).into_any_element());
        }
        let folded_away = (self.show_folded_away.then_some(&shown.folded_away))
            .into_iter()
            .flatten();
        for card in shown.others.iter().chain(folded_away) {
            let item = Item::Card(card.line.start);
            let row = self
                .item(
                    item,
                    format!("history-card-{}", card.line.start.short()),
                    highlighted,
                    colors,
                    cx,
                )
                .mx(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .hover(|style| style.bg(colors.hover_row))
                .child(self.title(card, colors))
                .children(self.from(card, colors, cx));
            keyed(item, row, &mut rows);
        }
        if let count @ 1.. = shown.folded_away.len() {
            let words = match (self.show_folded_away, count) {
                (true, _) => "Hide folded away versions".to_string(),
                (false, 1) => "Show 1 folded away version".to_string(),
                (false, _) => format!("Show {count} folded away versions"),
            };
            let row = self
                .item(
                    Item::FoldedAway,
                    "history-folded-away".into(),
                    highlighted,
                    colors,
                    cx,
                )
                .mx(px(8.))
                .mt(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .text_size(px(12.))
                .text_color(colors.muted)
                .hover(|style| style.text_color(colors.text))
                .child(words);
            keyed(Item::FoldedAway, row, &mut rows);
        }
        rows.push(div().h(px(16.)).into_any_element());
        self.items = items;
        rows
    }
}

fn label(text: &'static str, colors: &Colors) -> impl IntoElement {
    div()
        .px(px(20.))
        .pt(px(20.))
        .pb(px(6.))
        .text_size(px(12.))
        .text_color(colors.muted)
        .child(text)
}

fn dot(color: Hsla, size: f32) -> impl IntoElement {
    div().flex_none().size(px(size)).rounded_full().bg(color)
}

fn read(history: &History, wanted: Option<(PointId, usize)>) -> io::Result<Shown> {
    let lines = history.lines();
    let color = |start: PointId| {
        lines
            .iter()
            .position(|line| line.start == start)
            .unwrap_or(0)
    };
    let splits = history.splits()?;
    let folded_away = history.folded_away()?;
    let card = |line: &Line| -> io::Result<Card> {
        let tip = history.line_points(line.start).next().transpose()?;
        let from = splits.get(&line.start).map(|point| From {
            start: point.line,
            title: history
                .line(point.line)
                .map_or_else(String::new, Line::title),
            color: color(point.line),
        });
        Ok(Card {
            line: line.clone(),
            color: color(line.start),
            time: tip.map_or(SystemTime::UNIX_EPOCH, |tip| tip.time),
            from,
        })
    };

    let current = history.current();
    let open_line = history.line_of(current)?.cloned();
    let open = match &open_line {
        Some(line) => {
            let wanted = match wanted {
                Some((start, count)) if start == line.start => count,
                _ => SHOWN,
            };
            let mut changes = Vec::new();
            let mut points = history.line_points(line.start);
            let (mut more, mut seen_current) = (false, false);
            for point in points.by_ref() {
                let point = point?;
                if changes.len() >= wanted && seen_current {
                    more = true;
                    break;
                }
                seen_current |= point.id == current;
                changes.push(point);
            }
            // Lines that start from a change shown here, by that change.
            let mut started: HashMap<PointId, Vec<&Line>> = HashMap::new();
            let shown: HashSet<PointId> = changes.iter().map(|change| change.id).collect();
            for other in lines {
                if let Some(from) = splits.get(&other.start)
                    && shown.contains(&from.id)
                {
                    started.entry(from.id).or_default().push(other);
                }
            }
            let mut entries = Vec::new();
            for change in changes {
                for other in started.remove(&change.id).unwrap_or_default() {
                    entries.push(match other.is_version() {
                        true => Entry::Split {
                            start: other.start,
                            title: other.title(),
                            color: color(other.start),
                        },
                        false => Entry::Undone {
                            start: other.start,
                            count: history.line_points(other.start).count(),
                        },
                    });
                }
                entries.push(Entry::Change(change));
            }
            Some(Open {
                card: card(line)?,
                entries,
                more,
            })
        }
        None => None,
    };
    let open_start = open_line.map(|line| line.start);
    let mut others = Vec::new();
    let mut folded = Vec::new();
    for line in lines
        .iter()
        .filter(|line| line.is_version() && Some(line.start) != open_start)
    {
        let card = card(line)?;
        match folded_away.contains(&line.start) {
            true => folded.push(card),
            false => others.push(card),
        }
    }
    others.sort_by_key(|card| std::cmp::Reverse(card.time));
    folded.sort_by_key(|card| std::cmp::Reverse(card.time));
    Ok(Shown {
        open,
        others,
        folded_away: folded,
    })
}

/// How long ago, in the words of a person.
fn ago(time: SystemTime, now: SystemTime) -> SharedString {
    let minutes = now.duration_since(time).unwrap_or_default().as_secs() / 60;
    let (hours, days) = (minutes / 60, minutes / 60 / 24);
    match () {
        _ if minutes < 1 => "Just now".into(),
        _ if hours < 1 => format!("{minutes} min ago").into(),
        _ if days < 1 => format!("{hours} h ago").into(),
        _ if days == 1 => "1 day ago".into(),
        _ => format!("{days} days ago").into(),
    }
}

#[derive(Clone, Copy)]
struct Colors {
    lanes: [Hsla; 10],
    text: Hsla,
    body: Hsla,
    muted: Hsla,
    card: Hsla,
    current_row: Hsla,
    hover_row: Hsla,
    ring: Hsla,
}

impl Colors {
    /// The track colours, less the ones that mean something in the window: red, lavender.
    fn new(theme: &Theme) -> Self {
        Self {
            lanes: [
                theme.blue,
                theme.pink,
                theme.teal,
                theme.peach,
                theme.mauve,
                theme.yellow,
                theme.sky,
                theme.green,
                theme.flamingo,
                theme.sapphire,
            ],
            text: theme.gray_950,
            body: theme.gray_800,
            muted: theme.gray_700,
            card: theme.alpha_at(0.04),
            current_row: theme.alpha_at(0.06),
            hover_row: theme.alpha_at(0.03),
            ring: theme.lavender,
        }
    }

    fn lane(&self, color: usize) -> Hsla {
        self.lanes[color % self.lanes.len()]
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _rendering = sound_ui::renders::start(sound_ui::renders::Part::Sidebar, "history");
        self.read_if_changed(cx);
        let colors = Colors::new(cx.theme());
        let ring_shows = self.keyboard_focus.shows_ring(&self.focus_handle, window);
        let root = div()
            .id("history")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|panel, _, _, cx| {
                    panel.highlight = None;
                    panel.keyboard_focus.pressed(cx);
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .child(self.menu.clone());
        let shown = match &self.shown {
            Ok(Some(shown)) => shown.clone(),
            other => {
                let message = match other {
                    Err(message) => message.clone(),
                    _ => "This project keeps no history.".into(),
                };
                self.items.clear();
                return root.child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .text_size(px(13.))
                        .text_color(colors.muted)
                        .child(message),
                );
            }
        };
        // Once the keys moved it, the highlight shows after a click too.
        let keys_moved = self.highlight.is_some() && self.focus_handle.is_focused(window);
        let previous = self.highlighted(cx).map(|index| self.items[index].0);
        let highlighted = (ring_shows || keys_moved).then_some(previous).flatten();
        let rows = self.rows(&shown, highlighted, &colors, cx);
        root.child(
            div()
                .id("history-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .flex()
                .flex_col()
                .children(rows),
        )
    }
}
