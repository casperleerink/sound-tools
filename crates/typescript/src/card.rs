//! The card of an instance of a tool of the project, in a rack, or its page, the whole window:
//! the nodes its code made. What moves on it moves here, with no code: a space scrolls and
//! zooms, a drag moves what binds to it and turns what it turns, and live values follow the
//! playhead, the watches and the record. Code hears a press, its release and a click, and a drag
//! once, at its end.

mod controls;
mod drawing;
mod input;
mod paint;
mod shapes;
mod space;
mod sync;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, Div, Entity, EventEmitter, FocusHandle, MouseButton,
    Pixels, Point, ScrollHandle, StyleRefinement, Task, WeakFocusHandle, Window, canvas, div,
    prelude::*, px,
};
use serde_json::Value;
use sound_core::{InstanceId, ProjectEvent};
use sound_media::AudioAsset;
use sound_ui::components::device_card::{CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::components::dropdown_menu::DropdownMenu;
use sound_ui::components::text_input::TextInput;
use sound_ui::renders::{self, Part};
use sound_ui::{ActiveTheme, ControlEdit, Metering, Session, Waveforms, every_poll};

use crate::bun::PageSize;
use crate::feeds::Feeds;
use crate::live::SpaceView;
use crate::tools::ToolInfo;
use crate::tree::{NodeTable, Props, Uses};
use crate::window::{CardsChanged, Live, Surface, state_of};

use self::drawing::Drawing;
use self::input::{Held, Hovers, hearing_keys, hears_the_button};
use self::shapes::SpaceIndex;
use self::sync::Embedded;

/// How long an ended drag keeps its values at most, when the answer to its end does not come,
/// such as when Bun stopped.
const ENDED_DRAG: Duration = Duration::from_secs(2);

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
    /// The presses whose release its code hears, by node, with their button and what the
    /// press told, which the release tells again.
    pressed: Vec<(u64, MouseButton, Value)>,
    /// The left press of the innermost node whose code hears a click, and what it told, until
    /// it clicks or a button goes down again.
    clicking: Option<(u64, Value)>,
    /// It has the keys while it, or a control in it, has the focus: a click on it gives it
    /// them, and a page takes them when it opens and nothing else has them.
    focus: FocusHandle,
    /// The keys of what it shows in the title bar, out of the page, which hands them to the page.
    title_focus: FocusHandle,
    /// The keys that went down and not up yet. They go up when it loses the keys, when cmd
    /// goes down and when it closes, so a note a key holds is not held for ever.
    keys_down: BTreeSet<String>,
    /// What its live values read from the engine and the clock, once a frame.
    feeds: Feeds,
    /// The shapes of each space, by its key, with where those that stay are.
    indexes: RefCell<HashMap<String, SpaceIndex>>,
    /// The menu its code opens where the pointer is, and the question of the open one.
    menu: Option<(Entity<DropdownMenu>, Option<u64>)>,
    /// The newest overlay when the menu opened, whose closing closes it.
    menu_overlay: Option<u64>,
    /// The text fields of its nodes, by node, with the text the field last showed or told.
    inputs: HashMap<u64, (Entity<TextInput>, String)>,
    /// The nodes of its open popovers and dialogs, the last opened last.
    overlays: Vec<u64>,
    /// What had the keys before the first overlay opened, which gets them back when the last
    /// one closes.
    focus_before_overlays: Option<WeakFocusHandle>,
    /// The cards of other instances it shows, by node.
    embedded: HashMap<u64, Embedded>,
    /// The div it shows in the window's title bar, by its slot and index, as last drawn.
    title_bar: Option<(usize, Vec<usize>)>,
    /// What its nodes read by where they draw, for the version of the nodes and the slot of the
    /// title bar it was worked out for.
    parts: RefCell<Option<((u64, Option<usize>), Rc<Parts>)>>,
    /// The spaces of a page the layer over it draws, by node, with where the page laid them out.
    lifted: Rc<RefCell<HashMap<u64, Lifted>>>,
    /// Where the page is in the window, which the layer over it draws from.
    origin: Rc<Cell<Point<Pixels>>>,
    /// The title bar draws again alone, and reads the feeds for its frame.
    title_reads: bool,
    /// How each meter node falls and holds its peak, by node.
    meters: Rc<RefCell<HashMap<u64, Metering>>>,
    /// Where each node that hears a press was last laid out, by node, so a menu its press opens
    /// hangs under it.
    placed: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    /// The divs that hear the pointer come over them.
    hovers: Rc<RefCell<Hovers>>,
    /// Where the next menu opens: under the div, button or title last pressed with the left
    /// button. `None` opens it at the pointer, as after a press of a space or a right press.
    anchor: Rc<Cell<Option<Point<Pixels>>>>,
    /// The scroll of each div that scrolls, by its place in the tree.
    scrolls: RefCell<HashMap<Vec<usize>, ScrollHandle>>,
    /// Where each embedded card in a div that scrolls was last laid out, by node, as if it
    /// were not scrolled.
    scrolled_cards: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    /// Whether a mouse button is down in the window. A card out of view is drawn then too, so
    /// a knob dragged while the rack scrolls hears its mouse up.
    button_down: Rc<Cell<bool>>,
    _polling: Task<()>,
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

/// How many nodes the window keeps for each card of a tool of the project, by instance, in the
/// window of the page `page`: for tests that a list leaves none behind, and for measurements.
#[cfg(feature = "test-support")]
pub fn node_counts(page: &AnyView, cx: &App) -> std::collections::BTreeMap<String, usize> {
    let Ok(page) = page.clone().downcast::<Page>() else {
        return Default::default();
    };
    let card = page.read(cx).0.read(cx);
    card.live.read(cx).node_counts()
}

/// Flushes pending watches, and checks that the window applied Bun's replies.
#[cfg(feature = "test-support")]
pub fn page_ready(page: &AnyView, cx: &mut App) -> bool {
    let Ok(page) = page.clone().downcast::<Page>() else {
        return true;
    };
    let live = page.read(cx).0.read(cx).live.clone();
    live.update(cx, |live, cx| live.flush(cx))
}

/// Whether the page has a text node that says `text`, for tests waiting for a view change.
#[cfg(feature = "test-support")]
pub fn page_has_text(page: &AnyView, text: &str, cx: &App) -> bool {
    let Ok(page) = page.clone().downcast::<Page>() else {
        return false;
    };
    let card = page.read(cx).0.read(cx);
    card.live.read(cx).tree(card.card).is_some_and(|tree| {
        tree.as_ref().is_ok_and(|table| {
            table.entries().any(|entry| {
                matches!(&entry.props, Props::Text(props) if props.text.as_deref() == Some(text))
            })
        })
    })
}

/// What the page `page` shows in the window's title bar, see [`TitleBar`]. `None` for a view that
/// is no page of a tool of the project.
pub fn title_bar_of(page: &AnyView, cx: &mut App) -> Option<Entity<TitleBar>> {
    let card = page.clone().downcast::<Page>().ok()?.read(cx).0.clone();
    Some(cx.new(|cx| TitleBar::new(card, cx)))
}

/// A page, which draws again only when it changed, while the window draws again with every
/// move of the playhead: the spaces that show the playhead draw on the layer over it.
pub(crate) struct Page(pub(crate) Entity<TypeScriptCard>, Entity<Over>);

impl Page {
    pub(crate) fn new(card: Entity<TypeScriptCard>, cx: &mut Context<Self>) -> Self {
        let over = cx.new(|cx| Over::new(card.clone(), cx));
        Self(card, over)
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let over = StyleRefinement::default().absolute().top_0().left_0();
        div()
            .relative()
            .size_full()
            .child(
                self.0
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .child(self.1.clone().cached(over.size_full()))
    }
}

/// The spaces of a page that show the playhead, drawn over the page where it laid them out.
/// While nothing else moves, they draw again and the page does not.
pub(crate) struct Over(Entity<TypeScriptCard>);

impl Over {
    fn new(card: Entity<TypeScriptCard>, cx: &mut Context<Self>) -> Self {
        cx.observe(&card, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&card, |_, _, _: &OverChanged, cx| cx.notify())
            .detach();
        Self(card)
    }
}

impl Render for Over {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _rendering = renders::start(Part::Over, self.0.read(cx).id.as_str());
        let spaces = self.0.update(cx, |card, cx| card.draw_over(window, cx));
        div().size_full().children(spaces)
    }
}

/// Only what the spaces over a page show changed.
pub(crate) struct OverChanged;

impl EventEmitter<OverChanged> for TypeScriptCard {}

/// What the nodes of a card read, by where they draw.
#[derive(Default)]
pub(crate) struct Parts {
    /// Those the card draws itself.
    body: Uses,
    title_bar: Uses,
    /// The spaces of a page that show the playhead, which the layer over it draws, together.
    over: Uses,
    lifted: HashSet<u64>,
}

/// A space the layer over a page draws: its slot and place in the tree, and where the page laid
/// it out and cut it off, in the window.
#[derive(Clone)]
pub(crate) struct Lifted {
    slot: usize,
    index: Vec<usize>,
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
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
        let title_focus = cx.focus_handle();
        let hears_keys = live.read(cx).info(card).is_some_and(|info| info.keys);
        // Not from what has the focus, such as the composer of the agent while it is typed in.
        if page && hears_keys && window.focused(cx).is_none() {
            window.focus(&focus, cx);
        }
        cx.on_focus_out(&focus, window, |view, _, _, cx| view.let_go_of_keys(cx))
            .detach();
        cx.on_focus_out(&title_focus, window, |view, _, _, cx| {
            view.let_go_of_keys(cx)
        })
        .detach();
        // What had the focus in it let go: a text field in an overlay on a press that nothing
        // focusable took, such as on a space, or a button that went, such as a panel's close.
        // The page, or its part in the title bar, takes the keys back from the window, for escape
        // to close an overlay and for the keys of its page.
        cx.on_focus_lost(window, |view, window, cx| {
            let target = window.focus_lost_restore_target(cx);
            if let Some(target) =
                target.filter(|target| [&view.focus, &view.title_focus].contains(&target))
            {
                window.focus(&target, cx);
            }
        })
        .detach();
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Deleted(id) if *id == view.id => {
                view.state = None;
                view.edit.finish(&view.session, cx);
            }
            ProjectEvent::Created(id) if *id == view.id => view.state = None,
            // Its live values read the record.
            ProjectEvent::Changed(id) if *id == view.id => {
                view.state = None;
                cx.notify();
            }
            // And the records of instances inside it.
            ProjectEvent::Changed(id)
                if id.is_inside(&view.id)
                    && !view.live.read(cx).uses(view.card).inside.is_empty() =>
            {
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // Once per poll that moved it: a frame between two polls would draw the same tick.
        let playhead = session.read(cx).playhead().clone();
        cx.observe(&playhead, |view, _, cx| {
            let parts = view.parts(cx);
            view.draw_again([parts.body.tick, parts.title_bar.tick, parts.over.tick], cx);
        })
        .detach();
        cx.subscribe_in(
            &live,
            window,
            |view, _, changed: &CardsChanged, window, cx| {
                if changed.concerns(view.card) {
                    view.sync(window, cx);
                    cx.notify();
                }
            },
        )
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
        // A window in the back may never hear the button go up.
        cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() {
                view.clicking = None;
                view.let_go(None, cx);
            }
        })
        .detach();
        cx.on_release(|view, cx| {
            view.edit.finish(&view.session, cx);
            // A card that is gone hears no focus out, nor the button go up.
            view.let_go_of_keys(cx);
            view.let_go(None, cx);
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
            pressed: Vec::new(),
            clicking: None,
            focus,
            title_focus,
            keys_down: BTreeSet::new(),
            feeds: Feeds::default(),
            indexes: RefCell::default(),
            menu: None,
            menu_overlay: None,
            inputs: HashMap::new(),
            overlays: Vec::new(),
            focus_before_overlays: None,
            embedded: HashMap::new(),
            title_bar: None,
            parts: RefCell::default(),
            lifted: Rc::default(),
            origin: Rc::default(),
            title_reads: false,
            meters: Rc::default(),
            placed: Rc::default(),
            hovers: Rc::default(),
            anchor: Rc::default(),
            scrolls: RefCell::default(),
            scrolled_cards: Rc::default(),
            button_down: Rc::default(),
            _polling: every_poll(cx, Self::poll),
        }
    }

    /// Draws again when what it reads of the engine changed since it last drew, or still moves.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state(cx) else {
            return;
        };
        let parts = self.parts(cx);
        let session = self.session.read(cx);
        let tick = session.playhead().read(cx).tick;
        let input = session.recording().read(cx).input_levels();
        let feeds = &mut self.feeds;
        let mut changed =
            |uses, title| feeds.changed(session, (&self.id, &state), (uses, title), tick, input);
        let body = changed(&parts.body, false);
        let title_bar = !body && self.title_bar.is_some() && changed(&parts.title_bar, true);
        let over = !body && !parts.lifted.is_empty() && changed(&parts.over, false);
        self.draw_again([body, title_bar, over], cx);
    }

    /// Draws the whole card again, or only its title bar or the layer over the page when
    /// nothing else changed.
    fn draw_again(&mut self, [body, title_bar, over]: [bool; 3], cx: &mut Context<Self>) {
        if body {
            cx.notify();
            return;
        }
        if title_bar {
            self.title_reads = true;
            cx.emit(TitleBarChanged);
        }
        if over {
            cx.emit(OverChanged);
        }
    }

    /// What its nodes read: in the title bar, in the spaces of a page that show the playhead,
    /// which the layer over the page draws, and the rest.
    fn parts(&self, cx: &App) -> Rc<Parts> {
        let live = self.live.read(cx);
        let Some(Ok(table)) = live.tree(self.card) else {
            let body = live.uses(self.card);
            return Rc::new(Parts {
                body,
                ..Parts::default()
            });
        };
        let title = self.title_bar.as_ref().map(|(slot, _)| *slot);
        let key = (table.version(), title);
        if let Some((at, parts)) = &*self.parts.borrow()
            && *at == key
        {
            return parts.clone();
        }
        // Not where a div lies over others: drawn over the page, a space would hide it.
        let page = self.frame.is_none() && !table.lays_over();
        let spaces: Vec<(usize, u64)> = (table.spaces().into_iter())
            .filter(|(slot, _)| page && title.is_none_or(|title| !table.within(*slot, title)))
            .collect();
        let slots: Vec<usize> = spaces.iter().map(|(slot, _)| *slot).collect();
        let (_, each) = table.uses_split(&slots);
        let lifted: Vec<(usize, u64)> = (spaces.into_iter().zip(&each))
            .filter(|(_, uses)| uses.tick)
            .map(|(space, _)| space)
            .collect();
        let parts_of: Vec<usize> = (title.into_iter())
            .chain(lifted.iter().map(|(slot, _)| *slot))
            .collect();
        let (body, mut each) = table.uses_split(&parts_of);
        let title_bar = match title {
            Some(_) if !each.is_empty() => each.remove(0),
            _ => Uses::default(),
        };
        let mut over = Uses::default();
        each.iter().for_each(|uses| over.join(uses));
        let parts = Rc::new(Parts {
            body,
            title_bar,
            over,
            lifted: lifted.into_iter().map(|(_, id)| id).collect(),
        });
        *self.parts.borrow_mut() = Some((key, parts.clone()));
        parts
    }

    /// Whether its page takes the keys: its tool hears them, or it shows an overlay, for escape
    /// to close it.
    fn takes_keys(&self, info: Option<&ToolInfo>) -> bool {
        info.is_some_and(|info| info.keys) || !self.overlays.is_empty()
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

    /// Reads for a frame what the live values of its nodes, `uses`, read of the engine.
    fn read_frame(
        &mut self,
        state: &Value,
        uses: &Uses,
        tick: sound_core::Ticks,
        cx: &mut Context<Self>,
    ) {
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
        self.feeds.take_notes = (uses.takes.iter())
            .filter_map(|name| {
                let notes = recording.notes_of(&self.id.child(name).ok()?)?;
                Some((name.clone(), Rc::new(notes.clone())))
            })
            .collect();
        let now = cx.background_executor().now();
        let session = self.session.read(cx);
        (self.feeds).read(session, &self.id, state, uses, tick, now);
    }

    /// The spaces the layer over the page draws, each where the page laid it out.
    fn draw_over(&mut self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let places: Vec<(u64, Lifted)> = (self.lifted.borrow().iter())
            .map(|(id, place)| (*id, place.clone()))
            .collect();
        if places.is_empty() {
            return Vec::new();
        }
        let Some(state) = self.state(cx) else {
            return Vec::new();
        };
        let Some(Ok(table)) = self.live.read(cx).tree(self.card).cloned() else {
            return Vec::new();
        };
        let uses = self.live.read(cx).uses(self.card);
        let tick = self.session.read(cx).playhead().read(cx).tick;
        self.read_frame(&state, &uses, tick, cx);
        let origin = self.origin.get();
        let mut drawing = self.drawing(state, &table, window, cx);
        let spaces = (places.into_iter())
            .filter_map(|(id, place)| {
                let entry = table.get(place.slot).filter(|entry| entry.id == id)?;
                let Props::Space(space) = &entry.props else {
                    return None;
                };
                let view = drawing.space_view(space, &place.index);
                let element = drawing.space_element(id, space, &entry.children, view);
                let Lifted { bounds, mask, .. } = place;
                // Cut off where the page cuts it off, as by a div that scrolls.
                let cut = div()
                    .absolute()
                    .left(mask.origin.x - origin.x)
                    .top(mask.origin.y - origin.y)
                    .w(mask.size.width)
                    .h(mask.size.height)
                    .overflow_hidden();
                let at = div()
                    .absolute()
                    .left(bounds.origin.x - mask.origin.x)
                    .top(bounds.origin.y - mask.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height);
                Some(cut.child(at.child(element)).into_any_element())
            })
            .collect();
        let kept = drawing.spaces;
        self.keep_spaces(kept);
        spaces
    }

    /// A drawing of its nodes in `table`, with what they read now.
    fn drawing<'a, 'b>(
        &'a self,
        state: Rc<Value>,
        table: &'a NodeTable,
        window: &Window,
        cx: &'a mut Context<'b, Self>,
    ) -> Drawing<'a, 'b> {
        let live = self.live.read(cx);
        let info = live.info(self.card);
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let played = live.played_of(&self.id).cloned();
        let tick = self.session.read(cx).playhead().read(cx).tick.0 as f64;
        let drag =
            (self.held.as_ref()).and_then(|held| Some((held.drag.id.clone()?, held.numbers())));
        let takes_keys = self.takes_keys(info.as_ref());
        let title_focus = (info.as_ref())
            .is_some_and(|info| info.keys)
            .then(|| self.title_focus.clone());
        Drawing {
            info,
            state,
            watches,
            played,
            tick,
            spaces: self.spaces.borrow().clone(),
            laid_out: self.spaces.clone(),
            drag,
            hovered: self.hovered.clone(),
            table,
            feeds: &self.feeds,
            indexes: &self.indexes,
            inputs: &self.inputs,
            focus: &self.focus,
            viewport: window.viewport_size(),
            embedded: &self.embedded,
            meters: &self.meters,
            placed: self.placed.clone(),
            hovers: self.hovers.clone(),
            scrolls: &self.scrolls,
            scrolled_cards: self.scrolled_cards.clone(),
            button_down: self.button_down.get(),
            scroller: None,
            in_title_bar: false,
            out_of_page: false,
            under_tooltip: false,
            takes_keys,
            title_focus,
            title_bar: None,
            lifting: None,
            cx,
        }
    }

    /// The div its view shows in the window's title bar, with what the body last read, or what
    /// it reads itself when it draws alone.
    fn draw_title_bar(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (slot, index) = self.title_bar.clone()?;
        let state = self.state(cx)?;
        if std::mem::take(&mut self.title_reads) {
            let uses = self.live.read(cx).uses(self.card);
            let tick = self.session.read(cx).playhead().read(cx).tick;
            self.read_frame(&state, &uses, tick, cx);
        }
        // What moves in it moves the title bar alone.
        let body_moving = self.feeds.moving.take();
        let Some(Ok(table)) = self.live.read(cx).tree(self.card).cloned() else {
            return None;
        };
        let mut drawing = self.drawing(state, &table, window, cx);
        drawing.in_title_bar = true;
        drawing.out_of_page = true;
        let bar = drawing.draw(slot, &index);
        let spaces = drawing.spaces;
        self.keep_spaces(spaces);
        let moved = self.feeds.moving.replace(body_moving);
        (self.feeds.title_moving).set(self.feeds.title_moving.get() || moved);
        let theme = cx.theme();
        let bar = div()
            .text_size(px(12.))
            .text_color(theme.gray_950)
            .child(bar);
        // Out of the page, it hands the page the keys its controls do not keep, such as R to
        // record with Play focused, and lets go of them when the focus leaves it.
        let info = self.live.read(cx).info(self.card).filter(|info| info.keys);
        let bar = match info {
            Some(info) => {
                let context = crate::key_context(info.takes.as_deref());
                hearing_keys(bar, &self.title_focus.clone(), Some(context), cx)
            }
            None => bar,
        };
        Some(bar.into_any_element())
    }
}

/// The div of a page's view that shows in the window's title bar moved, came or went, or what
/// it shows changed while nothing else of the page did.
pub(crate) struct TitleBarChanged;

impl EventEmitter<TitleBarChanged> for TypeScriptCard {}

/// What the view of a page shows in the window's title bar: its div that says `titleBar`.
pub struct TitleBar(Entity<TypeScriptCard>);

impl TitleBar {
    pub(crate) fn new(card: Entity<TypeScriptCard>, cx: &mut Context<Self>) -> Self {
        cx.observe(&card, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&card, |_, _, _: &TitleBarChanged, cx| cx.notify())
            .detach();
        Self(card)
    }

    /// Whether the view has a div for the title bar.
    pub fn shows(&self, cx: &App) -> bool {
        self.0.read(cx).title_bar.is_some()
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _rendering = renders::start(Part::TitleBar, self.0.read(cx).id.as_str());
        let bar = self
            .0
            .update(cx, |card, cx| card.draw_title_bar(window, cx));
        // A click on it does not move the window, as the rest of the title row does.
        div().flex_none().occlude().children(bar)
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _rendering = renders::start(Part::Body, self.id.as_str());
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
        let playhead = *self.session.read(cx).playhead().read(cx);
        if uses.animates {
            window.request_animation_frame();
        }
        self.read_frame(&state, &uses, playhead.tick, cx);
        self.title_reads = false;
        let body = match tree {
            Some(Ok(table)) => {
                let parts = self.parts(cx);
                self.lifted
                    .borrow_mut()
                    .retain(|id, _| parts.lifted.contains(id));
                // A div left out of this frame, as a head scrolled out of view, is over nothing.
                self.hovers.borrow_mut().placed.clear();
                let mut drawing = self.drawing(state, &table, window, cx);
                drawing.lifting = Some((&parts.lifted, &self.lifted));
                let body = match table.root() {
                    Some(root) => drawing.draw(root, &[]),
                    None => div().into_any_element(),
                };
                let (spaces, title_bar) = (drawing.spaces, drawing.title_bar);
                if self.title_bar != title_bar {
                    self.title_bar = title_bar;
                    cx.emit(TitleBarChanged);
                }
                self.keep_spaces(spaces);
                body
            }
            Some(Err(error)) => div()
                .text_color(cx.theme().red)
                .child(format!(
                    "This view did not draw. Ask the agent to fix it: {error}"
                ))
                .into_any_element(),
            // Until its first nodes arrive, a moment after the card opens.
            None => div().into_any_element(),
        };
        let theme = cx.theme();
        let (text, background) = (theme.gray_950, theme.gray_100);
        // A click in what takes the keys gives them: the body of a card, a whole page. Also while
        // it shows an overlay, for escape to close it.
        let takes_keys = self.takes_keys(info.as_ref());
        let focus = self.focus.clone();
        let context = key_context.filter(|_| hears_keys);
        let takes_keys = move |element: Div, cx: &mut Context<Self>| match takes_keys {
            true => hearing_keys(element, &focus, context.clone(), cx),
            false => element,
        };
        let body = div()
            .text_size(px(12.))
            .text_color(text)
            .child(body)
            .child(hears_the_button(
                self.button_down.clone(),
                cx.entity().downgrade(),
            ))
            .children(self.menu.as_ref().map(|(menu, _)| menu.clone()));
        match &self.frame {
            // At least as wide as a plain card, so the title fits.
            Some(frame) => frame
                .card()
                .min_w(px(PLAIN_CARD_WIDTH))
                .child(takes_keys(body, cx))
                .into_any_element(),
            None => {
                // The room the page draws in: the whole main area.
                let (live, card, origin) = (self.live.clone(), self.card, self.origin.clone());
                let measure = canvas(
                    move |bounds, _, cx| {
                        origin.set(bounds.origin);
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
                takes_keys(div().size_full().bg(background).child(room), cx).into_any_element()
            }
        }
    }
}
