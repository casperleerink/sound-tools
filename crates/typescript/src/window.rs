//! The tools of `extensions/` while the window is open: Bun runs their cards and control loops,
//! the pickers offer them, and the cards play their live controls and triggers and follow their
//! watches.
//!
//! Any change in `extensions/`, a crash or a hang of Bun starts a new Bun, which loads every
//! file again, so a helper a tool imports is read again too, and every `memory` starts again.
//! A tool whose code hung or stopped Bun is left out of the new one until the folder changes.
//! Nothing here waits for Bun: what it says comes as events.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Global, Task, WeakEntity};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use sound_core::{GROUPING_WINDOW, InstanceId, Problem, Project};
use sound_notes::{Pitch, Velocity};
use sound_signals::{Signals, SignalsUpdate};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Session, Views};

use crate::bun::{Bun, Event, Looped, PageSize, Request, Runs, Skipped};
use crate::card::TypeScriptCard;
use crate::tools::{Control, Sounds, ToolInfo, ToolKind};
use crate::tree::Node;
use crate::{Extensions, FOLDER, Launched, Midi, OWNED};

/// How often the cards look at their watches and the control loops run: often enough for a
/// step light, a meter or a moving drawing.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    /// The `extensions/` folder of the project.
    folder: PathBuf,
    session: WeakEntity<Session>,
    host: Host,
    skipped: Skipped,
    /// The tools of the last load, shared with the pickers.
    tools: Rc<RefCell<Vec<ToolInfo>>>,
    /// Goes up when the tools do, so the pickers fill themselves again.
    generation: Rc<Cell<u64>>,
    cards: HashMap<u64, Card>,
    next_card: u64,
    /// Where each live control was put last, by instance: the processor keeps it on the audio
    /// side, and a control on the card shows it.
    played: HashMap<InstanceId, BTreeMap<String, f32>>,
    /// How many edits of each record this Bun sent and the window heard, so Bun knows when a
    /// record it gets misses its last edit. Never counted down: Bun counts on.
    edits: HashMap<InstanceId, u64>,
    /// When the control loops last ran.
    last_frame: Instant,
    /// What is wrong with `extensions/`: the last load, and what failed since.
    problems: Vec<Problem>,
    /// Hears the changes in `extensions/`, or why it cannot, which is a problem of its own.
    watcher: Result<RecommendedWatcher, Problem>,
    changed: smol::channel::Sender<()>,
    _tasks: [Task<()>; 2],
}

/// The Bun of the window.
enum Host {
    Running {
        bun: Bun,
        /// A step of the control loops is out. One at a time, so a slow loop does not pile
        /// steps up.
        ticking: bool,
        /// Hears what Bun says, until it ends.
        _hearing: Task<()>,
    },
    /// A new Bun starts and loads on a thread of its own. `again`: `extensions/` changed
    /// meanwhile, so another starts once this one has loaded.
    Starting { again: bool },
    /// No Bun runs, until the folder changes: it has no tool, Bun cannot start, or Bun stopped
    /// with no tool to blame.
    Stopped,
}

impl Host {
    /// Tells Bun something. Nothing while no Bun runs: the next one draws every card again.
    fn send(&mut self, request: &Request) {
        if let Self::Running { bun, .. } = self {
            bun.send(request);
        }
    }
}

struct Card {
    id: InstanceId,
    /// The name of its tool, which a card keeps for its life.
    tool: String,
    surface: Surface,
    /// The last tree with its version, which a click on it names, or why there is none. `None`
    /// until the first one arrives.
    tree: Option<Result<(u64, Rc<Node>), String>>,
    /// The watches the last tree was drawn with, which native elements read too.
    watches: BTreeMap<String, f32>,
    /// A render was asked for and its tree has not come yet.
    asked: bool,
    /// The record or a watch changed while a render was out, so one more is due when it comes.
    stale: bool,
}

/// What a tree is drawn on.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Surface {
    /// A card in a rack.
    Card,
    /// The whole window: its size once laid out, which the page draws from.
    Page(Option<PageSize>),
}

/// The tools of the window, for what the window hears: the MIDI keyboard.
struct WindowTools(Entity<Live>);

impl Global for WindowTools {}

/// Gives what the MIDI keyboard played to the tools that hear it: those on `track`, the track
/// it played into, and those at the top of the project.
pub fn hear_midi(track: Option<&InstanceId>, messages: &[Midi], cx: &mut App) {
    if messages.is_empty() {
        return;
    }
    let Some(live) = cx.try_global::<WindowTools>().map(|tools| tools.0.clone()) else {
        return;
    };
    live.update(cx, |live, cx| live.midi(track, messages, cx));
}

/// Starts the live part of `extensions` for the window of `session`: the cards of its tools in
/// `views`, and its tools in the pickers of `devices`.
pub fn start_window(
    extensions: Extensions,
    session: &Entity<Session>,
    views: &mut Views,
    devices: &mut Devices,
    cx: &mut App,
) {
    let tools = Rc::new(RefCell::new(Vec::new()));
    let generation = Rc::new(Cell::new(0));
    let live = Live::start(extensions, session, &tools, &generation, cx);
    cx.set_global(WindowTools(live.clone()));

    views.set_other_cards({
        let (tools, live) = (tools.clone(), live.clone());
        move |session, id, frame, window, cx| {
            let tool = session.read(cx).project().tool_of(id)?;
            find(&tools.borrow(), tool)?;
            let (live, session, id) = (live.clone(), session.clone(), id.clone());
            let card = cx.new(|cx| TypeScriptCard::new(live, session, id, Some(frame), window, cx));
            Some(card.into())
        }
    });
    views.set_other_views(
        {
            let tools = tools.clone();
            move |tool| find(&tools.borrow(), tool).is_some_and(|info| info.page)
        },
        move |session, id, window, cx| {
            let (live, session, id) = (live.clone(), session.clone(), id.clone());
            let page = cx.new(|cx| TypeScriptCard::new(live, session, id, None, window, cx));
            Some(page.into())
        },
    );
    devices.effects({
        let tools = tools.clone();
        move || offers(&tools.borrow(), |kind| kind == ToolKind::Effect)
    });
    devices.instruments({
        let tools = tools.clone();
        move || offers(&tools.borrow(), |kind| kind != ToolKind::Effect)
    });
    devices.offers_change(move || generation.get());
    devices.describe_others(move |project, id| {
        let tools = tools.borrow();
        let info = find(&tools, project.tool_of(id)?)?;
        Some(DeviceLabel {
            key: key(&info.name).into(),
            name: info.title.clone().into(),
        })
    });
}

/// Watches `folder` and its folders but `node_modules`, and says on `changed` when a file
/// that may be imported changes, or a folder. Not hidden files, such as an editor's.
fn watch(folder: &Path, changed: smol::channel::Sender<()>) -> Result<RecommendedWatcher, Problem> {
    let not_heard = |error: notify::Error| {
        crate::folder_problem(format!("changes in {FOLDER}/ are not heard: {error}"))
    };
    let skipped = |name: &str| name.starts_with('.') || name == "node_modules";
    // The paths of events are the real ones.
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let counts = move |path: &Path| {
        let inside = path.strip_prefix(&root).unwrap_or(path);
        let hidden = (inside.components()).any(|part| skipped(&part.as_os_str().to_string_lossy()));
        let importable = (path.extension()).is_some_and(|extension| {
            ["ts", "tsx", "js", "mjs", "json"]
                .iter()
                .any(|known| extension == *known)
        });
        let owned = OWNED.iter().any(|owned| inside == Path::new(owned));
        !hidden && !owned && (importable || path.is_dir())
    };
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => return eprintln!("error: watching {FOLDER}/: {error}"),
        };
        // An event with no paths asks to look again at everything, as after a lost event.
        let paths = &event.paths;
        if !event.kind.is_access() && (paths.is_empty() || paths.iter().any(|path| counts(path))) {
            changed.try_send(()).ok();
        }
    })
    .map_err(not_heard)?;
    watcher
        .watch(folder, RecursiveMode::NonRecursive)
        .map_err(not_heard)?;
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    for entry in entries.filter(|entry| entry.path().is_dir()) {
        if !skipped(&entry.file_name().to_string_lossy()) {
            watcher
                .watch(&entry.path(), RecursiveMode::Recursive)
                .map_err(not_heard)?;
        }
    }
    Ok(watcher)
}

impl Live {
    /// The live part of the tools of `extensions` for the window of `session`: it hears Bun and
    /// the folder, and runs the control loops.
    fn start(
        extensions: Extensions,
        session: &Entity<Session>,
        tools: &Rc<RefCell<Vec<ToolInfo>>>,
        generation: &Rc<Cell<u64>>,
        cx: &mut App,
    ) -> Entity<Self> {
        let Extensions {
            folder,
            bun,
            tools: loaded,
            skipped,
            problems,
        } = extensions;
        *tools.borrow_mut() = loaded;
        cx.new(|cx: &mut Context<Live>| {
            let (changed, changes) = smol::channel::unbounded();
            let watcher = watch(&folder, changed.clone());
            let hearing_changes = cx.spawn(async move |live, cx| {
                while changes.recv().await.is_ok() {
                    // Until it is quiet: an editor saves in several writes, an agent several
                    // files.
                    loop {
                        cx.background_executor().timer(GROUPING_WINDOW).await;
                        if changes.is_empty() {
                            break;
                        }
                        while changes.try_recv().is_ok() {}
                    }
                    if live.update(cx, |live, cx| live.changed(cx)).is_err() {
                        return;
                    }
                }
            });
            let framing = cx.spawn(async move |live, cx| {
                loop {
                    cx.background_executor().timer(FRAME_INTERVAL).await;
                    if live.update(cx, |live, cx| live.frame(cx)).is_err() {
                        break;
                    }
                }
            });
            let host = match bun {
                Some(bun) => Live::running(bun, cx),
                None => Host::Stopped,
            };
            let live = Live {
                folder,
                session: session.downgrade(),
                host,
                skipped,
                tools: tools.clone(),
                generation: generation.clone(),
                cards: HashMap::new(),
                next_card: 0,
                played: HashMap::new(),
                edits: HashMap::new(),
                last_frame: Instant::now(),
                problems,
                watcher,
                changed,
                _tasks: [hearing_changes, framing],
            };
            // Only the watcher's problem is new.
            if live.watcher.is_err() {
                live.report(cx);
            }
            live
        })
    }

    /// `bun` as the Bun of the window, heard until it ends.
    fn running(bun: Bun, cx: &mut Context<Self>) -> Host {
        let events = bun.events();
        let hearing = cx.spawn(async move |live, cx| {
            while let Ok(event) = events.recv().await {
                if live.update(cx, |live, cx| live.heard(event, cx)).is_err() {
                    return;
                }
            }
        });
        Host::Running {
            bun,
            ticking: false,
            _hearing: hearing,
        }
    }
}

/// The tool named `name`, as it last loaded.
fn find<'a>(tools: &'a [ToolInfo], name: &str) -> Option<&'a ToolInfo> {
    tools.iter().find(|info| info.name == name)
}

fn key(tool: &str) -> String {
    format!("{FOLDER}/{tool}")
}

/// The `state` of the record of `id`.
pub(crate) fn state_of(project: &Project, id: &InstanceId) -> Option<serde_json::Value> {
    serde_json::from_str(&project.state_json(id)?).ok()
}

/// Changes the record of `id` as one undo step called `label`.
pub(crate) fn edit_record(
    session: &Entity<Session>,
    id: &InstanceId,
    label: &str,
    change: impl FnOnce(&mut serde_json::Value),
    cx: &mut App,
) {
    session.update(cx, |session, cx| {
        session.edit(cx, |project| {
            let mut edit = project.begin(label);
            project.update_json(&mut edit, id, change)?;
            project.finish(edit)
        })
    });
}

/// An instance of `tool` as its code gets it: its record and its watches as they are now, and
/// how many of the `edits` Bun sent the record has.
fn looped<'a>(
    project: &Project,
    edits: &HashMap<InstanceId, u64>,
    id: &'a InstanceId,
    tool: &'a str,
) -> Looped<'a> {
    Looped {
        instance: id,
        tool,
        state: state_of(project, id).unwrap_or_default(),
        watches: (project.watches(id).into_iter())
            .map(|(name, watch)| (name, watch.get()))
            .collect(),
        edits: edits.get(id).copied().unwrap_or(0),
    }
}

/// Where the engine is now, in seconds: the clock `at` counts on.
fn engine_time(session: &Entity<Session>, cx: &mut App) -> f64 {
    session.update(cx, |session, _| {
        let rate = f64::from(session.project().clock().sample_rate());
        let frames = session.engine().poll().map_or(0, |status| status.frames);
        frames as f64 / rate
    })
}

/// What a picker offers of the tools of `kind`: each one's record at its defaults.
fn offers(tools: &[ToolInfo], kind: impl Fn(ToolKind) -> bool) -> Vec<DeviceOffer> {
    let tools = tools.iter().filter(|info| kind(info.kind));
    tools
        .map(|info| {
            let tool = info.name.clone();
            let write = move |project: &Project, slot: &InstanceId, changes: &mut _| {
                project.set_json(changes, slot.clone(), &tool, serde_json::json!({}))
            };
            DeviceOffer::new(
                key(&info.name),
                info.title.clone(),
                OfferGroup::Project,
                write,
            )
        })
        .collect()
}

impl Live {
    /// The last tree of a card, with its version. `None` until it first drew.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<(u64, Rc<Node>), String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// The watches a card last drew with.
    pub(crate) fn watches(&self, card: u64) -> Option<&BTreeMap<String, f32>> {
        self.cards.get(&card).map(|card| &card.watches)
    }

    /// The tool of a card, as it last loaded.
    pub(crate) fn info(&self, card: u64) -> Option<ToolInfo> {
        find(&self.tools.borrow(), &self.cards.get(&card)?.tool).cloned()
    }

    /// Where the live control `name` of the card's instance is, and its range: where it was
    /// put last, or its default.
    pub(crate) fn live_value(&self, card: u64, name: &str) -> Option<(f32, f32, f32)> {
        let entry = self.cards.get(&card)?;
        let tools = self.tools.borrow();
        let Some((
            _,
            Control::Live {
                min, max, default, ..
            },
        )) = find(&tools, &entry.tool)?.control(name)
        else {
            return None;
        };
        let played = (self.played.get(&entry.id)).and_then(|played| played.get(name));
        Some((played.copied().unwrap_or(*default), *min, *max))
    }

    /// A new card of the instance, which draws at once.
    pub(crate) fn add(
        &mut self,
        id: InstanceId,
        tool: String,
        surface: Surface,
        cx: &mut Context<Self>,
    ) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        let entry = Card {
            id,
            tool,
            surface,
            tree: None,
            watches: BTreeMap::new(),
            asked: false,
            stale: false,
        };
        self.cards.insert(card, entry);
        self.render(card, cx);
        card
    }

    pub(crate) fn remove(&mut self, card: u64) {
        self.cards.remove(&card);
        self.host.send(&Request::Drop { card });
    }

    /// Asks for the tree of the card again, from the record and the watches as they are now.
    /// While one is out, one more is asked for when it comes, so a drag that changes the record
    /// sixty times a second keeps one request in flight and not sixty.
    pub(crate) fn render(&mut self, card: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        if entry.asked {
            entry.stale = true;
            return;
        }
        let page = match entry.surface {
            Surface::Card => None,
            Surface::Page(Some(size)) => Some(size),
            // It draws once it is laid out and has a size.
            Surface::Page(None) => return,
        };
        let project = session.read(cx).project();
        let (Some(tool), Some(state)) = (project.tool_of(&entry.id), state_of(project, &entry.id))
        else {
            return;
        };
        entry.asked = true;
        let request = Request::Render {
            card,
            instance: &entry.id,
            tool,
            state,
            watches: &entry.watches,
            page,
        };
        self.host.send(&request);
    }

    /// The page was laid out at `size`: it draws again when that is new.
    pub(crate) fn resize(&mut self, card: u64, size: PageSize, cx: &mut Context<Self>) {
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        if entry.surface == Surface::Page(Some(size)) {
            return;
        }
        entry.surface = Surface::Page(Some(size));
        self.render(card, cx);
    }

    /// A click on an element of the tree of `version` of the card, or a press or a drag on a
    /// canvas `at` across and down, 0 to 1.
    pub(crate) fn event(
        &mut self,
        card: u64,
        version: u64,
        handler: usize,
        at: Option<(f32, f32)>,
    ) {
        if self.cards.contains_key(&card) {
            let request = Request::Event {
                card,
                version,
                handler,
                at,
            };
            self.host.send(&request);
        }
    }

    /// Sends what `update` makes for the tool of instance `id` to its processor, as the
    /// keyboard or a knob of a built-in tool would. Whether it was sent.
    fn send_update(
        &self,
        id: &InstanceId,
        update: impl FnOnce(&ToolInfo, &Session) -> Result<SignalsUpdate, String>,
        cx: &mut App,
    ) -> bool {
        let Some(session) = self.session.upgrade() else {
            return false;
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let Some(info) = tool.and_then(|tool| find(&tools, tool)) else {
            return false;
        };
        let update = match update(info, session.read(cx)) {
            Ok(update) => update,
            Err(error) => {
                eprintln!("error: {error}");
                return false;
            }
        };
        let processor = info.processor();
        drop(tools);
        session.update(cx, |session, cx| {
            let sent =
                session.background(cx, |project| project.send::<Signals>(id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
        true
    }

    /// Moves the live control `name` of instance `id` to `value`, or fires the trigger `name`
    /// when `value` is `None`. Not an edit: nothing is saved.
    pub(crate) fn control(
        &mut self,
        id: &InstanceId,
        name: &str,
        value: Option<f32>,
        at: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let update = |info: &ToolInfo, session: &Session| match (info.control(name), value) {
            (Some((index, Control::Live { .. })), Some(value)) => {
                Ok(SignalsUpdate::Live { index, value })
            }
            (Some((index, Control::Trigger { .. })), None) => Ok(SignalsUpdate::Trigger {
                index,
                at: at.map(|at| frames_of(at, session)),
            }),
            _ => Err(format!(
                "{} has no control {name} that takes that",
                info.name
            )),
        };
        if let (true, Some(value)) = (self.send_update(id, update, cx), value) {
            self.played
                .entry(id.clone())
                .or_default()
                .insert(name.to_string(), value);
            // A pad or a knob of the card shows where it is, also while nothing else redraws.
            cx.notify();
        }
    }

    /// Plays a key of the voices of an instance, or lets go of one, as the keyboard would:
    /// `update` makes what the processor gets.
    fn note(&self, id: &InstanceId, update: impl FnOnce(&Session) -> SignalsUpdate, cx: &mut App) {
        self.send_update(
            id,
            |info, session| match info.kind {
                ToolKind::Effect => {
                    Err(format!("{} is an effect, which plays no notes", info.name))
                }
                ToolKind::Instrument | ToolKind::Source => Ok(update(session)),
            },
            cx,
        );
    }

    /// A key of the computer keyboard went down or up on a card that has the keys.
    pub(crate) fn key(&mut self, card: u64, key: &str, down: bool, cx: &mut Context<Self>) {
        let (Some(session), Some(entry)) = (self.session.upgrade(), self.cards.get(&card)) else {
            return;
        };
        let time = engine_time(&session, cx);
        let project = session.read(cx).project();
        let Some(tool) = project.tool_of(&entry.id) else {
            return;
        };
        let request = Request::Key {
            time,
            instance: looped(project, &self.edits, &entry.id, tool),
            key,
            down,
        };
        self.host.send(&request);
    }

    /// What the MIDI keyboard played, for every instance whose tool hears it: on `track` or at
    /// the top of the project.
    fn midi(&mut self, track: Option<&InstanceId>, messages: &[Midi], cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let time = engine_time(&session, cx);
        let hears = |id: &InstanceId| {
            id.parent().is_none() || track.is_some_and(|track| id == track || id.is_inside(track))
        };
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        let instances: Vec<Looped> = (project.instances())
            .filter(|(id, tool)| hears(id) && find(&tools, tool).is_some_and(|info| info.midi))
            .map(|(id, tool)| looped(project, &self.edits, id, tool))
            .collect();
        if !instances.is_empty() {
            let request = Request::Midi {
                time,
                instances,
                messages,
            };
            self.host.send(&request);
        }
    }

    /// One frame of the window: the control loops run, a card whose watches moved draws again,
    /// and every instance whose sample was read on its thread runs again, so it plays it.
    fn frame(&mut self, cx: &mut Context<Self>) {
        self.run_loops(cx);
        self.look_at_watches(cx);
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let read = crate::samples::take_read(session.read(cx).project().assets());
        if !read.is_empty() {
            session.update(cx, |session, cx| session.rebind(&read, cx));
        }
    }

    /// Runs the control loop of every instance whose tool has one, cards or not.
    fn run_loops(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Host::Running { ticking: false, .. } = self.host else {
            return;
        };
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();
        let time = engine_time(&session, cx);
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        // An instance that is gone, which an undo may bring back at its defaults.
        self.played.retain(|id, _| project.tool_of(id).is_some());
        let instances: Vec<Looped> = (project.instances())
            .filter(|(_, tool)| find(&tools, tool).is_some_and(|info| info.tick))
            .map(|(id, tool)| looped(project, &self.edits, id, tool))
            .collect();
        if !instances.is_empty() {
            let request = Request::Frame {
                dt,
                time,
                instances,
            };
            self.host.send(&request);
            if let Host::Running { ticking, .. } = &mut self.host {
                *ticking = true;
            }
        }
    }

    /// Reads the watches of every card; a card whose watches moved draws again.
    fn look_at_watches(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let mut moved = Vec::new();
        let tools = self.tools.borrow();
        for (card, entry) in &mut self.cards {
            let watches = session.read(cx).project().watches(&entry.id);
            let watches: BTreeMap<String, f32> = watches
                .into_iter()
                .map(|(name, watch)| (name, watch.get()))
                .collect();
            if watches != entry.watches {
                entry.watches = watches;
                // The control loop draws the card of its tool after every step anyway.
                if !find(&tools, &entry.tool).is_some_and(|info| info.tick) {
                    moved.push(*card);
                }
            }
        }
        drop(tools);
        for card in &moved {
            self.render(*card, cx);
        }
        if !moved.is_empty() {
            cx.notify();
        }
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Framed => {
                if let Host::Running { ticking, .. } = &mut self.host {
                    *ticking = false;
                }
                return;
            }
            Event::Ended { blame, why } => return self.failed(blame, &why, cx),
            Event::Failed { tool, message } => {
                let problem = find(&self.tools.borrow(), &tool).map(|info| info.problem(message));
                if let Some(problem) = problem {
                    self.problems.push(problem);
                    self.report(cx);
                }
            }
            Event::Tree { card, tree } => {
                let Some(entry) = self.cards.get_mut(&card) else {
                    return;
                };
                entry.asked = false;
                entry.tree = Some(tree.and_then(|(version, tree)| {
                    let node = serde_json::from_value(tree).map_err(|error| error.to_string())?;
                    Ok((version, Rc::new(node)))
                }));
                if std::mem::take(&mut entry.stale) {
                    self.render(card, cx);
                }
            }
            Event::Edit {
                instance,
                label,
                state,
            } => {
                let Some(session) = self.session.upgrade() else {
                    return;
                };
                // Heard, applied or not: Bun starts from the record here again.
                *self.edits.entry(instance.clone()).or_default() += 1;
                edit_record(&session, &instance, &label, |old| *old = state, cx);
            }
            Event::Control {
                instance,
                name,
                value,
                at,
            } => self.control(&instance, &name, value, at, cx),
            Event::Note {
                instance,
                pitch,
                velocity,
                seconds,
                at,
            } => self.note(
                &instance,
                |session| SignalsUpdate::Note {
                    pitch: Pitch::nearest(i64::from(pitch)),
                    velocity: Velocity::nearest((velocity.clamp(0.0, 1.0) * 127.0).round() as i64),
                    at: at.map(|at| frames_of(at, session)),
                    frames: seconds.map(|seconds| frames_of(f64::from(seconds), session)),
                },
                cx,
            ),
            Event::Release {
                instance,
                pitch,
                at,
            } => self.note(
                &instance,
                |session| SignalsUpdate::Release {
                    pitch: Pitch::nearest(i64::from(pitch)),
                    at: at.map(|at| frames_of(at, session)),
                },
                cx,
            ),
        }
        cx.notify();
    }

    /// Something changed in `extensions/`: maybe a fix, so every file runs again.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.skipped.clear();
        // A new folder is watched too.
        self.watcher = watch(&self.folder, self.changed.clone());
        self.restart(cx);
    }

    /// Bun hung or stopped: the code it ran then is to blame, and the file of that tool runs
    /// no more until the folder changes, while a new Bun runs the others. With nothing to
    /// blame, such as code a timer ran, no Bun runs until the folder changes: it may stop again
    /// at once, for ever.
    fn failed(&mut self, blame: Option<Runs>, why: &str, cx: &mut Context<Self>) {
        let info =
            (blame.as_ref()).and_then(|runs| find(&self.tools.borrow(), &runs.tool).cloned());
        let (Some(info), Some(runs)) = (info, blame) else {
            self.host = Host::Stopped;
            let message = format!(
                "the TypeScript host {why} with no tool to blame, so the tools do not run: fix them and save"
            );
            self.problems.push(crate::folder_problem(message));
            return self.report(cx);
        };
        let problem = info.problem(format!(
            "its {} {why}, so the tools run without this file: fix it and save",
            runs.what
        ));
        self.skipped.insert(info.file, problem.message);
        self.restart(cx);
    }

    /// Ends Bun, if one runs, and starts a new one on the files of `extensions/` as they are now.
    fn restart(&mut self, cx: &mut Context<Self>) {
        match std::mem::replace(&mut self.host, Host::Starting { again: false }) {
            Host::Starting { .. } => self.host = Host::Starting { again: true },
            Host::Running { bun, .. } => self.launch(Some(bun), cx),
            Host::Stopped => self.launch(None, cx),
        }
    }

    /// Starts a new Bun on a thread of its own, once `old` ended there.
    fn launch(&mut self, old: Option<Bun>, cx: &mut Context<Self>) {
        let (folder, skipped) = (self.folder.clone(), self.skipped.clone());
        cx.spawn(async move |live, cx| {
            // Ending a Bun waits for it, and a load up to a timeout of each step: not on the
            // thread that draws.
            let launched = (cx.background_executor())
                .spawn(async move {
                    drop(old);
                    crate::launch(&folder, skipped)
                })
                .await;
            live.update(cx, |live, cx| live.launched(launched, cx)).ok();
        })
        .detach();
    }

    /// A new Bun loaded, or why none runs.
    fn launched(&mut self, launched: Result<Option<Launched>, String>, cx: &mut Context<Self>) {
        if let Host::Starting { again: true } = self.host {
            self.host = Host::Starting { again: false };
            let bun = launched.ok().flatten().map(|launched| launched.bun);
            return self.launch(bun, cx);
        }
        // The new Bun counts the edits it sends from none, and its first frame from now.
        self.edits.clear();
        self.last_frame = Instant::now();
        match launched {
            Ok(Some(Launched {
                bun,
                sounds,
                skipped,
                problems,
            })) => {
                self.skipped = skipped;
                self.host = Self::running(bun, cx);
                self.loaded(sounds, problems, cx);
            }
            // The last tool file is gone.
            Ok(None) => {
                self.host = Host::Stopped;
                self.loaded(Vec::new(), Vec::new(), cx);
            }
            Err(message) => {
                self.host = Host::Stopped;
                self.problems = vec![crate::folder_problem(message)];
                self.report(cx);
            }
        }
    }

    /// A new Bun loaded `sounds`, with these `problems`: every tool is defined again, and every
    /// card draws again.
    fn loaded(&mut self, sounds: Vec<Sounds>, mut problems: Vec<Problem>, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        // Every tool, changed or not: a file it imports may have changed its sound.
        let tools = crate::define(sounds, &mut problems, |tools| {
            let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
            let defined = session.update(cx, |session, cx| {
                session.background(cx, |project| project.define_json_tools(tools))
            });
            match defined {
                Ok(refused) => (refused.into_iter())
                    .map(|(name, error)| (name, error.to_string()))
                    .collect(),
                // The folder did not read: no tool of the load plays.
                Err(error) => (names.into_iter())
                    .map(|name| (name, error.to_string()))
                    .collect(),
            }
        });
        let gone: Vec<ToolInfo> = (self.tools.borrow().iter())
            .filter(|info| find(&tools, &info.name).is_none())
            .cloned()
            .collect();
        // A file Bun runs without says why already.
        let unexplained = gone
            .iter()
            .filter(|info| !self.skipped.contains_key(&info.file));
        problems.extend(unexplained.map(|info| {
            info.problem("it is gone from the code; its records play as they did until the project opens again")
        }));
        self.problems = problems;
        self.report(cx);
        *self.tools.borrow_mut() = gone.into_iter().chain(tools).collect();
        self.generation.set(self.generation.get() + 1);
        let cards: Vec<u64> = self.cards.keys().copied().collect();
        for card in cards {
            // A tree asked of the Bun before never comes.
            if let Some(entry) = self.cards.get_mut(&card) {
                entry.asked = false;
            }
            self.render(card, cx);
        }
    }

    /// Lists what is wrong with `extensions/` among the problems of the project.
    fn report(&self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let watcher = self.watcher.as_ref().err().cloned();
        let problems = self.problems.iter().cloned().chain(watcher).collect();
        session.update(cx, |session, cx| {
            session.background(cx, |project| {
                project.set_problems_in(&format!("{FOLDER}/"), problems);
            });
        });
    }
}

/// The frame of engine time at `seconds` of it.
fn frames_of(seconds: f64, session: &Session) -> u64 {
    let rate = f64::from(session.project().clock().sample_rate());
    (seconds.max(0.0) * rate).round() as u64
}
