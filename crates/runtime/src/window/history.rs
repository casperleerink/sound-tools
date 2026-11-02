//! The History tab of the left panel: the points of the project as a tree, newest at the top,
//! one lane per version. Clicking a row goes to its point; its menu renames, keeps, merges and
//! folds away. The list is one tab stop: up and down move a highlight, enter goes to its point,
//! the menu key or shift-F10 opens its menu, right and left open and close the fold under it.
//! Which points are rows is [`layout`].

mod layout;

use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, ScrollEvent,
    ScrollHandle, SharedString, Subscription, Task, Window, canvas, div, point, prelude::*, px,
};
use sound_core::{History, PointId};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::{self, TextInput};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::focus::KeyboardFocus;
use sound_ui::{ActiveTheme, Session, Theme};

use layout::{Edge, Layout, Name, Opened, Page, Row, Stroke};

/// How many points are read at a time, newest first.
const PAGE: usize = 300;
/// Closer than this to the end of what is read, the next page is read.
const READ_AHEAD: f32 = 800.;
const ROW_HEIGHT: f32 = 40.;
const LANE_WIDTH: f32 = 16.;
/// The middle of the first lane, from the left edge.
const FIRST_LANE: f32 = 20.;
const MENU_WIDTH: f32 = 280.;

pub(super) struct HistoryPanel {
    session: Entity<Session>,
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// The row the keys moved to. Until they move, the current point's.
    highlight: Option<PointId>,
    /// How many points to read.
    wanted: usize,
    /// What was read, at which generation of the history and how many points were wanted.
    read: Option<(u64, usize)>,
    page: Result<Option<Page>, SharedString>,
    opened: Opened,
    layout: Layout,
    scroll: ScrollHandle,
    menu: Entity<DropdownMenu>,
    /// The line of the row whose menu is open.
    menu_line: Option<PointId>,
    /// The line being renamed, in a field on its latest row.
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
            wanted: PAGE,
            read: None,
            page: Ok(None),
            opened: Opened::default(),
            layout: Layout::default(),
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
            self.page = Ok(None);
            self.layout = Layout::default();
            return;
        };
        let now = (history.generation(), self.wanted);
        if self.read == Some(now) {
            return;
        }
        self.read = Some(now);
        self.page = read_page(history, self.wanted)
            .map(Some)
            .map_err(|error| format!("The history could not be read: {error}").into());
        self.lay_out();
    }

    fn lay_out(&mut self) {
        self.layout = match &self.page {
            Ok(Some(page)) => Layout::new(page, &self.opened),
            _ => Layout::default(),
        };
    }

    fn toggle_fold(&mut self, first: PointId, cx: &mut Context<Self>) {
        if !self.opened.folds.remove(&first) {
            self.opened.folds.insert(first);
        }
        self.lay_out();
        cx.notify();
    }

    fn show_lines(&mut self, lines: &[PointId], show: bool, cx: &mut Context<Self>) {
        for line in lines {
            if show {
                self.opened.lines.insert(*line);
            } else {
                self.opened.lines.remove(line);
            }
        }
        self.lay_out();
        cx.notify();
    }

    fn read_more_near_the_end(&mut self, cx: &mut Context<Self>) {
        let all_read = match &self.page {
            Ok(Some(page)) => page.points.len() < self.wanted,
            _ => true,
        };
        let left = self.scroll.max_offset().y + self.scroll.offset().y;
        if !all_read && left < px(READ_AHEAD) {
            self.wanted += PAGE;
            cx.notify();
        }
    }

    fn open_menu(
        &mut self,
        line: PointId,
        at: Point<Pixels>,
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
        let name = (self.session.read(cx).project().history())
            .and_then(|history| history.line(start))
            .and_then(|line| line.name.clone())
            .unwrap_or_default();
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

    fn highlighted_row(&self) -> Option<usize> {
        let rows = &self.layout.rows;
        (self
            .highlight
            .and_then(|point| rows.iter().position(|row| row.point == point)))
        .or_else(|| rows.iter().position(|row| row.current))
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
        let Some(index) = self.highlighted_row() else {
            return;
        };
        let fold = (self.layout.folds.iter()).find(|fold| fold.rows.0 == index);
        match keys.key.as_str() {
            "up" => self.highlight_row(index.saturating_sub(1), cx),
            "down" => self.highlight_row(index + 1, cx),
            "enter" => {
                let point = self.layout.rows[index].point;
                self.session
                    .update(cx, |session, cx| session.go_to(point, cx));
            }
            "right" | "left" => {
                if let Some(fold) = fold.filter(|fold| fold.open == (keys.key == "left")) {
                    self.toggle_fold(fold.first, cx);
                }
            }
            _ if opens_menu => {
                let line = self.layout.rows[index].line;
                let at = self.scroll.bounds().origin
                    + self.scroll.offset()
                    + point(px(text_left(self.layout.lanes)), px(row_y(index)));
                self.open_menu(line, at, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Moves the highlight to a row and scrolls it into view.
    fn highlight_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.layout.rows.get(index) else {
            return;
        };
        self.highlight = Some(row.point);
        let (offset, height) = (self.scroll.offset(), self.scroll.bounds().size.height);
        let (top, bottom) = (
            px(index as f32 * ROW_HEIGHT),
            px((index + 1) as f32 * ROW_HEIGHT),
        );
        let shown_top = -offset.y;
        if top < shown_top {
            self.scroll.set_offset(point(offset.x, -top));
        } else if bottom > shown_top + height {
            self.scroll.set_offset(point(offset.x, height - bottom));
        }
        self.read_more_near_the_end(cx);
        cx.notify();
    }

    fn row(
        &self,
        index: usize,
        row: &Row,
        highlighted: bool,
        text_left: f32,
        colors: &Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let point = row.point;
        let lane_color = colors.lane(row.color);
        let name = match (&row.name, &self.renaming) {
            (_, Some((line, input))) if *line == row.line && row.tip => {
                Some(div().w(px(200.)).child(input.clone()).into_any_element())
            }
            (Name::ProjectStart, _) => {
                Some(name_text("Project start", colors.blue).into_any_element())
            }
            (Name::Line(title), _) => Some(name_text(title.clone(), lane_color).into_any_element()),
            (Name::None, _) => None,
        };
        let quiet = self.quiet_actions(row, colors, cx);
        let line = row.line;
        div()
            .id(("history-row", index))
            .debug_selector(move || format!("history-row-{}", point.short()))
            .absolute()
            .top(px(index as f32 * ROW_HEIGHT))
            .left_0()
            .right_0()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(text_left))
            .pr(px(16.))
            .cursor_pointer()
            .when(row.current, |row| row.bg(colors.current_row))
            .hover(|style| style.bg(colors.hover_row))
            .when(highlighted, |row| {
                row.shadow(vec![BoxShadow {
                    color: colors.ring,
                    offset: Point::default(),
                    blur_radius: px(0.),
                    spread_radius: px(1.5),
                    inset: true,
                }])
            })
            .on_click(cx.listener(move |panel, _, _, cx| {
                panel
                    .session
                    .update(cx, |session, cx| session.go_to(point, cx))
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    panel.open_menu(line, event.position, window, cx);
                }),
            )
            .children(name)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .text_size(px(13.))
                    .text_color(colors.muted)
                    .children(quiet),
            )
            .when(row.current, |row| row.child(current_badge(colors)))
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(colors.muted)
                    .child(ago(row.time, SystemTime::now())),
            )
    }

    /// "+1 version", "5 undone" and "Hide versions": gray words that show or hide the lines
    /// that leave a row.
    fn quiet_actions(&self, row: &Row, colors: &Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let point = row.point.short();
        let (muted, bright) = (colors.muted, colors.text);
        let action = |selector: String, lines: &[PointId], show: bool| {
            let lines = lines.to_vec();
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || format!("history-{selector}"))
                .text_color(muted)
                .hover(move |style| style.text_color(bright))
                .cursor_pointer()
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.show_lines(&lines, show, cx);
                }))
        };
        let mut actions = Vec::new();
        if let count @ 1.. = row.hidden_versions.len() {
            let label = match count {
                1 => "+1 version".to_string(),
                _ => format!("+{count} versions"),
            };
            actions
                .push(action(format!("versions-{point}"), &row.hidden_versions, true).child(label));
        }
        if !row.undone.is_empty() {
            let tooltip = Tooltip::new(
                "Taken back with undo. Click to show it. This undo keeps work as a version:",
            )
            .shortcut("alt+mod+z");
            actions.push(
                action(format!("undone-{point}"), &row.undone, true)
                    .tooltip(move |_, cx| tooltip.clone().view(cx))
                    .child(format!("{} undone", row.undone_points)),
            );
        }
        if !row.opened.is_empty() {
            actions
                .push(action(format!("hide-{point}"), &row.opened, false).child("Hide versions"));
        }
        let mut elements = Vec::new();
        for (index, action) in actions.into_iter().enumerate() {
            if index > 0 {
                elements.push(div().px(px(6.)).child("·").into_any_element());
            }
            elements.push(action.into_any_element());
        }
        elements
    }
}

fn name_text(text: impl Into<SharedString>, color: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .max_w(px(220.))
        .truncate()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(text.into())
}

fn current_badge(colors: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(colors.text)
        .text_color(colors.badge_text)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .child("Current")
}

fn read_page(history: &History, wanted: usize) -> std::io::Result<Page> {
    let points = history.points(wanted)?;
    Ok(Page {
        points,
        lines: history.lines().to_vec(),
        current: history.current(),
        folded_away: history.folded_away()?,
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
    blue: Hsla,
    text: Hsla,
    muted: Hsla,
    badge_text: Hsla,
    current_row: Hsla,
    hover_row: Hsla,
    background: Hsla,
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
            blue: theme.blue,
            text: theme.gray_950,
            muted: theme.gray_700,
            badge_text: theme.gray_50,
            current_row: theme.alpha_at(0.05),
            hover_row: theme.alpha_at(0.03),
            background: theme.gray_100,
            ring: theme.lavender,
        }
    }

    fn lane(&self, color: usize) -> Hsla {
        self.lanes[color % self.lanes.len()]
    }
}

fn lane_x(lane: usize) -> f32 {
    FIRST_LANE + lane as f32 * LANE_WIDTH
}

/// Where the text of the rows begins, right of every lane.
fn text_left(lanes: usize) -> f32 {
    lane_x(lanes.max(1)) + 4.
}

fn row_y(row: usize) -> f32 {
    row as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.
}

/// The edges, the dots and the threads from a dot to its name, for the rows in view.
struct Drawing {
    edges: Vec<Edge>,
    rows: Vec<DrawnRow>,
    text_left: f32,
    colors: Colors,
}

/// What the canvas draws of a row.
struct DrawnRow {
    lane: usize,
    color: Hsla,
    /// A full dot, at the latest point of its line.
    tip: bool,
    current: bool,
    /// A thread from the dot to the line's name.
    named: bool,
    /// A stub for the lines hidden that leave it.
    stub: bool,
}

fn paint(bounds: Bounds<Pixels>, drawing: Drawing, window: &mut Window) {
    let origin = bounds.origin;
    let at = |x: f32, y: f32| origin + point(px(x), px(y));
    let visible = window.content_mask().bounds;
    let (top, bottom) = (
        f32::from(visible.top() - origin.y) - ROW_HEIGHT,
        f32::from(visible.bottom() - origin.y) + ROW_HEIGHT,
    );
    let in_view = |from: f32, to: f32| to >= top && from <= bottom;
    let stroke = |kind: Stroke| {
        let builder = PathBuilder::stroke(px(1.5));
        match kind {
            Stroke::Solid => builder,
            Stroke::Dotted => builder.dash_array(&[px(1.5), px(3.)]),
            Stroke::Dashed => builder.dash_array(&[px(4.), px(3.)]),
        }
    };
    for edge in &drawing.edges {
        let (x1, y1, x2, y2) = (
            lane_x(edge.from.1),
            row_y(edge.from.0),
            lane_x(edge.to.1),
            row_y(edge.to.0),
        );
        if !in_view(y1, y2) {
            continue;
        }
        let mut path = stroke(edge.stroke);
        path.move_to(at(x1, y1));
        if x1 == x2 {
            path.line_to(at(x2, y2));
        } else if edge.stroke == Stroke::Dashed {
            // A merge leaves its row for the lane of what it merged, then runs down to it.
            let bend = y1 + ROW_HEIGHT;
            path.cubic_bezier_to(
                at(x2, bend),
                at(x1, bend - ROW_HEIGHT / 2.),
                at(x2, y1 + ROW_HEIGHT / 2.),
            );
            path.line_to(at(x2, y2));
        } else {
            // A line runs down its own lane and bends into the one it splits off.
            let bend = y2 - ROW_HEIGHT;
            path.line_to(at(x1, bend));
            path.cubic_bezier_to(
                at(x2, y2),
                at(x1, bend + ROW_HEIGHT / 2.),
                at(x2, y2 - ROW_HEIGHT / 2.),
            );
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, drawing.colors.lane(edge.color));
        }
    }
    let rows = || drawing.rows.iter().enumerate();
    let row_in_view = |row: usize| in_view(row_y(row), row_y(row));
    for (row, drawn) in rows().filter(|(row, drawn)| drawn.stub && row_in_view(*row)) {
        let (x, y) = (lane_x(drawn.lane), row_y(row));
        let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(2.), px(2.)]);
        path.move_to(at(x + 4., y - 4.));
        path.curve_to(at(x + 12., y - 16.), at(x + 10., y - 6.));
        if let Ok(path) = path.build() {
            window.paint_path(path, drawn.color.opacity(0.4));
        }
    }
    for (row, drawn) in rows().filter(|(row, drawn)| drawn.named && row_in_view(*row)) {
        let (x, y) = (lane_x(drawn.lane), row_y(row));
        let mut path = PathBuilder::stroke(px(1.));
        path.move_to(at(x, y));
        path.line_to(at(drawing.text_left - 2., y));
        if let Ok(path) = path.build() {
            window.paint_path(path, drawn.color.opacity(0.6));
        }
    }
    for (row, drawn) in rows().filter(|(row, _)| row_in_view(*row)) {
        let centre = at(lane_x(drawn.lane), row_y(row));
        if drawn.current {
            ring(window, centre, 8., 1.5, drawing.colors.text);
        }
        if drawn.tip {
            circle(window, centre, 5., drawn.color);
        } else {
            circle(window, centre, 4., drawn.color);
            circle(window, centre, 2.5, drawing.colors.background);
        }
    }
}

fn circle(window: &mut Window, centre: Point<Pixels>, radius: f32, color: Hsla) {
    let bounds = Bounds::centered_at(centre, gpui::size(px(radius * 2.), px(radius * 2.)));
    window.paint_quad(gpui::fill(bounds, color).corner_radii(px(radius)));
}

fn ring(window: &mut Window, centre: Point<Pixels>, radius: f32, width: f32, color: Hsla) {
    let bounds = Bounds::centered_at(centre, gpui::size(px(radius * 2.), px(radius * 2.)));
    let quad = gpui::outline(bounds, color, gpui::BorderStyle::Solid);
    window.paint_quad(quad.corner_radii(px(radius)).border_widths(px(width)));
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
        let message = match &self.page {
            Err(message) => Some(message.clone()),
            Ok(None) => Some("This project keeps no history.".into()),
            Ok(Some(_)) => None,
        };
        if let Some(message) = message {
            return root.child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .text_size(px(13.))
                    .text_color(colors.muted)
                    .child(message),
            );
        }
        let layout = &self.layout;
        let text_left = text_left(layout.lanes);
        let drawing = Drawing {
            edges: layout.edges.clone(),
            rows: (layout.rows.iter())
                .map(|row| DrawnRow {
                    lane: row.lane,
                    color: colors.lane(row.color),
                    tip: row.tip,
                    current: row.current,
                    named: matches!(row.name, Name::Line(_)),
                    stub: !row.undone.is_empty() || !row.hidden_versions.is_empty(),
                })
                .collect(),
            text_left,
            colors,
        };
        // Once the keys moved it, the highlight shows after a click too.
        let keys_moved = self.highlight.is_some() && self.focus_handle.is_focused(window);
        let highlighted = (ring_shows || keys_moved)
            .then(|| self.highlighted_row())
            .flatten();
        let rows: Vec<_> = (layout.rows.iter().enumerate())
            .map(|(index, row)| {
                self.row(
                    index,
                    row,
                    highlighted == Some(index),
                    text_left,
                    &colors,
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let hover = colors.hover_row;
        let folds = (self.layout.folds.iter().enumerate()).map(|(index, fold)| {
            let first = fold.first;
            let (top, bottom) = (row_y(fold.rows.0) + 8., row_y(fold.rows.1) - 8.);
            let action = if fold.open { "fold" } else { "show" };
            let text =
                SharedString::from(format!("{} changes. Click to {action} them", fold.count));
            div()
                .id(("history-fold", index))
                .debug_selector(move || format!("history-fold-{}", first.short()))
                .absolute()
                .top(px(top))
                .h(px(bottom - top))
                .left(px(lane_x(fold.lane) - LANE_WIDTH / 2.))
                .w(px(LANE_WIDTH))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .tooltip(move |_, cx| Tooltip::new(text.clone()).view(cx))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.toggle_fold(first, cx);
                }))
        });
        let height = self.layout.rows.len() as f32 * ROW_HEIGHT;
        root.child(
            div()
                .id("history-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .on_scroll(
                    cx.listener(|panel, _: &ScrollEvent, _, cx| panel.read_more_near_the_end(cx)),
                )
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(height + ROW_HEIGHT))
                        .children(rows)
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, (), window, _| paint(bounds, drawing, window),
                            )
                            .absolute()
                            .inset_0(),
                        )
                        .children(folds),
                ),
        )
    }
}
