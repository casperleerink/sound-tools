//! The History tab of the left panel: the versions of the project as cards. The card of the
//! current point is open on its latest changes, with where other versions start between them;
//! the other versions follow, newest first, each saying what it started from. A press on a
//! change goes to it, on a card to its latest point, and a double press renames; its menu
//! renames, keeps, merges and folds away. The list is one Tab stop: the arrows move, Enter goes,
//! the menu key or shift-F10 opens the menu.

mod read;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow, bail};
use rune::{Color, El, Key, KeyEvent, PointerPress, ScrollSpec, TextEdit, Tx, col, el, row, text};
use rune_kit::{Menu, MenuItem, TextInput};
use serde_json::Value;
use sound_core::{Line, PointId, Project};

use super::State;
use super::panel::{PanelTab, Tab};
use crate::controls::{Control, Target};
use read::{Card, Entry, MORE, SHOWN, Shown};

const MENU: &str = "history-menu";
const MENU_WIDTH: f32 = 280.0;
const RENAME: &str = "history-rename";
/// The track colours, less the ones that mean something in the window: red, lavender.
const COLORS: [&str; 10] = [
    "blue", "pink", "teal", "peach", "mauve", "yellow", "sky", "green", "flamingo", "sapphire",
];

#[derive(Default)]
pub(crate) struct History {
    /// More changes asked for, of the line that starts here.
    wanted: Option<(PointId, usize)>,
    show_folded_away: bool,
    menu: Option<OpenMenu>,
    renaming: Option<Renaming>,
    /// What Rune's field writes to once the rename is over, in the change that ended it.
    spare: TextEdit,
    /// What was read, at which generation of the history and how many changes were wanted.
    read: RefCell<Option<(Option<u64>, Option<(PointId, usize)>, Rc<Read>)>>,
    /// The minute the times were drawn in, so they age.
    minute: Cell<u64>,
}

/// What shows: an error in plain words, or nothing for a project without history.
type Read = Result<Option<Shown>, String>;

struct OpenMenu {
    /// The line it is the menu of, by its start.
    line: PointId,
    opener: Item,
    /// Where the pointer or the keys opened it; at its row for an agent.
    at: Option<PointerPress>,
}

struct Renaming {
    start: PointId,
    edit: TextEdit,
    /// It had the keys at the last frame, so their leaving keeps the name.
    had_keys: bool,
}

/// A row of the list, in the order shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Item {
    Change(PointId),
    /// A line, by its start.
    Card(PointId),
    Split(PointId),
    Undone(PointId),
    /// The "from" of the card of `card`, which goes to the line `to`.
    From {
        card: PointId,
        to: PointId,
    },
    More,
    FoldedAway,
}

impl Item {
    fn key(self) -> String {
        match self {
            Item::Change(point) => format!("history-row-{}", point.short()),
            Item::Card(start) => format!("history-card-{}", start.short()),
            Item::Split(start) => format!("history-split-{}", start.short()),
            Item::Undone(start) => format!("history-undone-{}", start.short()),
            Item::From { card, .. } => format!("history-from-{}", card.short()),
            Item::More => "history-more".into(),
            Item::FoldedAway => "history-folded-away".into(),
        }
    }

    /// The line whose menu it opens, `open` for a change of the open card.
    fn line(self, open: Option<PointId>) -> Option<PointId> {
        match self {
            Item::Change(_) => open,
            Item::Card(start) | Item::Split(start) | Item::Undone(start) => Some(start),
            Item::From { .. } | Item::More | Item::FoldedAway => None,
        }
    }
}

/// A choice of the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pick {
    Rename,
    Keep,
    Merge {
        keep_mine: bool,
    },
    /// Folds away, or brings back.
    Aside,
}

/// What runs an action of a control of the tab.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Act {
    Item(Item),
    Pick(Pick),
    /// The name field of a rename.
    Name,
}

impl PanelTab for History {
    fn view(&self, state: &State) -> El {
        view(self, state)
    }
}

impl History {
    /// What shows, read again only when the history changed or more of it is wanted.
    fn shown(&self, project: &Project) -> Rc<Read> {
        let history = project.history();
        let now = (history.map(sound_core::History::generation), self.wanted);
        let mut cached = self.read.borrow_mut();
        if let Some((generation, wanted, read)) = cached.as_ref()
            && (*generation, *wanted) == now
        {
            return read.clone();
        }
        let read = match history {
            None => Ok(None),
            Some(history) => read::read(history, self.wanted)
                .map(Some)
                .map_err(|error| format!("The history could not be read: {error}")),
        };
        let read = Rc::new(read);
        *cached = Some((now.0, now.1, read.clone()));
        read
    }

    fn edit(&mut self) -> &mut TextEdit {
        match &mut self.renaming {
            Some(renaming) => &mut renaming.edit,
            None => &mut self.spare,
        }
    }
}

fn open_line(project: &Project) -> Option<PointId> {
    let history = project.history()?;
    let line = history.line_of(history.current()).ok().flatten()?;
    Some(line.start)
}

fn line(project: &Project, start: PointId) -> Option<&Line> {
    project.history()?.line(start)
}

impl State {
    fn history_shows(&self) -> bool {
        self.panel.open && self.panel.tab == Tab::History
    }

    /// A press on `item`: a change goes there, a line to its latest point.
    fn go(&mut self, item: Item) -> Result<()> {
        let point = match item {
            Item::Change(point) => Some(point),
            Item::Card(start)
            | Item::Split(start)
            | Item::Undone(start)
            | Item::From { to: start, .. } => line(self.project(), start).map(|line| line.tip),
            Item::More => {
                self.show_more();
                None
            }
            Item::FoldedAway => {
                let tab = &mut self.panel.history;
                tab.show_folded_away = !tab.show_folded_away;
                None
            }
        };
        let Some(point) = point else {
            return Ok(());
        };
        if !self.session.go_to(point) {
            bail!("{}", self.session.notice().unwrap_or_default());
        }
        Ok(())
    }

    fn show_more(&mut self) {
        let Some(open) = open_line(self.project()) else {
            return;
        };
        let tab = &mut self.panel.history;
        let count = match tab.wanted {
            Some((line, count)) if line == open => count,
            _ => SHOWN,
        };
        tab.wanted = Some((open, count + MORE));
    }

    fn open_history_menu(&mut self, item: Item, at: Option<PointerPress>) -> Result<()> {
        let line = item.line(open_line(self.project()));
        let line = line.ok_or_else(|| anyhow!("it has no menu"))?;
        self.panel.history.menu = Some(OpenMenu {
            line,
            opener: item,
            at,
        });
        Ok(())
    }

    fn pick(&mut self, pick: Pick) -> Result<()> {
        let menu = self.panel.history.menu.take();
        let start = menu
            .map(|menu| menu.line)
            .ok_or_else(|| anyhow!("no menu is open"))?;
        let done = match pick {
            Pick::Rename => {
                self.start_renaming(start);
                return Ok(());
            }
            Pick::Keep => self
                .session
                .edit(|project| project.keep_as_version(start))
                .is_some(),
            Pick::Aside => {
                let aside = line(self.project(), start).is_some_and(|line| line.set_aside);
                (self.session)
                    .edit(|project| project.set_aside(start, !aside))
                    .is_some()
            }
            Pick::Merge { keep_mine } => self.session.merge(start, keep_mine),
        };
        match done {
            true => Ok(()),
            false => Err(anyhow!("{}", self.session.notice().unwrap_or_default())),
        }
    }

    /// The name of the version `start` in a field in place of its title, all of it chosen.
    fn start_renaming(&mut self, start: PointId) {
        let Some(line) = line(self.project(), start).filter(|line| line.is_version()) else {
            return;
        };
        let mut edit = TextEdit::new(line.name.as_deref().unwrap_or_default());
        (edit.anchor, edit.focus, edit.focused) = (0, edit.text.len(), true);
        self.panel.history.renaming = Some(Renaming {
            start,
            edit,
            had_keys: false,
        });
    }

    /// Keeps the name typed, or lets it go. An empty name gives naming back. The card it was in.
    fn finish_renaming(&mut self, keep: bool) -> Option<Item> {
        let Renaming { start, edit, .. } = self.panel.history.renaming.take()?;
        if keep {
            self.session
                .edit(|project| project.rename_version(start, &edit.text));
        }
        Some(Item::Card(start))
    }
}

/// What every frame does to the tab: a rename the keys left, or whose tab went, keeps its name;
/// the times age a minute at a time. Whether it changed what shows.
pub(super) fn follow(state: &mut State) -> bool {
    let shows = state.history_shows();
    let mut changed = false;
    if let Some(renaming) = &mut state.panel.history.renaming {
        let left = !shows || (renaming.had_keys && !renaming.edit.focused);
        renaming.had_keys = renaming.edit.focused;
        if left {
            state.finish_renaming(true);
            changed = true;
        }
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    let minute = now.unwrap_or_default().as_secs() / 60;
    let tab = &state.panel.history;
    if shows && tab.minute.replace(minute) != minute {
        changed = true;
    }
    changed
}

/// The action `action` of a control of the tab, as an agent runs it.
pub(super) fn run(state: &mut State, act: Act, action: &str, value: Option<Value>) -> Result<()> {
    match (act, action) {
        (Act::Item(item), "press") => state.go(item),
        (Act::Item(item), "open") => state.open_history_menu(item, None),
        (Act::Pick(pick), "press") => state.pick(pick),
        (Act::Name, "commit") => {
            let name = value.as_ref().and_then(Value::as_str);
            let name = name.ok_or_else(|| anyhow!("a name is a text, such as \"Dry\""))?;
            state.panel.history.edit().text = name.to_string();
            state.finish_renaming(true);
            Ok(())
        }
        (_, other) => bail!("it takes no {other}"),
    }
}

/// What the keys and an agent reach, in the order shown, with how each reads.
struct Reached {
    item: Item,
    label: String,
    role: &'static str,
    /// The point you are at.
    here: bool,
}

fn reached(state: &State, shown: &Shown) -> Vec<Reached> {
    let tab = &state.panel.history;
    let current = state.project().history().map(sound_core::History::current);
    let mut all = Vec::new();
    let mut push = |item, label: String, role, here| {
        all.push(Reached {
            item,
            label,
            role,
            here,
        })
    };
    let card = |card: &Card, push: &mut dyn FnMut(Item, String, &'static str, bool)| {
        push(
            Item::Card(card.line.start),
            card.line.title(),
            "option",
            false,
        );
        if let Some(from) = &card.from {
            let item = Item::From {
                card: card.line.start,
                to: from.start,
            };
            push(item, format!("from {}", from.title), "link", false);
        }
    };
    if let Some(open) = &shown.open {
        card(&open.card, &mut push);
        for entry in &open.entries {
            match entry {
                Entry::Change(change) => push(
                    Item::Change(change.id),
                    change.label.clone(),
                    "option",
                    Some(change.id) == current,
                ),
                Entry::Split { start, title, .. } => push(
                    Item::Split(*start),
                    format!("{title} started here"),
                    "option",
                    false,
                ),
                Entry::Undone { start, count } => {
                    push(Item::Undone(*start), taken_back(*count), "option", false)
                }
            }
        }
        if open.more {
            push(Item::More, "Show more changes".into(), "button", false);
        }
    }
    let folded = (tab.show_folded_away.then_some(&shown.folded_away)).into_iter();
    for other in shown.others.iter().chain(folded.flatten()) {
        card(other, &mut push);
    }
    if let Some(words) = folded_words(tab.show_folded_away, shown.folded_away.len()) {
        push(Item::FoldedAway, words, "button", false);
    }
    all
}

fn taken_back(count: usize) -> String {
    match count {
        1 => "1 change taken back".into(),
        _ => format!("{count} changes taken back"),
    }
}

fn folded_words(showing: bool, count: usize) -> Option<String> {
    Some(match (showing, count) {
        (_, 0) => return None,
        (true, _) => "Hide folded away versions".into(),
        (false, 1) => "Show 1 folded away version".into(),
        (false, _) => format!("Show {count} folded away versions"),
    })
}

/// The choices of the menu of the line `start`, with their words.
fn menu_picks(project: &Project, start: PointId) -> Vec<(Pick, String)> {
    let Some(line) = line(project, start) else {
        return Vec::new();
    };
    let current = open_line(project).filter(|current| *current != start);
    let mut picks = vec![match line.is_version() {
        true => (Pick::Rename, "Rename".to_string()),
        false => (Pick::Keep, "Keep as version".to_string()),
    }];
    if let Some(current) = current.and_then(|current| self::line(project, current)) {
        let title = current.title();
        picks.push((
            Pick::Merge { keep_mine: false },
            format!("Merge into \u{201c}{title}\u{201d}"),
        ));
        picks.push((
            Pick::Merge { keep_mine: true },
            format!("Merge into \u{201c}{title}\u{201d}, keep mine"),
        ));
        if line.is_version() {
            let words = if line.set_aside {
                "Bring back"
            } else {
                "Fold away"
            };
            picks.push((Pick::Aside, words.to_string()));
        }
    }
    picks
}

/// The controls of the tab while it shows, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Vec<Control> {
    if !state.history_shows() {
        return Vec::new();
    }
    let control = |key: String, label: String, role: &str, act: Act, actions: &[&str]| Control {
        key,
        label,
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: actions.iter().map(|action| action.to_string()).collect(),
        place: None,
        target: Target::Chrome(super::Act::History(act)),
        menu: None,
        drag: None,
    };
    let tab = &state.panel.history;
    let shown = tab.shown(state.project());
    let Ok(Some(shown)) = &*shown else {
        return Vec::new();
    };
    let open = open_line(state.project());
    let mut controls: Vec<Control> = (reached(state, shown).into_iter())
        .map(|each| {
            let menu = each.item.line(open).is_some();
            let actions: &[&str] = if menu { &["press", "open"] } else { &["press"] };
            Control {
                checked: matches!(each.item, Item::Change(_)).then_some(each.here),
                expanded: menu.then(|| tab.menu.as_ref().is_some_and(|m| m.opener == each.item)),
                ..control(
                    each.item.key(),
                    each.label,
                    each.role,
                    Act::Item(each.item),
                    actions,
                )
            }
        })
        .collect();
    if let Some(renaming) = &tab.renaming {
        controls.push(Control {
            value: Some(Value::String(renaming.edit.text.clone())),
            ..control(
                RENAME.into(),
                "Name".into(),
                "textbox",
                Act::Name,
                &["commit"],
            )
        });
    }
    if let Some(menu) = &tab.menu {
        let picks = menu_picks(state.project(), menu.line)
            .into_iter()
            .enumerate();
        controls.extend(picks.map(|(index, (pick, label))| {
            let key = format!("{MENU}-item{index}");
            control(key, label, "menuitem", Act::Pick(pick), &["press"])
        }));
    }
    controls
}

/// The menu while it is open, over the window.
pub(super) fn overlays(state: &State) -> Vec<El> {
    let Some(menu) = state
        .panel
        .history
        .menu
        .as_ref()
        .filter(|_| state.history_shows())
    else {
        return Vec::new();
    };
    let mut built = Menu::new(MENU)
        .min_width(MENU_WIDTH)
        .open(true)
        .on_close(|s: &mut State| s.panel.history.menu = None);
    if let Some(at) = menu.at {
        built = built.at(at.x, at.y);
    }
    for (pick, label) in menu_picks(state.project(), menu.line) {
        built = built.item(MenuItem::new(label).on_select(move |s: &mut State| {
            // What refused is the session's notice already.
            s.pick(pick).ok();
            s.handled();
        }));
    }
    let mut layer = Vec::new();
    built.build(&state.theme.kit, &mut layer);
    // The keys go back to the row itself: the place it opened at goes with the menu.
    let opener = menu.opener.key();
    (layer.into_iter())
        .map(|e| e.focus_return(opener.clone()))
        .collect()
}

fn view(tab: &History, state: &State) -> El {
    let theme = &state.theme;
    let read = tab.shown(state.project());
    let shown = match &*read {
        Ok(Some(shown)) => shown,
        other => {
            let words = match other {
                Err(error) => error.clone(),
                _ => "This project keeps no history.".into(),
            };
            return text(words)
                .font(theme.text("body"))
                .color(theme.color("text-muted"))
                .wrap()
                .px(theme.space(6))
                .py(theme.space(4));
        }
    };
    let rows = Rows::new(state, shown).build();
    col().key("history").w_full().h_full().min_h(0.0).child(
        col()
            .key("history-rows")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(
                col()
                    .key("history-list")
                    .role("listbox")
                    .label("History")
                    .focus_group()
                    .w_full()
                    .px(theme.space(2))
                    .items_stretch()
                    .pb(theme.space(4))
                    .children(rows),
            ),
    )
}

/// What the rows are drawn with.
struct Rows<'a> {
    state: &'a State,
    shown: &'a Shown,
    text: Color,
    body: Color,
    muted: Color,
    card: Color,
    current: Color,
    hover: Color,
    open: Option<PointId>,
}

impl<'a> Rows<'a> {
    fn new(state: &'a State, shown: &'a Shown) -> Self {
        let theme = &state.theme;
        Self {
            state,
            shown,
            text: theme.color("text"),
            body: theme.color("text-secondary"),
            muted: theme.color("text-muted"),
            card: theme.color("fill"),
            current: theme.color("selected"),
            hover: theme.color("fill-strong"),
            open: shown.open.as_ref().map(|open| open.card.line.start),
        }
    }

    fn lane(&self, color: usize) -> Color {
        self.state.theme.color(COLORS[color % COLORS.len()])
    }

    fn space(&self, step: u32) -> f32 {
        self.state.theme.space(step)
    }

    fn build(&self) -> Vec<El> {
        let tab = &self.state.panel.history;
        let reached = reached(self.state, self.shown);
        let label = |item: Item| {
            let found = reached.iter().find(|each| each.item == item);
            found.map_or_else(String::new, |each| each.label.clone())
        };
        let mut rows = Vec::new();
        if let Some(open) = &self.shown.open {
            let start = open.card.line.start;
            let header = self
                .item(Item::Card(start), label(Item::Card(start)), "option", false)
                .pt(self.space(3))
                .pb(self.space(2))
                .corners([8.0, 8.0, 0.0, 0.0])
                .mt(self.space(2))
                .child(self.title(&open.card))
                .children(self.from(&open.card));
            rows.push(self.on_card(header));
            let color = self.lane(open.card.color);
            for entry in &open.entries {
                let row = match entry {
                    Entry::Change(change) => {
                        let item = Item::Change(change.id);
                        let here = (self.state.project().history())
                            .is_some_and(|history| history.current() == change.id);
                        let right = match here {
                            true => text("You are here").color(self.body),
                            false => text(read::ago(change.time, SystemTime::now()))
                                .color(self.muted)
                                .group_opacity(0.0, 1.0),
                        };
                        let content = row()
                            .h(28.0)
                            .px(self.space(2))
                            .gap(self.space(2))
                            .items_center()
                            .radius(self.state.theme.kit.radius.sm)
                            .fill(if here {
                                self.current
                            } else {
                                Color::TRANSPARENT
                            })
                            .hover(self.hover)
                            .child(
                                text(change.label.clone())
                                    .font(self.state.theme.text("label"))
                                    .color(if here { self.text } else { self.body })
                                    .truncate()
                                    .grow(1.0)
                                    .min_w(0.0),
                            )
                            .child(right.font(self.state.theme.text("label")));
                        self.item(item, change.label.clone(), "option", here)
                            .hover_group()
                            .child(self.bar(color, content))
                    }
                    Entry::Split {
                        start,
                        title,
                        color: its,
                    } => {
                        let item = Item::Split(*start);
                        let label_style = self.state.theme.text("label");
                        let content = row()
                            .h(28.0)
                            .px(self.space(2))
                            .gap(6.0)
                            .items_center()
                            .radius(self.state.theme.kit.radius.sm)
                            .hover(self.hover)
                            .child(dot(self.lane(*its), 6.0))
                            .child(
                                text(title.clone())
                                    .font(rune::TextStyle {
                                        weight: 500.0,
                                        ..label_style
                                    })
                                    .color(self.body),
                            )
                            .child(text("started here").font(label_style).color(self.muted));
                        self.item(item, label(item), "option", false)
                            .child(self.bar(color, content))
                    }
                    Entry::Undone { start, count } => {
                        let item = Item::Undone(*start);
                        let content = row()
                            .h(28.0)
                            .px(self.space(2))
                            .items_center()
                            .radius(self.state.theme.kit.radius.sm)
                            .hover(self.hover)
                            .child(
                                text(taken_back(*count))
                                    .font(self.state.theme.text("label"))
                                    .color(self.muted),
                            );
                        self.undone_help(self.item(item, label(item), "option", false))
                            .child(self.bar(color, content))
                    }
                };
                rows.push(self.on_card(row));
            }
            let footer = match open.more {
                true => self
                    .item(Item::More, label(Item::More), "button", false)
                    .pt(self.space(1))
                    .pb(10.0)
                    .pl(32.0)
                    .child(
                        text("Show more changes")
                            .font(self.state.theme.text("label"))
                            .color(self.muted)
                            .group_color(self.text),
                    )
                    .hover_group(),
                false => el().key("history-open-end").h(self.space(2)),
            };
            rows.push(self.on_card(footer).corners([0.0, 0.0, 8.0, 8.0]));
        }
        if !self.shown.others.is_empty() {
            rows.push(
                text("Other versions")
                    .font(self.state.theme.text("label"))
                    .color(self.muted)
                    .px(20.0)
                    .pt(20.0)
                    .pb(6.0),
            );
        }
        let folded = (tab.show_folded_away.then_some(&self.shown.folded_away)).into_iter();
        for card in self.shown.others.iter().chain(folded.flatten()) {
            let item = Item::Card(card.line.start);
            rows.push(
                self.item(item, label(item), "option", false)
                    .px(self.space(3))
                    .py(10.0)
                    .radius(self.state.theme.kit.radius.lg)
                    .hover(self.card)
                    .child(self.title(card))
                    .children(self.from(card)),
            );
        }
        if let Some(words) = folded_words(tab.show_folded_away, self.shown.folded_away.len()) {
            rows.push(
                self.item(Item::FoldedAway, words.clone(), "button", false)
                    .mt(self.space(2))
                    .px(self.space(3))
                    .py(self.space(2))
                    .hover_group()
                    .child(
                        text(words)
                            .font(self.state.theme.text("label"))
                            .color(self.muted)
                            .group_color(self.text),
                    ),
            );
        }
        rows
    }

    /// A row the keys reach: a press goes, a double press on a card renames, a secondary press
    /// or the menu key opens the menu of its line.
    fn item(&self, item: Item, label: String, role: &'static str, here: bool) -> El {
        let mut e = col()
            .key(item.key())
            .role(role)
            .label(label)
            .focusable(true)
            .focus_selected(here)
            .focus_ring(1.5, -1.5, self.state.theme.color("focus"))
            .items_stretch()
            .on_press(move |s: &mut State| {
                // What refused is the session's notice already.
                s.go(item).ok();
                s.handled();
            });
        if let Item::Change(_) = item {
            e = e.checked(here);
        }
        if let Item::Card(start) = item {
            e = e.on_double_press(move |s: &mut State| {
                s.start_renaming(start);
                s.handled();
            });
        }
        if item.line(self.open).is_some() {
            e = e.on_secondary_press(move |s: &mut State, at: PointerPress, _: &mut Tx| {
                s.open_history_menu(item, Some(at)).ok();
                s.handled();
            });
        }
        // Where an agent's menu opens.
        match &self.state.panel.history.menu {
            Some(menu) if menu.opener == item => col()
                .key(format!("{MENU}-trigger"))
                .items_stretch()
                .child(e),
            _ => e,
        }
    }

    fn on_card(&self, row: El) -> El {
        row.px(self.space(3)).fill(self.card)
    }

    /// The bar of the line's colour down the left of its changes.
    fn bar(&self, color: Color, content: El) -> El {
        row()
            .pl(3.0)
            .gap(6.0)
            .items_stretch()
            .child(el().w(2.0).fill(color.alpha(0.45)))
            .child(content.grow(1.0).min_w(0.0))
    }

    fn undone_help(&self, row: El) -> El {
        let words = "Taken back with undo. Click to go there.";
        let keys = &self.state.keys.keymap;
        match keys.keys("app.undo-keeping-version").first() {
            Some(key) => row
                .help(format!("{words} This undo keeps work as a version:"))
                .help_keys(caps(key)),
            None => row.help(words),
        }
    }

    /// The dot, the title or its rename field, and the time.
    fn title(&self, card: &Card) -> El {
        let theme = &self.state.theme;
        let renaming = (self.state.panel.history.renaming.as_ref())
            .filter(|renaming| renaming.start == card.line.start);
        let title = match renaming {
            Some(renaming) => {
                let back = Item::Card(card.line.start).key();
                // As tall as the title it stands in for, so the card keeps its height.
                el().grow(1.0).min_w(0.0).my(-4.0).child(
                    TextInput::new(&renaming.edit)
                        .label("Name")
                        .placeholder("Name")
                        .text_style(theme.text("heading"))
                        .width(None)
                        .on_edit(|s: &mut State| s.panel.history.edit())
                        // Its own keys: in the list, Return would press the card.
                        .on_key(move |s: &mut State, key: &KeyEvent, tx: &mut Tx| {
                            let keep = match key.key {
                                Key::Enter => true,
                                Key::Escape => false,
                                _ => return false,
                            };
                            s.finish_renaming(keep);
                            tx.focus(back.clone(), false);
                            s.handled();
                            true
                        })
                        .build(&theme.kit)
                        .key(RENAME),
                )
            }
            None => text(card.line.title())
                .font(theme.text("heading"))
                .color(self.text)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        };
        row()
            .w_full()
            .items_center()
            .gap(self.space(2))
            .child(dot(self.lane(card.color), 8.0))
            .child(title)
            .child(
                text(read::ago(card.time, SystemTime::now()))
                    .font(theme.text("label"))
                    .color(self.muted),
            )
    }

    /// "from ● Longer tail": a press on the name goes to that version.
    fn from(&self, card: &Card) -> Option<El> {
        let from = card.from.as_ref()?;
        let item = Item::From {
            card: card.line.start,
            to: from.start,
        };
        let label = self.state.theme.text("label");
        let name = text(from.title.clone())
            .key(item.key())
            .role("link")
            .label(format!("from {}", from.title))
            .font(label)
            .color(self.body)
            .truncate()
            .min_w(0.0)
            .on_background_press(move |s: &mut State| {
                s.go(item).ok();
                s.handled();
            });
        Some(
            row()
                .pl(self.space(4))
                .pt(2.0)
                .gap(self.space(1))
                .items_center()
                .child(text("from").font(label).color(self.muted))
                .child(dot(self.lane(from.color), 6.0))
                .child(name),
        )
    }
}

fn dot(color: Color, size: f32) -> El {
    el().size(size, size)
        .radius(size / 2.0)
        .fill(color)
        .a11y_hidden()
}

/// The keycaps of a key of the keymap: `cmd-alt-z` is ⌘ ⌥ Z.
fn caps(key: &str) -> Vec<String> {
    (key.split('-'))
        .map(|part| match part {
            "cmd" => "⌘".into(),
            "alt" => "⌥".into(),
            "shift" => "⇧".into(),
            "ctrl" => "⌃".into(),
            other => other.to_uppercase(),
        })
        .collect()
}
