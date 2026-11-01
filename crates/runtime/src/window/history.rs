//! The history panel: every state the project was in, as a tree of versions. A click on a
//! point goes there; a change made there starts a new version. By default the tree shows the
//! points that matter, and the rest folds away behind a click.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla, MouseButton,
    MouseDownEvent, PathBuilder, Pixels, Point, SharedString, Task, UniformListScrollHandle,
    Window, canvas, div, point, prelude::*, px, uniform_list,
};
use sound_history::{Drawing, Graph, History, Layout, ObjectId, Recorded, Row, Scope, Shape};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::TextInput;
use sound_ui::{ActiveTheme, Session};

const ROW_HEIGHT: f32 = 44.;
const LANE_WIDTH: f32 = 16.;
/// From the left of the graph to the middle of the first lane.
const GRAPH_INSET: f32 = 16.;
/// How often the panel looks for finished undo steps.
const LOOK_EVERY: Duration = Duration::from_millis(250);

pub(super) struct HistoryPanel {
    session: Entity<Session>,
    history: Option<History>,
    layout: Layout,
    /// The folds that are open.
    open: BTreeSet<ObjectId>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _watching: Task<()>,
    /// The project's [`steps_changed`](sound_core::Project::steps_changed) at the last save.
    saved_steps: u64,
    was_playing: bool,
    /// The context menu, and the version it is for.
    menu: Entity<DropdownMenu>,
    menu_version: Option<usize>,
    /// The version being named, and the field for it.
    renaming: Option<(ObjectId, Entity<TextInput>)>,
}

impl HistoryPanel {
    pub(super) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let root = session.read(cx).project().root().to_path_buf();
        let history = match History::open(&root, scope()) {
            Ok(history) => Some(history),
            Err(error) => {
                session.update(cx, |session, cx| {
                    session.report(format!("The history is not kept: {error}"), cx)
                });
                None
            }
        };
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |panel, playhead, cx| {
            let playing = playhead.read(cx).playing;
            // Stopping is a pause: what is there now is a moment.
            if panel.was_playing && !playing {
                panel.save_moment(cx);
            }
            panel.was_playing = playing;
        })
        .detach();
        let menu = cx.new(|cx| {
            DropdownMenu::new("", Vec::new(), cx)
                .trigger(Trigger::Pointer)
                .width(260.)
        });
        cx.subscribe_in(&menu, window, Self::picked).detach();
        let mut panel = Self {
            session,
            history,
            layout: Layout::default(),
            open: BTreeSet::new(),
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _watching: Task::ready(()),
            saved_steps: 0,
            was_playing: false,
            menu,
            menu_version: None,
            renaming: None,
        };
        // What the folder holds as it opens. The first point, and one that changed while the
        // app was closed, are moments.
        let first = panel
            .history
            .as_ref()
            .is_some_and(|history| history.now().is_none());
        let label = if first {
            "Start of the history"
        } else {
            "Changed while the app was closed"
        };
        if panel.save(label, cx) || first {
            panel.mark_moment(cx);
        }
        panel.relayout();
        panel._watching = cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor().timer(LOOK_EVERY).await;
                // Ends with the panel.
                if panel.update(cx, Self::save_finished_steps).is_err() {
                    return;
                }
            }
        });
        panel
    }

    /// The agent finished a turn: what it made is a moment.
    pub(super) fn turn_ended(&mut self, cx: &mut Context<Self>) {
        self.save_moment(cx);
    }

    fn save_moment(&mut self, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.mark_moment(cx);
    }

    fn mark_moment(&mut self, cx: &mut Context<Self>) {
        let Some(history) = &mut self.history else {
            return;
        };
        match history.mark_moment() {
            Ok(true) => {
                self.relayout();
                cx.notify();
            }
            Ok(false) => {}
            Err(error) => self.report(error, cx),
        }
    }

    /// Every finished undo step is a point: a drag that ended, edits of a tool that were
    /// written, an agent's files, an undo. Their files are written by then.
    fn save_finished_steps(&mut self, cx: &mut Context<Self>) {
        let steps = self.session.read(cx).project().steps_changed();
        if steps != self.saved_steps {
            self.save_now(cx);
        }
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        self.saved_steps = self.session.read(cx).project().steps_changed();
        let label = self
            .session
            .read(cx)
            .project()
            .undo_label()
            .unwrap_or("Change")
            .to_string();
        if self.save(&label, cx) {
            cx.notify();
        }
    }

    /// Saves the folder as a point named `label`, or moves to the point it matches. Says
    /// whether the tree changed.
    fn save(&mut self, label: &str, cx: &mut Context<Self>) -> bool {
        let Some(history) = &mut self.history else {
            return false;
        };
        match history.record(label) {
            Ok(Recorded::Unchanged) => false,
            Ok(Recorded::Moved(_) | Recorded::New(_)) => {
                self.relayout();
                true
            }
            Err(error) => {
                self.report(error, cx);
                false
            }
        }
    }

    fn relayout(&mut self) {
        if let Some(history) = &self.history {
            self.layout = Graph::of(history).layout(&self.open);
        }
    }

    fn report(&self, error: impl std::fmt::Display, cx: &mut Context<Self>) {
        let message = format!("History: {error}");
        self.session
            .update(cx, |session, cx| session.report(message, cx));
    }

    /// Whether the project may go elsewhere now. Not during a drag or a recording.
    fn may_move(&self, cx: &mut Context<Self>) -> bool {
        let session = self.session.read(cx);
        let recording = session.playhead().read(cx).playing
            && session.recording().read(cx).armed().next().is_some();
        if session.gesture_open() || recording {
            self.report("stop recording first", cx);
            return false;
        }
        true
    }

    fn go_to(&mut self, target: ObjectId, label: &str, cx: &mut Context<Self>) {
        if !self.may_move(cx) {
            return;
        }
        self.save_now(cx);
        let Some(history) = &mut self.history else {
            return;
        };
        if history.now() == Some(target) {
            return;
        }
        let plan = match history.go_to(target) {
            Ok(plan) => plan,
            Err(error) => return self.report(error, cx),
        };
        self.restore(&format!("Go to {label}"), plan, cx);
    }

    fn merge(&mut self, version: usize, keep_mine: bool, cx: &mut Context<Self>) {
        if !self.may_move(cx) {
            return;
        }
        self.save_now(cx);
        let Some(merged) = self.layout.versions.get(version).cloned() else {
            return;
        };
        let Some(history) = &mut self.history else {
            return;
        };
        // A version with no name is called after where it got to.
        let name = (merged.name.clone())
            .or_else(|| history.point(merged.tip).map(|tip| tip.label.clone()))
            .unwrap_or_default();
        let plan = match history.merge(merged.tip, keep_mine) {
            Ok(plan) => plan,
            Err(error) => return self.report(error, cx),
        };
        let label = format!("Merge “{name}”");
        self.restore(&label, plan, cx);
        self.mark_moment(cx);
    }

    /// Writes the files of a plan into the project as one undo step, and saves what it made.
    fn restore(
        &mut self,
        label: &str,
        plan: Vec<(PathBuf, Option<Vec<u8>>)>,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.restore(label, plan));
        });
        self.save(label, cx);
        self.relayout();
        cx.notify();
    }

    fn toggle_fold(&mut self, key: ObjectId, cx: &mut Context<Self>) {
        if !self.open.remove(&key) {
            self.open.insert(key);
        }
        self.relayout();
        cx.notify();
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
        let now = self.layout.versions.iter().find(|version| version.now);
        let mut items = vec![MenuItem::new("rename", "Rename").selectable(false)];
        if !info.now {
            let into = (now.and_then(|now| now.name.as_ref()))
                .map_or("what you are on".to_string(), |name| format!("“{name}”"));
            items.push(
                MenuItem::new("merge", format!("Merge into {into}"))
                    .description("Where both changed something, this one wins")
                    .selectable(false),
            );
            items.push(
                MenuItem::new("merge-mine", format!("Merge into {into}, keep mine"))
                    .description("Where both changed something, yours stays")
                    .selectable(false),
            );
            let (value, label) = match info.aside {
                true => ("aside", "Bring back"),
                false => ("aside", "Fold away"),
            };
            items.push(MenuItem::new(value, label).selectable(false));
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
            "rename" => self.start_renaming(version, window, cx),
            _ => {}
        }
    }

    fn toggle_aside(&mut self, version: usize, cx: &mut Context<Self>) {
        let (Some(info), Some(history)) = (self.layout.versions.get(version), &mut self.history)
        else {
            return;
        };
        if let Err(error) = history.set_aside(info.start, !info.aside) {
            self.report(error, cx);
        }
        self.relayout();
        cx.notify();
    }

    fn start_renaming(&mut self, version: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(info) = self.layout.versions.get(version).cloned() else {
            return;
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.set_text(info.name.clone().unwrap_or_default(), cx);
            input.select_all_text(cx);
            input
        });
        let panel = cx.entity().downgrade();
        input.update(cx, |input, _| {
            input.set_on_submit(move |name, _, cx| {
                let name = name.to_string();
                // Fails only when the panel is gone.
                panel
                    .update(cx, |panel, cx| panel.finish_renaming(&name, cx))
                    .ok();
            })
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        self.renaming = Some((info.start, input));
        cx.notify();
    }

    fn finish_renaming(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some((start, _)) = self.renaming.take() else {
            return;
        };
        if let Some(history) = &mut self.history
            && let Err(error) = history.set_name(start, name)
        {
            self.report(error, cx);
        }
        self.relayout();
        cx.notify();
    }

    fn rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let lanes = self.layout.lanes(range.clone());
        let graph_width = GRAPH_INSET * 2. + LANE_WIDTH * lanes.len().saturating_sub(1) as f32;
        let colors = Colors::of(cx);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
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
        let (text, muted, hover, selected, link, now_text) = (
            theme.gray_950,
            theme.gray_700,
            theme.alpha_at(0.05),
            theme.alpha_at(0.08),
            theme.blue,
            theme.gray_100,
        );
        let version = self.layout.versions.get(row.version);
        let color = colors.version(row.version);
        let graph = {
            let colors = colors.clone();
            let moment = row.moment;
            let is_now = row.now;
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    paint(bounds, &drawing, moment, is_now, &colors, window)
                },
            )
            .flex_none()
            .w(px(graph_width))
            .h_full()
        };
        // A version is named on its latest row, unless the row already says that name.
        let renaming = (self.renaming.as_ref())
            .filter(|(start, _)| version.is_some_and(|version| version.start == *start))
            .map(|(_, input)| div().w(px(140.)).child(input.clone()).into_any_element());
        let given = version
            .filter(|_| row.tip)
            .and_then(|version| version.name.clone());
        let name = renaming.or_else(|| {
            given.filter(|name| *name != row.label).map(|name| {
                div()
                    .px(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color.opacity(0.6))
                    .text_color(color)
                    .child(SharedString::from(name))
                    .into_any_element()
            })
        });
        let fold = |id: &str, fold: &sound_history::Fold, words: String, cx: &mut Context<Self>| {
            let key = fold.key;
            div()
                .id(SharedString::from(format!("{id}-{key}")))
                .text_color(link)
                .cursor_pointer()
                .child(words)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.toggle_fold(key, cx)
                }))
        };
        let changes = row.changes.as_ref().map(|changes| {
            let words = match changes.open {
                true => "hide changes".to_string(),
                false => plural(changes.count, "change"),
            };
            fold("changes", changes, words, cx)
        });
        let more = row.more.as_ref().map(|more| {
            let words = match more.open {
                true => "hide versions".to_string(),
                false => format!("+ {}", plural(more.count, "more version")),
            };
            fold("more", more, words, cx)
        });
        let point = row.point;
        let label = row.label.clone();
        let version_index = row.version;
        div()
            .id(("history-row", index))
            .debug_selector(move || format!("history-row-{index}"))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .pr(px(12.))
            .cursor_pointer()
            .when(row.now, |this| this.bg(selected))
            .hover(|style| style.bg(hover))
            .on_click(cx.listener(move |panel, _, _, cx| panel.go_to(point, &label, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    panel.open_menu(version_index, event.position, window, cx)
                }),
            )
            .child(graph)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .text_color(if row.inside_fold { muted } else { text })
                            .when(row.moment, |label| label.font_weight(FontWeight::MEDIUM))
                            .child(SharedString::from(row.label.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .text_color(muted)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(ago(now, row.time))
                            .children(name)
                            .when(row.now, |meta| {
                                meta.child(
                                    div()
                                        .px(px(4.))
                                        .rounded(px(4.))
                                        .bg(text)
                                        .text_color(now_text)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Now"),
                                )
                            })
                            .children(changes)
                            .children(more),
                    ),
            )
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
            .child(
                div()
                    .flex_none()
                    .h(px(56.))
                    .flex()
                    .items_center()
                    .pl(px(24.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("History"),
            )
            .child(list)
            .child(self.menu.clone())
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
                theme.pink,
                theme.sapphire,
                theme.flamingo,
                theme.sky,
                theme.rosewater,
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
    moment: bool,
    is_now: bool,
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
            window.paint_path(path, colors.version(line.version).opacity(0.7));
        }
    }
    let centre = point(x(drawing.dot), middle);
    let color = colors.version(drawing.version);
    if is_now {
        ring(window, centre, 9., 1.5, colors.now);
    }
    if moment {
        circle(window, centre, 5.5, color);
    } else {
        circle(window, centre, 4., color);
        circle(window, centre, 2., colors.background);
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

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => format!("1 {word}"),
        _ => format!("{count} {word}s"),
    }
}

fn ago(now: u64, then: u64) -> SharedString {
    let minutes = now.saturating_sub(then) / 60_000;
    let text = match minutes {
        0 => "just now".to_string(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => plural((minutes / 1440) as usize, "day") + " ago",
    };
    text.into()
}

/// What of the project folder is the piece: its records, its settings and the code of its own
/// tools, without what the runtime writes there itself.
fn scope() -> Scope {
    Scope {
        roots: ["project.json", "state", sound_typescript::FOLDER]
            .map(PathBuf::from)
            .to_vec(),
        skip: crate::templates::written::written_by_the_runtime,
    }
}
