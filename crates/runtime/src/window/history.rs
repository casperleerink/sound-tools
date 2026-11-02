//! The history tab of the left panel: every state the project was in, as a tree of versions.
//! A click on a point goes there; a change made there starts a new version. Each version shows
//! its first and its latest point, with the rest folded between, and is named by the agent's
//! small model after what it changed.

use std::collections::BTreeSet;
use std::ops::Range;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla, MouseButton,
    MouseDownEvent, PathBuilder, Pixels, Point, SharedString, Task, UniformListScrollHandle,
    Window, canvas, div, point, prelude::*, px, uniform_list,
};
use sound_core::history::{FileChange, ObjectId, PAGE};
use sound_history::{Drawing, Graph, Layout, Row, Shape, Version};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::TextInput;
use sound_ui::components::tooltip::Tooltip;
use sound_ui::{ActiveTheme, Session};

use super::{QuickAnswer, TellAgent};

const ROW_HEIGHT: f32 = 40.;
const LANE_WIDTH: f32 = 16.;
/// From the left of the graph to the middle of the first lane.
const GRAPH_INSET: f32 = 24.;
/// How often the tab looks for a change of the history.
const LOOK_EVERY: Duration = Duration::from_millis(500);
/// A version is named once it has not grown for this long, so a run of edits is named once.
const NAME_AFTER: Duration = Duration::from_secs(5);
/// The most of the changed files a name is asked for with, so the question stays cheap.
const PROMPT_FILE: usize = 1_500;
const PROMPT_ALL: usize = 12_000;

/// The tabs of the left panel.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum PanelTab {
    Agent,
    History,
}

/// The tabs of the left panel, which the header of each tab shows. Without the agent there is
/// only the history, and no tabs.
pub(super) struct PanelTabs {
    selected: PanelTab,
    has_agent: bool,
}

impl PanelTabs {
    pub(super) fn new(has_agent: bool) -> Self {
        Self {
            selected: PanelTab::Agent,
            has_agent,
        }
    }

    pub(super) fn selected(&self) -> PanelTab {
        match self.has_agent {
            true => self.selected,
            false => PanelTab::History,
        }
    }

    pub(super) fn select(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        if self.selected != tab {
            self.selected = tab;
            cx.notify();
        }
    }
}

impl Render for PanelTabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (text, muted, fill, hover) = (
            theme.gray_950,
            theme.gray_700,
            theme.alpha_at(0.07),
            theme.alpha_at(0.04),
        );
        let selected = self.selected();
        let tab = |tab: PanelTab, label: &'static str, cx: &mut Context<Self>| {
            let on = selected == tab;
            div()
                .id(label)
                .debug_selector(move || format!("panel-tab-{}", label.to_lowercase()))
                .flex()
                .items_center()
                .h(px(28.))
                .px(px(10.))
                .rounded(px(8.))
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if on { text } else { muted })
                .when(on, |this| this.bg(fill))
                .when(!on, |this| {
                    this.hover(|style| style.bg(hover).text_color(text))
                })
                .on_click(cx.listener(move |tabs, _, _, cx| tabs.select(tab, cx)))
                .child(label)
        };
        if !self.has_agent {
            return div()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .child("History");
        }
        div()
            .ml(px(-10.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(tab(PanelTab::Agent, "Agent", cx))
            .child(tab(PanelTab::History, "History", cx))
    }
}

pub(super) struct HistoryPanel {
    session: Entity<Session>,
    tabs: Entity<PanelTabs>,
    quick_answer: Option<QuickAnswer>,
    tell_agent: Option<TellAgent>,
    layout: Layout,
    /// The folds that are open.
    open: BTreeSet<ObjectId>,
    /// [`sound_history::History::changes`] when the layout was made.
    laid_out: Option<u64>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _watching: Task<()>,
    /// The context menu, and the version it is for.
    menu: Entity<DropdownMenu>,
    menu_version: Option<usize>,
    /// The undone work the menu's "Keep as version" is for.
    keep: Vec<ObjectId>,
    /// The question for a name that runs.
    naming: Option<Task<()>>,
    /// The rows in view reached the end of what is read of the history.
    wants_older: bool,
    /// Versions asked for a name, with the latest point they were asked for: one that failed
    /// is asked again only once it grew.
    asked: BTreeSet<(ObjectId, ObjectId)>,
    /// The version being named by hand: its first and latest point, and the field.
    renaming: Option<(ObjectId, ObjectId, Entity<TextInput>)>,
}

impl HistoryPanel {
    pub(super) fn new(
        session: Entity<Session>,
        tabs: Entity<PanelTabs>,
        quick_answer: Option<QuickAnswer>,
        tell_agent: Option<TellAgent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&tabs, |_, _, cx| cx.notify()).detach();
        let menu = cx.new(|cx| {
            DropdownMenu::new("", Vec::new(), cx)
                .trigger(Trigger::Pointer)
                .width(260.)
        });
        cx.subscribe_in(&menu, window, Self::picked).detach();
        let watching = cx.spawn(async move |panel, cx| {
            loop {
                // Ends with the panel.
                if panel.update(cx, Self::look).is_err() {
                    return;
                }
                cx.background_executor().timer(LOOK_EVERY).await;
            }
        });
        Self {
            session,
            tabs,
            quick_answer,
            tell_agent,
            layout: Layout::default(),
            open: BTreeSet::new(),
            laid_out: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _watching: watching,
            menu,
            menu_version: None,
            keep: Vec::new(),
            naming: None,
            wants_older: false,
            asked: BTreeSet::new(),
            renaming: None,
        }
    }

    /// Lays the tree out again when the history changed, and names what waits for a name.
    fn look(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.wants_older) {
            self.session.update(cx, |session, cx| {
                session.edit(cx, |project| project.read_more_history(PAGE))
            });
        }
        let changes = (self.session.read(cx).project().history()).map(|history| history.changes());
        if changes != self.laid_out {
            self.relayout(cx);
        }
        self.name_a_version(cx);
    }

    fn relayout(&mut self, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        if let Some(history) = project.history() {
            self.laid_out = Some(history.changes());
            self.layout = Graph::of(history).layout(&self.open);
        }
        cx.notify();
    }

    /// Whether the project may go elsewhere now. Not during a drag or a recording.
    fn may_move(&self, cx: &mut Context<Self>) -> bool {
        let session = self.session.read(cx);
        let recording = session.playhead().read(cx).playing
            && session.recording().read(cx).armed().next().is_some();
        if session.gesture_open() || recording {
            let message = "Stop recording before going to another point";
            self.session
                .update(cx, |session, cx| session.report(message, cx));
            return false;
        }
        true
    }

    fn go_to(&mut self, target: ObjectId, version: usize, cx: &mut Context<Self>) {
        let now = |cx: &App| {
            (self.session.read(cx).project().history()).and_then(|history| history.now())
        };
        if now(cx) == Some(target) || !self.may_move(cx) {
            return;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.go_to(target))
        });
        // The agent works on the files, which now hold another state: its next message says so.
        if now(cx) == Some(target)
            && let (Some(tell), Some(info)) = (&self.tell_agent, self.layout.versions.get(version))
        {
            let name = name_of(info, version);
            let short: String = target.to_string().chars().take(8).collect();
            let for_agent = format!(
                "Note: the composer went back in the history of the project to point {short} of the version \"{name}\". The files hold that state now. The history is a git repository in .sound-tools/history."
            );
            tell(for_agent, format!("Went back to “{name}”"), cx);
        }
        self.relayout(cx);
    }

    fn merge(&mut self, version: usize, keep_mine: bool, cx: &mut Context<Self>) {
        if !self.may_move(cx) {
            return;
        }
        let Some(merged) = self.layout.versions.get(version) else {
            return;
        };
        let label = format!("Merge “{}”", name_of(merged, version));
        let tip = merged.tip;
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.merge(tip, keep_mine, &label))
        });
        self.relayout(cx);
    }

    fn toggle_aside(&mut self, version: usize, cx: &mut Context<Self>) {
        let Some(info) = self.layout.versions.get(version) else {
            return;
        };
        let (start, aside) = (info.start, !info.aside);
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.set_aside(start, aside))
        });
        self.relayout(cx);
    }

    fn start_renaming(&mut self, version: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(info) = self.layout.versions.get(version) else {
            return;
        };
        let (start, tip, name) = (info.start, info.tip, name_of(info, version));
        let input = cx.new(|cx| {
            let mut input =
                TextInput::new(cx).placeholder("Name it, or leave empty for the agent's");
            input.set_text(name, cx);
            input.select_all_text(cx);
            input
        });
        let panel = cx.entity().downgrade();
        input.update(cx, |input, _| {
            let submitted = panel.clone();
            input.set_on_submit(move |name, _, cx| {
                let name = name.to_string();
                // Fails only when the panel is gone.
                (submitted.update(cx, |panel, cx| panel.finish_renaming(Some(name), cx))).ok();
            });
            input.set_on_cancel(move |_, _, cx| {
                (panel.update(cx, |panel, cx| panel.finish_renaming(None, cx))).ok();
            });
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        self.renaming = Some((start, tip, input));
        cx.notify();
    }

    /// Gives the version the name typed, by hand, so the agent's names leave it. An empty
    /// name gives it back to the agent. `None` keeps what it had.
    fn finish_renaming(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        let Some((start, tip, _)) = self.renaming.take() else {
            return;
        };
        if let Some(name) = name {
            self.session.update(cx, |session, cx| {
                session.edit(cx, |project| project.name_version(start, &name, tip, true))
            });
        }
        self.relayout(cx);
    }

    fn keep_as_version(&mut self, cx: &mut Context<Self>) {
        let keep = std::mem::take(&mut self.keep);
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                (keep.iter()).try_for_each(|start| project.keep_as_version(*start))
            })
        });
        self.relayout(cx);
    }

    fn toggle_fold(&mut self, key: ObjectId, cx: &mut Context<Self>) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
        self.relayout(cx);
    }

    /// Asks the agent's small model to name one shown version whose name is missing or older
    /// than its latest point, the one the project is on first. One question at a time.
    fn name_a_version(&mut self, cx: &mut Context<Self>) {
        let Some(quick_answer) = self.quick_answer.clone() else {
            return;
        };
        if self.naming.is_some() {
            return;
        }
        let project = self.session.read(cx).project();
        let Some(history) = project.history() else {
            return;
        };
        let shown: BTreeSet<usize> = self.layout.rows.iter().map(|row| row.version).collect();
        let settled = now_millis().saturating_sub(NAME_AFTER.as_millis() as u64);
        let mut waiting: Vec<&Version> = (shown.iter())
            .filter_map(|version| self.layout.versions.get(*version))
            .filter(|version| {
                let named = (version.name.as_ref())
                    .is_some_and(|named| named.by_hand || named.tip == version.tip.to_string());
                let grown = history.point(version.tip).map_or(u64::MAX, |tip| tip.time);
                !named && grown <= settled && !self.asked.contains(&(version.start, version.tip))
            })
            .collect();
        waiting.sort_by_key(|version| !version.now);
        let Some(version) = waiting.first() else {
            return;
        };
        let (start, tip) = (version.start, version.tip);
        let Ok(changes) = history.difference_between(version.split, tip) else {
            return;
        };
        let Some(answer) = quick_answer(naming_prompt(&changes), cx) else {
            return;
        };
        self.asked.insert((start, tip));
        self.naming = Some(cx.spawn(async move |panel, cx| {
            let answer = cx.background_spawn(answer).await;
            // Fails only when the panel is gone.
            panel
                .update(cx, |panel, cx| {
                    panel.naming = None;
                    let Some(name) = answer.ok().as_deref().and_then(clean_name) else {
                        return;
                    };
                    panel.session.update(cx, |session, cx| {
                        session.edit(cx, |project| project.name_version(start, &name, tip, false))
                    });
                    panel.relayout(cx);
                })
                .ok();
        }));
    }

    /// The menu of a row: of its version, and "Keep as version" for undone work, its own or
    /// that which left it.
    fn open_menu(
        &mut self,
        version: usize,
        undone_from: Option<ObjectId>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.layout.versions.get(version) else {
            return;
        };
        let keep: Vec<ObjectId> = (self.layout.versions.iter())
            .filter(|each| {
                each.undone
                    && (each.start == info.start
                        || (undone_from.is_some() && each.split == undone_from))
            })
            .map(|each| each.start)
            .collect();
        let mut items = vec![MenuItem::new("rename", "Rename").selectable(false)];
        if !keep.is_empty() {
            items.push(
                MenuItem::new("keep", "Keep as version")
                    .description("Undone work, shown as a version of its own")
                    .selectable(false),
            );
        }
        self.keep = keep;
        if info.now {
            self.show_menu(version, items, position, window, cx);
            return;
        }
        let into = (self.layout.versions.iter().enumerate())
            .find(|(_, version)| version.now)
            .map_or("this version".to_string(), |(index, now)| {
                format!("“{}”", name_of(now, index))
            });
        let aside = match info.aside {
            true => "Bring back",
            false => "Fold away",
        };
        items.extend([
            MenuItem::new("merge", format!("Merge into {into}"))
                .description("Where both changed something, this one wins")
                .selectable(false),
            MenuItem::new("merge-mine", format!("Merge into {into}, keep mine"))
                .description("Where both changed something, yours stays")
                .selectable(false),
            MenuItem::new("aside", aside).selectable(false),
        ]);
        self.show_menu(version, items, position, window, cx);
    }

    fn show_menu(
        &mut self,
        version: usize,
        items: Vec<MenuItem>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if items.is_empty() {
            return;
        }
        self.menu_version = Some(version);
        self.menu.update(cx, |menu, cx| {
            menu.set_entries(vec![MenuEntry::Group(MenuGroup::new().items(items))], cx);
            menu.open_at(position, window, cx);
        });
    }

    fn picked(
        &mut self,
        _: &Entity<DropdownMenu>,
        picked: &MenuPicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(version) = self.menu_version.take() else {
            return;
        };
        match picked.0.as_ref() {
            "merge" => self.merge(version, false, cx),
            "merge-mine" => self.merge(version, true, cx),
            "aside" => self.toggle_aside(version, cx),
            "keep" => self.keep_as_version(cx),
            "rename" => self.start_renaming(version, window, cx),
            _ => {}
        }
    }

    fn rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        // Older points are read as the end comes into view.
        let has_more =
            (self.session.read(cx).project().history()).is_some_and(|history| history.has_more());
        self.wants_older |= has_more && range.end + 20 >= self.layout.rows.len();
        let lanes = self.layout.lanes(range.clone());
        let graph_width = GRAPH_INSET + LANE_WIDTH * lanes.len().max(1) as f32;
        let colors = Colors::of(cx);
        let now = now_millis();
        range
            .filter_map(|index| {
                let row = self.layout.rows.get(index)?.clone();
                let drawing = self.layout.drawing(index, &lanes);
                Some(self.row(index, row, drawing, graph_width, &colors, now, cx))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        index: usize,
        row: Row,
        drawing: Drawing,
        graph_width: f32,
        colors: &Colors,
        now: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let (text, muted, hover, selected, link, fill) = (
            theme.gray_950,
            theme.gray_700,
            theme.alpha_at(0.04),
            theme.alpha_at(0.07),
            theme.blue,
            theme.alpha_at(0.07),
        );
        let is_fold = row.is_fold();
        let graph = {
            let colors = colors.clone();
            let dot = Dot::of(&row);
            let stub = row.undone.as_ref().is_some_and(|fold| !fold.open);
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| paint(bounds, &drawing, dot, stub, &colors, window),
            )
            .flex_none()
            .w(px(graph_width))
            .h_full()
        };
        let now_tag = row.now.then(|| {
            badge(text, text)
                .text_color(theme.gray_100)
                .font_weight(FontWeight::SEMIBOLD)
                .child("Current")
        });
        let badge_to = |id: &str, key: ObjectId, words: String, cx: &mut Context<Self>| {
            badge(link, link.opacity(0.1))
                .id(SharedString::from(format!("{id}-{key}")))
                .debug_selector(move || format!("history-{id}-{index}"))
                .cursor_pointer()
                .hover(|style| style.bg(link.opacity(0.18)))
                .child(words)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.toggle_fold(key, cx)
                }))
        };
        let hide = (row.changes.as_ref())
            .filter(|fold| fold.open)
            .map(|fold| badge_to("hide", fold.key, "Hide".to_string(), cx));
        let more = row.more.as_ref().map(|more| {
            let words = match more.open {
                true => "Hide versions".to_string(),
                false => format!("+{}", plural(more.count, "version")),
            };
            badge_to("more", more.key, words, cx)
        });
        let undone = row.undone.as_ref().map(|undone| {
            let words = match undone.open {
                true => "Hide undone".to_string(),
                false => format!("{} undone", undone.count),
            };
            badge_to("undone", undone.key, words, cx).tooltip(|_, cx| {
                Tooltip::new(
                    "Taken back with undo. Click to show it. To keep work as a version, undo with",
                )
                .shortcut("mod+alt+z")
                .view(cx)
            })
        });
        let version_info = self.layout.versions.get(row.version);
        let start = version_info
            .filter(|version| version.split.is_none() && version.start == row.point)
            .map(|_| {
                badge(link, link.opacity(0.18))
                    .debug_selector(move || format!("history-start-{index}"))
                    .child("Project start")
            });
        let name = row.tip.then(|| {
            let name = version_info
                .map(|version| name_of(version, row.version))
                .unwrap_or_default();
            // In the colour of its line, so the name reads as that line's across others.
            let color = colors.version(row.version);
            let renaming = (self.renaming.as_ref())
                .filter(|(start, ..)| version_info.is_some_and(|version| version.start == *start))
                .map(|(.., input)| input.clone());
            match renaming {
                Some(input) => div().flex_1().child(input).into_any_element(),
                None => div()
                    .px(px(7.))
                    .rounded(px(6.))
                    .bg(color.opacity(0.16))
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(name)
                    .into_any_element(),
            }
        });
        let fold = (row.changes.as_ref()).filter(|_| is_fold).map(|fold| {
            badge(muted, fill)
                .debug_selector(move || format!("history-fold-{index}"))
                .child(format!("+{}", fold.count))
        });
        // One line: what the row is on the left, when and whether it is current on the right.
        let content = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .overflow_hidden()
                    .children(name)
                    .children(start)
                    .children(fold)
                    .children(hide)
                    .children(more)
                    .children(undone),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(now_tag)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .whitespace_nowrap()
                            .child(ago(now, row.time)),
                    ),
            );
        let undone_from = (row.undone.as_ref())
            .filter(|fold| !fold.open)
            .map(|fold| fold.key);
        let (point, key, version) = (
            row.point,
            row.changes.as_ref().map(|fold| fold.key),
            row.version,
        );
        div()
            .id(("history-row", index))
            .debug_selector(move || format!("history-row-{index}"))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .pr(px(16.))
            .cursor_pointer()
            .when(row.now, |this| this.bg(selected))
            .hover(|style| style.bg(hover))
            .on_click(cx.listener(move |panel, _, _, cx| match (is_fold, key) {
                (true, Some(key)) => panel.toggle_fold(key, cx),
                _ => panel.go_to(point, version, cx),
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    panel.open_menu(version, undone_from, event.position, window, cx)
                }),
            )
            .child(graph)
            .child(div().flex_1().min_w_0().child(content))
            .into_any_element()
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hairline = cx.theme().alpha_at(0.05);
        let count = self.layout.rows.len();
        let list = uniform_list(
            "history-rows",
            count,
            cx.processor(|panel, range: Range<usize>, _, cx| panel.rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .w_full()
        .flex_1()
        .min_h_0();
        div()
            .id("history-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            // As the agent's sidebar: the top edge parts it from the title row.
            .border_t_1()
            .border_r_1()
            .border_color(hairline)
            .child(
                div()
                    .flex_none()
                    .h(px(56.))
                    .flex()
                    .items_center()
                    .pl(px(24.))
                    .child(self.tabs.clone()),
            )
            .child(list)
            .child(self.menu.clone())
    }
}

/// What the dot of a row is.
#[derive(Copy, Clone)]
enum Dot {
    /// The latest point of a version.
    Tip {
        now: bool,
    },
    Point {
        now: bool,
    },
    /// Points folded into the row: the line runs through.
    Fold,
}

impl Dot {
    fn of(row: &Row) -> Self {
        match (row.is_fold(), row.tip) {
            (true, _) => Self::Fold,
            (false, true) => Self::Tip { now: row.now },
            (false, false) => Self::Point { now: row.now },
        }
    }
}

/// The colours of the versions: marks only, never a meaning of their own.
#[derive(Clone)]
struct Colors {
    versions: Vec<Hsla>,
    background: Hsla,
    now: Hsla,
}

impl Colors {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            versions: vec![
                theme.blue,
                theme.mauve,
                theme.teal,
                theme.peach,
                theme.pink,
                theme.green,
                theme.sapphire,
                theme.flamingo,
            ],
            background: theme.gray_100,
            now: theme.gray_950,
        }
    }

    fn version(&self, version: usize) -> Hsla {
        self.versions
            .get(version % self.versions.len().max(1))
            .copied()
            .unwrap_or(self.now)
    }
}

/// `stub` draws the work undone from this point: a faint branch that ends at once.
fn paint(
    bounds: Bounds<Pixels>,
    drawing: &Drawing,
    dot: Dot,
    stub: bool,
    colors: &Colors,
    window: &mut Window,
) {
    let x = |lane: usize| bounds.left() + px(GRAPH_INSET + LANE_WIDTH * lane as f32);
    let (top, bottom) = (bounds.top(), bounds.bottom());
    let middle = bounds.center().y;
    let bend = px(ROW_HEIGHT * 0.35);
    for line in &drawing.lines {
        let mut path = PathBuilder::stroke(px(2.));
        if line.merge {
            path = path.dash_array(&[px(3.), px(3.)]);
        }
        match line.shape {
            Shape::Through(lane) => {
                path.move_to(point(x(lane), top));
                path.line_to(point(x(lane), bottom));
            }
            Shape::Down(lane) => {
                path.move_to(point(x(lane), middle));
                path.line_to(point(x(lane), bottom));
            }
            Shape::Up(lane) => {
                path.move_to(point(x(lane), top));
                path.line_to(point(x(lane), middle));
            }
            Shape::FromTop { from, to } => {
                path.move_to(point(x(from), top));
                path.cubic_bezier_to(
                    point(x(to), middle),
                    point(x(from), top + bend),
                    point(x(to), middle - bend),
                );
            }
            Shape::FromBottom { from, to } => {
                path.move_to(point(x(from), bottom));
                path.cubic_bezier_to(
                    point(x(to), middle),
                    point(x(from), bottom - bend),
                    point(x(to), middle + bend),
                );
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, colors.version(line.version).opacity(0.55));
        }
    }
    let centre = point(x(drawing.dot), middle);
    let color = colors.version(drawing.version);
    if stub {
        let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(2.), px(2.5)]);
        let end = centre + point(px(LANE_WIDTH * 0.7), px(-ROW_HEIGHT * 0.4));
        path.move_to(centre);
        path.cubic_bezier_to(end, centre + point(px(LANE_WIDTH * 0.5), px(0.)), end);
        if let Ok(path) = path.build() {
            window.paint_path(path, color.opacity(0.45));
        }
    }
    match dot {
        Dot::Fold => {
            for offset in [-5., 0., 5.] {
                circle(window, centre + point(px(0.), px(offset)), 1.5, color);
            }
        }
        Dot::Tip { now } => {
            if now {
                ring(window, centre, 9., 1.5, colors.now);
            }
            circle(window, centre, 5.5, color);
        }
        Dot::Point { now } => {
            if now {
                ring(window, centre, 8., 1.5, colors.now);
            }
            circle(window, centre, 4., color);
            circle(window, centre, 2., colors.background);
        }
    }
}

/// A small pill of the tree, such as "+12" or "Current".
fn badge(color: Hsla, fill: Hsla) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .h(px(20.))
        .px(px(7.))
        .rounded(px(10.))
        .bg(fill)
        .text_color(color)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
}

fn circle(window: &mut Window, centre: Point<Pixels>, radius: f32, color: Hsla) {
    let origin = centre - point(px(radius), px(radius));
    let bounds = Bounds::new(origin, gpui::size(px(radius * 2.), px(radius * 2.)));
    window.paint_quad(gpui::fill(bounds, color).corner_radii(px(radius)));
}

fn ring(window: &mut Window, centre: Point<Pixels>, radius: f32, width: f32, color: Hsla) {
    let origin = centre - point(px(radius), px(radius));
    let bounds = Bounds::new(origin, gpui::size(px(radius * 2.), px(radius * 2.)));
    let quad = gpui::outline(bounds, color, gpui::BorderStyle::Solid);
    window.paint_quad(quad.corner_radii(px(radius)).border_widths(px(width)));
}

/// The name of a version, or its number until it has one.
fn name_of(version: &Version, index: usize) -> String {
    match &version.name {
        Some(named) => named.name.clone(),
        None => format!("Version {}", index + 1),
    }
}

/// The question that names a version: what it changed, file by file.
fn naming_prompt(changes: &[FileChange]) -> String {
    let mut prompt = String::from(
        "You name versions in Sound Tools, a music app. A version is one idea a musician \
         tried. The project is a folder of JSON files: instruments, effects, tracks, clips, \
         notes, tempo and connections, and TypeScript for tools of its own. Below is what this \
         version changed. Reply with only a short name for the musical idea, 2 to 4 words, no \
         quotes and no full stop. Name the music, not the files.\n",
    );
    for change in changes {
        let text = |bytes: &Option<Vec<u8>>| match bytes {
            Some(bytes) => String::from_utf8_lossy(bytes)
                .chars()
                .take(PROMPT_FILE)
                .collect(),
            None => "(none)".to_string(),
        };
        let part = format!(
            "\n{}\nbefore: {}\nafter: {}\n",
            change.path.display(),
            text(&change.before),
            text(&change.after),
        );
        if prompt.len() + part.len() > PROMPT_ALL {
            prompt.push_str("\n(and more files)\n");
            break;
        }
        prompt.push_str(&part);
    }
    prompt
}

/// The name in an answer: its last line, without quotes or a full stop.
fn clean_name(answer: &str) -> Option<String> {
    let line = answer
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    let name: String = line
        .trim_matches(|c: char| c == '"' || c == '“' || c == '”' || c == '.' || c == '*')
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => format!("1 {word}"),
        _ => format!("{count} {word}s"),
    }
}

fn ago(now: u64, then: u64) -> SharedString {
    let minutes = now.saturating_sub(then) / 60_000;
    let text = match minutes {
        0 => "Just now".to_string(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => plural((minutes / 1440) as usize, "day") + " ago",
    };
    text.into()
}
