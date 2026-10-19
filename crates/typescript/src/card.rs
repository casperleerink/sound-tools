//! The card of an instance of a tool of the project, in a rack, or its page, the whole window:
//! the nodes its code made. What moves on it moves here, with no code: a space scrolls and
//! zooms, a drag moves what binds to it and turns what it turns, and live values follow the
//! playhead, the watches and the record. Code hears a press, and a drag once, at its end.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle, Hitbox,
    HitboxBehavior, Hsla, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent,
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

use crate::bun::{EventKind, PageSize};
use crate::live::{self, DragNumbers, Sources, SpaceView};
use crate::tools::{ToolInfo, Unit};
use crate::tree::{
    self, Align, Axis, Cursor, DivProps, Drag, NodeTable, Props, Reach, SampleProps, ShapeKind,
    SpaceProps, TextProps, Words,
};
use crate::window::{Live, Surface, edit_record, state_of};

/// How long an ended drag keeps its values at most, when the answer to its end does not come,
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
    /// The record as last read, with the load of the tools it was read for.
    state: Option<(u64, Rc<Value>)>,
    /// The drag of the pointer, from its press until the answer to its end arrives.
    held: Option<Held>,
    /// The shape the pointer is over: the key of its space and its node.
    hovered: Option<(String, u64)>,
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
    /// The node that hears it: the shape it holds, or the space.
    node: u64,
    /// Whether it holds a shape, whose drag it takes again as the shape changes.
    on_shape: bool,
    drag: Drag,
    /// Where it was pressed and where the pointer is, in the window.
    start: Point<Pixels>,
    now: Point<Pixels>,
    /// The press in the units of its space, and how many units a point is there.
    start_units: (f64, f64),
    /// What a drag that snaps to the grid puts on it: the anchor of the shape it holds.
    anchor: (f64, f64),
    per_point: (f64, f64),
    /// Where the space was at the press, for what turns by where the pointer is.
    bounds: Bounds<Pixels>,
    /// What it turns across and up, as it was at the press.
    from: [Option<f32>; 2],
    modifiers: Modifiers,
    moved: bool,
    /// When it ended, and the number of the event that told its code: its values stay until
    /// the answer to that event arrives.
    ended: Option<(u64, Instant)>,
}

impl Held {
    /// Takes the drag of what it holds as it is now: its code may give it another.
    fn follow(&mut self, frame: &SpaceFrame) {
        let drag = match self.on_shape {
            false => frame.drag.clone(),
            true => (frame.pointed.iter().flatten())
                .find(|pointer| pointer.node == self.node)
                .and_then(|pointer| pointer.drag.clone()),
        };
        if let Some(drag) = drag {
            self.drag = drag;
        }
    }

    fn numbers(&self) -> DragNumbers {
        let mut dx = f64::from(f32::from(self.now.x - self.start.x)) * self.per_point.0;
        let mut dy = f64::from(f32::from(self.now.y - self.start.y)) * self.per_point.1;
        match self.drag.axis {
            Some(Axis::X) => dy = 0.0,
            Some(Axis::Y) => dx = 0.0,
            None => {}
        }
        let snap = self.drag.snap.unwrap_or_default();
        let whole = |moved: f64, step: Option<f32>, anchor: f64| {
            let step = f64::from(step.unwrap_or(0.0));
            match snap.grid {
                true => live::snap(anchor + moved, step) - anchor,
                false => live::snap(moved, step),
            }
        };
        DragNumbers {
            start_x: self.start_units.0,
            start_y: self.start_units.1,
            dx: whole(dx, snap.x, self.anchor.0),
            dy: whole(dy, snap.y, self.anchor.1),
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
            ProjectEvent::Deleted(id) if *id == view.id => {
                view.state = None;
                view.edit.finish(&view.session, cx);
            }
            ProjectEvent::Created(id) if *id == view.id => view.state = None,
            // Its code reads the records of its children too.
            // Its live values read the record.
            ProjectEvent::Changed(id) if *id == view.id => {
                view.state = None;
                cx.notify();
            }
            _ => {}
        })
        .detach();
        cx.observe(&live, |_, _, cx| cx.notify()).detach();
        // Escape lets go of a drag wherever the keys are, as a drag does not take them.
        cx.observe_keystrokes(|view, event, _, cx| {
            if event.keystroke.key == "escape" {
                view.cancel_drag(cx);
            }
        })
        .detach();
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
            state: None,
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
        if down && keystroke.key == "escape" && self.cancel_drag(cx) {
            cx.stop_propagation();
            return;
        }
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
        let controlled = (self.live).update(cx, |live, cx| {
            live.control(&id, name, Some(value), None, cx)
        });
        if let Err(error) = controlled {
            eprintln!("error: {id} {error}");
        }
    }

    /// Runs the handler of `kind` of the node `node` with what it hears. The number of the
    /// event.
    fn event(&mut self, node: u64, kind: EventKind, payload: Value, cx: &mut App) -> u64 {
        let card = self.card;
        (self.live).update(cx, |live, cx| live.event(card, node, kind, payload, cx))
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

    /// The record as `record` gives it, read again only when it or the tools changed: a page
    /// draws every frame while something moves, and a record of many notes is long to read.
    fn state(&mut self, cx: &App) -> Option<Rc<Value>> {
        let generation = self.live.read(cx).generation();
        if let Some((made, state)) = &self.state
            && *made == generation
        {
            return Some(state.clone());
        }
        state_of(self.session.read(cx).project(), &self.id)?;
        let state = Rc::new(self.record(cx).0);
        self.state = Some((generation, state.clone()));
        Some(state)
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
        let units = frame.units_at(bounds, event.position);
        let (on_press, drag, node, on_shape, anchor) = match target {
            Some(index) => match frame.pointed.get(index).and_then(Option::as_ref) {
                Some(pointer) => (
                    pointer.on_press,
                    pointer.drag.clone(),
                    pointer.node,
                    true,
                    pointer.anchor,
                ),
                None => return,
            },
            None => (frame.on_press, frame.drag.clone(), frame.node, false, units),
        };
        if on_press {
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
            self.event(node, EventKind::Press, press, cx);
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
            node,
            on_shape,
            drag,
            start: event.position,
            now: event.position,
            start_units: units,
            anchor,
            per_point: (1.0 / across, 1.0 / down),
            bounds,
            from,
            modifiers: event.modifiers,
            moved: false,
            ended: None,
        };
        // What turns by where the pointer is turns at the press already, as on a pad.
        if [&held.drag.x, &held.drag.y]
            .into_iter()
            .flatten()
            .any(|turns| turns.absolute)
        {
            self.turn_held(&held, cx);
        }
        self.held = Some(held);
        cx.notify();
    }

    /// The pointer moved while it holds a drag of the space of `frame`.
    fn drag_to(
        &mut self,
        frame: &SpaceFrame,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(held) = self
            .held
            .as_mut()
            .filter(|held| held.space == frame.key && held.ended.is_none())
        else {
            return;
        };
        held.follow(frame);
        if held.now == position && held.modifiers == modifiers {
            return;
        }
        held.now = position;
        held.modifiers = modifiers;
        held.moved = true;
        let held = held.clone();
        self.turn_held(&held, cx);
        if held.drag.on_move {
            self.event(held.node, EventKind::Move, held.result(), cx);
        }
        cx.notify();
    }

    /// The pointer let go of the drag it held, over the space of `frame`.
    fn drag_ends(&mut self, frame: &SpaceFrame, cx: &mut Context<Self>) {
        let Some(mut held) = self
            .held
            .clone()
            .filter(|held| held.space == frame.key && held.ended.is_none())
        else {
            return;
        };
        held.follow(frame);
        if !held.moved {
            self.held = None;
            return;
        }
        let fields = [&held.drag.x, &held.drag.y]
            .into_iter()
            .flatten()
            .any(|turns| turns.field.is_some());
        if fields {
            self.edit.apply_json(
                &self.session,
                &self.id,
                "",
                ValueChange::<f32>::DragEnd,
                |_, _| {},
                cx,
            );
        }
        self.held = match held.drag.on_end {
            true => {
                let number = self.event(held.node, EventKind::End, held.result(), cx);
                Some(Held {
                    ended: Some((number, Instant::now())),
                    ..held
                })
            }
            false => None,
        };
        cx.notify();
    }

    /// Escape: the drag the pointer holds lets go of what it moved and turned, and its code
    /// hears nothing.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(held) = self.held.take_if(|held| held.ended.is_none()) else {
            return false;
        };
        let turns = [&held.drag.x, &held.drag.y];
        if turns
            .iter()
            .copied()
            .flatten()
            .any(|turns| turns.field.is_some())
        {
            (self.edit).apply_json(
                &self.session,
                &self.id,
                "",
                ValueChange::<f32>::DragCancel,
                |_, _| {},
                cx,
            );
        }
        for (turns, from) in turns.into_iter().zip(held.from) {
            if let (Some(control), Some(from)) =
                (turns.as_ref().and_then(|turns| turns.control.clone()), from)
            {
                self.control(&control, from, cx);
            }
        }
        cx.notify();
        true
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
                let label = (turns.label.clone())
                    .unwrap_or_else(|| format!("Change {}", field.replace('_', " ")));
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
    /// The spaces it follows move with it, on their axes.
    fn moved_space(
        &mut self,
        key: &str,
        view: SpaceView,
        follow: &tree::Follow,
        cx: &mut Context<Self>,
    ) {
        let mut spaces = self.spaces.borrow_mut();
        spaces.insert(key.to_string(), view);
        if let Some(leader) = follow.x.as_ref().and_then(|key| spaces.get_mut(key)) {
            (leader.left, leader.zoom_x) = (view.left, view.zoom_x);
        }
        if let Some(leader) = follow.y.as_ref().and_then(|key| spaces.get_mut(key)) {
            (leader.top, leader.zoom_y) = (view.top, view.zoom_y);
        }
        drop(spaces);
        cx.notify();
    }

    fn hover(&mut self, hovered: Option<(String, u64)>, cx: &mut Context<Self>) {
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
    node: u64,
    on_press: bool,
    drag: Option<Drag>,
    cursor: Option<Cursor>,
    anchor: (f64, f64),
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
    /// The node of the space, which hears a press and a drag on it.
    node: u64,
    view: SpaceView,
    placed: Vec<Placed>,
    /// What each shape does with the pointer, by its place.
    pointed: Vec<Option<Pointer>>,
    background: Option<Hsla>,
    radius: f32,
    on_press: bool,
    drag: Option<Drag>,
    cursor: Option<Cursor>,
    extent: tree::Ranges,
    zoom: tree::Ranges,
    follow: tree::Follow,
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
        bounds.origin
            + point(
                px(((x - self.view.left) * across) as f32),
                px(((y - self.view.top) * down) as f32),
            )
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
        self.extent.x.is_some()
            || self.extent.y.is_some()
            || self.zoom.x.is_some()
            || self.zoom.y.is_some()
    }
}

/// `view` with the scroll and zoom of the spaces it follows, on their axes.
fn followed(
    mut view: SpaceView,
    follow: &tree::Follow,
    spaces: &HashMap<String, SpaceView>,
) -> SpaceView {
    if let Some(leader) = follow.x.as_ref().and_then(|key| spaces.get(key)) {
        (view.left, view.zoom_x) = (leader.left, leader.zoom_x);
    }
    if let Some(leader) = follow.y.as_ref().and_then(|key| spaces.get(key)) {
        (view.top, view.zoom_y) = (leader.top, leader.zoom_y);
    }
    view
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
            Bounds::new(
                centre - point(px(radius), px(radius)),
                size(px(radius * 2.), px(radius * 2.)),
            )
        };
        match self {
            Self::Rect(bounds, ..) => *bounds,
            Self::Circle(centre, radius) => square(*centre, *radius),
            Self::Arc(centre, radius, _, width) => square(*centre, radius + width / 2.),
            Self::Line(from, to, width) => {
                let (left, top) = (from.x.min(to.x), from.y.min(to.y));
                let (right, bottom) = (from.x.max(to.x), from.y.max(to.y));
                let pad = px(width / 2. + 2.);
                Bounds::from_corners(
                    point(left - pad, top - pad),
                    point(right + pad, bottom + pad),
                )
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
                let (dx, dy) = (
                    f32::from(position.x - centre.x),
                    f32::from(position.y - centre.y),
                );
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
    state: Rc<Value>,
    watches: BTreeMap<String, f32>,
    played: Option<BTreeMap<String, f32>>,
    tick: f64,
    spaces: HashMap<String, SpaceView>,
    /// The card's own, which the paint of a space writes its size in.
    laid_out: Rc<RefCell<HashMap<String, SpaceView>>>,
    drag: Option<(String, DragNumbers)>,
    hovered: Option<(String, u64)>,
    table: &'a NodeTable,
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

    /// Draws the node in `slot`. `index` tells siblings apart, so every element that needs an
    /// id gets its own.
    fn draw(&mut self, slot: usize, index: &[usize]) -> AnyElement {
        let table = self.table;
        let Some(entry) = table.get(slot) else {
            return div().into_any_element();
        };
        let id = entry.id;
        match &entry.props {
            Props::Text(TextProps { text }) => {
                SharedString::from(text.clone().unwrap_or_default()).into_any_element()
            }
            Props::Sample(SampleProps { path, label }) => {
                self.sample(path.as_deref().unwrap_or_default(), label.as_deref())
            }
            Props::Space(space) => self.space(id, space, &entry.children, index),
            // A shape outside a space, or a marker, draws nothing.
            Props::Shape(_) | Props::Marker => div().into_any_element(),
            Props::Div(DivProps {
                key,
                style,
                on_click,
            }) => {
                let children: Vec<AnyElement> = (entry.children.iter())
                    .filter(|child| {
                        !matches!(
                            table.get(**child).map(|child| &child.props),
                            Some(Props::Marker)
                        )
                    })
                    .enumerate()
                    .map(|(position, child)| self.draw(*child, &[index, &[position]].concat()))
                    .collect();
                let element = style
                    .clone()
                    .unwrap_or_default()
                    .apply(div())
                    .children(children);
                if *on_click != Some(true) {
                    return element.into_any_element();
                }
                element
                    .id(ElementId::Name(
                        element_name("div", key.as_deref(), index).into(),
                    ))
                    .cursor_pointer()
                    .on_click(
                        (self.cx).listener(move |view, event: &gpui::ClickEvent, _, cx| {
                            let mut press = json!({
                                "x": 0, "y": 0, "button": "left",
                                "clicks": event.click_count(),
                            });
                            add_modifiers(&mut press, event.modifiers());
                            view.event(id, EventKind::Click, press, cx);
                        }),
                    )
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

    /// The shapes of a space with their numbers worked out, each in the colour it shows, those
    /// of its groups among them, moved by the group.
    fn place(
        &self,
        children: &[usize],
        key: &str,
        view: &SpaceView,
    ) -> (Vec<Placed>, Vec<Option<Pointer>>) {
        let mut placing = Placing {
            sources: self.sources(),
            table: self.table,
            shown: Shown::of(view),
            hovered: (self.hovered.as_ref())
                .filter(|(space, _)| space == key)
                .map(|(_, shape)| *shape),
            placed: Vec::with_capacity(children.len()),
            pointed: Vec::with_capacity(children.len()),
        };
        placing.shapes(children, (0.0, 0.0));
        (placing.placed, placing.pointed)
    }

    fn space(
        &mut self,
        id: u64,
        space: &SpaceProps,
        children: &[usize],
        index: &[usize],
    ) -> AnyElement {
        let key = element_name("space", space.key.as_deref(), index);
        let (unit_x, unit_y) = space.unit();
        let extent = space.extent.unwrap_or_default();
        let scroll = space.scroll.unwrap_or_default();
        let first = |scroll: Option<f32>, extent: Option<[f32; 2]>| {
            scroll.or(extent.map(|[low, _]| low)).map_or(0.0, f64::from)
        };
        let mut view = self.spaces.get(&key).copied().unwrap_or(SpaceView {
            left: first(scroll.x, extent.x),
            top: first(scroll.y, extent.y),
            ..SpaceView::default()
        });
        (view.unit_x, view.unit_y) = (unit_x, unit_y);
        let follow = space.follow.clone().unwrap_or_default();
        let view = followed(clamped(view, &extent), &follow, &self.spaces);
        self.spaces.insert(key.clone(), view);
        let (placed, pointed) = self.place(children, &key, &view);
        let frame = Rc::new(SpaceFrame {
            key,
            node: id,
            view,
            placed,
            pointed,
            background: space.background.map(|color| color.0),
            radius: space.radius.unwrap_or(0.0),
            on_press: space.on_press == Some(true),
            drag: self.table.drag(space.drag).cloned(),
            cursor: space.cursor,
            extent: space.extent.unwrap_or_default(),
            zoom: space.zoom.unwrap_or_default(),
            follow,
        });
        let selector = format!("space-{}", frame.key);
        let card = self.cx.entity().downgrade();
        let spaces = self.laid_out.clone();
        let element = canvas(
            {
                let frame = frame.clone();
                move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                    prepaint(&frame, bounds, window)
                }
            },
            move |bounds: Bounds<Pixels>,
                  (hitbox, painted): (Hitbox, Rc<Vec<Painted>>),
                  window: &mut Window,
                  cx: &mut App| {
                // The size it was laid out at, for live values of what shows and the next scroll.
                if let Some(view) = spaces.borrow_mut().get_mut(&frame.key) {
                    view.width = f64::from(f32::from(bounds.size.width));
                    view.height = f64::from(f32::from(bounds.size.height));
                }
                paint(&frame, bounds, &hitbox, &painted, window, cx);
                listen(&frame, &card, bounds, hitbox, painted, window);
            },
        );
        let holder = div()
            .debug_selector(move || selector)
            .child(element.size_full());
        let holder = match space.width {
            Some(width) => holder.w(px(width)),
            None => holder.w_full(),
        };
        let holder = match space.height {
            Some(height) => holder.h(px(height)),
            None => holder.h_full(),
        };
        let holder = if space.grow == Some(true) {
            holder.flex_grow(1.).min_h_0()
        } else {
            holder.flex_none()
        };
        holder.into_any_element()
    }
}

/// What of a space shows, in its units, as it was last laid out; with points per unit, to
/// reach as far as a shape sized in points does.
struct Shown {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    per_point: (f64, f64),
}

impl Shown {
    /// `None` before the space was laid out: then everything is placed.
    fn of(view: &SpaceView) -> Option<Self> {
        let (across, down) = view.scale();
        (view.width > 0.0 && across > 0.0 && down > 0.0).then(|| Self {
            left: view.left,
            right: view.left + view.width / across,
            top: view.top,
            bottom: view.top + view.height / down,
            per_point: (1.0 / across, 1.0 / down),
        })
    }

    /// Whether what spans `left..right` and `top..bottom`, and `reach` points further each way,
    /// may show.
    fn reaches(&self, (left, right): (f64, f64), (top, bottom): (f64, f64), reach: (f32, f32)) -> bool {
        let (x, y) = (
            f64::from(reach.0) * self.per_point.0,
            f64::from(reach.1) * self.per_point.1,
        );
        left.min(right) - x <= self.right
            && left.max(right) + x >= self.left
            && top.min(bottom) - y <= self.bottom
            && top.max(bottom) + y >= self.top
    }

    fn shows(&self, geometry: &Geometry) -> bool {
        match *geometry {
            Geometry::Rect {
                x,
                y,
                width,
                height,
                ..
            } => self.reaches((x, x + width), (y, y + height), (1.0, 1.0)),
            Geometry::Circle { x, y, radius } | Geometry::Arc { x, y, radius, .. } => {
                self.reaches((x, x), (y, y), (radius + 2.0, radius + 2.0))
            }
            Geometry::Line { from, to, width } => {
                self.reaches((from.0, to.0), (from.1, to.1), (width, width))
            }
            // A line of text is as wide as its words: a long label far off to the left may
            // still reach in.
            Geometry::Text { x, y, size, .. } => {
                self.reaches((x, x), (y, y), (size * 40.0, size * 2.0))
            }
        }
    }
}

/// The shapes of a space as they are placed, in the order they paint.
struct Placing<'a> {
    sources: Sources<'a>,
    table: &'a NodeTable,
    /// Shapes outside it are not placed, so a long piece costs what shows.
    shown: Option<Shown>,
    /// The node of the shape of this space the pointer is over.
    hovered: Option<u64>,
    placed: Vec<Placed>,
    /// What each shape does with the pointer, by its place; a group has none.
    pointed: Vec<Option<Pointer>>,
}

impl Placing<'_> {
    fn number(&self, number: &Option<tree::Num>) -> f64 {
        number
            .as_ref()
            .map_or(0.0, |number| self.sources.number(number))
    }

    fn shapes(&mut self, children: &[usize], (left, top): (f64, f64)) {
        let table = self.table;
        for slot in children {
            let Some(entry) = table.get(*slot) else {
                continue;
            };
            let Props::Shape(shape) = &entry.props else {
                continue;
            };
            if shape.kind == Some(ShapeKind::Group) {
                let offset = (left + self.number(&shape.x), top + self.number(&shape.y));
                self.shapes(&entry.children, offset);
            } else {
                self.shape(entry.id, shape, (left, top));
            }
        }
    }

    /// How far the drag that goes on moves a shape that follows it with `follows`.
    fn followed(&self, follows: &Option<Vec<String>>) -> (f64, f64) {
        match (self.sources.drag, follows) {
            (Some((held, numbers)), Some(ids)) if ids.iter().any(|id| id == held) => {
                (numbers.dx, numbers.dy)
            }
            _ => (0.0, 0.0),
        }
    }

    fn shape(&mut self, id: u64, shape: &tree::ShapeProps, (left, top): (f64, f64)) {
        let index = self.pointed.len();
        let hovered = self.hovered == Some(id);
        let (moved_x, moved_y) = self.followed(&shape.moves);
        let (left, top) = (left + moved_x, top + moved_y);
        let (grown_x, grown_y) = self.followed(&shape.grows);
        let reach = |reach: &Option<Reach>| match reach {
            Some(Reach::Point([x, y])) => {
                (left + self.sources.number(x), top + self.sources.number(y))
            }
            Some(Reach::Turn(turn)) => (self.sources.number(turn), 0.0),
            None => (left, top),
        };
        let (x, y) = (left + self.number(&shape.x), top + self.number(&shape.y));
        let Some(kind) = shape.kind else {
            return;
        };
        let geometry = match kind {
            ShapeKind::Rect => {
                let border_width = self.number(&shape.border_width) as f32;
                Geometry::Rect {
                    x,
                    y,
                    width: (self.number(&shape.width) + grown_x).max(0.0),
                    height: (self.number(&shape.height) + grown_y).max(0.0),
                    radius: self.number(&shape.radius) as f32,
                    border: (shape.border)
                        .filter(|_| border_width > 0.0)
                        .map(|border| (border.0, border_width)),
                }
            }
            ShapeKind::Circle => Geometry::Circle {
                x,
                y,
                radius: self.number(&shape.radius) as f32,
            },
            ShapeKind::Line => Geometry::Line {
                from: reach(&shape.from),
                to: reach(&shape.to),
                width: self.number(&shape.width).max(0.0) as f32,
            },
            ShapeKind::Arc => Geometry::Arc {
                x,
                y,
                radius: self.number(&shape.radius) as f32,
                turns: (reach(&shape.from).0 as f32, reach(&shape.to).0 as f32),
                width: self.number(&shape.width).max(0.0) as f32,
            },
            ShapeKind::Text => Geometry::Text {
                x,
                y,
                text: match &shape.text {
                    Some(Words::Fixed(text)) => text.clone().into(),
                    Some(Words::Live(live)) => {
                        readout(self.sources.number(live) as f32, shape.unit).into()
                    }
                    None => SharedString::default(),
                },
                size: shape.size.unwrap_or(12.0),
                align: shape.align.unwrap_or(Align::Left),
            },
            ShapeKind::Group => return,
        };
        if self.shown.as_ref().is_some_and(|shown| !shown.shows(&geometry)) {
            return;
        }
        let origin = match &geometry {
            Geometry::Line { from, .. } => *from,
            _ => (x, y),
        };
        let anchor = shape.anchor.unwrap_or_default();
        self.pointed.push(shape.is_pointed().then(|| Pointer {
            node: id,
            on_press: shape.on_press == Some(true),
            drag: self.table.drag(shape.drag).cloned(),
            cursor: shape.cursor,
            anchor: (
                anchor.x.map_or(origin.0, |x| left + f64::from(x)),
                anchor.y.map_or(origin.1, |y| top + f64::from(y)),
            ),
        }));
        let opacity = (shape.opacity.as_ref())
            .map_or(1.0, |opacity| self.sources.number(opacity).clamp(0.0, 1.0))
            as f32;
        if opacity <= 0.0 {
            return;
        }
        let shown = if hovered {
            shape.hover.or(shape.color)
        } else {
            shape.color
        };
        let mut color = shown.map_or(gpui::white(), |color| color.0);
        color.a *= opacity;
        self.placed.push(Placed {
            index,
            geometry,
            color,
        });
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
fn prepaint(
    frame: &SpaceFrame,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> (Hitbox, Rc<Vec<Painted>>) {
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
    let mut frame_view = frame.view;
    frame_view.width = f64::from(f32::from(bounds.size.width));
    frame_view.height = f64::from(f32::from(bounds.size.height));
    let (across, down) = frame_view.scale();
    let at = |(x, y): (f64, f64)| frame.point_at(bounds, (x, y));
    let mut painted = Vec::new();
    for placed in &frame.placed {
        let shape = match &placed.geometry {
            Geometry::Rect {
                x,
                y,
                width,
                height,
                radius,
                border,
            } => {
                let origin = at((*x, *y));
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Rect(Bounds::new(origin, size), *radius, *border)
            }
            Geometry::Circle { x, y, radius } => PaintedShape::Circle(at((*x, *y)), *radius),
            Geometry::Line { from, to, width } => PaintedShape::Line(at(*from), at(*to), *width),
            Geometry::Arc {
                x,
                y,
                radius,
                turns,
                width,
            } => PaintedShape::Arc(at((*x, *y)), *radius, *turns, *width),
            Geometry::Text {
                x,
                y,
                text,
                size,
                align,
            } => PaintedShape::Text(at((*x, *y)), text.clone(), *size, *align),
        };
        // What does not show is not painted and not found.
        if !shape.bounds().intersects(&bounds) {
            continue;
        }
        let cursor = (frame.pointed.get(placed.index))
            .and_then(Option::as_ref)
            .and_then(|pointer| pointer.cursor);
        let hitbox = cursor.map(|cursor| {
            (
                window.insert_hitbox(shape.bounds(), HitboxBehavior::Normal),
                cursor,
            )
        });
        painted.push(Painted {
            index: placed.index,
            shape,
            color: placed.color,
            hitbox,
        });
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
            // A shape with no colour is there for the pointer, such as the handle at the end of
            // a note; GPUI orders a quad by what it overlaps, so painting it costs as much.
            if each.color.a > 0.0 {
                paint_shape(&each.shape, each.color, window, cx);
            }
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
            let bounds = Bounds::new(
                *centre - point(px(*radius), px(*radius)),
                size(px(radius * 2.), px(radius * 2.)),
            );
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
            path.arc_to(
                radii,
                px(0.),
                (to - from).abs() > 0.5,
                to > from,
                on_circle(*to),
            );
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
            let shaped =
                window
                    .text_system()
                    .shape_line(text.clone(), px(*font_size), &[run], None);
            let left = match align {
                Align::Left => origin.x,
                Align::Center => origin.x - shaped.width / 2.,
                Align::Right => origin.x - shaped.width,
            };
            let line_height = px(font_size * 1.3);
            if let Err(error) = shaped.paint(
                point(left, origin.y),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            ) {
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
        let (frame, card, painted, hitbox) =
            (frame.clone(), card.clone(), painted.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let target = found(&frame, &painted, event.position);
            if target.is_none() && !frame.on_press && frame.drag.is_none() {
                return;
            }
            card.update(cx, |card, cx| {
                if card
                    .live
                    .read(cx)
                    .info(card.card)
                    .is_some_and(|info| info.keys)
                {
                    window.focus(&card.focus, cx);
                }
                card.press(&frame, target, bounds, event, cx);
            })
            .ok();
            cx.stop_propagation();
        }
    });
    window.on_mouse_event({
        let (frame, card, hitbox) = (frame.clone(), card.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window);
            card.update(cx, |card, cx| {
                if event.dragging() {
                    card.drag_to(&frame, event.position, event.modifiers, cx);
                    return;
                }
                let hovered = found(&frame, &painted, event.position)
                    .filter(|_| over)
                    .and_then(|index| frame.pointed.get(index).cloned().flatten())
                    .map(|pointer| (frame.key.clone(), pointer.node));
                let hovered_here = card
                    .hovered
                    .as_ref()
                    .is_some_and(|(space, _)| *space == frame.key);
                if hovered.is_some() || hovered_here {
                    card.hover(hovered, cx);
                }
            })
            .ok();
        }
    });
    window.on_mouse_event({
        let (frame, card) = (frame.clone(), card.clone());
        move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                card.update(cx, |card, cx| card.drag_ends(&frame, cx)).ok();
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
            card.update(cx, |card, cx| {
                card.moved_space(&frame.key, view, &frame.follow, cx)
            })
            .ok();
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
        card.update(cx, |card, cx| {
            card.moved_space(&frame.key, view, &frame.follow, cx)
        })
        .ok();
        cx.stop_propagation();
    });
}

/// The frame with its view as the card has it now: several wheel events may come before the
/// next draw.
fn frame_now(
    frame: &SpaceFrame,
    card: &WeakEntity<TypeScriptCard>,
    bounds: Bounds<Pixels>,
    cx: &App,
) -> SpaceFrame {
    let mut view = (card.upgrade())
        .and_then(|card| {
            let spaces = card.read(cx).spaces.borrow();
            let view = spaces.get(&frame.key).copied()?;
            Some(followed(view, &frame.follow, &spaces))
        })
        .unwrap_or(frame.view);
    view.width = f64::from(f32::from(bounds.size.width));
    view.height = f64::from(f32::from(bounds.size.height));
    SpaceFrame {
        key: frame.key.clone(),
        node: frame.node,
        view,
        placed: Vec::new(),
        pointed: Vec::new(),
        background: None,
        radius: 0.0,
        on_press: false,
        drag: None,
        cursor: None,
        extent: frame.extent,
        zoom: frame.zoom,
        follow: frame.follow.clone(),
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state(cx) else {
            return div().into_any_element();
        };
        // An ended drag lets go once the answer to its end arrived.
        let answered = self.live.read(cx).events_answered(self.card);
        if let Some(Held {
            ended: Some((number, at)),
            ..
        }) = &self.held
            && (answered >= *number || at.elapsed() > ENDED_DRAG)
        {
            self.held = None;
        }
        let live = self.live.read(cx);
        let info = live.info(self.card);
        let hears_keys = info.as_ref().is_some_and(|info| info.keys);
        let tree = live.tree(self.card).cloned();
        let uses = live.uses(self.card);
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let played = live.played_of(&self.id).cloned();
        let playhead = *self.session.read(cx).playhead().read(cx);
        if uses.tick && playhead.playing {
            window.request_animation_frame();
        }
        let body = match tree {
            Some(Ok(table)) => {
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
                    table: &table,
                    cx,
                };
                let body = match table.root() {
                    Some(root) => drawing.draw(root, &[]),
                    None => div().into_any_element(),
                };
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
            // Until its first nodes arrive, a moment after the card opens.
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
                let room = div()
                    .relative()
                    .size_full()
                    .child(measure)
                    .child(body.size_full());
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
            node: 1,
            view,
            placed: Vec::new(),
            pointed: Vec::new(),
            background: None,
            radius: 0.0,
            on_press: false,
            drag: None,
            cursor: None,
            extent,
            zoom,
            follow: tree::Follow::default(),
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
        let extent = tree::Ranges {
            x: Some([0.0, 38_400.0]),
            y: Some([0.0, 128.0]),
        };
        let zoom = tree::Ranges {
            x: Some([0.25, 8.0]),
            y: None,
        };
        let frame = frame(view, extent, zoom);
        let pointer = point(px(300.), px(150.));
        assert_eq!(frame.units_at(bounds, pointer), (2960.0, 10.0));
        let zoomed = frame.zoomed(2.0, bounds, pointer);
        assert_eq!(zoomed.zoom_x, 2.0);
        assert_eq!(zoomed.zoom_y, 1.0);
        let after = SpaceFrame {
            view: zoomed,
            ..frame
        };
        assert_eq!(after.units_at(bounds, pointer), (2960.0, 10.0));
    }

    #[test]
    fn a_space_scrolls_inside_its_extent_and_not_along_an_axis_without_one() {
        let view = SpaceView {
            left: 100.0,
            top: 3.0,
            width: 400.0,
            height: 200.0,
            ..SpaceView::default()
        };
        let held = clamped(
            view,
            &tree::Ranges {
                x: Some([0.0, 450.0]),
                y: None,
            },
        );
        assert_eq!((held.left, held.top), (50.0, 0.0));
    }
}
