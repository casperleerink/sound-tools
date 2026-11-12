//! What the pointer and the keys do on a card: the presses, releases and clicks its code
//! hears, the drags that turn what they turn, hovers, drops and keys.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    App, Bounds, Context, DispatchPhase, Div, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke,
    Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, WeakEntity, Window, canvas, div, point, prelude::*, px,
};
use serde_json::{Value, json};
use sound_core::ValueRange;
use sound_ui::ActiveTheme;
use sound_ui::components::gesture::{ValueChange, ValueKey};
use sound_ui::components::knob::{FINE_KEY_STEP, KEY_STEP};

use crate::bun::EventKind;
use crate::feeds::{Moment, lane_value};
use crate::live::{self, DragNumbers, Sources, SpaceView};
use crate::tools::ToolInfo;
use crate::tree::{self, Axis, Drag, Turned};
use crate::window::state_of;

use super::TypeScriptCard;
use super::drawing::typing;
use super::space::SpaceFrame;

/// Points of pointer travel for the whole range of what a drag turns, as a knob of the window.
const TRAVEL: f32 = 200.0;
/// Points of a drag for one step of what turns in steps, when it says none.
const STEP_TRAVEL: f32 = 10.0;
/// With shift, a drag turns this much as far.
pub(super) const FINE: f32 = 0.1;

/// Takes no room and writes into `down` whether a mouse button is down in the window. A left or
/// right button that goes down ends the click of `card`; one that goes up releases its presses of
/// it, before a click or a drag's end.
pub(super) fn hears_the_button(
    down: Rc<Cell<bool>>,
    card: WeakEntity<TypeScriptCard>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            window.on_mouse_event({
                let (down, card) = (down.clone(), card.clone());
                move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        down.set(true);
                        if heard(event.button) {
                            (card.update(cx, |card, _| card.clicking = None)).ok();
                        }
                    }
                }
            });
            window.on_mouse_event({
                let (down, card) = (down.clone(), card.clone());
                move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        down.set(false);
                        (card.update(cx, |card, cx| card.let_go(Some(event.button), cx))).ok();
                    }
                }
            });
            // A button that came up outside the window.
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    down.set(event.pressed_button.is_some());
                    if event.pressed_button.is_none() {
                        (card.update(cx, |card, cx| card.let_go(None, cx))).ok();
                    }
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// What a drag of a node carries, by its `carry`.
#[derive(Clone)]
pub(super) struct Carried(pub(super) String);

/// Where a menu hangs under a node laid out at `bounds`: from its bottom left, as a select's list.
pub(super) fn under(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(
        bounds.origin.x,
        bounds.origin.y + bounds.size.height + px(6.),
    )
}

/// Takes no room and writes where its parent, the node `id`, was laid out into `placed`.
pub(super) fn placed(
    id: u64,
    placed: &Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
) -> impl IntoElement {
    let placed = placed.clone();
    canvas(
        move |bounds, _, _| {
            placed.borrow_mut().insert(id, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The divs that hear the pointer come over them: where each one this frame drew was laid out,
/// those the pointer is over, and whether it is in the window.
#[derive(Default)]
pub(super) struct Hovers {
    pub(super) placed: HashMap<u64, Bounds<Pixels>>,
    over: HashSet<u64>,
    inside: bool,
}

/// Takes no room and tells the div `id`, its parent, when the pointer comes over it and when it
/// leaves. Its bounds tell, not its hitbox: a child that takes the pointer, as a toggle on a
/// head does, would end the hover of a hitbox.
pub(super) fn hears_hover(
    id: u64,
    hovers: Rc<RefCell<Hovers>>,
    card: WeakEntity<TypeScriptCard>,
) -> impl IntoElement {
    canvas(
        {
            let hovers = hovers.clone();
            move |bounds, _, _| {
                hovers.borrow_mut().placed.insert(id, bounds);
            }
        },
        move |_, (), window, cx| {
            // What moved under a pointer that stood still, as heads do while the timeline
            // scrolls, once every div of the frame is laid out.
            let pointer = window.mouse_position();
            window.defer(cx, {
                let (hovers, card) = (hovers.clone(), card.clone());
                move |_, cx| tell_hovers(&hovers, Some(pointer), &card, cx)
            });
            window.on_mouse_event({
                let (hovers, card) = (hovers.clone(), card.clone());
                move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        hovers.borrow_mut().inside = true;
                        tell_hovers(&hovers, Some(event.position), &card, cx);
                    }
                }
            });
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    hovers.borrow_mut().inside = false;
                    tell_hovers(&hovers, None, &card, cx);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// Tells each div that hears the pointer come over it whether it is over it now, at `pointer`,
/// if that changed.
fn tell_hovers(
    hovers: &RefCell<Hovers>,
    pointer: Option<Point<Pixels>>,
    card: &WeakEntity<TypeScriptCard>,
    cx: &mut App,
) {
    let changed: Vec<(u64, bool)> = {
        let mut hovers = hovers.borrow_mut();
        let over: HashSet<u64> = match pointer.filter(|_| hovers.inside) {
            Some(pointer) => (hovers.placed.iter())
                .filter(|(_, bounds)| bounds.contains(&pointer))
                .map(|(id, _)| *id)
                .collect(),
            None => HashSet::new(),
        };
        let left = (hovers.over.difference(&over)).map(|id| (*id, false));
        let came = (over.difference(&hovers.over)).map(|id| (*id, true));
        let changed = left.chain(came).collect();
        hovers.over = over;
        changed
    };
    for (id, over) in changed {
        (card.update(cx, |card, cx| {
            card.event(id, EventKind::Hover, json!(over), cx)
        }))
        .ok();
    }
}

impl Render for Carried {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(theme.lavender)
            .text_color(theme.text())
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

/// A drag the pointer holds, on a shape or a space.
#[derive(Clone)]
pub(super) struct Held {
    pub(super) space: String,
    /// The node that hears it: the shape it holds, or the space.
    node: u64,
    /// Whether it holds a shape, whose drag it takes again as the shape changes.
    on_shape: bool,
    pub(super) drag: Rc<Drag>,
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
    pub(super) ended: Option<(u64, Instant)>,
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

    pub(super) fn numbers(&self) -> DragNumbers {
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
        // Cmd held is off the grid in time.
        let free = self.modifiers.platform;
        DragNumbers {
            start_x: self.start_units.0,
            start_y: self.start_units.1,
            dx: if free {
                dx
            } else {
                whole(dx, snap.x, self.anchor.0)
            },
            dy: whole(dy, snap.y, self.anchor.1),
            active: true,
            alt: self.modifiers.alt,
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

/// Code hears only the left and the right button.
pub(super) fn heard(button: MouseButton) -> bool {
    matches!(button, MouseButton::Left | MouseButton::Right)
}

/// What a press tells its node's code: where in the units of its space (0 on a div), which
/// button, how many clicks in a row and the modifiers.
pub(super) fn press_of(
    (x, y): (f64, f64),
    button: MouseButton,
    clicks: usize,
    modifiers: Modifiers,
) -> Value {
    let button = match button {
        MouseButton::Right => "right",
        _ => "left",
    };
    let mut press = json!({ "x": x, "y": y, "button": button, "clicks": clicks });
    add_modifiers(&mut press, modifiers);
    press
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
    /// The next menu hangs under the node `id`, as a select's list does, when it was laid out.
    pub(super) fn hang_menus_under(&self, id: u64) {
        self.anchor
            .set(self.placed.borrow().get(&id).copied().map(under));
    }

    /// Copies dropped audio files into `assets/audio/` and reads dropped MIDI files into
    /// timelines, in the background, and tells the code of `node` what came and where it fell.
    pub(super) fn drop_files(
        &mut self,
        node: u64,
        paths: Vec<std::path::PathBuf>,
        (x, y): (f64, f64),
        cx: &mut Context<Self>,
    ) {
        let assets = self.session.read(cx).project().assets().clone();
        let imported = cx.background_spawn(async move {
            (paths.iter())
                .map(|path| match timelines_in(path) {
                    Some(timelines) => timelines.map(Dropped::Timelines),
                    None => (sound_media::import(&assets, path))
                        .map(|file| Dropped::File(file.asset.to_string()))
                        .map_err(|error| error.to_string()),
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |view, cx| {
            let imported = imported.await;
            view.update(cx, |view, cx| {
                let (mut files, mut timelines) = (Vec::new(), Vec::new());
                for dropped in imported {
                    match dropped {
                        Ok(Dropped::File(file)) => files.push(file),
                        Ok(Dropped::Timelines(each)) => timelines.extend(each),
                        Err(error) => view
                            .session
                            .update(cx, |session, cx| session.report(error, cx)),
                    }
                }
                if !files.is_empty() || !timelines.is_empty() {
                    let dropped = dropped(&files, &timelines, None, (x, y));
                    view.event(node, EventKind::Drop, dropped, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// A key went down or up while the card has the keys, not a `repeat`: of a key held
    /// here, or of one that went down while something else had the keys, such as the computer
    /// keys. A key that went down here comes up here, whatever is held by then. A key with
    /// cmd, ctrl, alt or fn only goes down, as macOS sends no up while cmd is held; one the
    /// window binds, such as cmd-z, is the window's and never comes here. A key that does
    /// actions of its tool runs them as it goes down, and `onKey` hears nothing of it.
    fn key(&mut self, keystroke: &Keystroke, down: bool, repeat: bool, cx: &mut Context<Self>) {
        if down && keystroke.key == "escape" {
            if self.cancel_drag(cx) {
                cx.stop_propagation();
                return;
            }
            if let Some(&newest) = self.overlays.last() {
                self.event(newest, EventKind::Close, json!({}), cx);
                cx.stop_propagation();
                return;
            }
        }
        // A dialog keeps every key from its code, the window's included. A key held before it
        // opened comes up all the same.
        if down && self.live.read(cx).shows_dialog(&self.id) {
            cx.stop_propagation();
            return;
        }
        let Some(info) = self.live.read(cx).info(self.card) else {
            return;
        };
        let actions = match down {
            true => crate::actions_on(&info.name, keystroke, cx),
            false => Vec::new(),
        };
        if !actions.is_empty() {
            if !repeat {
                let id = self.id.clone();
                self.live.update(cx, |live, cx| live.act(&id, &actions, cx));
            }
            cx.stop_propagation();
            return;
        }
        if !info.keys {
            return;
        }
        let modifiers = keystroke.modifiers;
        let chord = modifiers.platform || modifiers.control || modifiers.alt || modifiers.function;
        let key = keystroke.key.clone();
        // A tool that names its keys leaves the rest to the window, such as space to play. A key
        // it took comes up here all the same.
        if let Some(takes) = &info.takes
            && !takes.contains(&sound_ui::global_keys::chord_name(keystroke))
            && (down || !self.keys_down.contains(&key))
        {
            return;
        }
        // A key that went down before the card had the keys has no up here.
        let changed = match (down, chord) {
            (true, true) => !repeat,
            (true, false) => !repeat && self.keys_down.insert(key.clone()),
            (false, _) => self.keys_down.remove(&key),
        };
        if changed {
            let id = self.id.clone();
            self.live
                .update(cx, |live, cx| live.key(&id, &key, down, modifiers, cx));
        }
        cx.stop_propagation();
    }

    /// Every key the card holds goes up.
    pub(super) fn let_go_of_keys(&mut self, cx: &mut App) {
        for key in std::mem::take(&mut self.keys_down) {
            self.live.update(cx, |live, cx| {
                live.key(&self.id, &key, false, Modifiers::default(), cx)
            });
        }
    }

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
    pub(super) fn event(
        &mut self,
        node: u64,
        kind: EventKind,
        payload: Value,
        cx: &mut App,
    ) -> u64 {
        let card = self.card;
        (self.live).update(cx, |live, cx| live.event(card, node, kind, payload, cx))
    }

    /// The pointer pressed the node `node`: its code hears the press now and the release as the
    /// button goes up.
    pub(super) fn pointer_down(
        &mut self,
        node: u64,
        hears: Hears,
        button: MouseButton,
        press: Value,
        cx: &mut App,
    ) {
        if !hears.press {
            return;
        }
        if hears.release {
            self.pressed.push((node, button, press.clone()));
        }
        self.event(node, EventKind::Press, press, cx);
    }

    /// The left button went up over the node `node`: its left press, with no drag since, clicks
    /// it.
    pub(super) fn released_over(&mut self, node: u64, cx: &mut App) {
        let Some((_, press)) = self.clicking.take_if(|(pressed, _)| *pressed == node) else {
            return;
        };
        let dragged = cx.has_active_drag()
            || (self.held.as_ref()).is_some_and(|held| held.ended.is_none() && held.moved);
        if !dragged {
            self.hang_menus_under(node);
            self.event(node, EventKind::Click, press, cx);
        }
    }

    /// `button` went up, or with `None` every button: the nodes pressed with it hear their
    /// release.
    pub(super) fn let_go(&mut self, button: Option<MouseButton>, cx: &mut App) {
        let (released, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pressed)
            .into_iter()
            .partition(|(_, pressed, _)| button.is_none_or(|button| button == *pressed));
        self.pressed = kept;
        for (node, _, press) in released {
            self.event(node, EventKind::Release, press, cx);
        }
    }

    /// A press on a space, on a shape that hears the pointer or on the space itself.
    pub(super) fn press(
        &mut self,
        frame: &SpaceFrame,
        target: Option<usize>,
        bounds: Bounds<Pixels>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let units = frame.units_at(bounds, event.position);
        let (hears, drag, node, on_shape, anchor) = match target {
            Some(index) => match frame.pointed.get(index).and_then(Option::as_ref) {
                Some(pointer) => (
                    pointer.hears,
                    pointer.drag.clone(),
                    pointer.node,
                    true,
                    pointer.anchor,
                ),
                None => return,
            },
            None => (frame.hears, frame.drag.clone(), frame.node, false, units),
        };
        self.anchor.set(None);
        let press = press_of(units, event.button, event.click_count, event.modifiers);
        if hears.click && event.button == MouseButton::Left {
            self.clicking = Some((node, press.clone()));
        }
        self.pointer_down(node, hears, event.button, press, cx);
        let Some(drag) = drag.filter(|_| event.button == MouseButton::Left) else {
            return;
        };
        let (state, info) = self.record(cx);
        let turns = [&drag.x, &drag.y];
        if event.click_count == 2 {
            for (axis, turns) in turns.into_iter().enumerate() {
                let Some((turns, default)) =
                    (turns.as_ref()).and_then(|turns| Some((turns, turns.default?)))
                else {
                    continue;
                };
                match turns.target(info.as_deref()) {
                    Turned::Kept => self.tell_kept(node, axis, json!({ "reset": true }), true, cx),
                    _ => self.turn(turns, ValueChange::Set(default), cx),
                }
            }
            return;
        }
        let from =
            turns.map(|turns| self.turned_value(turns.as_ref()?, &state, info.as_deref(), cx));
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
    pub(super) fn drag_to(
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
        cx.notify();
    }

    /// The left button went up while the pointer is over the node `over` of the space of
    /// `frame`, if any: it may click it, and the drag it held ends.
    pub(super) fn pointer_up(
        &mut self,
        frame: &SpaceFrame,
        over: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(over) = over {
            self.released_over(over, cx);
        }
        self.drag_ends(frame, cx);
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
        let info = self.live.read(cx).info(self.card);
        let fields = [&held.drag.x, &held.drag.y]
            .into_iter()
            .flatten()
            .any(|turns| matches!(turns.target(info.as_deref()), Turned::Field { .. }));
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
        self.tell_turns(true, cx);
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
    /// hears no end and no click; the release still comes as the button goes up.
    pub(super) fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(held) = self.held.take_if(|held| held.ended.is_none()) else {
            return false;
        };
        self.clicking = None;
        let turns = [&held.drag.x, &held.drag.y];
        let info = self.live.read(cx).info(self.card);
        let field =
            |turns: &tree::Turns| matches!(turns.target(info.as_deref()), Turned::Field { .. });
        if turns.iter().copied().flatten().any(field) {
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
            let (Some(turns), Some(from)) = (turns.as_ref(), from) else {
                continue;
            };
            if let Turned::Input(input) = turns.target(info.as_deref()) {
                self.control(input, from, cx);
            }
        }
        // What code keeps goes back to where the press found it.
        for (axis, turns) in turns.into_iter().enumerate() {
            if (turns.as_ref()).is_some_and(|turns| turns.target(None) == Turned::Kept) {
                let back = json!({ "moved": 0, "held": true });
                self.tell_kept(held.node, axis, back, true, cx);
            }
        }
        cx.notify();
        true
    }

    /// What `turns` turns is now. What code keeps is its code's to say.
    fn turned_value(
        &self,
        turns: &tree::Turns,
        state: &Value,
        info: Option<&ToolInfo>,
        cx: &App,
    ) -> Option<f32> {
        match turns.target(info) {
            Turned::Field {
                child: Some(child),
                name,
            } => {
                // What plays, a lane's value when one moves it, so a drag does not jump.
                if let Some((played, _)) = self.feeds.played.get(&format!("{child}.{name}")) {
                    return Some(*played);
                }
                let child = self.id.child(child).ok()?;
                let record = state_of(self.session.read(cx).project(), &child)?;
                let value = crate::path::at(&record, name).and_then(tree::number_of);
                // A number its record leaves out turns from what its device plays, such as the
                // level of a pad; one no lane can move, from its default.
                let project = self.session.read(cx).project();
                let plays = || project.automation(&child, name)?.record;
                value.or_else(plays).or(turns.default).or_else(|| {
                    let range = range_of(info, turns).or_else(|| self.child_range(turns, cx));
                    range.map(|range| range.value(0.5))
                })
            }
            target => {
                let played = self.live.read(cx).played_of(&self.id).cloned();
                let sources = Sources {
                    tick: 0.0,
                    state,
                    info,
                    played: played.as_ref(),
                    drag: None,
                    spaces: &HashMap::new(),
                    hovered: None,
                    touched: &[],
                    feeds: &self.feeds,
                };
                match target {
                    Turned::Input(name) => sources.input(name),
                    // What plays, as for a child's.
                    Turned::Field { name, .. } => (self.feeds.played.get(name))
                        .map(|(played, _)| *played)
                        .or_else(|| sources.field(name)),
                    Turned::Kept => None,
                }
            }
        }
    }

    /// A key on the focused control of what `turns` turns, as a knob of the window takes it: an
    /// arrow steps it a fiftieth of its travel, a five-hundredth with shift, or one of its own
    /// steps; backspace puts it at its default. One undo step each. Whether the key was the
    /// control's: an arrow is, at an end too; backspace only with a default; escape is what
    /// holds it.
    pub(super) fn turn_by_key(
        &mut self,
        KeyTurned { turns, node, axis }: &KeyTurned,
        key: ValueKey,
        cx: &mut Context<Self>,
    ) -> bool {
        let (state, info) = self.record(cx);
        let kept = turns.target(info.as_deref()) == Turned::Kept;
        let value = match key {
            ValueKey::Cancel => return false,
            ValueKey::Reset => match turns.default {
                None => return false,
                Some(_) if kept => {
                    self.tell_kept(*node, *axis, json!({ "reset": true }), true, cx);
                    None
                }
                Some(default) => Some(default),
            },
            ValueKey::Step { up, fine } => {
                let by = if up { 1.0 } else { -1.0 };
                let moved = match turns.step {
                    Some(_) => Moved::Steps(by),
                    None if fine => Moved::By(by * FINE_KEY_STEP),
                    None => Moved::By(by * KEY_STEP),
                };
                if kept {
                    self.tell_kept(*node, *axis, moved.told(false), true, cx);
                    None
                } else {
                    let now = self.turned_value(turns, &state, info.as_deref(), cx);
                    let range =
                        range_of(info.as_deref(), turns).or_else(|| self.child_range(turns, cx));
                    moved.value(turns, now, range)
                }
            }
        };
        if let Some(value) = value {
            self.turn(turns, ValueChange::Set(value), cx);
        }
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
            let points = match axis {
                0 => f32::from(held.now.x - held.start.x),
                _ => f32::from(held.start.y - held.now.y),
            };
            let moved = if turns.absolute {
                let bounds = held.bounds;
                let fraction = match axis {
                    0 => (held.now.x - bounds.origin.x) / bounds.size.width,
                    _ => 1.0 - (held.now.y - bounds.origin.y) / bounds.size.height,
                };
                Moved::To(fraction.clamp(0.0, 1.0))
            } else if turns.step.is_some() {
                Moved::Steps((points / turns.per.unwrap_or(STEP_TRAVEL)).round())
            } else {
                let fine = if held.modifiers.shift { FINE } else { 1.0 };
                Moved::By(points * fine / turns.travel.unwrap_or(TRAVEL))
            };
            if turns.target(info.as_deref()) == Turned::Kept {
                self.tell_kept(held.node, axis, moved.told(true), false, cx);
                continue;
            }
            let range = range_of(info.as_deref(), turns).or_else(|| self.child_range(turns, cx));
            if let Some(value) = moved.value(turns, held.from[axis], range) {
                self.turn(turns, ValueChange::Drag(value), cx);
            }
        }
    }

    /// How the field of an instance inside it a drag turns spreads over its travel: as that
    /// instance's number.
    fn child_range(&self, turns: &tree::Turns, cx: &App) -> Option<ValueRange> {
        let Turned::Field {
            child: Some(child),
            name: field,
        } = turns.target(None)
        else {
            return None;
        };
        let child = self.id.child(child).ok()?;
        let number = self.session.read(cx).project().automation(&child, field)?;
        Some(number.range)
    }

    pub(super) fn turn(
        &mut self,
        turns: &tree::Turns,
        change: ValueChange<f32>,
        cx: &mut Context<Self>,
    ) {
        let info = self.live.read(cx).info(self.card);
        match turns.target(info.as_deref()) {
            Turned::Field { child, name } => {
                let label = (turns.label.clone())
                    .unwrap_or_else(|| format!("Change {}", name.replace('_', " ")));
                let field = name.to_string();
                let id = match child {
                    Some(child) => match self.id.child(child) {
                        Ok(child) => child,
                        Err(_) => return,
                    },
                    None => self.id.clone(),
                };
                // A lane's number is not the composer's to turn: no drag, key or reset moves it.
                let session = self.session.read(cx);
                let at = Moment::of(session, *session.playhead().read(cx));
                if lane_value(session.project(), &id, name, at).is_some_and(|(_, held)| held) {
                    return;
                }
                self.edit.apply_json(
                    &self.session,
                    &id,
                    &label,
                    change,
                    |state, value| {
                        crate::path::set(state, &field, Some(tree::number_value(value)));
                    },
                    cx,
                );
            }
            Turned::Input(input) => {
                if let ValueChange::Drag(value) | ValueChange::Set(value) = change {
                    let input = input.to_string();
                    self.control(&input, value, cx);
                }
            }
            // Its code works out its value, from what [`Self::tell_kept`] tells it.
            Turned::Kept => {}
        }
    }

    /// Tells the code of what it keeps how the drag of `node` or a key moved it on `axis` (0
    /// across, 1 up): the window measures the move, as for a field, and its code, which alone
    /// can read what it keeps, works out the value. A drag tells its last move once its code
    /// answered the one before, so a drag faster than its code skips moves and never queues
    /// them; with `now`, at once, as a key and the end of a drag do.
    fn tell_kept(&mut self, node: u64, axis: usize, moved: Value, now: bool, cx: &mut App) {
        if (self.turning.as_ref()).is_none_or(|turning| turning.node != node) {
            // What another control still waits to tell goes first.
            self.tell_turns(true, cx);
            self.turning = Some(Turning::of(node));
        }
        if let Some(turning) = &mut self.turning {
            turning.waiting[axis] = Some(moved);
        }
        self.tell_turns(now, cx);
    }

    /// Tells what [`Self::tell_kept`] still holds back, once the last one was answered or `now`.
    pub(super) fn tell_turns(&mut self, now: bool, cx: &mut App) {
        let Some(turning) = &mut self.turning else {
            return;
        };
        let (node, sent) = (turning.node, turning.sent);
        if !now && self.live.read(cx).events_answered(self.card) < sent {
            return;
        }
        let waiting = (self.turning.as_mut()).map(|turning| std::mem::take(&mut turning.waiting));
        for (axis, moved) in waiting.into_iter().flatten().enumerate() {
            let Some(Value::Object(mut moved)) = moved else {
                continue;
            };
            moved.insert("axis".into(), json!(if axis == 0 { "x" } else { "y" }));
            let sent = self.event(node, EventKind::Turn, Value::Object(moved), cx);
            if let Some(turning) = &mut self.turning {
                turning.sent = sent;
            }
        }
    }

    /// A wheel or a pinch scrolled or zoomed a space.
    /// The spaces it follows move with it, on their axes.
    pub(super) fn moved_space(
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

    /// The pointer is `over` the control of the space `key`, which `drag` turns, or not. What
    /// reads it draws again, and nothing does where nothing reads it.
    pub(super) fn point(&mut self, key: &str, drag: &Drag, over: bool, cx: &mut Context<Self>) {
        let here = self.pointed.first().is_some_and(|pointed| pointed == key);
        if over == here {
            return;
        }
        let parts = self.parts(cx);
        let reads = [&parts.body, &parts.title_bar, &parts.over].map(|uses| uses.touched);
        if reads.contains(&true) {
            self.pointed = if over { drag.control(key) } else { Vec::new() };
            self.draw_again(reads, cx);
        }
    }

    pub(super) fn hover(&mut self, hovered: Option<(String, u64)>, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
}

/// How what a drag turns spreads over its travel: as it says, or as the field or the input.
fn range_of(info: Option<&ToolInfo>, turns: &tree::Turns) -> Option<ValueRange> {
    if turns.fader {
        return Some(ValueRange {
            min: f32::NEG_INFINITY,
            max: 6.0,
            scale: sound_core::Scale::Fader,
        });
    }
    if let (Some(min), Some(max)) = (turns.min, turns.max) {
        return Some(crate::tools::knob_range(min, max, turns.unit));
    }
    match turns.target(info) {
        Turned::Field { child: None, name } => live::field_range(info, name),
        Turned::Input(name) => live::input_range(info, name),
        Turned::Field { .. } | Turned::Kept => None,
    }
}

/// How far a drag or a key moves what it turns.
#[derive(Clone, Copy)]
enum Moved {
    /// Along its travel, as a part of the whole.
    By(f32),
    /// Whole steps of its own.
    Steps(f32),
    /// To a place on its travel, wherever it was.
    To(f32),
}

impl Moved {
    /// What `turns` turns to from `from`, over `range`.
    fn value(
        self,
        turns: &tree::Turns,
        from: Option<f32>,
        range: Option<ValueRange>,
    ) -> Option<f32> {
        match self {
            Self::By(by) => {
                let (range, from) = range.zip(from)?;
                Some(range.value((range.position(from) + by).clamp(0.0, 1.0)))
            }
            Self::Steps(steps) => {
                let value = from? + steps * turns.step?;
                let value = turns.min.map_or(value, |min| value.max(min));
                Some(turns.max.map_or(value, |max| value.min(max)))
            }
            Self::To(place) => Some(range?.value(place)),
        }
    }

    /// What the code of what it keeps hears: how far it moved since the press of a drag that
    /// is `held`, else from where it is now, or the place it goes to.
    fn told(self, held: bool) -> Value {
        match self {
            Self::By(moved) | Self::Steps(moved) => json!({ "moved": moved, "held": held }),
            Self::To(place) => json!({ "at": place }),
        }
    }
}

/// A space the keys turn: the one number its drag turns, of the node that drag is on.
#[derive(Clone)]
pub(super) struct KeyTurned {
    pub(super) turns: tree::Turns,
    pub(super) node: u64,
    /// The axis of the drag it is on: 0 across, 1 up.
    pub(super) axis: usize,
}

/// What a control of what code keeps still has to tell its code, see
/// [`TypeScriptCard::tell_kept`].
pub(super) struct Turning {
    node: u64,
    /// The number of the last event that told a move.
    sent: u64,
    /// The last move across and up its code has not heard.
    waiting: [Option<Value>; 2],
}

impl Turning {
    fn of(node: u64) -> Self {
        Self {
            node,
            sent: 0,
            waiting: [None, None],
        }
    }
}

/// Which pointer events the code of a node hears.
#[derive(Clone, Copy, Default)]
pub(super) struct Hears {
    /// Its presses, also for a node that hears only their release: Bun keeps the release of a
    /// press for a node gone since.
    pub(super) press: bool,
    pub(super) release: bool,
    pub(super) click: bool,
}

impl Hears {
    pub(super) fn of(press: Option<bool>, release: Option<bool>, click: Option<bool>) -> Self {
        Self {
            press: press == Some(true) || release == Some(true),
            release: release == Some(true),
            click: click == Some(true),
        }
    }

    pub(super) fn any(self) -> bool {
        self.press || self.click
    }

    pub(super) fn unless(self, disabled: Option<bool>) -> Self {
        match disabled {
            Some(true) => Self::default(),
            _ => self,
        }
    }
}

/// A callback of an element that `card` hears, as `Context::listener` makes one.
pub(super) fn listener<E: ?Sized>(
    card: &WeakEntity<TypeScriptCard>,
    hear: impl Fn(&mut TypeScriptCard, &E, &mut Window, &mut Context<TypeScriptCard>) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    let card = card.clone();
    move |event, window, cx| {
        card.update(cx, |view, cx| hear(view, event, window, cx))
            .ok();
    }
}

/// `element` takes the keys of the page of `card` on `focus`, and hands them to its code.
pub(super) fn hearing_keys(
    element: Div,
    focus: &FocusHandle,
    context: Option<gpui::KeyContext>,
    card: &WeakEntity<TypeScriptCard>,
) -> Div {
    element
        .track_focus(focus)
        .when_some(context, |element, context| element.key_context(context))
        .on_key_down(listener(card, |view, event: &KeyDownEvent, window, cx| {
            // A text field in it has the keys first.
            if !typing(window) {
                view.key(&event.keystroke, true, event.is_held, cx)
            }
        }))
        .on_key_up(listener(card, |view, event: &KeyUpEvent, _, cx| {
            view.key(&event.keystroke, false, false, cx)
        }))
        // macOS sends no key up while cmd is held.
        .on_modifiers_changed(listener(
            card,
            |view, event: &ModifiersChangedEvent, _, cx| {
                if event.modifiers.platform {
                    view.let_go_of_keys(cx);
                }
            },
        ))
}

enum Dropped {
    File(String),
    Timelines(Vec<Value>),
}

/// The timelines of the parts of the MIDI file at `path`, or what went wrong: `None` when it is
/// not a MIDI file.
fn timelines_in(path: &std::path::Path) -> Option<Result<Vec<Value>, String>> {
    let mut start = [0; 4];
    let mut file = std::fs::File::open(path).ok()?;
    std::io::Read::read_exact(&mut file, &mut start).ok()?;
    if !sound_notes::midi_file::is_midi_file(&start) {
        return None;
    }
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_default();
    let not_imported = |error: String| format!("{name} was not imported. {error}");
    let read = std::fs::read(path).map_err(|error| not_imported(error.to_string()));
    let parts = read.and_then(|bytes| {
        sound_notes::midi_file::read(&bytes, &name).map_err(|error| not_imported(error.to_string()))
    });
    Some(parts.and_then(|parts| {
        (parts.iter())
            .map(|(title, clip)| crate::timeline_state(clip, title))
            .collect()
    }))
}

/// What the code of a node that hears drops is told: see `Dropped` in `sdk/live.ts`.
pub(super) fn dropped(
    files: &[String],
    timelines: &[Value],
    carried: Option<&str>,
    (x, y): (f64, f64),
) -> Value {
    json!({ "files": files, "timelines": timelines, "carried": carried, "x": x, "y": y })
}
