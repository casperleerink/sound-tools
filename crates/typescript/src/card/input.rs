//! What the pointer and the keys do on a card: the presses, releases and clicks its code
//! hears, the drags that turn what they turn, hovers, drops and keys.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
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
use crate::live::{self, DragNumbers, Sources, SpaceView};
use crate::tools::ToolInfo;
use crate::tree::{self, Axis, Drag, Turned};
use crate::window::state_of;

use super::TypeScriptCard;
use super::drawing::typing;
use super::space::SpaceFrame;

/// Points of pointer travel for the whole range of what a drag turns, as a knob of the window.
const TRAVEL: f32 = 200.0;
/// With shift, a drag turns this much as far.
const FINE: f32 = 0.1;

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
            .text_color(theme.gray_950)
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

/// A drag the pointer holds, on a shape or a space.
#[derive(Clone)]
pub(super) struct Held {
    space: String,
    /// The node that hears it: the shape it holds, or the space.
    node: u64,
    /// Whether it holds a shape, whose drag it takes again as the shape changes.
    on_shape: bool,
    pub(super) drag: Drag,
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
        // Cmd held is off the grid in time, as the arrangement's drags were.
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

    /// Copies dropped files into `assets/audio/`, in the background, and tells the code of
    /// `node` their names there and where they fell.
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
                .map(|path| sound_media::import(&assets, path))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |view, cx| {
            let imported = imported.await;
            view.update(cx, |view, cx| {
                let mut files = Vec::new();
                for file in imported {
                    match file {
                        Ok(file) => files.push(file.asset.to_string()),
                        Err(error) => view
                            .session
                            .update(cx, |session, cx| session.report(error, cx)),
                    }
                }
                if !files.is_empty() {
                    let dropped = json!({ "files": files, "carried": null, "x": x, "y": y });
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
    /// window binds, such as cmd-z, is the window's and never comes here.
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
        let Some(info) = self.live.read(cx).info(self.card).filter(|info| info.keys) else {
            return;
        };
        let modifiers = keystroke.modifiers;
        let chord = modifiers.platform || modifiers.control || modifiers.alt || modifiers.function;
        let key = keystroke.key.clone();
        // A tool that names its keys leaves the rest to the window, such as space to play. A key
        // it took comes up here all the same.
        if let Some(takes) = info.takes
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
            for turns in turns.into_iter().flatten() {
                if let Some(default) = turns.default {
                    self.turn(turns, ValueChange::Set(default), cx);
                }
            }
            return;
        }
        let from = turns.map(|turns| self.turned_value(turns.as_ref()?, &state, info.as_ref(), cx));
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
            .any(|turns| matches!(turns.target(info.as_ref()), Turned::Field { .. }));
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
    /// hears no end and no click; the release still comes as the button goes up.
    pub(super) fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(held) = self.held.take_if(|held| held.ended.is_none()) else {
            return false;
        };
        self.clicking = None;
        let turns = [&held.drag.x, &held.drag.y];
        let info = self.live.read(cx).info(self.card);
        let target = |turns: &tree::Turns| turns.target(info.as_ref()).to_owned_input();
        if turns
            .iter()
            .copied()
            .flatten()
            .any(|turns| target(turns).is_none())
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
            if let (Some(input), Some(from)) = (turns.as_ref().and_then(target), from) {
                self.control(&input, from, cx);
            }
        }
        cx.notify();
        true
    }

    /// What `turns` turns is now.
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
                let child = self.id.child(child).ok()?;
                let record = state_of(self.session.read(cx).project(), &child)?;
                let value = record.get(name).and_then(tree::number_of);
                value.or_else(|| self.child_range(turns, cx).map(|range| range.value(0.5)))
            }
            target => {
                let played = self.live.read(cx).played_of(&self.id).cloned();
                let sources = Sources {
                    tick: 0.0,
                    watches: &BTreeMap::new(),
                    state,
                    info,
                    played: played.as_ref(),
                    drag: None,
                    spaces: &HashMap::new(),
                    hovered: None,
                    feeds: &self.feeds,
                };
                match target {
                    Turned::Input(name) => sources.input(name),
                    Turned::Field { name, .. } => sources.field(name),
                }
            }
        }
    }

    /// A key on the focused control of what `turns` turns, as a knob of the window takes it: an
    /// arrow steps it a fiftieth of its travel, a five-hundredth with shift, backspace puts it at
    /// its default. One undo step each. Whether the key was the control's: an arrow is, at an
    /// end too; backspace only with a default; escape is what holds it.
    pub(super) fn turn_by_key(
        &mut self,
        turns: &tree::Turns,
        key: ValueKey,
        cx: &mut Context<Self>,
    ) -> bool {
        let value = match key {
            ValueKey::Cancel => return false,
            ValueKey::Reset => match turns.default {
                Some(default) => Some(default),
                None => return false,
            },
            ValueKey::Step { up, fine } => {
                let by = if fine { FINE_KEY_STEP } else { KEY_STEP };
                let by = if up { by } else { -by };
                let (state, info) = self.record(cx);
                let range = range_of(info.as_ref(), turns).or_else(|| self.child_range(turns, cx));
                let now = self.turned_value(turns, &state, info.as_ref(), cx);
                (range.zip(now))
                    .map(|(range, now)| range.value((range.position(now) + by).clamp(0.0, 1.0)))
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
            let Some(range) =
                range_of(info.as_ref(), turns).or_else(|| self.child_range(turns, cx))
            else {
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
        match turns.target(info.as_ref()) {
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
                self.edit.apply_json(
                    &self.session,
                    &id,
                    &label,
                    change,
                    |state, value| tree::set_field(state, &field, tree::number_value(value)),
                    cx,
                );
            }
            Turned::Input(input) => {
                if let ValueChange::Drag(value) | ValueChange::Set(value) = change {
                    let input = input.to_string();
                    self.control(&input, value, cx);
                }
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

    pub(super) fn hover(&mut self, hovered: Option<(String, u64)>, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
}

/// How what a drag turns spreads over its travel: as it says, or as the field or the input.
fn range_of(info: Option<&ToolInfo>, turns: &tree::Turns) -> Option<ValueRange> {
    if let (Some(min), Some(max)) = (turns.min, turns.max) {
        return Some(crate::tools::knob_range(min, max, turns.unit));
    }
    match turns.target(info) {
        Turned::Field { child: None, name } => live::field_range(info, name),
        Turned::Input(name) => live::input_range(info, name),
        Turned::Field { .. } => None,
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

/// `element` takes the keys of the page of `cx`'s card on `focus`, and hands them to its code.
pub(super) fn hearing_keys(
    element: Div,
    focus: &FocusHandle,
    context: Option<gpui::KeyContext>,
    cx: &mut Context<TypeScriptCard>,
) -> Div {
    element
        .track_focus(focus)
        .when_some(context, |element, context| element.key_context(context))
        .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
            // A text field in it has the keys first.
            if !typing(window) {
                view.key(&event.keystroke, true, event.is_held, cx)
            }
        }))
        .on_key_up(cx.listener(|view, event: &KeyUpEvent, _, cx| {
            view.key(&event.keystroke, false, false, cx)
        }))
        // macOS sends no key up while cmd is held.
        .on_modifiers_changed(cx.listener(|view, event: &ModifiersChangedEvent, _, cx| {
            if event.modifiers.platform {
                view.let_go_of_keys(cx);
            }
        }))
}
