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
use sound_ui::{ActiveTheme, Session};

use super::QuickAnswer;

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
    /// The question for a name that runs.
    naming: Option<Task<()>>,
    /// The rows in view reached the end of what is read of the history.
    wants_older: bool,
    /// Versions asked for a name, with the latest point they were asked for: one that failed
    /// is asked again only once it grew.
    asked: BTreeSet<(ObjectId, ObjectId)>,
}

impl HistoryPanel {
    pub(super) fn new(
        session: Entity<Session>,
        tabs: Entity<PanelTabs>,
        quick_answer: Option<QuickAnswer>,
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
            layout: Layout::default(),
            open: BTreeSet::new(),
            laid_out: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _watching: watching,
            menu,
            menu_version: None,
            naming: None,
            wants_older: false,
            asked: BTreeSet::new(),
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

    fn go_to(&mut self, target: ObjectId, cx: &mut Context<Self>) {
        let now = (self.session.read(cx).project().history()).and_then(|history| history.now());
        if now == Some(target) || !self.may_move(cx) {
            return;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.go_to(target))
        });
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
                    .is_some_and(|named| named.tip == version.tip.to_string());
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
                        session.edit(cx, |project| project.name_version(start, &name, tip))
                    });
                    panel.relayout(cx);
                })
                .ok();
        }));
    }

    fn open_menu(
        &mut self,
        version: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.layout.versions.get(version) else {
            return;
        };
        if info.now {
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
        let items = vec![
            MenuItem::new("merge", format!("Merge into {into}"))
                .description("Where both changed something, this one wins")
                .selectable(false),
            MenuItem::new("merge-mine", format!("Merge into {into}, keep mine"))
                .description("Where both changed something, yours stays")
                .selectable(false),
            MenuItem::new("aside", aside).selectable(false),
        ];
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(version) = self.menu_version.take() else {
            return;
        };
        match picked.0.as_ref() {
            "merge" => self.merge(version, false, cx),
            "merge-mine" => self.merge(version, true, cx),
            "aside" => self.toggle_aside(version, cx),
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
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| paint(bounds, &drawing, dot, &colors, window),
            )
            .flex_none()
            .w(px(graph_width))
            .h_full()
        };
        let now_tag = row.now.then(|| {
            div()
                .px(px(5.))
                .rounded(px(4.))
                .bg(text)
                .text_color(theme.gray_100)
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Current")
        });
        let link_to = |id: &str, key: ObjectId, words: String, cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("{id}-{key}")))
                .debug_selector(move || format!("history-{id}-{index}"))
                .text_color(link)
                .cursor_pointer()
                .child(words)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.toggle_fold(key, cx)
                }))
        };
        let hide = (row.changes.as_ref())
            .filter(|fold| fold.open)
            .map(|fold| link_to("hide", fold.key, "Hide".to_string(), cx));
        let more = row.more.as_ref().map(|more| {
            let words = match more.open {
                true => "Hide versions".to_string(),
                false => format!("+{}", plural(more.count, "version")),
            };
            link_to("more", more.key, words, cx)
        });
        let undone = row.undone.as_ref().map(|undone| {
            let words = match undone.open {
                true => "Hide undone".to_string(),
                false => format!("{} undone", undone.count),
            };
            link_to("undone", undone.key, words, cx)
        });
        let more = more.into_iter().chain(undone);
        let ago = div().text_color(muted).child(ago(now, row.time));
        let meta = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .whitespace_nowrap()
            .overflow_hidden();
        let content = if is_fold {
            let count = row.changes.as_ref().map_or(0, |fold| fold.count);
            meta.child(
                div()
                    .debug_selector(move || format!("history-fold-{index}"))
                    .px(px(8.))
                    .rounded(px(10.))
                    .bg(fill)
                    .text_color(muted)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("+{count}")),
            )
            .children(more)
            .into_any_element()
        } else if row.tip {
            let name = (self.layout.versions.get(row.version))
                .map(|version| name_of(version, row.version))
                .unwrap_or_default();
            // In the colour of its line, so the name reads as that line's across others.
            let color = colors.version(row.version);
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .min_w_0()
                .child(
                    div().flex().min_w_0().child(
                        div()
                            .ml(px(-6.))
                            .px(px(6.))
                            .rounded(px(6.))
                            .bg(color.opacity(0.16))
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color)
                            .child(name),
                    ),
                )
                .child(
                    meta.child(ago)
                        .children(now_tag)
                        .children(hide)
                        .children(more),
                )
                .into_any_element()
        } else {
            meta.child(ago)
                .children(now_tag)
                .children(hide)
                .children(more)
                .into_any_element()
        };
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
                _ => panel.go_to(point, cx),
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    panel.open_menu(version, event.position, window, cx)
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

fn paint(
    bounds: Bounds<Pixels>,
    drawing: &Drawing,
    dot: Dot,
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
