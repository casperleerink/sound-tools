//! The card of an instance of a tool of the project, in a rack, or its page, the whole window:
//! the tree its code draws. What moves on it moves here, with no code: a space scrolls and
//! zooms, a drag moves what binds to it and turns what it turns, and live values follow the
//! playhead, the watches and the record. Code hears a press, and a drag once, at its end.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle, Hitbox,
    HitboxBehavior, Hsla, KeyDownEvent, KeyUpEvent, Keystroke, ModifiersChangedEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, StyleRefinement, TextAlign, TextRun, WeakEntity, Window,
    canvas, div, fill, outline, point, prelude::*, px, size,
};
use serde_json::{Value, json};
use sound_core::{InstanceId, ProjectEvent, ValueRange};
use sound_ui::components::device_card::{CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    decibels_readout, hertz_readout, milliseconds_readout, percent_readout, short,
};
use sound_ui::import::{choose_file, import_file};
use sound_ui::{ActiveTheme, ControlEdit, Session, typography};

use crate::bun::PageSize;
use crate::live::{self, DragNumbers, Sources, SpaceView};
use crate::tools::{ToolInfo, Unit};
use crate::tree::{self, Align, Axis, Cursor, Drag, Node, Reach, ShapeKind, SpaceNode, Words};
use crate::window::{Live, Surface, edit_record, state_of};

/// How long an ended drag keeps its values at most, when the tree after its end does not come,
/// such as when Bun stopped.
const ENDED_DRAG: Duration = Duration::from_secs(2);
/// Points of pointer travel for the whole range of what a drag turns, as a knob of the window.
const TRAVEL: f32 = 200.0;
/// With shift, a drag turns this much as far.
const FINE: f32 = 0.1;

pub(crate) struct TypeScriptCard {
    live: Entity<Live>,
    session: Entity<Session>,
    id: InstanceId,
    /// The number the live host knows this card by.
    card: u64,
    /// The frame of its card in a rack; `None` for a page.
    frame: Option<CardFrame>,
    /// The drag of a field of the record.
    edit: ControlEdit,
    /// The scroll and zoom of each space, by its key, and the size it was last laid out at,
    /// which its paint writes.
    spaces: Rc<RefCell<HashMap<String, SpaceView>>>,
    /// The drag of the pointer, from its press until the tree after its end arrives.
    held: Option<Held>,
    /// The shape the pointer is over: the key of its space and its own.
    hovered: Option<(String, String)>,
    /// It has the keys while it, or a control in it, has the focus: a click on it gives it
    /// them, and a page takes them when it opens and nothing else has them.
    focus: FocusHandle,
    /// The keys that went down and not up yet. They go up when it loses the keys, when cmd
    /// goes down and when it closes, so a note a key holds is not held for ever.
    keys_down: BTreeSet<String>,
}

/// A page, which draws again only when it changed, while the window draws every frame as it
/// plays.
pub(crate) struct Page(pub(crate) Entity<TypeScriptCard>);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0
            .clone()
            .cached(StyleRefinement::default().size_full())
    }
}

/// A drag the pointer holds, on a shape or a space.
#[derive(Clone)]
struct Held {
    space: String,
    /// The version of the tree whose handlers it calls.
    version: u64,
    drag: Drag,
    /// Where it was pressed and where the pointer is, in the window.
    start: Point<Pixels>,
    now: Point<Pixels>,
    /// The press in the units of its space, and how many units a point is there.
    start_units: (f64, f64),
    per_point: (f64, f64),
    /// Where the space was at the press, for what turns by where the pointer is.
    bounds: Bounds<Pixels>,
    /// What it turns across and up, as it was at the press.
    from: [Option<f32>; 2],
    modifiers: Modifiers,
    moved: bool,
    /// When it ended, and the number of the event that told its code: its values stay until
    /// the tree after that event arrives.
    ended: Option<(u64, Instant)>,
}

impl Held {
    fn numbers(&self) -> DragNumbers {
        let mut dx = f64::from(f32::from(self.now.x - self.start.x)) * self.per_point.0;
        let mut dy = f64::from(f32::from(self.now.y - self.start.y)) * self.per_point.1;
        match self.drag.axis {
            Some(Axis::X) => dy = 0.0,
            Some(Axis::Y) => dx = 0.0,
            None => {}
        }
        let snap = self.drag.snap.unwrap_or_default();
        DragNumbers {
            start_x: self.start_units.0,
            start_y: self.start_units.1,
            dx: live::snap(dx, f64::from(snap.x.unwrap_or(0.0))),
            dy: live::snap(dy, f64::from(snap.y.unwrap_or(0.0))),
            active: self.ended.is_none(),
        }
    }

    /// What its code hears of it.
    fn result(&self) -> Value {
        let numbers = self.numbers();
        let mut result = json!({
            "startX": numbers.start_x,
            "startY": numbers.start_y,
            "dx": numbers.dx,
            "dy": numbers.dy,
            "x": numbers.start_x + numbers.dx,
            "y": numbers.start_y + numbers.dy,
        });
        add_modifiers(&mut result, self.modifiers);
        result
    }
}

fn add_modifiers(value: &mut Value, modifiers: Modifiers) {
    if let Some(object) = value.as_object_mut() {
        object.insert("shift".into(), modifiers.shift.into());
        object.insert("alt".into(), modifiers.alt.into());
        object.insert("cmd".into(), modifiers.platform.into());
        object.insert("ctrl".into(), modifiers.control.into());
    }
}

impl TypeScriptCard {
    pub(crate) fn new(
        live: Entity<Live>,
        session: Entity<Session>,
        id: InstanceId,
        frame: Option<CardFrame>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tool = session
            .read(cx)
            .project()
            .tool_of(&id)
            .unwrap_or_default()
            .to_string();
        let page = frame.is_none();
        let surface = if page {
            Surface::Page(None)
        } else {
            Surface::Card
        };
        let card = live.update(cx, |live, cx| live.add(id.clone(), tool, surface, cx));
        let focus = cx.focus_handle();
        let hears_keys = live.read(cx).info(card).is_some_and(|info| info.keys);
        // Not from what has the focus, such as the composer of the agent while it is typed in.
        if page && hears_keys && window.focused(cx).is_none() {
            window.focus(&focus, cx);
        }
        cx.on_focus_out(&focus, window, |view, _, _, cx| view.let_go_of_keys(cx))
            .detach();
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Deleted(id) if *id == view.id => view.edit.finish(&view.session, cx),
            // Its code reads the records of its children too.
            ProjectEvent::Created(id) | ProjectEvent::Changed(id) | ProjectEvent::Deleted(id)
                if *id == view.id || id.parent().as_ref() == Some(&view.id) =>
            {
                let (card, id) = (view.card, id.clone());
                view.live
                    .update(cx, |live, cx| live.record_changed(card, &id, cx));
                // Its live values read the record, also when the tree stays the same.
                cx.notify();
            }
            _ => {}
        })
        .detach();
        cx.observe(&live, |_, _, cx| cx.notify()).detach();
        cx.on_release(|view, cx| {
            view.edit.finish(&view.session, cx);
            // A card that is gone hears no focus out.
            view.let_go_of_keys(cx);
            view.live.update(cx, |live, _| live.remove(view.card));
        })
        .detach();
        Self {
            live,
            session,
            id,
            card,
            frame,
            edit: ControlEdit::default(),
            spaces: Rc::default(),
            held: None,
            hovered: None,
            focus,
            keys_down: BTreeSet::new(),
        }
    }

    /// A key went down or up while the card has the keys, not a `repeat`: of a key held
    /// here, or of one that went down while something else had the keys, such as the computer
    /// keys. A key that went down here comes up here, whatever is held by then. A key with
    /// cmd, ctrl, alt or fn only goes down, as macOS sends no up while cmd is held; one the
    /// window binds, such as cmd-z, is the window's and never comes here.
    fn key(&mut self, keystroke: &Keystroke, down: bool, repeat: bool, cx: &mut Context<Self>) {
        let modifiers = keystroke.modifiers;
        let chord = modifiers.platform || modifiers.control || modifiers.alt || modifiers.function;
        let key = keystroke.key.clone();
        // A key that went down before the card had the keys has no up here.
        let changed = match (down, chord) {
            (true, true) => !repeat,
            (true, false) => !repeat && self.keys_down.insert(key.clone()),
            (false, _) => self.keys_down.remove(&key),
        };
        if changed {
            let card = self.card;
            self.live
                .update(cx, |live, cx| live.key(card, &key, down, modifiers, cx));
        }
        cx.stop_propagation();
    }

    /// Every key the card holds goes up.
    fn let_go_of_keys(&mut self, cx: &mut App) {
        let card = self.card;
        for key in std::mem::take(&mut self.keys_down) {
            self.live.update(cx, |live, cx| {
                live.key(card, &key, false, Modifiers::default(), cx)
            });
        }
    }

    /// Moves a live control.
    fn control(&mut self, name: &str, value: f32, cx: &mut Context<Self>) {
        let id = self.id.clone();
        let controlled =
            (self.live).update(cx, |live, cx| live.control(&id, name, Some(value), None, cx));
        if let Err(error) = controlled {
            eprintln!("error: {id} {error}");
        }
    }

    /// Runs a handler of the tree of `version` with what it hears. The number of the event.
    fn event(&mut self, version: u64, handler: usize, payload: Value, cx: &mut App) -> u64 {
        let card = self.card;
        (self.live).update(cx, |live, cx| live.event(card, version, handler, payload, cx))
    }

    /// The record with each field it leaves out at its default, and its tool.
    fn record(&self, cx: &App) -> (Value, Option<ToolInfo>) {
        let info = self.live.read(cx).info(self.card);
        let state = state_of(self.session.read(cx).project(), &self.id).unwrap_or(Value::Null);
        let state = match &info {
            Some(info) => info.with_defaults(state),
            None => state,
        };
        (state, info)
    }

    /// A press on a space, on a shape that hears the pointer or on the space itself.
    fn press(
        &mut self,
        frame: &SpaceFrame,
        target: Option<usize>,
        bounds: Bounds<Pixels>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let (on_press, drag) = match target {
            Some(index) => match frame.pointed.get(index).and_then(Option::as_ref) {
                Some(pointer) => (pointer.on_press, pointer.drag.clone()),
                None => return,
            },
            None => (frame.on_press, frame.drag.clone()),
        };
        let units = frame.units_at(bounds, event.position);
        if let Some(handler) = on_press {
            let button = match event.button {
                MouseButton::Right => "right",
                _ => "left",
            };
            let mut press = json!({
                "x": units.0,
                "y": units.1,
                "button": button,
                "clicks": event.click_count,
            });
            add_modifiers(&mut press, event.modifiers);
            self.event(frame.version, handler, press, cx);
        }
        let Some(drag) = drag.filter(|_| event.button == MouseButton::Left) else {
            return;
        };
        let (state, info) = self.record(cx);
        let turns = [&drag.x, &drag.y];
        if event.click_count == 2 {
            for turns in turns.into_iter().flatten() {
                if let Some(default) = turns.default {
                    self.turn(turns, ValueChange::Set(default), cx);
                }
            }
            return;
        }
        let played = self.live.read(cx).played_of(&self.id).cloned();
        let sources = Sources {
            tick: 0.0,
            watches: &BTreeMap::new(),
            state: &state,
            info: info.as_ref(),
            played: played.as_ref(),
            drag: None,
            spaces: &HashMap::new(),
        };
        let from = turns.map(|turns| {
            let turns = turns.as_ref()?;
            match (&turns.field, &turns.control) {
                (Some(field), _) => sources.field(field),
                (None, Some(control)) => sources.control(control),
                (None, None) => None,
            }
        });
        let (across, down) = frame.view.scale();
        let held = Held {
            space: frame.key.clone(),
            version: frame.version,
            drag,
            start: event.position,
            now: event.position,
            start_units: units,
            per_point: (1.0 / across, 1.0 / down),
            bounds,
            from,
            modifiers: event.modifiers,
            moved: false,
            ended: None,
        };
        // What turns by where the pointer is turns at the press already, as on a pad.
        if [&held.drag.x, &held.drag.y].into_iter().flatten().any(|turns| turns.absolute) {
            self.turn_held(&held, cx);
        }
        self.held = Some(held);
        cx.notify();
    }

    /// The pointer moved while it holds a drag of the space `space`.
    fn drag_to(&mut self, space: &str, position: Point<Pixels>, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(held) = self.held.as_mut().filter(|held| held.space == space && held.ended.is_none()) else {
            return;
        };
        if held.now == position && held.modifiers == modifiers {
            return;
        }
        held.now = position;
        held.modifiers = modifiers;
        held.moved = true;
        let held = held.clone();
        self.turn_held(&held, cx);
        if let Some(handler) = held.drag.on_move {
            self.event(held.version, handler, held.result(), cx);
        }
        cx.notify();
    }

    /// The pointer let go of the drag it held.
    fn drag_ends(&mut self, cx: &mut Context<Self>) {
        let Some(held) = self.held.clone().filter(|held| held.ended.is_none()) else {
            return;
        };
        if !held.moved {
            self.held = None;
            return;
        }
        let fields = [&held.drag.x, &held.drag.y].into_iter().flatten().any(|turns| turns.field.is_some());
        if fields {
            self.edit.apply_json(&self.session, &self.id, "", ValueChange::<f32>::DragEnd, |_, _| {}, cx);
        }
        self.held = match held.drag.on_end {
            Some(handler) => {
                let number = self.event(held.version, handler, held.result(), cx);
                Some(Held { ended: Some((number, Instant::now())), ..held })
            }
            None => None,
        };
        cx.notify();
    }

    /// Turns what the drag turns to where the pointer is.
    fn turn_held(&mut self, held: &Held, cx: &mut Context<Self>) {
        let info = self.live.read(cx).info(self.card);
        let axes = [(&held.drag.x, 0), (&held.drag.y, 1)];
        for (turns, axis) in axes {
            let Some(turns) = turns else {
                continue;
            };
            let Some(range) = range_of(info.as_ref(), turns) else {
                continue;
            };
            let position = if turns.absolute {
                let bounds = held.bounds;
                let fraction = match axis {
                    0 => (held.now.x - bounds.origin.x) / bounds.size.width,
                    _ => 1.0 - (held.now.y - bounds.origin.y) / bounds.size.height,
                };
                fraction.clamp(0.0, 1.0)
            } else {
                let Some(from) = held.from[axis] else {
                    continue;
                };
                let moved = match axis {
                    0 => f32::from(held.now.x - held.start.x),
                    _ => f32::from(held.start.y - held.now.y),
                };
                let fine = if held.modifiers.shift { FINE } else { 1.0 };
                let travel = turns.travel.unwrap_or(TRAVEL);
                (range.position(from) + moved * fine / travel).clamp(0.0, 1.0)
            };
            self.turn(turns, ValueChange::Drag(range.value(position)), cx);
        }
    }

    fn turn(&mut self, turns: &tree::Turns, change: ValueChange<f32>, cx: &mut Context<Self>) {
        match (&turns.field, &turns.control) {
            (Some(field), _) => {
                let label = (turns.label.clone()).unwrap_or_else(|| format!("Change {}", field.replace('_', " ")));
                let field = field.clone();
                self.edit.apply_json(
                    &self.session,
                    &self.id,
                    &label,
                    change,
                    |state, value| tree::set_field(state, &field, tree::decimal(value)),
                    cx,
                );
            }
            (None, Some(control)) => {
                if let ValueChange::Drag(value) | ValueChange::Set(value) = change {
                    self.control(&control.clone(), value, cx);
                }
            }
            (None, None) => {}
        }
    }

    /// A wheel or a pinch scrolled or zoomed a space.
    fn moved_space(&mut self, key: &str, view: SpaceView, cx: &mut Context<Self>) {
        self.spaces.borrow_mut().insert(key.to_string(), view);
        cx.notify();
    }

    fn hover(&mut self, hovered: Option<(String, String)>, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
}

/// How what a drag turns spreads over its travel: as it says, or as the field or the control.
fn range_of(info: Option<&ToolInfo>, turns: &tree::Turns) -> Option<ValueRange> {
    if let (Some(min), Some(max)) = (turns.min, turns.max) {
        return Some(crate::tools::knob_range(min, max, turns.unit));
    }
    match (&turns.field, &turns.control) {
        (Some(field), _) => live::field_range(info, field),
        (None, Some(control)) => live::control_range(info, control),
        (None, None) => None,
    }
}

/// What a shape of a space does with the pointer.
#[derive(Clone)]
struct Pointer {
    key: String,
    on_press: Option<usize>,
    drag: Option<Drag>,
    cursor: Option<Cursor>,
}

/// A shape with its numbers worked out, in the units of its space.
struct Placed {
    /// Its place among the shapes of the space, which its pointer is found by.
    index: usize,
    geometry: Geometry,
    color: Hsla,
}

enum Geometry {
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f32,
        border: Option<(Hsla, f32)>,
    },
    Circle {
        x: f64,
        y: f64,
        radius: f32,
    },
    Line {
        from: (f64, f64),
        to: (f64, f64),
        width: f32,
    },
    Arc {
        x: f64,
        y: f64,
        radius: f32,
        turns: (f32, f32),
        width: f32,
    },
    Text {
        x: f64,
        y: f64,
        text: SharedString,
        size: f32,
        align: Align,
    },
}

/// Everything a space paints and hears the pointer with, for one draw.
struct SpaceFrame {
    key: String,
    version: u64,
    view: SpaceView,
    placed: Vec<Placed>,
    /// What each shape does with the pointer, by its place.
    pointed: Vec<Option<Pointer>>,
    background: Option<Hsla>,
    radius: f32,
    on_press: Option<usize>,
    drag: Option<Drag>,
    cursor: Option<Cursor>,
    extent: tree::Ranges,
    zoom: tree::Ranges,
}

impl SpaceFrame {
    /// The units at a point of the window.
    fn units_at(&self, bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f64, f64) {
        let (across, down) = self.view.scale();
        (
            self.view.left + f64::from(f32::from(position.x - bounds.origin.x)) / across,
            self.view.top + f64::from(f32::from(position.y - bounds.origin.y)) / down,
        )
    }

    fn point_at(&self, bounds: Bounds<Pixels>, (x, y): (f64, f64)) -> Point<Pixels> {
        let (across, down) = self.view.scale();
        bounds.origin + point(px(((x - self.view.left) * across) as f32), px(((y - self.view.top) * down) as f32))
    }

    /// The view after a wheel: it scrolls over the extent, and zooms with cmd.
    fn scrolled(&self, event: &ScrollWheelEvent, bounds: Bounds<Pixels>) -> SpaceView {
        let delta = event.delta.pixel_delta(px(32.));
        if event.modifiers.secondary() {
            let factor = (f64::from(f32::from(delta.y)) * 0.01).exp();
            return self.zoomed(factor, bounds, event.position);
        }
        let (across, down) = self.view.scale();
        let (mut dx, mut dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        if event.modifiers.shift && dx == 0.0 {
            (dx, dy) = (dy, 0.0);
        }
        let view = SpaceView {
            left: self.view.left - dx / across,
            top: self.view.top - dy / down,
            ..self.view
        };
        clamped(view, &self.extent)
    }

    /// The view zoomed by `factor` about the pointer, along each axis that zooms.
    fn zoomed(&self, factor: f64, bounds: Bounds<Pixels>, position: Point<Pixels>) -> SpaceView {
        let (at_x, at_y) = self.units_at(bounds, position);
        let mut view = self.view;
        if let Some([low, high]) = self.zoom.x {
            view.zoom_x = (view.zoom_x * factor).clamp(f64::from(low), f64::from(high));
            view.left = at_x - (at_x - view.left) * self.view.zoom_x / view.zoom_x;
        }
        if let Some([low, high]) = self.zoom.y {
            view.zoom_y = (view.zoom_y * factor).clamp(f64::from(low), f64::from(high));
            view.top = at_y - (at_y - view.top) * self.view.zoom_y / view.zoom_y;
        }
        clamped(view, &self.extent)
    }

    fn scrolls(&self) -> bool {
        self.extent.x.is_some() || self.extent.y.is_some() || self.zoom.x.is_some() || self.zoom.y.is_some()
    }
}

/// `view` scrolled inside `extent`: an axis without one shows from 0.
fn clamped(mut view: SpaceView, extent: &tree::Ranges) -> SpaceView {
    let (across, down) = view.scale();
    let hold = |edge: f64, range: Option<[f32; 2]>, shown: f64| match range {
        Some([low, high]) => edge.min(f64::from(high) - shown).max(f64::from(low)),
        None => 0.0,
    };
    view.left = hold(view.left, extent.x, view.width / across);
    view.top = hold(view.top, extent.y, view.height / down);
    view
}

/// A shape placed in points, as it paints and as the pointer finds it.
struct Painted {
    index: usize,
    shape: PaintedShape,
    color: Hsla,
    hitbox: Option<(Hitbox, Cursor)>,
}

enum PaintedShape {
    Rect(Bounds<Pixels>, f32, Option<(Hsla, f32)>),
    Circle(Point<Pixels>, f32),
    Line(Point<Pixels>, Point<Pixels>, f32),
    Arc(Point<Pixels>, f32, (f32, f32), f32),
    Text(Point<Pixels>, SharedString, f32, Align),
}

impl PaintedShape {
    fn bounds(&self) -> Bounds<Pixels> {
        let square = |centre: Point<Pixels>, radius: f32| {
            Bounds::new(centre - point(px(radius), px(radius)), size(px(radius * 2.), px(radius * 2.)))
        };
        match self {
            Self::Rect(bounds, ..) => *bounds,
            Self::Circle(centre, radius) => square(*centre, *radius),
            Self::Arc(centre, radius, _, width) => square(*centre, radius + width / 2.),
            Self::Line(from, to, width) => {
                let (left, top) = (from.x.min(to.x), from.y.min(to.y));
                let (right, bottom) = (from.x.max(to.x), from.y.max(to.y));
                let pad = px(width / 2. + 2.);
                Bounds::from_corners(point(left - pad, top - pad), point(right + pad, bottom + pad))
            }
            Self::Text(origin, text, font, align) => {
                let width = px(text.len() as f32 * font * 0.6);
                let left = match align {
                    Align::Left => origin.x,
                    Align::Center => origin.x - width / 2.,
                    Align::Right => origin.x - width,
                };
                Bounds::new(point(left, origin.y), size(width, px(font * 1.3)))
            }
        }
    }

    fn contains(&self, position: Point<Pixels>) -> bool {
        match self {
            Self::Circle(centre, radius) => {
                let (dx, dy) = (f32::from(position.x - centre.x), f32::from(position.y - centre.y));
                dx * dx + dy * dy <= radius * radius
            }
            _ => self.bounds().contains(&position),
        }
    }
}

/// What one draw of the card reads.
struct Drawing<'a, 'b> {
    /// The tool, as it last loaded.
    info: Option<ToolInfo>,
    /// The record, with each field it leaves out at its default.
    state: Value,
    watches: BTreeMap<String, f32>,
    played: Option<BTreeMap<String, f32>>,
    tick: f64,
    spaces: HashMap<String, SpaceView>,
    /// The card's own, which the paint of a space writes its size in.
    laid_out: Rc<RefCell<HashMap<String, SpaceView>>>,
    drag: Option<(String, DragNumbers)>,
    hovered: Option<(String, String)>,
    /// The version of the tree, which its handlers name: the one on screen, not a newer one.
    version: u64,
    cx: &'a mut Context<'b, TypeScriptCard>,
}

impl Drawing<'_, '_> {
    fn sources(&self) -> Sources<'_> {
        Sources {
            tick: self.tick,
            watches: &self.watches,
            state: &self.state,
            info: self.info.as_ref(),
            played: self.played.as_ref(),
            drag: (self.drag.as_ref()).map(|(id, numbers)| (id.as_str(), *numbers)),
            spaces: &self.spaces,
        }
    }

    /// Draws `node`. `index` tells siblings apart, so every element that needs an id gets its
    /// own.
    fn draw(&mut self, node: &Node, index: &[usize]) -> AnyElement {
        match node {
            Node::Text { text } => SharedString::from(text.clone()).into_any_element(),
            Node::Sample { path, label } => self.sample(path, label.as_deref()),
            Node::Space(space) => self.space(space, index),
            Node::Div {
                key,
                style,
                on_click,
                children,
            } => {
                let children: Vec<AnyElement> = (children.iter().enumerate())
                    .map(|(position, child)| self.draw(child, &[index, &[position]].concat()))
                    .collect();
                let element = style.apply(div()).children(children);
                let Some(handler) = *on_click else {
                    return element.into_any_element();
                };
                let version = self.version;
                element
                    .id(ElementId::Name(element_name("div", key.as_deref(), index).into()))
                    .cursor_pointer()
                    .on_click((self.cx).listener(move |view, event: &gpui::ClickEvent, _, cx| {
                        let mut press = json!({
                            "x": 0, "y": 0, "button": "left",
                            "clicks": event.click_count(),
                        });
                        add_modifiers(&mut press, event.modifiers());
                        view.event(version, handler, press, cx);
                    }))
                    .into_any_element()
            }
        }
    }

    fn sample(&mut self, path: &str, label: Option<&str>) -> AnyElement {
        let file = self.state.get(path).and_then(serde_json::Value::as_str);
        let shown = match file {
            Some(file) if !file.is_empty() => file.to_string(),
            _ => "No sound".to_string(),
        };
        let theme = self.cx.theme();
        let (quiet, button) = (theme.gray_700, theme.gray_300);
        let path = path.to_string();
        let choose = div()
            .id(ElementId::Name(format!("{path}-choose").into()))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(button)
            .cursor_pointer()
            .child("Choose…")
            .on_click(self.cx.listener(move |view, _, _, cx| {
                let path = path.clone();
                let session = view.session.clone();
                choose_file(&session, "Choose a sound", cx, move |view, file, cx| {
                    let session = view.session.clone();
                    import_file(&session, file, cx, move |view, imported, cx| {
                        let name = imported.asset.to_string();
                        edit_record(
                            &view.session,
                            &view.id,
                            "Choose a sound",
                            |state| {
                                tree::set_field(state, &path, name.into());
                            },
                            cx,
                        );
                    });
                });
            }));
        let mut row = div().flex().flex_row().items_center().gap(px(8.));
        if let Some(label) = label {
            row = row.child(label.to_string());
        }
        row.child(div().text_color(quiet).child(shown))
            .child(choose)
            .into_any_element()
    }

    /// The shapes of a space with their numbers worked out, each in the colour it shows.
    fn place(&self, space: &SpaceNode, key: &str) -> (Vec<Placed>, Vec<Option<Pointer>>) {
        let sources = self.sources();
        let number = |number: &Option<tree::Num>| number.as_ref().map_or(0.0, |number| sources.number(number));
        let mut placed = Vec::with_capacity(space.children.len());
        let mut pointed = Vec::with_capacity(space.children.len());
        for (index, shape) in space.children.iter().enumerate() {
            let shape_key = shape.key.clone().unwrap_or_else(|| index.to_string());
            let hovered = self.hovered.as_ref().is_some_and(|(space, hovered)| space == key && *hovered == shape_key);
            pointed.push(shape.is_pointed().then(|| Pointer {
                key: shape_key,
                on_press: shape.on_press,
                drag: shape.drag.clone(),
                cursor: shape.cursor,
            }));
            let opacity = shape.opacity.as_ref().map_or(1.0, |opacity| sources.number(opacity).clamp(0.0, 1.0)) as f32;
            if opacity <= 0.0 {
                continue;
            }
            let shown = if hovered { shape.hover.or(shape.color) } else { shape.color };
            let mut color = shown.map_or(gpui::white(), |color| color.0);
            color.a *= opacity;
            let reach = |reach: &Option<Reach>| match reach {
                Some(Reach::Point([x, y])) => (sources.number(x), sources.number(y)),
                Some(Reach::Turn(turn)) => (sources.number(turn), 0.0),
                None => (0.0, 0.0),
            };
            let geometry = match shape.kind {
                ShapeKind::Rect => {
                    let border_width = number(&shape.border_width) as f32;
                    Geometry::Rect {
                        x: number(&shape.x),
                        y: number(&shape.y),
                        width: number(&shape.width),
                        height: number(&shape.height),
                        radius: number(&shape.radius) as f32,
                        border: (shape.border)
                            .filter(|_| border_width > 0.0)
                            .map(|border| (border.0, border_width)),
                    }
                }
                ShapeKind::Circle => Geometry::Circle {
                    x: number(&shape.x),
                    y: number(&shape.y),
                    radius: number(&shape.radius) as f32,
                },
                ShapeKind::Line => Geometry::Line {
                    from: reach(&shape.from),
                    to: reach(&shape.to),
                    width: number(&shape.width).max(0.0) as f32,
                },
                ShapeKind::Arc => Geometry::Arc {
                    x: number(&shape.x),
                    y: number(&shape.y),
                    radius: number(&shape.radius) as f32,
                    turns: (reach(&shape.from).0 as f32, reach(&shape.to).0 as f32),
                    width: number(&shape.width).max(0.0) as f32,
                },
                ShapeKind::Text => Geometry::Text {
                    x: number(&shape.x),
                    y: number(&shape.y),
                    text: match &shape.text {
                        Some(Words::Fixed(text)) => text.clone().into(),
                        Some(Words::Live(live)) => readout(sources.number(live) as f32, shape.unit).into(),
                        None => SharedString::default(),
                    },
                    size: shape.size.unwrap_or(12.0),
                    align: shape.align.unwrap_or(Align::Left),
                },
            };
            placed.push(Placed { index, geometry, color });
        }
        (placed, pointed)
    }

    fn space(&mut self, space: &SpaceNode, index: &[usize]) -> AnyElement {
        let key = element_name("space", space.key.as_deref(), index);
        let (unit_x, unit_y) = space.unit();
        let mut view = self.spaces.get(&key).copied().unwrap_or(SpaceView {
            left: space.extent.and_then(|extent| extent.x).map_or(0.0, |[low, _]| f64::from(low)),
            top: space.extent.and_then(|extent| extent.y).map_or(0.0, |[low, _]| f64::from(low)),
            ..SpaceView::default()
        });
        (view.unit_x, view.unit_y) = (unit_x, unit_y);
        let view = clamped(view, &space.extent.unwrap_or_default());
        self.spaces.insert(key.clone(), view);
        let (placed, pointed) = self.place(space, &key);
        let frame = Rc::new(SpaceFrame {
            key,
            version: self.version,
            view,
            placed,
            pointed,
            background: space.background.map(|color| color.0),
            radius: space.radius.unwrap_or(0.0),
            on_press: space.on_press,
            drag: space.drag.clone(),
            cursor: space.cursor,
            extent: space.extent.unwrap_or_default(),
            zoom: space.zoom.unwrap_or_default(),
        });
        let card = self.cx.entity().downgrade();
        let spaces = self.laid_out.clone();
        let element = canvas(
            {
                let frame = frame.clone();
                move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| prepaint(&frame, bounds, window)
            },
            move |bounds: Bounds<Pixels>, (hitbox, painted): (Hitbox, Rc<Vec<Painted>>), window: &mut Window, cx: &mut App| {
                // The size it was laid out at, for live values of what shows and the next scroll.
                if let Some(view) = spaces.borrow_mut().get_mut(&frame.key) {
                    view.width = f64::from(f32::from(bounds.size.width));
                    view.height = f64::from(f32::from(bounds.size.height));
                }
                paint(&frame, bounds, &hitbox, &painted, window, cx);
                listen(&frame, &card, bounds, hitbox, painted, window);
            },
        );
        let element = match space.width {
            Some(width) => element.w(px(width)),
            None => element.w_full(),
        };
        let element = match space.height {
            Some(height) => element.h(px(height)),
            None => element.h_full(),
        };
        let element = if space.grow { element.flex_grow(1.).min_h_0() } else { element.flex_none() };
        element.into_any_element()
    }
}

/// An id for an element, from its key, else from where it is in the tree.
fn element_name(kind: &str, key: Option<&str>, index: &[usize]) -> String {
    match key {
        Some(key) => key.to_string(),
        None => {
            let path: Vec<String> = index.iter().map(usize::to_string).collect();
            format!("{kind}-{}", path.join("-"))
        }
    }
}

/// A number as a readout of `unit`.
fn readout(value: f32, unit: Option<Unit>) -> String {
    match unit {
        Some(Unit::Hz) => hertz_readout(value),
        Some(Unit::Ms) => milliseconds_readout(value),
        Some(Unit::Db) => decibels_readout(value),
        Some(Unit::Percent) => percent_readout(value),
        None => short(value),
    }
}

/// The shapes of a frame in points, those that show, with a hitbox for each that shows a cursor.
fn prepaint(frame: &SpaceFrame, bounds: Bounds<Pixels>, window: &mut Window) -> (Hitbox, Rc<Vec<Painted>>) {
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
    let mut frame_view = frame.view;
    frame_view.width = f64::from(f32::from(bounds.size.width));
    frame_view.height = f64::from(f32::from(bounds.size.height));
    let (across, down) = frame_view.scale();
    let at = |(x, y): (f64, f64)| frame.point_at(bounds, (x, y));
    let mut painted = Vec::new();
    for placed in &frame.placed {
        let shape = match &placed.geometry {
            Geometry::Rect { x, y, width, height, radius, border } => {
                let origin = at((*x, *y));
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Rect(Bounds::new(origin, size), *radius, *border)
            }
            Geometry::Circle { x, y, radius } => PaintedShape::Circle(at((*x, *y)), *radius),
            Geometry::Line { from, to, width } => PaintedShape::Line(at(*from), at(*to), *width),
            Geometry::Arc { x, y, radius, turns, width } => PaintedShape::Arc(at((*x, *y)), *radius, *turns, *width),
            Geometry::Text { x, y, text, size, align } => PaintedShape::Text(at((*x, *y)), text.clone(), *size, *align),
        };
        // What does not show is not painted and not found.
        if !shape.bounds().intersects(&bounds) {
            continue;
        }
        let cursor = (frame.pointed.get(placed.index))
            .and_then(Option::as_ref)
            .and_then(|pointer| pointer.cursor);
        let hitbox = cursor.map(|cursor| (window.insert_hitbox(shape.bounds(), HitboxBehavior::Normal), cursor));
        painted.push(Painted { index: placed.index, shape, color: placed.color, hitbox });
    }
    (hitbox, Rc::new(painted))
}

fn paint(
    frame: &SpaceFrame,
    bounds: Bounds<Pixels>,
    hitbox: &Hitbox,
    painted: &[Painted],
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(background) = frame.background {
        window.paint_quad(fill(bounds, background).corner_radii(px(frame.radius)));
    }
    if let Some(cursor) = frame.cursor {
        window.set_cursor_style(cursor.style(), hitbox);
    }
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        for each in painted {
            paint_shape(&each.shape, each.color, window, cx);
            if let Some((hitbox, cursor)) = &each.hitbox {
                window.set_cursor_style(cursor.style(), hitbox);
            }
        }
    });
}

fn paint_shape(shape: &PaintedShape, color: Hsla, window: &mut Window, cx: &mut App) {
    match shape {
        PaintedShape::Rect(bounds, radius, border) => {
            window.paint_quad(fill(*bounds, color).corner_radii(px(*radius)));
            if let Some((border, width)) = border {
                let quad = outline(*bounds, *border, gpui::BorderStyle::Solid);
                window.paint_quad(quad.corner_radii(px(*radius)).border_widths(px(*width)));
            }
        }
        PaintedShape::Circle(centre, radius) => {
            let bounds = Bounds::new(*centre - point(px(*radius), px(*radius)), size(px(radius * 2.), px(radius * 2.)));
            window.paint_quad(fill(bounds, color).corner_radii(px(*radius)));
        }
        PaintedShape::Line(from, to, width) => {
            let mut line = gpui::PathBuilder::stroke(px(*width));
            line.move_to(*from);
            line.line_to(*to);
            if let Ok(path) = line.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Arc(centre, radius, (from, to), width) => {
            if (to - from).abs() < 0.0001 {
                return;
            }
            let on_circle = |turns: f32| {
                let radians = turns * std::f32::consts::TAU;
                *centre + point(px(radius * radians.sin()), px(-radius * radians.cos()))
            };
            let mut path = gpui::PathBuilder::stroke(px(*width));
            path.move_to(on_circle(*from));
            // Clockwise on the screen is the positive way round of SVG, with y down.
            let radii = point(px(*radius), px(*radius));
            path.arc_to(radii, px(0.), (to - from).abs() > 0.5, to > from, on_circle(*to));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Text(origin, text, font_size, align) => {
            let run = TextRun {
                len: text.len(),
                font: typography::ui_font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(text.clone(), px(*font_size), &[run], None);
            let left = match align {
                Align::Left => origin.x,
                Align::Center => origin.x - shaped.width / 2.,
                Align::Right => origin.x - shaped.width,
            };
            let line_height = px(font_size * 1.3);
            if let Err(error) = shaped.paint(point(left, origin.y), line_height, TextAlign::Left, None, window, cx) {
                eprintln!("error: a label of a card: {error}");
            }
        }
    }
}

/// The topmost shape at `position` that hears the pointer: its place among the shapes.
fn found(frame: &SpaceFrame, painted: &[Painted], position: Point<Pixels>) -> Option<usize> {
    (painted.iter().rev())
        .filter(|each| frame.pointed.get(each.index).is_some_and(Option::is_some))
        .find(|each| each.shape.contains(position))
        .map(|each| each.index)
}

/// Hears the pointer on a space: presses, drags, hovers, the wheel and pinches.
fn listen(
    frame: &Rc<SpaceFrame>,
    card: &WeakEntity<TypeScriptCard>,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    painted: Rc<Vec<Painted>>,
    window: &mut Window,
) {
    window.on_mouse_event({
        let (frame, card, painted, hitbox) = (frame.clone(), card.clone(), painted.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let target = found(&frame, &painted, event.position);
            if target.is_none() && frame.on_press.is_none() && frame.drag.is_none() {
                return;
            }
            card.update(cx, |card, cx| {
                if card.live.read(cx).info(card.card).is_some_and(|info| info.keys) {
                    window.focus(&card.focus, cx);
                }
                card.press(&frame, target, bounds, event, cx);
            })
            .ok();
            cx.stop_propagation();
        }
    });
    window.on_mouse_event({
        let (frame, card, painted, hitbox) = (frame.clone(), card.clone(), painted.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window);
            card.update(cx, |card, cx| {
                if event.dragging() {
                    card.drag_to(&frame.key, event.position, event.modifiers, cx);
                    return;
                }
                let hovered = found(&frame, &painted, event.position)
                    .filter(|_| over)
                    .and_then(|index| frame.pointed.get(index).cloned().flatten())
                    .map(|pointer| (frame.key.clone(), pointer.key));
                let hovered_here = card.hovered.as_ref().is_some_and(|(space, _)| *space == frame.key);
                if hovered.is_some() || hovered_here {
                    card.hover(hovered, cx);
                }
            })
            .ok();
        }
    });
    window.on_mouse_event({
        let card = card.clone();
        move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                card.update(cx, |card, cx| card.drag_ends(cx)).ok();
            }
        }
    });
    if !frame.scrolls() {
        return;
    }
    window.on_mouse_event({
        let (frame, card, hitbox) = (frame.clone(), card.clone(), hitbox.clone());
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let view = frame_now(&frame, &card, bounds, cx).scrolled(event, bounds);
            card.update(cx, |card, cx| card.moved_space(&frame.key, view, cx)).ok();
            cx.stop_propagation();
        }
    });
    let (frame, card) = (frame.clone(), card.clone());
    window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
            return;
        }
        let factor = f64::from(1.0 + event.delta);
        let view = frame_now(&frame, &card, bounds, cx).zoomed(factor, bounds, event.position);
        card.update(cx, |card, cx| card.moved_space(&frame.key, view, cx)).ok();
        cx.stop_propagation();
    });
}

/// The frame with its view as the card has it now: several wheel events may come before the
/// next draw.
fn frame_now(frame: &SpaceFrame, card: &WeakEntity<TypeScriptCard>, bounds: Bounds<Pixels>, cx: &App) -> SpaceFrame {
    let mut view = (card.upgrade())
        .and_then(|card| card.read(cx).spaces.borrow().get(&frame.key).copied())
        .unwrap_or(frame.view);
    view.width = f64::from(f32::from(bounds.size.width));
    view.height = f64::from(f32::from(bounds.size.height));
    SpaceFrame {
        key: frame.key.clone(),
        version: frame.version,
        view,
        placed: Vec::new(),
        pointed: Vec::new(),
        background: None,
        radius: 0.0,
        on_press: None,
        drag: None,
        cursor: None,
        extent: frame.extent,
        zoom: frame.zoom,
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = state_of(self.session.read(cx).project(), &self.id) else {
            return div().into_any_element();
        };
        // An ended drag lets go once the tree after its end arrived.
        let answered = self.live.read(cx).events_answered(self.card);
        if let Some(Held { ended: Some((number, at)), .. }) = &self.held
            && (answered >= *number || at.elapsed() > ENDED_DRAG)
        {
            self.held = None;
        }
        let live = self.live.read(cx);
        let info = live.info(self.card);
        let hears_keys = info.as_ref().is_some_and(|info| info.keys);
        let state = match &info {
            Some(info) => info.with_defaults(state),
            None => state,
        };
        let tree = live.tree(self.card).cloned();
        let uses = live.uses(self.card);
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let played = live.played_of(&self.id).cloned();
        let playhead = *self.session.read(cx).playhead().read(cx);
        if uses.tick && playhead.playing {
            window.request_animation_frame();
        }
        let body = match tree {
            Some(Ok((version, node))) => {
                let drag = (self.held.as_ref())
                    .and_then(|held| Some((held.drag.id.clone()?, held.numbers())));
                let mut drawing = Drawing {
                    info,
                    state,
                    watches,
                    played,
                    tick: playhead.tick.0 as f64,
                    spaces: self.spaces.borrow().clone(),
                    laid_out: self.spaces.clone(),
                    drag,
                    hovered: self.hovered.clone(),
                    version,
                    cx,
                };
                let body = drawing.draw(&node, &[]);
                let spaces = drawing.spaces;
                // Scroll and zoom as drawn, so a wheel scrolls from there; sizes stay the paint's.
                let mut kept = self.spaces.borrow_mut();
                for (key, view) in spaces {
                    let size = kept.get(&key).map(|old| (old.width, old.height));
                    let entry = kept.entry(key).or_insert(view);
                    *entry = view;
                    if let Some((width, height)) = size {
                        (entry.width, entry.height) = (width, height);
                    }
                }
                body
            }
            Some(Err(error)) => div()
                .text_color(cx.theme().red)
                .child(format!("ui: {error}"))
                .into_any_element(),
            // Until its first tree arrives, a moment after the card opens.
            None => div().into_any_element(),
        };
        let theme = cx.theme();
        let (text, background) = (theme.gray_950, theme.gray_100);
        // A click in what takes the keys gives them: the body of a card, a whole page.
        let takes_keys = |element: Div| {
            element.when(hears_keys, |element| {
                element
                    .track_focus(&self.focus)
                    .key_context(crate::KEY_CONTEXT)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        view.key(&event.keystroke, true, event.is_held, cx)
                    }))
                    .on_key_up(cx.listener(|view, event: &KeyUpEvent, _, cx| {
                        view.key(&event.keystroke, false, false, cx)
                    }))
                    // macOS sends no key up while cmd is held.
                    .on_modifiers_changed(cx.listener(
                        |view, event: &ModifiersChangedEvent, _, cx| {
                            if event.modifiers.platform {
                                view.let_go_of_keys(cx);
                            }
                        },
                    ))
            })
        };
        let body = div().text_size(px(12.)).text_color(text).child(body);
        match &self.frame {
            // At least as wide as a plain card, so the title fits.
            Some(frame) => frame
                .card()
                .min_w(px(PLAIN_CARD_WIDTH))
                .child(takes_keys(body))
                .into_any_element(),
            None => {
                // The room inside the padding, which the page draws from.
                let (live, card) = (self.live.clone(), self.card);
                let measure = canvas(
                    move |bounds, _, cx| {
                        let size = PageSize {
                            width: f32::from(bounds.size.width).round() as u32,
                            height: f32::from(bounds.size.height).round() as u32,
                        };
                        cx.defer(move |cx| live.update(cx, |live, cx| live.resize(card, size, cx)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full();
                let room = div().relative().size_full().child(measure).child(body.size_full());
                takes_keys(div().size_full().p(px(24.)).bg(background).child(room))
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(view: SpaceView, extent: tree::Ranges, zoom: tree::Ranges) -> SpaceFrame {
        SpaceFrame {
            key: "roll".into(),
            version: 1,
            view,
            placed: Vec::new(),
            pointed: Vec::new(),
            background: None,
            radius: 0.0,
            on_press: None,
            drag: None,
            cursor: None,
            extent,
            zoom,
        }
    }

    #[test]
    fn a_space_finds_the_units_under_the_pointer_and_zooms_about_them() {
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(400.), px(200.)));
        let view = SpaceView {
            left: 960.0,
            unit_x: 0.1,
            unit_y: 10.0,
            width: 400.0,
            height: 200.0,
            ..SpaceView::default()
        };
        let extent = tree::Ranges { x: Some([0.0, 38_400.0]), y: Some([0.0, 128.0]) };
        let zoom = tree::Ranges { x: Some([0.25, 8.0]), y: None };
        let frame = frame(view, extent, zoom);
        let pointer = point(px(300.), px(150.));
        assert_eq!(frame.units_at(bounds, pointer), (2960.0, 10.0));
        let zoomed = frame.zoomed(2.0, bounds, pointer);
        assert_eq!(zoomed.zoom_x, 2.0);
        assert_eq!(zoomed.zoom_y, 1.0);
        let after = SpaceFrame { view: zoomed, ..frame };
        assert_eq!(after.units_at(bounds, pointer), (2960.0, 10.0));
    }

    #[test]
    fn a_space_scrolls_inside_its_extent_and_not_along_an_axis_without_one() {
        let view = SpaceView { left: 100.0, top: 3.0, width: 400.0, height: 200.0, ..SpaceView::default() };
        let held = clamped(view, &tree::Ranges { x: Some([0.0, 450.0]), y: None });
        assert_eq!((held.left, held.top), (50.0, 0.0));
    }
}
