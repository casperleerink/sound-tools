//! The card of an instance of a tool of the project, in a rack, or its page, the whole window:
//! the nodes its code made. What moves on it moves here, with no code: a space scrolls and
//! zooms, a drag moves what binds to it and turns what it turns, and live values follow the
//! playhead, the watches and the record. Code hears a press, and a drag once, at its end.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, Hitbox, HitboxBehavior, Hsla, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PinchEvent, Pixels, Point, ScrollWheelEvent, SharedString, StyleRefinement,
    TextAlign, TextRun, WeakEntity, Window, canvas, div, fill, outline, point, prelude::*, px,
    size,
};
use serde_json::{Value, json};
use sound_core::{InstanceId, ProjectEvent, ValueRange};
use sound_media::AudioAsset;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::device_card::{CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MENU_WIDTH, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::icon::Icon;
use sound_ui::components::knob::{
    decibels_readout, hertz_readout, milliseconds_readout, pan_readout, percent_readout, short,
};
use sound_ui::components::meter::Meter;
use sound_ui::components::text_input::{self, TextInput};
use sound_ui::components::toggle::Toggle;
use sound_ui::{
    ActiveTheme, ControlEdit, Devices, Metering, Session, Views, Waveforms, typography,
};

use crate::bun::{EventKind, PageSize};
use crate::feeds::Feeds;
use crate::live::{self, DragNumbers, Sources, SpaceView};
use crate::tools::{ToolInfo, Unit};
use crate::tree::{
    self, Align, Axis, ButtonLook, ButtonProps, ButtonSized, CardProps, Cursor, DivProps, Drag,
    IconProps, InputProps, List, MeterProps, NodeTable, Paint, Props, Reach, Readout, ShapeKind,
    SpaceProps, TextProps, ToggleProps, Turned, Weight, Words,
};
use crate::window::OpenedMenu;
use crate::window::{Live, Surface, state_of};

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
    /// What its live values read from the engine and the clock, once a frame.
    feeds: Feeds,
    /// The shapes of each space, by its key, with where those that stay are.
    indexes: RefCell<HashMap<String, SpaceIndex>>,
    /// The menu its code opens where the pointer is, and the question of the open one.
    menu: Option<(Entity<DropdownMenu>, Option<u64>)>,
    /// The text fields of its nodes, by node, with the text the field last showed or told.
    inputs: HashMap<u64, (Entity<TextInput>, String)>,
    /// The cards of other instances it shows, by node.
    embedded: HashMap<u64, Embedded>,
    /// The div it shows in the window's title bar, by its slot and index, as last drawn.
    title_bar: Option<(usize, Vec<usize>)>,
    /// How each meter node falls and holds its peak, by node.
    meters: Rc<RefCell<HashMap<u64, Metering>>>,
    /// Where each node that hears a press was last laid out, by node, so a menu its press opens
    /// hangs under it.
    placed: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    /// Where the next menu opens: under the div, button or title last pressed with the left
    /// button. `None` opens it at the pointer, as after a press of a space or a right press.
    anchor: Rc<Cell<Option<Point<Pixels>>>>,
}

/// The view of the card of `instance` that the page `page` shows, for tests that read what a
/// device's card holds.
pub fn embedded_view(page: &AnyView, instance: &InstanceId, cx: &App) -> Option<AnyView> {
    let page = page.clone().downcast::<Page>().ok()?;
    let embedded = page.read(cx).0.read(cx).embedded.values();
    embedded
        .filter(|card| card.instance == *instance)
        .find_map(|card| card.view.clone())
}

/// One frame of the tools of the project of the page `page`, as its timer runs them every
/// 33 ms: for snapshots, whose headless window runs no timer.
pub fn run_frame(page: &AnyView, cx: &mut App) {
    if let Ok(page) = page.clone().downcast::<Page>() {
        let live = page.read(cx).0.read(cx).live.clone();
        live.update(cx, |live, cx| live.frame(cx));
    }
}

/// How many nodes the window keeps for the page `page`, for tests that a list leaves none behind.
#[cfg(feature = "test-support")]
pub fn node_count(page: &AnyView, cx: &App) -> usize {
    let Ok(page) = page.clone().downcast::<Page>() else {
        return 0;
    };
    let card = page.read(cx).0.read(cx);
    match card.live.read(cx).tree(card.card) {
        Some(Ok(table)) => table.entries().count(),
        _ => 0,
    }
}

/// What the page `page` shows in the window's title bar, see [`TitleBar`]. `None` for a view that
/// is no page of a tool of the project.
pub fn title_bar_of(page: &AnyView, cx: &mut App) -> Option<Entity<TitleBar>> {
    let card = page.clone().downcast::<Page>().ok()?.read(cx).0.clone();
    Some(cx.new(|cx| TitleBar::new(card, cx)))
}

/// The card of another instance, as a node shows it.
struct Embedded {
    instance: InstanceId,
    tool: String,
    /// Whether its power icon and its close icon show, and whether its title is a menu.
    icons: (bool, bool, bool),
    on: Rc<Cell<bool>>,
    frame: CardFrame,
    view: Option<AnyView>,
}

/// The title of an embedded card.
/// The title of an embedded card, as the window's cards have theirs: 14 pt medium, and with a
/// menu, a chevron after it and the wash of the pointer over it.
struct CardTitle {
    name: SharedString,
    /// For tests, which find it as `card-title-<instance>`.
    instance: String,
    on_press: Option<Rc<dyn Fn(Option<Bounds<Pixels>>, &mut App)>>,
    /// Where it was last laid out, so the menu of a press hangs under it.
    placed: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Render for CardTitle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (hover, muted) = (theme.alpha_at(0.08), theme.gray_700);
        let title = div()
            .id("card-title")
            .debug_selector({
                let instance = self.instance.clone();
                move || format!("card-title-{instance}")
            })
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(4.))
            .h(px(24.))
            .pl(px(8.))
            .pr(px(6.))
            .rounded(px(6.))
            .text_size(px(14.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.gray_950)
            .child(div().min_w_0().truncate().child(self.name.clone()));
        match self.on_press.clone() {
            Some(on_press) => {
                let placed = self.placed.clone();
                let mark = self.placed.clone();
                // The border a menu's trigger keeps for its focus ring, so the name sits where
                // the window's pickers put it.
                title
                    .relative()
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .child(Icon::new("chevron-down").size(12.).color(muted))
                    .child(
                        canvas(move |bounds, _, _| mark.set(Some(bounds)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_click(move |_, _, cx| on_press(placed.get(), cx))
            }
            None => title,
        }
    }
}

/// What a drag of a node carries, by its `carry`.
#[derive(Clone)]
struct Carried(String);

/// Where a menu hangs under a node laid out at `bounds`: from its bottom left, as a select's list.
fn under(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(
        bounds.origin.x,
        bounds.origin.y + bounds.size.height + px(6.),
    )
}

/// Takes no room and writes where its parent, the node `id`, was laid out into `placed`.
fn placed(id: u64, placed: &Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>) -> impl IntoElement {
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
    /// The next menu hangs under the node `id`, as a select's list does, when it was laid out.
    fn hang_menus_under(&self, id: u64) {
        self.anchor
            .set(self.placed.borrow().get(&id).copied().map(under));
    }

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
        // A seek while stopped moves the playhead with no frame to draw it.
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |view, playhead, cx| {
            let uses = view.live.read(cx).uses(view.card);
            if uses.tick && !playhead.read(cx).playing {
                cx.notify();
            }
        })
        .detach();
        cx.observe_in(&live, window, |view, _, window, cx| {
            view.sync(window, cx);
            cx.notify();
        })
        .detach();
        // A waveform it draws is made in the background.
        let waveforms = Waveforms::entity(cx);
        cx.observe(&waveforms, |view, _, cx| {
            if !view.live.read(cx).uses(view.card).files.is_empty() {
                cx.notify();
            }
        })
        .detach();
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
            feeds: Feeds::default(),
            indexes: RefCell::default(),
            menu: None,
            inputs: HashMap::new(),
            embedded: HashMap::new(),
            title_bar: None,
            meters: Rc::default(),
            placed: Rc::default(),
            anchor: Rc::default(),
        }
    }

    /// Makes what its nodes show that is an entity of the window: a text field, the card of
    /// another instance, and a menu its code opened. Outside a draw, which may not make them.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let card = self.card;
        if let Some(menu) = self.live.update(cx, |live, _| live.take_menu(card)) {
            self.open_menu(menu, window, cx);
        }
        let Some(Ok(table)) = self.live.read(cx).tree(card).cloned() else {
            return;
        };
        let mut inputs = HashMap::new();
        let mut embedded = HashMap::new();
        for entry in table.entries() {
            match &entry.props {
                Props::Input(props) => {
                    let input = self.input(entry.id, props, window, cx);
                    inputs.insert(entry.id, input);
                }
                Props::Card(props) => {
                    if let Some(card) = self.embed(entry.id, props, window, cx) {
                        embedded.insert(entry.id, card);
                    }
                }
                _ => {}
            }
        }
        self.inputs = inputs;
        self.embedded = embedded;
    }

    /// The text field of the node `node`: the one it had, showing `value` when that changed and
    /// it is not typed in, or a new one.
    fn input(
        &mut self,
        node: u64,
        props: &InputProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<TextInput>, String) {
        let value = props.value.clone().unwrap_or_default();
        if let Some((input, shown)) = self.inputs.remove(&node) {
            if shown != value && !input.focus_handle(cx).is_focused(window) {
                input.update(cx, |input, cx| input.set_text(value.clone(), cx));
                return (input, value);
            }
            return (input, shown);
        }
        let placeholder = props.placeholder.clone().unwrap_or_default();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(placeholder));
        let card = cx.weak_entity();
        input.update(cx, |input, cx| {
            input.set_text(value.clone(), cx);
            input.set_on_submit({
                let card = card.clone();
                move |text, _, cx| {
                    card.update(cx, |view, cx| view.commit(node, text, true, cx)).ok();
                }
            });
            // Escape shows the text it had again, once the field's own update is over.
            input.set_on_cancel(move |_, window, cx| {
                let card = card.clone();
                window.defer(cx, move |window, cx| {
                    card.update(cx, |view, cx| view.revert(node, window, cx))
                        .ok();
                });
            });
        });
        let focus = input.focus_handle(cx);
        // What was typed counts when the field loses the keys too.
        cx.on_blur(&focus, window, move |view, _, cx| {
            if let Some(text) = view
                .inputs
                .get(&node)
                .map(|(input, _)| input.read(cx).text().to_string())
            {
                view.commit(node, &text, false, cx);
            }
        })
        .detach();
        if props.focus == Some(true) {
            window.focus(&focus, cx);
            input.update(cx, TextInput::select_all_text);
        }
        (input, value)
    }

    /// Tells the code of the field `node` its text: on enter always, so a field the code shows
    /// to type in closes with its text as it was; on blur unless it shows what it last told.
    fn commit(&mut self, node: u64, text: &str, entered: bool, cx: &mut Context<Self>) {
        let Some((_, shown)) = self.inputs.get_mut(&node) else {
            return;
        };
        if shown == text && !entered {
            return;
        }
        *shown = text.to_string();
        self.event(node, EventKind::Commit, json!({ "text": text }), cx);
    }

    /// Shows in the field `node` what it last showed or told, and gives the keys back.
    fn revert(&mut self, node: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, shown)) = self.inputs.get(&node) else {
            return;
        };
        let shown = shown.clone();
        input.update(cx, |input, cx| input.set_text(shown, cx));
        window.focus(&self.focus, cx);
    }

    /// The card of the instance a node shows: the one it had, or a new one when the instance,
    /// its tool or the icons changed.
    fn embed(
        &mut self,
        node: u64,
        props: &CardProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Embedded> {
        let path = props.instance.as_deref()?;
        let instance = (path.split('/')).try_fold(self.id.clone(), |id, name| id.child(name));
        let instance = instance.ok()?;
        let tool = self
            .session
            .read(cx)
            .project()
            .tool_of(&instance)?
            .to_string();
        let on = props.on.unwrap_or(true);
        let icons = (
            props.on_power == Some(true),
            props.on_close == Some(true),
            props.on_title == Some(true),
        );
        if let Some(old) = self.embedded.remove(&node)
            && old.instance == instance
            && old.tool == tool
            && old.icons == icons
        {
            old.on.set(on);
            return Some(old);
        }
        let name = (Devices::label_of(&self.session, &instance, cx))
            .map_or_else(|| SharedString::from(tool.clone()), |label| label.name);
        let on_press = icons.2.then(|| {
            let card = cx.weak_entity();
            Rc::new(move |bounds: Option<Bounds<Pixels>>, cx: &mut App| {
                card.update(cx, |view, cx| {
                    view.anchor.set(bounds.map(under));
                    view.event(node, EventKind::Title, json!({}), cx)
                })
                .ok();
            }) as Rc<dyn Fn(Option<Bounds<Pixels>>, &mut App)>
        });
        let instance_name = instance.as_str().to_string();
        let title = cx.new(|_| CardTitle {
            name,
            instance: instance_name,
            on_press,
            placed: Rc::default(),
        });
        let mut frame = CardFrame::new(format!("card-{}", instance.as_str()), title);
        let shown = Rc::new(Cell::new(on));
        if icons.0 {
            let (card, shown) = (cx.weak_entity(), shown.clone());
            frame = frame.power(
                move |_| shown.get(),
                move |_, cx| {
                    card.update(cx, |view, cx| {
                        view.event(node, EventKind::Power, json!({}), cx);
                    })
                    .ok();
                },
            );
        }
        if icons.1 {
            let card = cx.weak_entity();
            frame = frame.close(move |_, cx| {
                card.update(cx, |view, cx| {
                    view.event(node, EventKind::Close, json!({}), cx);
                })
                .ok();
            });
        }
        let view = Views::card_of(&self.session, &instance, frame.clone(), window, cx);
        Some(Embedded {
            instance,
            tool,
            icons,
            on: shown,
            frame,
            view,
        })
    }

    /// Opens a menu its code asked for: under the div, button or title pressed last, or where
    /// the pointer is. A menu still open answers that nothing was picked.
    fn open_menu(&mut self, menu: OpenedMenu, window: &mut Window, cx: &mut Context<Self>) {
        let OpenedMenu {
            question,
            items,
            notes,
            width,
            max_height,
            search,
        } = menu;
        // A group a run of items of one `group`, under its heading; `None` a separator.
        let mut entries: Vec<(Option<String>, Option<MenuGroup>)> = Vec::new();
        for (index, spec) in items.into_iter().enumerate() {
            let mut item = MenuItem::new(index.to_string(), spec.label)
                .disabled(spec.disabled)
                .checked(spec.checked);
            if let Some(icon) = spec.icon {
                item = item.icon(icon);
            }
            if let Some(detail) = spec.detail {
                item = item.description(detail);
            }
            if let Some(keys) = spec.shortcut {
                item = item.shortcut(shortcut_of(&keys));
            }
            if spec.separator {
                entries.push((None, None));
            }
            match entries.last_mut() {
                Some((group, Some(items))) if *group == spec.group => {
                    *items = std::mem::take(items).item(item);
                }
                _ => {
                    let heading = spec.group.clone();
                    let group = MenuGroup::new().item(item);
                    let group = match &heading {
                        Some(heading) => group.label(heading.clone()),
                        None => group,
                    };
                    entries.push((heading, Some(group)));
                }
            }
        }
        let entries: Vec<MenuEntry> = (entries.into_iter())
            .map(|(_, group)| group.map_or(MenuEntry::Separator, MenuEntry::Group))
            .chain(notes.into_iter().map(|note| MenuEntry::Note(note.into())))
            .collect();
        // A menu with a search field is another menu than one without.
        let cached = self.menu.take();
        let cached = match cached {
            Some((menu, waiting)) if menu.read(cx).is_searchable() != search => {
                if let Some(waiting) = waiting {
                    self.live
                        .update(cx, |live, _| live.menu_picked(waiting, None));
                }
                None
            }
            cached => cached,
        };
        let menu = match cached {
            Some((menu, waiting)) => {
                if let Some(waiting) = waiting {
                    self.live
                        .update(cx, |live, _| live.menu_picked(waiting, None));
                }
                menu
            }
            None => {
                let menu = cx.new(|cx| {
                    let menu = DropdownMenu::new("", Vec::new(), cx)
                        .trigger(Trigger::Pointer)
                        .debug_name("menu");
                    match search {
                        true => menu.searchable("Search", cx),
                        false => menu,
                    }
                });
                cx.subscribe(&menu, |view, _, picked: &MenuPicked, cx| {
                    let item = picked.0.parse().ok();
                    view.answer_menu(item, cx);
                })
                .detach();
                // Closed with nothing picked: after a pick's event, which comes after its close.
                cx.observe(&menu, |_, menu, cx| {
                    if !menu.read(cx).is_open() {
                        let card = cx.entity().downgrade();
                        cx.defer(move |cx| {
                            card.update(cx, |view, cx| view.answer_menu(None, cx)).ok();
                        });
                    }
                })
                .detach();
                menu
            }
        };
        let under = self.anchor.take();
        menu.update(cx, |menu, cx| {
            menu.set_entries(entries, cx);
            menu.set_size(width.unwrap_or(MENU_WIDTH), max_height);
            match under {
                Some(position) => menu.open_under(position, window, cx),
                None => menu.open_at(window.mouse_position(), window, cx),
            }
        });
        self.menu = Some((menu, Some(question)));
    }

    /// Answers the open menu's question, once.
    fn answer_menu(&mut self, item: Option<usize>, cx: &mut Context<Self>) {
        let Some(question) = self.menu.as_mut().and_then(|(_, waiting)| waiting.take()) else {
            return;
        };
        self.live
            .update(cx, |live, _| live.menu_picked(question, item));
    }

    /// Copies dropped files into `assets/audio/`, in the background, and tells the code of
    /// `node` their names there and where they fell.
    fn drop_files(
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
        if down && keystroke.key == "escape" && self.cancel_drag(cx) {
            cx.stop_propagation();
            return;
        }
        let modifiers = keystroke.modifiers;
        let chord = modifiers.platform || modifiers.control || modifiers.alt || modifiers.function;
        let key = keystroke.key.clone();
        // A tool that names its keys leaves the rest to the window, such as space to play. A key
        // it took comes up here all the same.
        let takes = (self.live.read(cx).info(self.card)).and_then(|info| info.takes);
        if let Some(takes) = takes
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
    fn let_go_of_keys(&mut self, cx: &mut App) {
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
            self.anchor.set(None);
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
            feeds: &self.feeds,
        };
        let from = turns.map(|turns| {
            let turns = turns.as_ref()?;
            match turns.target(info.as_ref()) {
                Turned::Field {
                    child: Some(child),
                    name,
                } => {
                    let child = self.id.child(child).ok()?;
                    let record = state_of(self.session.read(cx).project(), &child)?;
                    let value = record
                        .get(name)
                        .and_then(Value::as_f64)
                        .map(|value| value as f32);
                    value.or_else(|| self.child_range(turns, cx).map(|range| range.value(0.5)))
                }
                Turned::Field { child: None, name } => sources.field(name),
                Turned::Input(name) => sources.input(name),
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
            if held.drag.on_click {
                self.event(held.node, EventKind::Click, held.result(), cx);
            }
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
    /// hears nothing.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(held) = self.held.take_if(|held| held.ended.is_none()) else {
            return false;
        };
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

    fn turn(&mut self, turns: &tree::Turns, change: ValueChange<f32>, cx: &mut Context<Self>) {
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
                    |state, value| tree::set_field(state, &field, tree::decimal(value)),
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
    /// The box of the groups around it, outside which it does not show.
    clip: Option<Area>,
}

/// A box in the units of a space: left, top, right and bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area([f64; 4]);

impl Area {
    /// The part of it inside `other` as well, when there is one.
    fn within(self, other: Option<Area>) -> Area {
        let Some(Area([left, top, right, bottom])) = other else {
            return self;
        };
        let [l, t, r, b] = self.0;
        Area([l.max(left), t.max(top), r.min(right), b.min(bottom)])
    }

    fn meets(self, extent: &Extent) -> bool {
        let [left, top, right, bottom] = self.0;
        let (across, down) = (extent.across, extent.down);
        across.0.min(across.1) <= right
            && across.0.max(across.1) >= left
            && down.0.min(down.1) <= bottom
            && down.0.max(down.1) >= top
    }
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
        /// Its parts by their length, with their own colour and weight; none for one part.
        parts: Rc<[TextPart]>,
        size: f32,
        align: Align,
    },
    Wave {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        wave: Rc<Wave>,
    },
    Path {
        points: Rc<[(f64, f64)]>,
    },
}

impl Geometry {
    /// Where it starts: its top left, its centre, or where a line starts.
    fn origin(&self) -> (f64, f64) {
        match *self {
            Self::Rect { x, y, .. }
            | Self::Circle { x, y, .. }
            | Self::Arc { x, y, .. }
            | Self::Text { x, y, .. }
            | Self::Wave { x, y, .. } => (x, y),
            Self::Line { from, .. } => from,
            Self::Path { ref points } => points.first().copied().unwrap_or_default(),
        }
    }
}

/// The values a wave draws, oldest first, between `low` at its bottom and `high` at its top.
struct Wave {
    values: Vec<f32>,
    low: f32,
    high: f32,
    band: bool,
    line: f32,
    /// What the values are multiplied by along it, as `(fraction across, factor)` in order.
    level: Vec<(f32, f32)>,
}

impl Wave {
    /// What the values are multiplied by `fraction` across it: straight between the points of
    /// its level, and as the nearest one past its ends.
    fn level_at(&self, fraction: f32) -> f32 {
        let (Some(first), Some(last)) = (self.level.first(), self.level.last()) else {
            return 1.0;
        };
        if fraction <= first.0 {
            return first.1;
        }
        for pair in self.level.windows(2) {
            let [(from, start), (to, end)] = [pair[0], pair[1]];
            if fraction <= to {
                let along = if to > from {
                    (fraction - from) / (to - from)
                } else {
                    1.0
                };
                return start + (end - start) * along;
            }
        }
        last.1
    }
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
/// `view` zoomed and scrolled so `fit` spans its width, as far as `zoom` goes.
fn fitted(mut view: SpaceView, fit: tree::Fit, zoom: Option<[f32; 2]>) -> SpaceView {
    let [from, to] = fit.x.map(f64::from);
    let room = (view.width - f64::from(fit.before + fit.after)).max(1.0);
    let wanted = room / ((to - from).max(f64::EPSILON) * view.unit_x);
    view.zoom_x = match zoom {
        Some([low, high]) => wanted.clamp(f64::from(low), f64::from(high)),
        None => wanted,
    };
    view.left = from - f64::from(fit.before) / view.scale().0;
    view.fitted = true;
    view
}

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
    /// What of the window it shows in, inside the box of its groups.
    clip: Option<Bounds<Pixels>>,
}

/// A part of a label's line, by its length in bytes.
#[derive(Clone, Debug)]
struct TextPart {
    len: usize,
    color: Option<Hsla>,
    weight: Option<Weight>,
    border: Option<Hsla>,
}

enum PaintedShape {
    Rect(Bounds<Pixels>, f32, Option<(Hsla, f32)>),
    Circle(Point<Pixels>, f32),
    Line(Point<Pixels>, Point<Pixels>, f32),
    Arc(Point<Pixels>, f32, (f32, f32), f32),
    Text(Point<Pixels>, SharedString, Rc<[TextPart]>, f32, Align),
    Wave(Bounds<Pixels>, Rc<Wave>),
    Path(Rc<[Point<Pixels>]>),
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
            Self::Rect(bounds, ..) | Self::Wave(bounds, _) => *bounds,
            Self::Path(points) => {
                let (mut low, mut high) = (
                    point(px(f32::MAX), px(f32::MAX)),
                    point(px(f32::MIN), px(f32::MIN)),
                );
                for each in points.iter() {
                    low = point(low.x.min(each.x), low.y.min(each.y));
                    high = point(high.x.max(each.x), high.y.max(each.y));
                }
                match points.is_empty() {
                    true => Bounds::default(),
                    false => Bounds::from_corners(low, high),
                }
            }
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
            Self::Text(origin, text, _, font, align) => {
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

    /// Where GPUI paints its primitives among those of the same order: quads, paths, text.
    fn batch_rank(&self) -> u8 {
        match self {
            Self::Rect(..) | Self::Circle(..) => 0,
            Self::Line(..) | Self::Arc(..) | Self::Wave(..) | Self::Path(..) => 1,
            Self::Text(..) => 2,
        }
    }

    /// Whether GPUI paints it as a filled path: a band of a wave, or a path.
    fn is_filled_path(&self) -> bool {
        match self {
            Self::Wave(_, wave) => wave.band,
            Self::Path(_) => true,
            _ => false,
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
    feeds: &'a Feeds,
    indexes: &'a RefCell<HashMap<String, SpaceIndex>>,
    inputs: &'a HashMap<u64, (Entity<TextInput>, String)>,
    embedded: &'a HashMap<u64, Embedded>,
    meters: &'a RefCell<HashMap<u64, Metering>>,
    placed: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    /// Whether it draws the title bar's div, which the body leaves out.
    in_title_bar: bool,
    /// The title bar's div the body left out.
    title_bar: Option<(usize, Vec<usize>)>,
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
            feeds: self.feeds,
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
            Props::Space(space) => self.space(id, space, &entry.children, index),
            // A shape outside a space, or a marker, draws nothing.
            Props::Shape(_) | Props::Marker => div().into_any_element(),
            Props::Input(_) => match self.inputs.get(&id) {
                Some((input, _)) => {
                    let width = match &entry.props {
                        Props::Input(InputProps {
                            width: Some(width), ..
                        }) => *width,
                        _ => 160.0,
                    };
                    div().w(px(width)).child(input.clone()).into_any_element()
                }
                None => div().into_any_element(),
            },
            Props::Button(props) => self.button(id, props, index),
            Props::Toggle(props) => self.toggle(id, props, index),
            Props::Meter(MeterProps {
                of,
                input,
                horizontal,
                length,
                ..
            }) => {
                let amplitudes = match (of, input) {
                    // One channel shows on both bars.
                    (_, Some(channels)) => {
                        let level = |at: usize| {
                            let channel = channels.get(at).or(channels.first());
                            channel
                                .and_then(|channel| self.feeds.input.get(*channel))
                                .copied()
                        };
                        [level(0).unwrap_or(0.0), level(1).unwrap_or(0.0)]
                    }
                    (Some(name), None) => self.feeds.peaks.get(name).copied().unwrap_or_default(),
                    (None, None) => self.feeds.output,
                };
                let mut meters = self.meters.borrow_mut();
                let metering = meters.entry(id).or_default();
                metering.read_amplitudes(amplitudes);
                let card = self.cx.entity().downgrade();
                let meter = Meter::new(ElementId::Integer(id), metering.level()).on_clear_clip(
                    move |_, cx| {
                        card.update(cx, |card, cx| {
                            if let Some(metering) = card.meters.borrow_mut().get_mut(&id) {
                                metering.clear_clip();
                            }
                            cx.notify();
                        })
                        .ok();
                    },
                );
                let meter = if *horizontal == Some(true) {
                    meter.horizontal()
                } else {
                    meter
                };
                match length {
                    Some(length) => meter.length(*length).into_any_element(),
                    None => meter.into_any_element(),
                }
            }
            Props::Icon(IconProps { name, size, color }) => {
                let color = color.map_or(self.cx.theme().gray_950, |color| color.0);
                Icon::new(name.clone().unwrap_or_default())
                    .size(size.unwrap_or(16.0))
                    .color(color)
                    .into_any_element()
            }
            Props::Card(_) => match self.embedded.get(&id) {
                Some(Embedded {
                    view: Some(view), ..
                }) => view.clone().into_any_element(),
                // A tool with no card shows its frame with its name.
                Some(Embedded { frame, .. }) => frame.card().into_any_element(),
                None => div().into_any_element(),
            },
            Props::Div(DivProps {
                key,
                style,
                on_press,
                follow,
                carry,
                on_drop,
                title_bar,
            }) => {
                if *title_bar == Some(true) && !std::mem::take(&mut self.in_title_bar) {
                    self.title_bar = Some((slot, index.to_vec()));
                    return div().into_any_element();
                }
                let style = style.clone().unwrap_or_default();
                let shown: Vec<usize> = (entry.children.iter())
                    .filter(|child| {
                        !matches!(
                            table.get(**child).map(|child| &child.props),
                            Some(Props::Marker)
                        )
                    })
                    .copied()
                    .collect();
                // Rows of fixed heights that follow a space down are drawn only where they show:
                // the heads of a hundred tracks cost what the dozen on screen cost.
                let rows = (follow.as_ref().and_then(|follow| follow.y.as_ref()))
                    .and_then(|key| self.spaces.get(key))
                    .zip(style.stacks_down())
                    .and_then(|(view, gap)| {
                        let heights: Option<Vec<f32>> = (shown.iter())
                            .map(|child| match table.get(*child).map(|child| &child.props) {
                                Some(Props::Div(child)) => {
                                    child.style.as_ref().and_then(tree::Style::fixed_height)
                                }
                                _ => None,
                            })
                            .collect();
                        let top = (view.top * view.scale().1) as f32;
                        Some((heights?, gap, top..top + view.height as f32))
                    });
                let mut children: Vec<AnyElement> = Vec::new();
                let mut hidden = None::<f32>;
                let mut at = 0.0;
                for (position, child) in shown.iter().enumerate() {
                    if let Some((heights, gap, showing)) = &rows {
                        let height = heights.get(position).copied().unwrap_or_default();
                        let shows = at < showing.end && at + height > showing.start;
                        at += height + gap;
                        if !shows {
                            // Their room, gaps between them included, as one spacer.
                            hidden = Some(hidden.map_or(height, |room| room + gap + height));
                            continue;
                        }
                    }
                    if let Some(room) = hidden.take() {
                        children.push(div().flex_none().h(px(room)).into_any_element());
                    }
                    children.push(self.draw(*child, &[index, &[position]].concat()));
                }
                if let Some(room) = hidden {
                    children.push(div().flex_none().h(px(room)).into_any_element());
                }
                let element = match follow {
                    // Its children move with the scroll of the spaces it follows, cut off at its
                    // edges: the heads of the rows of a timeline that scrolls. They lie over it,
                    // so how tall they are never makes it taller: its style says its size.
                    Some(follow) => {
                        let scrolled = |key: &Option<String>, along: fn(&SpaceView) -> f64| {
                            key.as_ref()
                                .and_then(|key| self.spaces.get(key))
                                .map_or(0.0, along) as f32
                        };
                        let dx = scrolled(&follow.x, |view| view.left * view.scale().0);
                        let dy = scrolled(&follow.y, |view| view.top * view.scale().1);
                        let inside = style
                            .lines_up(div())
                            .absolute()
                            .left(px(-dx))
                            .top(px(-dy))
                            .when(follow.x.is_none(), |inside| inside.w_full())
                            .when(follow.y.is_none(), |inside| inside.h_full())
                            .children(children);
                        (style.apply(div()).relative().overflow_hidden()).child(inside)
                    }
                    None => style.apply(div()).children(children),
                };
                // Tests find a div by its key.
                let element = match key {
                    Some(key) => {
                        let selector = format!("div-{key}");
                        element.debug_selector(move || selector)
                    }
                    None => element,
                };
                let hears = *on_press == Some(true) || *on_drop == Some(true) || carry.is_some();
                if let Some(axis) = style.scroll() {
                    // The box it scrolls in has its size; it is as large as its children.
                    let name = element_name("scroll", key.as_deref(), index);
                    let inner = match hears {
                        true => self.heard_div(
                            element,
                            id,
                            key.as_deref(),
                            index,
                            on_press,
                            carry,
                            on_drop,
                        ),
                        false => element.into_any_element(),
                    };
                    let scrolling = style.sized(div()).id(ElementId::Name(name.into()));
                    let scrolling = match axis {
                        tree::ScrollAxis::X => scrolling.overflow_x_scroll(),
                        tree::ScrollAxis::Y => scrolling.overflow_y_scroll(),
                    };
                    return scrolling.child(inner).into_any_element();
                }
                if !hears {
                    return element.into_any_element();
                }
                self.heard_div(element, id, key.as_deref(), index, on_press, carry, on_drop)
            }
        }
    }

    /// A div that hears presses, drops or carries something when dragged.
    #[allow(clippy::too_many_arguments)]
    /// A switch of the app, which tells a press as a pressed div does: its code turns it.
    fn toggle(&mut self, id: u64, props: &ToggleProps, index: &[usize]) -> AnyElement {
        let name: SharedString = element_name("toggle", props.key.as_deref(), index).into();
        let on = props.on == Some(true);
        let toggle = match (&props.icon, props.dot == Some(true)) {
            (Some(icon), _) => Toggle::icon(
                name,
                icon.clone(),
                props.label.clone().unwrap_or_default(),
                on,
            )
            .count(props.count.unwrap_or_default().max(0.0) as usize),
            (None, true) => Toggle::dot(name, on),
            (None, false) => Toggle::new(name, props.label.clone().unwrap_or_default(), on),
        };
        let selector = format!("toggle-{}", props.key.as_deref().unwrap_or_default());
        let toggle = toggle
            .when_some(props.color, |toggle, color| toggle.color(color.0))
            .disabled(props.disabled == Some(true))
            .tab_stop(false)
            .when(props.on_press == Some(true), |toggle| {
                let card = self.cx.entity().downgrade();
                toggle.on_change(move |_, _, cx| {
                    let press = json!({ "x": 0, "y": 0, "button": "left", "clicks": 1 });
                    (card.update(cx, |view, cx| view.event(id, EventKind::Press, press, cx))).ok();
                })
            });
        // A click on it is its own, not the row's under it.
        (div().flex_none().occlude().debug_selector(move || selector))
            .child(toggle)
            .into_any_element()
    }

    /// A button of the app, which tells a press as a pressed div does.
    fn button(&mut self, id: u64, props: &ButtonProps, index: &[usize]) -> AnyElement {
        let name = element_name("button", props.key.as_deref(), index);
        let accent = props.color.map(|color| color.0);
        let variant = match (props.variant, accent) {
            (Some(ButtonLook::Subtle), Some(accent)) => ButtonVariant::SubtleColor(accent),
            (Some(ButtonLook::Ghost), Some(accent)) => ButtonVariant::GhostColor(accent),
            (Some(ButtonLook::Solid), accent) => {
                ButtonVariant::Solid(accent.unwrap_or(self.cx.theme().lavender))
            }
            (Some(ButtonLook::Subtle), None) => ButtonVariant::Subtle,
            (Some(ButtonLook::Ghost), None) => ButtonVariant::Ghost,
            (Some(ButtonLook::Primary) | None, _) => ButtonVariant::Primary,
        };
        let size = match props.size {
            Some(ButtonSized::Xs) => ButtonSize::Xs,
            Some(ButtonSized::Sm) => ButtonSize::Sm,
            Some(ButtonSized::Md) | None => ButtonSize::Md,
        };
        let button = match (&props.label, &props.icon) {
            (Some(label), icon) => Button::new(ElementId::Name(name.into()), label.clone())
                .when_some(icon.clone(), |button, icon| button.icon(icon)),
            (None, icon) => Button::icon_only(
                ElementId::Name(name.into()),
                icon.clone().unwrap_or_default(),
            ),
        };
        let selector = format!("button-{}", props.key.as_deref().unwrap_or_default());
        let button = button
            .variant(variant)
            .size(size)
            .rounded(props.rounded == Some(true))
            .disabled(props.disabled == Some(true))
            .when_some(props.opacity, |button, opacity| button.opacity(opacity))
            .debug_selector(move || selector)
            .when(props.on_press == Some(true), |button| {
                button.on_click(
                    self.cx
                        .listener(move |view, event: &gpui::ClickEvent, _, cx| {
                            view.hang_menus_under(id);
                            let mut press = json!({
                                "x": 0, "y": 0, "button": "left", "clicks": event.click_count(),
                            });
                            add_modifiers(&mut press, event.modifiers());
                            view.event(id, EventKind::Press, press, cx);
                        }),
                )
            });
        (div().flex_none().relative())
            .child(button)
            .when(props.on_press == Some(true), |wrap| {
                wrap.child(placed(id, &self.placed))
            })
            .into_any_element()
    }

    fn heard_div(
        &mut self,
        element: Div,
        id: u64,
        key: Option<&str>,
        index: &[usize],
        on_press: &Option<bool>,
        carry: &Option<String>,
        on_drop: &Option<bool>,
    ) -> AnyElement {
        {
            {
                let lavender = self.cx.theme().lavender;
                let element = element
                    .id(ElementId::Name(element_name("div", key, index).into()))
                    .when(*on_press == Some(true), |element| {
                        element
                            .cursor_pointer()
                            .on_click((self.cx).listener(
                                move |view, event: &gpui::ClickEvent, _, cx| {
                                    view.hang_menus_under(id);
                                    let mut press = json!({
                                        "x": 0, "y": 0, "button": "left",
                                        "clicks": event.click_count(),
                                    });
                                    add_modifiers(&mut press, event.modifiers());
                                    view.event(id, EventKind::Press, press, cx);
                                },
                            ))
                            .child(placed(id, &self.placed))
                            .on_mouse_down(
                                MouseButton::Right,
                                (self.cx).listener(move |view, event: &MouseDownEvent, _, cx| {
                                    view.anchor.set(None);
                                    let mut press = json!({
                                        "x": 0, "y": 0, "button": "right",
                                        "clicks": event.click_count,
                                    });
                                    add_modifiers(&mut press, event.modifiers);
                                    view.event(id, EventKind::Press, press, cx);
                                    cx.stop_propagation();
                                }),
                            )
                    });
                let element = match carry {
                    Some(carry) => element
                        .on_drag(Carried(carry.clone()), |carried: &Carried, _, _, cx| {
                            cx.new(|_| carried.clone())
                        }),
                    None => element,
                };
                if *on_drop != Some(true) {
                    return element.into_any_element();
                }
                element
                    .drag_over::<ExternalPaths>(move |style, _, _, _| {
                        style.bg(lavender.opacity(0.2))
                    })
                    .drag_over::<Carried>(move |style, _, _, _| style.bg(lavender.opacity(0.2)))
                    .on_drop(
                        (self.cx).listener(move |view, paths: &ExternalPaths, _, cx| {
                            view.drop_files(id, paths.paths().to_vec(), (0.0, 0.0), cx);
                        }),
                    )
                    .on_drop((self.cx).listener(move |view, carried: &Carried, _, cx| {
                        let dropped = json!({ "files": [], "carried": carried.0, "x": 0, "y": 0 });
                        view.event(id, EventKind::Drop, dropped, cx);
                    }))
                    .into_any_element()
            }
        }
    }

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
        let mut indexes = self.indexes.borrow_mut();
        let version = self.table.version();
        let index = match indexes.get(key) {
            Some(index) if index.version == version => index,
            _ => {
                let index = placing.index(children, version);
                indexes.insert(key.to_string(), index);
                &indexes[key]
            }
        };
        placing.indexed(index);
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
        // Fitted once it has a width, which the paint of its first draw gives; the spaces that
        // follow it see that in the draw after.
        let fit = space.fit.filter(|_| !view.fitted);
        if let Some(fit) = fit
            && view.width > 0.0
        {
            view = fitted(view, fit, space.zoom.unwrap_or_default().x);
        }
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
        // Where it was painted, for where a drop falls in its units.
        let painted_at = Rc::new(Cell::new(None));
        let element = canvas(
            {
                let frame = frame.clone();
                move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                    prepaint(&frame, bounds, window)
                }
            },
            {
                let (frame, painted_at) = (frame.clone(), painted_at.clone());
                move |bounds: Bounds<Pixels>,
                      (hitbox, painted): (Hitbox, Rc<Vec<Painted>>),
                      window: &mut Window,
                      cx: &mut App| {
                    painted_at.set(Some(bounds));
                    if fit.is_some() {
                        window.request_animation_frame();
                    }
                    // The size it was laid out at, for live values of what shows and the next scroll.
                    if let Some(view) = spaces.borrow_mut().get_mut(&frame.key) {
                        view.width = f64::from(f32::from(bounds.size.width));
                        view.height = f64::from(f32::from(bounds.size.height));
                    }
                    paint(&frame, bounds, &hitbox, &painted, window, cx);
                    listen(&frame, &card, bounds, hitbox, painted, window);
                }
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
        if space.on_drop != Some(true) {
            return holder.into_any_element();
        }
        let element_id = ElementId::Name(format!("{}-drop", frame.key).into());
        let units = move |window: &Window| match painted_at.get() {
            Some(bounds) => frame.units_at(bounds, window.mouse_position()),
            None => (0.0, 0.0),
        };
        let lavender = self.cx.theme().lavender;
        let dropped_files = units.clone();
        holder
            .id(element_id)
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(lavender.opacity(0.1)))
            .on_drop(
                (self.cx).listener(move |view, paths: &ExternalPaths, window, cx| {
                    let at = dropped_files(window);
                    view.drop_files(id, paths.paths().to_vec(), at, cx);
                }),
            )
            .on_drop(
                (self.cx).listener(move |view, carried: &Carried, window, cx| {
                    let (x, y) = units(window);
                    let dropped = json!({ "files": [], "carried": carried.0, "x": x, "y": y });
                    view.event(id, EventKind::Drop, dropped, cx);
                }),
            )
            .into_any_element()
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
    fn reaches(
        &self,
        (left, right): (f64, f64),
        (top, bottom): (f64, f64),
        reach: (f32, f32),
    ) -> bool {
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
        let Extent {
            across,
            down,
            reach,
        } = Extent::of(geometry);
        self.reaches(across, down, reach)
    }
}

/// Where a shape is in the units of its space, and how many points it reaches further each
/// way, as a shape sized in points does.
#[derive(Clone, Copy)]
struct Extent {
    across: (f64, f64),
    down: (f64, f64),
    reach: (f32, f32),
}

impl Extent {
    fn of(geometry: &Geometry) -> Self {
        let extent = |across, down, reach| Self {
            across,
            down,
            reach,
        };
        match *geometry {
            Geometry::Rect {
                x,
                y,
                width,
                height,
                ..
            }
            | Geometry::Wave {
                x,
                y,
                width,
                height,
                ..
            } => extent((x, x + width), (y, y + height), (1.0, 1.0)),
            Geometry::Circle { x, y, radius } | Geometry::Arc { x, y, radius, .. } => {
                extent((x, x), (y, y), (radius + 2.0, radius + 2.0))
            }
            Geometry::Line { from, to, width } => {
                extent((from.0, to.0), (from.1, to.1), (width, width))
            }
            Geometry::Path { ref points } => {
                let fold = |pick: fn(&(f64, f64)) -> f64| {
                    (points.iter().map(pick))
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                            (low.min(value), high.max(value))
                        })
                };
                extent(fold(|point| point.0), fold(|point| point.1), (1.0, 1.0))
            }
            // A line of text is as wide as its words: a long label far off to the left may
            // still reach in.
            Geometry::Text { x, y, size, .. } => extent((x, x), (y, y), (size * 40.0, size * 2.0)),
        }
    }
}

/// The shapes of a space in the order they paint, kept from one batch of ops to the next. A
/// frame works out only the shapes that move and those of the rest that show, so a long piece
/// costs what shows.
pub(crate) struct SpaceIndex {
    version: u64,
    shapes: Vec<Indexed>,
}

enum Indexed {
    /// A shape that stays where it is, at `offset` from the groups around it, inside `clip`.
    Stays {
        slot: usize,
        offset: (f64, f64),
        clip: Option<Area>,
        extent: Extent,
    },
    /// A shape that moves, or one in a group that does: worked out on every frame, from
    /// `offset` and `clip` and the groups in `groups` that move.
    Moves {
        slot: usize,
        offset: (f64, f64),
        clip: Option<Area>,
        groups: Rc<[usize]>,
    },
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
    fn paint(&self, paint: &Paint) -> Hsla {
        match paint {
            Paint::Fixed(color) => color.0,
            Paint::Mix { from, to, by } => {
                let by = self.sources.number(by).clamp(0.0, 1.0) as f32;
                let (from, to) = (from.0.to_rgb(), to.0.to_rgb());
                let mix = |from: f32, to: f32| from + (to - from) * by;
                gpui::Rgba {
                    r: mix(from.r, to.r),
                    g: mix(from.g, to.g),
                    b: mix(from.b, to.b),
                    a: mix(from.a, to.a),
                }
                .into()
            }
        }
    }

    /// The values of a wave, oldest first.
    fn list(&self, list: Option<&List>) -> Vec<f32> {
        let feeds = self.sources.feeds;
        match list {
            Some(List::Scope {
                name,
                channel,
                frames,
            }) => {
                let Some((kept, _)) = feeds.scopes.get(name) else {
                    return Vec::new();
                };
                let count = frames.map_or(kept.len(), |frames| frames as usize);
                let skip = kept.len().saturating_sub(count);
                let pick = |frame: &[f32; 2]| match channel {
                    Some(0) => frame[0],
                    Some(_) => frame[1],
                    None => (frame[0] + frame[1]) / 2.0,
                };
                kept.iter().skip(skip).map(pick).collect()
            }
            Some(List::History { of, frames, key }) => {
                feeds.history(key, *frames, self.sources.number(of))
            }
            Some(List::File { asset, from, to }) => feeds.wave(asset, *from, *to).to_vec(),
            Some(List::Take { name }) => (feeds.takes.get(name))
                .map(|wave| wave.to_vec())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn number(&self, number: &Option<tree::Num>) -> f64 {
        number
            .as_ref()
            .map_or(0.0, |number| self.sources.number(number))
    }

    /// The shapes under `children`, with where those that stay are.
    fn index(&self, children: &[usize], version: u64) -> SpaceIndex {
        let mut shapes = Vec::with_capacity(children.len());
        self.index_into(children, ((0.0, 0.0), None), &Rc::from([]), &mut shapes);
        SpaceIndex { version, shapes }
    }

    fn index_into(
        &self,
        children: &[usize],
        (offset, clip): ((f64, f64), Option<Area>),
        groups: &Rc<[usize]>,
        into: &mut Vec<Indexed>,
    ) {
        let table = self.table;
        for slot in children {
            let Some(entry) = table.get(*slot) else {
                continue;
            };
            let Props::Shape(shape) = &entry.props else {
                continue;
            };
            if shape.kind == Some(ShapeKind::Group) {
                if groups.is_empty() && shape.group_stays() {
                    let placed = self.group(shape, (offset, clip));
                    self.index_into(&entry.children, placed, groups, into);
                } else {
                    let groups: Rc<[usize]> = groups.iter().copied().chain([*slot]).collect();
                    self.index_into(&entry.children, (offset, clip), &groups, into);
                }
            } else if groups.is_empty() && shape.stays() {
                let Some(geometry) = self.geometry(shape, offset) else {
                    continue;
                };
                into.push(Indexed::Stays {
                    slot: *slot,
                    offset,
                    clip,
                    extent: Extent::of(&geometry),
                });
            } else {
                into.push(Indexed::Moves {
                    slot: *slot,
                    offset,
                    clip,
                    groups: groups.clone(),
                });
            }
        }
    }

    /// Where a group puts its shapes, from where its own group put it: moved by its `x` and
    /// `y` and the drags it moves with, inside its box when it has one.
    fn group(
        &self,
        group: &tree::ShapeProps,
        ((left, top), clip): ((f64, f64), Option<Area>),
    ) -> ((f64, f64), Option<Area>) {
        let (moved_x, moved_y) = self.followed(&group.moves);
        let left = left + self.number(&group.x) + moved_x;
        let top = top + self.number(&group.y) + moved_y;
        let clip = match &group.clip {
            Some(tree::Clip { width, height }) => {
                let (grown_x, grown_y) = self.followed(&group.grows);
                let right = left + self.sources.number(width) + grown_x;
                let bottom = top + self.sources.number(height) + grown_y;
                Some(Area([left, top, right, bottom]).within(clip))
            }
            None => clip,
        };
        ((left, top), clip)
    }

    /// Places the shapes of `index` that move, and those of the rest that show.
    fn indexed(&mut self, index: &SpaceIndex) {
        let table = self.table;
        for indexed in &index.shapes {
            let (slot, placed) = match indexed {
                Indexed::Stays {
                    slot,
                    offset,
                    clip,
                    extent,
                } => {
                    let shows = self.shown.as_ref().is_none_or(|shown| {
                        shown.reaches(extent.across, extent.down, extent.reach)
                    }) && clip.is_none_or(|clip| clip.meets(extent));
                    if !shows {
                        continue;
                    }
                    (*slot, (*offset, *clip))
                }
                Indexed::Moves {
                    slot,
                    offset,
                    clip,
                    groups,
                } => {
                    // A shape that moves inside the box of a group that stays shows only when
                    // that box does: the handle at the end of a clip far off.
                    let boxed_off = |shown: &Shown| {
                        clip.is_some_and(|Area([left, top, right, bottom])| {
                            !shown.reaches((left, right), (top, bottom), (0.0, 0.0))
                        })
                    };
                    if groups.is_empty() && self.shown.as_ref().is_some_and(boxed_off) {
                        continue;
                    }
                    let mut placed = (*offset, *clip);
                    for group in groups.iter() {
                        if let Some(Props::Shape(group)) =
                            table.get(*group).map(|entry| &entry.props)
                        {
                            placed = self.group(group, placed);
                        }
                    }
                    (*slot, placed)
                }
            };
            if let Some(entry) = table.get(slot)
                && let Props::Shape(shape) = &entry.props
            {
                self.shape(entry.id, shape, placed);
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

    fn shape(
        &mut self,
        id: u64,
        shape: &tree::ShapeProps,
        ((left, top), clip): ((f64, f64), Option<Area>),
    ) {
        let index = self.pointed.len();
        let hovered = self.hovered == Some(id);
        let (moved_x, moved_y) = self.followed(&shape.moves);
        let (left, top) = (left + moved_x, top + moved_y);
        let Some(geometry) = self.geometry(shape, (left, top)) else {
            return;
        };
        let extent = Extent::of(&geometry);
        let hidden = self
            .shown
            .as_ref()
            .is_some_and(|shown| !shown.shows(&geometry))
            || clip.is_some_and(|clip| !clip.meets(&extent));
        if hidden {
            return;
        }
        let origin = geometry.origin();
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
        let mut color = match (hovered.then_some(shape.hover).flatten(), &shape.color) {
            (Some(hover), _) => hover.0,
            (None, Some(paint)) => self.paint(paint),
            (None, None) => gpui::white(),
        };
        color.a *= opacity;
        self.placed.push(Placed {
            index,
            geometry,
            color,
            clip,
        });
    }

    /// Where `shape` is and how big, at `left` and `top` in its space; `None` for a group.
    fn geometry(&self, shape: &tree::ShapeProps, (left, top): (f64, f64)) -> Option<Geometry> {
        let (grown_x, grown_y) = self.followed(&shape.grows);
        let (shrunk_x, shrunk_y) = self.followed(&shape.shrinks);
        let (grown_x, grown_y) = (grown_x - shrunk_x, grown_y - shrunk_y);
        let reach = |reach: &Option<Reach>| match reach {
            Some(Reach::Point([x, y])) => {
                (left + self.sources.number(x), top + self.sources.number(y))
            }
            Some(Reach::Turn(turn)) => (self.sources.number(turn), 0.0),
            None => (left, top),
        };
        let (x, y) = (left + self.number(&shape.x), top + self.number(&shape.y));
        let geometry = match shape.kind? {
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
                    Some(Words::Parts(parts)) => (parts.iter())
                        .map(|part| part.text.as_str())
                        .collect::<String>()
                        .into(),
                    Some(Words::Live(live)) => {
                        let value = self.sources.number(live);
                        text_of(value, shape.unit, self.sources.feeds.clock.as_ref()).into()
                    }
                    None => SharedString::default(),
                },
                parts: match &shape.text {
                    Some(Words::Parts(parts)) => (parts.iter())
                        .map(|part| TextPart {
                            len: part.text.len(),
                            color: part.color.as_ref().map(|color| color.0),
                            weight: part.weight,
                            border: part.border.as_ref().map(|color| color.0),
                        })
                        .collect(),
                    _ => Rc::new([]),
                },
                size: shape.size.unwrap_or(12.0),
                align: shape.align.unwrap_or(Align::Left),
            },
            ShapeKind::Wave => {
                let width = self.number(&shape.width);
                let level = (shape.level.iter().flatten())
                    .map(|[along, factor]| {
                        let along = self.sources.number(along) / width.max(f64::MIN_POSITIVE);
                        (along as f32, self.sources.number(factor) as f32)
                    })
                    .collect();
                Geometry::Wave {
                    x,
                    y,
                    width,
                    height: self.number(&shape.height),
                    wave: Rc::new(Wave {
                        values: self.list(shape.list.as_ref()),
                        low: shape.low.unwrap_or(-1.0),
                        high: shape.high.unwrap_or(1.0),
                        band: shape.band == Some(true),
                        line: shape.size.unwrap_or(1.0),
                        level,
                    }),
                }
            }
            ShapeKind::Path => Geometry::Path {
                points: (shape.points.iter().flatten())
                    .map(|[x, y]| (left + self.sources.number(x), top + self.sources.number(y)))
                    .collect(),
            },
            ShapeKind::Group => return None,
        };
        Some(geometry)
    }
}

/// An id for an element, from its key, else from where it is in the tree.
/// Whether a text field has the keys.
fn typing(window: &Window) -> bool {
    (window.context_stack().iter()).any(|context| context.contains(text_input::KEY_CONTEXT))
}

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

/// A live number as text shows it: a tick as a place in the piece (`5.3`), as minutes and seconds
/// (`1:07`), or as the time signature there (`7/8`).
fn text_of(value: f64, shown: Option<Readout>, clock: Option<&sound_core::Clock>) -> String {
    let unit = |unit| readout(value as f32, Some(unit));
    let tick = sound_core::Ticks(value.max(0.0) as u64);
    let signatures = clock.map(|clock| clock.tempo_map().time_signatures());
    match shown {
        None => readout(value as f32, None),
        Some(Readout::Hz) => unit(Unit::Hz),
        Some(Readout::Ms) => unit(Unit::Ms),
        Some(Readout::Db) => unit(Unit::Db),
        Some(Readout::Percent) => unit(Unit::Percent),
        Some(Readout::Pan) => pan_readout(value as f32),
        Some(Readout::Position) => signatures.map_or_else(String::new, |signatures| {
            let position = signatures.bar_beat_of(tick);
            format!("{}.{}", position.bar, position.beat)
        }),
        Some(Readout::Time) => clock.map_or_else(String::new, |clock| {
            let seconds = clock.seconds_of(tick).max(0.0) as u64;
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }),
        Some(Readout::Signature) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.to_string()
        }),
        Some(Readout::Beats) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.numerator().to_string()
        }),
        Some(Readout::Note) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.denominator().to_string()
        }),
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
                parts,
                size,
                align,
            } => PaintedShape::Text(at((*x, *y)), text.clone(), parts.clone(), *size, *align),
            Geometry::Wave {
                x,
                y,
                width,
                height,
                wave,
            } => {
                let size = size(px((width * across) as f32), px((height * down) as f32));
                PaintedShape::Wave(Bounds::new(at((*x, *y)), size), wave.clone())
            }
            Geometry::Path { points } => {
                PaintedShape::Path(points.iter().map(|each| at(*each)).collect())
            }
        };
        let clip = (placed.clip).map(|Area([left, top, right, bottom])| {
            Bounds::from_corners(at((left, top)), at((right, bottom))).intersect(&bounds)
        });
        let shown = clip.unwrap_or(bounds).intersect(&shape.bounds());
        // What does not show is not painted and not found.
        if shown.is_empty() {
            continue;
        }
        let cursor = (frame.pointed.get(placed.index))
            .and_then(Option::as_ref)
            .and_then(|pointer| pointer.cursor);
        let hitbox =
            cursor.map(|cursor| (window.insert_hitbox(shown, HitboxBehavior::Normal), cursor));
        painted.push(Painted {
            index: placed.index,
            shape,
            color: placed.color,
            hitbox,
            clip,
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
        // GPUI orders each primitive by what it overlaps, which costs most of a frame of many
        // shapes. Inside a layer they share one order and paint as their kinds come: quads, then
        // paths, then text. So a run of shapes whose kinds come in that order is one layer, and a
        // shape of an earlier kind starts the next one, which paints over the run before.
        // A filled path ends its layer too: GPUI draws the paths of one layer over each other's
        // boxes, so a path over another one would show the one under it twice.
        let mut runs: Vec<&[Painted]> = Vec::new();
        let mut start = 0;
        let mut latest = 0;
        let mut filled = false;
        for (index, each) in painted.iter().enumerate() {
            let rank = each.shape.batch_rank();
            if rank < latest || filled {
                runs.extend(painted.get(start..index));
                start = index;
            }
            latest = rank;
            filled = each.shape.is_filled_path();
        }
        runs.extend(painted.get(start..));
        for run in runs {
            window.paint_layer(bounds, |window| {
                for each in run {
                    // A shape with no colour is there for the pointer, such as the handle at the
                    // end of a note.
                    if each.color.a > 0.0 {
                        match each.clip {
                            Some(clip) => window.with_content_mask(
                                Some(gpui::ContentMask { bounds: clip }),
                                |window| paint_shape(&each.shape, each.color, window, cx),
                            ),
                            None => paint_shape(&each.shape, each.color, window, cx),
                        }
                    }
                    if let Some((hitbox, cursor)) = &each.hitbox {
                        window.set_cursor_style(cursor.style(), hitbox);
                    }
                }
            });
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
        PaintedShape::Wave(bounds, wave) => paint_wave(*bounds, wave, color, window),
        PaintedShape::Path(points) => {
            let mut path = gpui::PathBuilder::fill();
            for (index, each) in points.iter().enumerate() {
                match index {
                    0 => path.move_to(*each),
                    _ => path.line_to(*each),
                }
            }
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        PaintedShape::Text(origin, text, parts, font_size, align) => {
            let run = |len: usize, color: Hsla, weight: Option<Weight>| TextRun {
                len,
                font: gpui::Font {
                    weight: match weight {
                        None | Some(Weight::Normal) => gpui::FontWeight::NORMAL,
                        Some(Weight::Medium) => gpui::FontWeight::MEDIUM,
                        Some(Weight::Semibold) => gpui::FontWeight::SEMIBOLD,
                    },
                    ..typography::tabular()
                },
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs: Vec<TextRun> = match parts.is_empty() {
                true => vec![run(text.len(), color, None)],
                false => (parts.iter())
                    .filter(|part| part.len > 0)
                    .map(|part| run(part.len, part.color.unwrap_or(color), part.weight))
                    .collect(),
            };
            let shaped = window
                .text_system()
                .shape_line(text.clone(), px(*font_size), &runs, None);
            let left = match align {
                Align::Left => origin.x,
                Align::Center => origin.x - shaped.width / 2.,
                Align::Right => origin.x - shaped.width,
            };
            let line_height = px(font_size * 1.3);
            // Parts next to each other with one border share a box.
            let mut boxes: Vec<(usize, usize, Hsla)> = Vec::new();
            let mut start = 0;
            for part in parts.iter() {
                let end = start + part.len;
                match (part.border, boxes.last_mut()) {
                    (Some(border), Some(last)) if last.1 == start && last.2 == border => last.1 = end,
                    (Some(border), _) => boxes.push((start, end, border)),
                    (None, _) => {}
                }
                start = end;
            }
            for (start, end, border) in boxes {
                let (from, to) = (shaped.x_for_index(start), shaped.x_for_index(end));
                let bounds = Bounds::from_corners(
                    point(left + from - px(BOXED.0), origin.y - px(BOXED.1)),
                    point(left + to + px(BOXED.0), origin.y + line_height + px(BOXED.1)),
                );
                let back = border.alpha(border.a * 0.15);
                window.paint_quad(fill(bounds, back).corner_radii(px(4.)));
                let quad = outline(bounds, border, gpui::BorderStyle::Solid);
                window.paint_quad(quad.corner_radii(px(4.)).border_widths(px(1.)));
            }
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

/// Paints a wave across `bounds`: a line through one value per point, or a band from the lowest
/// to the highest value under each point.
fn paint_wave(bounds: Bounds<Pixels>, wave: &Wave, color: Hsla, window: &mut Window) {
    let values = &wave.values;
    let columns = (f32::from(bounds.size.width).round() as usize).clamp(2, 4096);
    if values.len() < 2 {
        return;
    }
    let span = wave.high - wave.low;
    let height = f32::from(bounds.size.height);
    let y_of = |value: f32| {
        let along = if span == 0.0 {
            0.5
        } else {
            (value - wave.low) / span
        };
        bounds.origin.y + px(height * (1.0 - along.clamp(0.0, 1.0)))
    };
    let x_of = |column: usize, of: usize| {
        bounds.origin.x + bounds.size.width * (column as f32 / (of - 1).max(1) as f32)
    };
    let columns = columns.min(values.len());
    // The values under each column.
    let under = |column: usize| {
        let start = column * values.len() / columns;
        let end = ((column + 1) * values.len() / columns).max(start + 1);
        values.get(start..end.min(values.len())).unwrap_or_default()
    };
    if wave.band {
        // One shape through the middle of each column, top edge then bottom edge back, so its
        // outline is smooth.
        let middle = (wave.low + wave.high) / 2.0;
        let edges: Vec<(Pixels, Pixels, Pixels)> = (0..columns)
            .map(|column| {
                let under = under(column);
                let fraction = (column as f32 + 0.5) / columns as f32;
                let factor = wave.level_at(fraction);
                let scaled = |value: f32| middle + (value - middle) * factor;
                let low = scaled(under.iter().copied().fold(f32::INFINITY, f32::min));
                let high = scaled(under.iter().copied().fold(f32::NEG_INFINITY, f32::max));
                let (top, bottom) = (y_of(high), y_of(low));
                // At least a point thick, so silence shows as a line.
                let thin = (px(1.0) - (bottom - top)).max(px(0.0)) / 2.0;
                let x = bounds.origin.x + bounds.size.width * fraction;
                (x, top - thin, bottom + thin)
            })
            .collect();
        let (Some(first), Some(last)) = (edges.first(), edges.last()) else {
            return;
        };
        let right = bounds.origin.x + bounds.size.width;
        let mut shape = gpui::PathBuilder::fill();
        shape.move_to(point(bounds.origin.x, first.1));
        for (x, top, _) in &edges {
            shape.line_to(point(*x, *top));
        }
        shape.line_to(point(right, last.1));
        shape.line_to(point(right, last.2));
        for (x, _, bottom) in edges.iter().rev() {
            shape.line_to(point(*x, *bottom));
        }
        shape.line_to(point(bounds.origin.x, first.2));
        shape.close();
        if let Ok(path) = shape.build() {
            window.paint_path(path, color);
        }
        return;
    }
    let mut path = gpui::PathBuilder::stroke(px(wave.line));
    let middle = (wave.low + wave.high) / 2.0;
    for column in 0..columns {
        let value = under(column).first().copied().unwrap_or_default();
        let factor = wave.level_at(column as f32 / (columns - 1).max(1) as f32);
        let value = middle + (value - middle) * factor;
        let at = point(x_of(column, columns), y_of(value));
        if column == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The topmost shape at `position` that hears the pointer: its place among the shapes.
fn found(frame: &SpaceFrame, painted: &[Painted], position: Point<Pixels>) -> Option<usize> {
    (painted.iter().rev())
        .filter(|each| frame.pointed.get(each.index).is_some_and(Option::is_some))
        .find(|each| {
            each.shape.contains(position) && each.clip.is_none_or(|clip| clip.contains(&position))
        })
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

impl TypeScriptCard {
    /// Scroll and zoom as drawn, so a wheel scrolls from there; sizes stay the paint's.
    fn keep_spaces(&self, spaces: HashMap<String, SpaceView>) {
        let mut kept = self.spaces.borrow_mut();
        for (key, view) in spaces {
            let size = kept.get(&key).map(|old| (old.width, old.height));
            let entry = kept.entry(key).or_insert(view);
            *entry = view;
            if let Some((width, height)) = size {
                (entry.width, entry.height) = (width, height);
            }
        }
    }

    /// The div its view shows in the window's title bar, with what the body last read.
    fn draw_title_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (slot, index) = self.title_bar.clone()?;
        let state = self.state(cx)?;
        let live = self.live.read(cx);
        let info = live.info(self.card);
        let Some(Ok(table)) = live.tree(self.card).cloned() else {
            return None;
        };
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let played = live.played_of(&self.id).cloned();
        let tick = self.session.read(cx).playhead().read(cx).tick.0 as f64;
        let drag =
            (self.held.as_ref()).and_then(|held| Some((held.drag.id.clone()?, held.numbers())));
        let mut drawing = Drawing {
            info,
            state,
            watches,
            played,
            tick,
            spaces: self.spaces.borrow().clone(),
            laid_out: self.spaces.clone(),
            drag,
            hovered: self.hovered.clone(),
            table: &table,
            feeds: &self.feeds,
            indexes: &self.indexes,
            inputs: &self.inputs,
            embedded: &self.embedded,
            meters: &self.meters,
            placed: self.placed.clone(),
            in_title_bar: true,
            title_bar: None,
            cx,
        };
        let bar = drawing.draw(slot, &index);
        let spaces = drawing.spaces;
        self.keep_spaces(spaces);
        let theme = cx.theme();
        Some(
            div()
                .text_size(px(12.))
                .text_color(theme.gray_950)
                .child(bar)
                .into_any_element(),
        )
    }
}

/// The div of a page's view that shows in the window's title bar moved, came or went.
pub(crate) struct TitleBarMoved;

impl EventEmitter<TitleBarMoved> for TypeScriptCard {}

/// What the view of a page shows in the window's title bar: its div that says `titleBar`.
pub struct TitleBar(Entity<TypeScriptCard>);

impl TitleBar {
    pub(crate) fn new(card: Entity<TypeScriptCard>, cx: &mut Context<Self>) -> Self {
        cx.observe(&card, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&card, |_, _, _: &TitleBarMoved, cx| cx.notify())
            .detach();
        Self(card)
    }

    /// Whether the view has a div for the title bar.
    pub fn shows(&self, cx: &App) -> bool {
        self.0.read(cx).title_bar.is_some()
    }
}

impl Render for TitleBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = self.0.update(cx, |card, cx| card.draw_title_bar(cx));
        // A click on it does not move the window, as the rest of the title row does.
        div().flex_none().occlude().children(bar)
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
        // What of the plain keys the computer keys leave to it: all, or those it names.
        let key_context = (info.as_ref()).map(|info| crate::key_context(info.takes.as_deref()));
        let tree = live.tree(self.card).cloned();
        let uses = live.uses(self.card);
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let played = live.played_of(&self.id).cloned();
        let playhead = *self.session.read(cx).playhead().read(cx);
        if (uses.tick && playhead.playing) || uses.animates {
            window.request_animation_frame();
        }
        self.feeds.files.retain(|file, _| uses.files.contains(file));
        let assets = self.session.read(cx).project().assets().clone();
        for file in &uses.files {
            let Ok(asset) = AudioAsset::new(file) else {
                continue;
            };
            // Made in the background; the cache tells the card when it is ready.
            if let Some(overview) = Waveforms::overview(&assets, &asset, cx) {
                self.feeds.files.insert(file.clone(), overview);
            }
        }
        let recording = self.session.read(cx).recording().read(cx);
        self.feeds.input = match uses.input {
            true => recording.input_levels().to_vec(),
            false => Vec::new(),
        };
        self.feeds.takes = (uses.takes.iter())
            .filter_map(|name| {
                let recorder = self.id.child(name).ok()?;
                let sound = &recording.take_of(&recorder)?.sound;
                Some((name.clone(), sound.overview.read(Feeds::take)))
            })
            .collect();
        let session = self.session.read(cx);
        (self.feeds).read(session, &self.id, &state, &uses, playhead.tick);
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
                    feeds: &self.feeds,
                    indexes: &self.indexes,
                    inputs: &self.inputs,
                    embedded: &self.embedded,
                    meters: &self.meters,
                    placed: self.placed.clone(),
                    in_title_bar: false,
                    title_bar: None,
                    cx,
                };
                let body = match table.root() {
                    Some(root) => drawing.draw(root, &[]),
                    None => div().into_any_element(),
                };
                let (spaces, title_bar) = (drawing.spaces, drawing.title_bar);
                if self.title_bar != title_bar {
                    self.title_bar = title_bar;
                    cx.emit(TitleBarMoved);
                }
                self.keep_spaces(spaces);
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
                    .when_some(key_context.clone(), |element, context| {
                        element.key_context(context)
                    })
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
                    .on_modifiers_changed(cx.listener(
                        |view, event: &ModifiersChangedEvent, _, cx| {
                            if event.modifiers.platform {
                                view.let_go_of_keys(cx);
                            }
                        },
                    ))
            })
        };
        let body = div()
            .text_size(px(12.))
            .text_color(text)
            .child(body)
            .children(self.menu.as_ref().map(|(menu, _)| menu.clone()));
        match &self.frame {
            // At least as wide as a plain card, so the title fits.
            Some(frame) => frame
                .card()
                .min_w(px(PLAIN_CARD_WIDTH))
                .child(takes_keys(body))
                .into_any_element(),
            None => {
                // The room the page draws in: the whole main area.
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
                    // A column, so a page whose top div grows fills the window and no more.
                    .child(body.size_full().flex().flex_col());
                takes_keys(div().size_full().bg(background).child(room)).into_any_element()
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

    #[test]
    fn a_wave_is_scaled_straight_between_the_points_of_its_level_and_as_the_nearest_past_them() {
        let wave = Wave {
            values: Vec::new(),
            low: -1.0,
            high: 1.0,
            band: true,
            line: 1.0,
            level: vec![(0.1, 0.0), (0.3, 0.5), (0.9, 0.5), (1.0, 0.0)],
        };
        let near =
            |at: f32, expected: f32| assert!((wave.level_at(at) - expected).abs() < 1e-5, "{at}");
        near(0.0, 0.0);
        near(0.2, 0.25);
        near(0.6, 0.5);
        near(0.95, 0.25);
        let plain = Wave {
            level: Vec::new(),
            ..wave
        };
        assert_eq!(plain.level_at(0.5), 1.0);
    }
}

/// How far a boxed part's box reaches past its text, across and down.
const BOXED: (f32, f32) = (5., 2.);

/// A key as the SDK names it, `cmd-alt-c`, as a menu draws it: `mod+alt+c`.
fn shortcut_of(mut keys: &str) -> String {
    let mut parts = Vec::new();
    for (prefix, drawn) in [("cmd-", "mod"), ("ctrl-", "ctrl"), ("alt-", "alt")] {
        if let Some(rest) = keys.strip_prefix(prefix) {
            parts.push(drawn);
            keys = rest;
        }
    }
    parts.push(keys);
    parts.join("+")
}
