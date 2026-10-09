//! The tools of `extensions/` while the window is open: a save of a file defines the tools
//! again and draws every card again, the pickers offer the tools, and the cards play their
//! live controls and triggers and follow their watches.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Global, Task, WeakEntity};
use sound_core::{InstanceId, Problem, Project};
use sound_hum::{Hum, HumUpdate};
use sound_notes::{Pitch, Velocity};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Session, Views};

use crate::bun::{ANSWER_TIMEOUT, Bun, Event, Loaded, Looped, PageSize, Request};
use crate::card::TypeScriptCard;
use crate::tools::{Control, ToolInfo, ToolKind};
use crate::tree::Node;
use crate::{Extensions, FOLDER, Midi};

/// How often the cards look at their watches and the control loops run: often enough for a
/// step light, a meter or a moving drawing.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    bun: Arc<Bun>,
    session: WeakEntity<Session>,
    /// The tools of the last load, shared with the pickers.
    tools: Rc<RefCell<Vec<ToolInfo>>>,
    /// Goes up when the tools do, so the pickers fill themselves again.
    generation: Rc<Cell<u64>>,
    cards: HashMap<u64, Card>,
    next_card: u64,
    /// Where each live control was put last, by instance: the processor keeps it on the audio
    /// side, and a control on the card shows it.
    played: HashMap<InstanceId, BTreeMap<String, f32>>,
    /// When the control loops last ran.
    last_frame: Instant,
    /// What is wrong with `extensions/`: the last load, and what failed since.
    problems: Vec<Problem>,
    /// When the frame Bun has not finished yet was sent. One at a time, so a slow loop does
    /// not pile frames up.
    frame_out: Option<Instant>,
    _tasks: [Task<()>; 2],
}

struct Card {
    id: InstanceId,
    /// The name of its tool, which a card keeps for its life.
    tool: String,
    surface: Surface,
    /// The last tree, or why there is none. `None` until the first one arrives.
    tree: Option<Result<Rc<Node>, String>>,
    /// The version of the last tree, which a click on it names.
    version: u64,
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

/// The live part of the tools once Bun runs. Until then the cards and pickers have no tools.
type Slot = Rc<RefCell<Option<Entity<Live>>>>;

/// The live part of the tools of the window, for what the window hears: the MIDI keyboard.
struct Running(Slot);

impl Global for Running {}

/// Gives what the MIDI keyboard played to the tools that hear it: those on `track`, the track
/// it played into, and those at the top of the project. Nothing while no tool runs.
pub fn hear_midi(track: Option<&InstanceId>, messages: &[Midi], cx: &mut App) {
    if messages.is_empty() {
        return;
    }
    let running = cx.try_global::<Running>();
    let Some(live) = running.and_then(|running| running.0.borrow().clone()) else {
        return;
    };
    live.update(cx, |live, cx| live.midi(track, messages, cx));
}

/// How often a window whose project has no tool yet looks for the first one.
const WAIT_INTERVAL: Duration = Duration::from_secs(1);

/// Starts the live part of `extensions` for the window of `session`: the cards of its tools in
/// `views`, and its tools in the pickers of `devices`. A project with no tool yet starts Bun
/// when the first tool file comes.
pub fn start_window(
    extensions: Extensions,
    session: &Entity<Session>,
    views: &mut Views,
    devices: &mut Devices,
    cx: &mut App,
) {
    let tools = Rc::new(RefCell::new(Vec::new()));
    let generation = Rc::new(Cell::new(0));
    let slot = Slot::default();
    cx.set_global(Running(slot.clone()));
    match extensions.running {
        Some((bun, loaded)) => {
            *tools.borrow_mut() = loaded.tools;
            *slot.borrow_mut() = Some(Live::start(bun, session, &tools, &generation, cx));
        }
        // Bun did not start for tools that are there: the problems of the project say why.
        None if crate::has_tools(&extensions.folder) => {}
        None => {
            let waiting = (tools.clone(), generation.clone(), slot.clone());
            wait_for_tools(extensions.folder, session.downgrade(), waiting, cx);
        }
    }

    views.set_other_cards({
        let (tools, slot) = (tools.clone(), slot.clone());
        move |session, id, frame, window, cx| {
            let tool = session.read(cx).project().tool_of(id)?;
            if !tools.borrow().iter().any(|info| info.name == tool) {
                return None;
            }
            let live = slot.borrow().clone()?;
            let (session, id) = (session.clone(), id.clone());
            let card = cx.new(|cx| TypeScriptCard::new(live, session, id, Some(frame), window, cx));
            Some(card.into())
        }
    });
    views.set_other_views(
        {
            let tools = tools.clone();
            move |tool| {
                tools
                    .borrow()
                    .iter()
                    .any(|info| info.name == tool && info.page)
            }
        },
        move |session, id, window, cx| {
            let live = slot.borrow().clone()?;
            let (session, id) = (session.clone(), id.clone());
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
        let tool = project.tool_of(id)?;
        let tools = tools.borrow();
        let info = tools.iter().find(|info| info.name == tool)?;
        Some(DeviceLabel {
            key: key(&info.name).into(),
            name: info.title.clone().into(),
        })
    });
}

/// Looks for the first tool file in `folder` while the window is open, then starts Bun on it
/// and defines its tools, as a save would. An open project with no tool runs no Bun.
fn wait_for_tools(
    folder: PathBuf,
    session: WeakEntity<Session>,
    (tools, generation, slot): (Rc<RefCell<Vec<ToolInfo>>>, Rc<Cell<u64>>, Slot),
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(WAIT_INTERVAL).await;
            if session.upgrade().is_none() {
                return;
            }
            let looked = folder.clone();
            let found = (cx.background_executor())
                .spawn(async move { crate::has_tools(&looked) })
                .await;
            if found {
                break;
            }
        }
        // Up to the timeout of the first load, so not on the thread that draws.
        let launched = (cx.background_executor())
            .spawn(async move { crate::launch(&folder) })
            .await;
        cx.update(|cx| {
            let Some(session) = session.upgrade() else {
                return;
            };
            match launched {
                Ok((bun, loaded)) => {
                    let live = Live::start(bun, &session, &tools, &generation, cx);
                    *slot.borrow_mut() = Some(live.clone());
                    live.update(cx, |live, cx| live.loaded(loaded, cx));
                }
                Err(message) => session.update(cx, |session, cx| {
                    session.background(cx, |project| {
                        let problems = vec![crate::folder_problem(message)];
                        project.set_problems_in(&format!("{FOLDER}/"), problems);
                    });
                }),
            }
        });
    })
    .detach();
}

impl Live {
    /// The live part of the tools of `bun` for the window of `session`: it hears Bun and runs
    /// the control loops.
    fn start(
        bun: Arc<Bun>,
        session: &Entity<Session>,
        tools: &Rc<RefCell<Vec<ToolInfo>>>,
        generation: &Rc<Cell<u64>>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Live>| {
            let events = bun.events();
            let hearing = cx.spawn(async move |live, cx| {
                while let Ok(event) = events.recv().await {
                    if live.update(cx, |live, cx| live.heard(event, cx)).is_err() {
                        return;
                    }
                }
                // Bun is gone: what plays was built and plays on, but nothing new can be built.
                live.update(cx, |live, cx| live.stopped(cx)).ok();
            });
            let framing = cx.spawn(async move |live, cx| {
                loop {
                    cx.background_executor().timer(FRAME_INTERVAL).await;
                    if live.update(cx, |live, cx| live.frame(cx)).is_err() {
                        break;
                    }
                }
            });
            Live {
                bun,
                session: session.downgrade(),
                tools: tools.clone(),
                generation: generation.clone(),
                cards: HashMap::new(),
                next_card: 0,
                played: HashMap::new(),
                last_frame: Instant::now(),
                problems: Vec::new(),
                frame_out: None,
                _tasks: [hearing, framing],
            }
        })
    }
}

fn key(tool: &str) -> String {
    format!("{FOLDER}/{tool}")
}

/// The `state` of the record of `id`.
pub(crate) fn state_of(project: &Project, id: &InstanceId) -> Option<serde_json::Value> {
    serde_json::from_str(&project.state_json(id)?).ok()
}

/// An instance of `tool` as its code gets it: its record and its watches as they are now.
fn looped<'a>(project: &Project, id: &'a InstanceId, tool: &'a str) -> Looped<'a> {
    Looped {
        instance: id.as_str(),
        tool,
        state: state_of(project, id).unwrap_or_default(),
        watches: (project.watches(id).into_iter())
            .map(|(name, watch)| (name, watch.get()))
            .collect(),
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
    /// The last tree of a card. `None` until it first drew.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<Rc<Node>, String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// The watches a card last drew with.
    pub(crate) fn watches(&self, card: u64) -> Option<&BTreeMap<String, f32>> {
        self.cards.get(&card).map(|card| &card.watches)
    }

    /// The tool of a card, as it last loaded.
    pub(crate) fn info(&self, card: u64) -> Option<ToolInfo> {
        let tool = &self.cards.get(&card)?.tool;
        let tools = self.tools.borrow();
        tools.iter().find(|info| info.name == *tool).cloned()
    }

    /// Where the live control `name` of the card's instance is, and its range: where it was
    /// put last, or its default.
    pub(crate) fn live_value(&self, card: u64, name: &str) -> Option<(f32, f32, f32)> {
        let entry = self.cards.get(&card)?;
        let tools = self.tools.borrow();
        let info = tools.iter().find(|info| info.name == entry.tool)?;
        let Some((
            _,
            Control::Live {
                min, max, default, ..
            },
        )) = info.control(name)
        else {
            return None;
        };
        let played = self
            .played
            .get(&entry.id)
            .and_then(|played| played.get(name));
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
            version: 0,
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
        self.bun.send(&Request::Drop { card });
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
        self.bun.send(&Request::Render {
            card,
            instance: entry.id.as_str(),
            tool,
            state,
            watches: &entry.watches,
            page,
        });
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

    /// A click on an element of the card, or a press or a drag on a canvas at `x` and `y`
    /// across and down, 0 to 1.
    pub(crate) fn event(&self, card: u64, handler: usize, at: Option<(f32, f32)>) {
        let Some(entry) = self.cards.get(&card) else {
            return;
        };
        let (x, y) = (at.map(|at| at.0), at.map(|at| at.1));
        self.bun.send(&Request::Event {
            card,
            version: entry.version,
            handler,
            x,
            y,
        });
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
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let Some(info) = tools.iter().find(|info| Some(info.name.as_str()) == tool) else {
            return;
        };
        let update = match (info.control(name), value) {
            (Some((index, Control::Live { .. })), Some(value)) => HumUpdate::Live { index, value },
            (Some((index, Control::Trigger { .. })), None) => HumUpdate::Trigger {
                index,
                at: at.map(|at| frames_of(at, session.read(cx))),
            },
            _ => return eprintln!("error: {} has no control {name} that takes that", info.name),
        };
        let processor = info.processor();
        drop(tools);
        if let Some(value) = value {
            self.played
                .entry(id.clone())
                .or_default()
                .insert(name.to_string(), value);
            // A pad or a knob of the card shows where it is, also while nothing else redraws.
            cx.notify();
        }
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| project.send::<Hum>(id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// Plays a key of the voices of an instance, or lets go of one, as the keyboard would:
    /// `update` makes what the processor gets.
    fn note(
        &mut self,
        id: &InstanceId,
        update: impl FnOnce(&Session) -> HumUpdate,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let Some(info) = tools.iter().find(|info| Some(info.name.as_str()) == tool) else {
            return;
        };
        if info.kind == ToolKind::Effect {
            return eprintln!("error: {} is an effect, which plays no notes", info.name);
        }
        let processor = info.processor();
        drop(tools);
        let update = update(session.read(cx));
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| project.send::<Hum>(id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// A key of the computer keyboard went down or up on a card that has the keys.
    pub(crate) fn key(&mut self, card: u64, key: &str, down: bool, cx: &mut Context<Self>) {
        let (Some(session), Some(entry)) = (self.session.upgrade(), self.cards.get(&card)) else {
            return;
        };
        let id = entry.id.clone();
        let time = engine_time(&session, cx);
        let project = session.read(cx).project();
        let Some(tool) = project.tool_of(&id) else {
            return;
        };
        self.bun.send(&Request::Key {
            time,
            instance: looped(project, &id, tool),
            key,
            down,
        });
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
            .filter(|(id, tool)| {
                hears(id) && tools.iter().any(|info| info.name == *tool && info.midi)
            })
            .map(|(id, tool)| looped(project, id, tool))
            .collect();
        if !instances.is_empty() {
            self.bun.send(&Request::Midi {
                time,
                instances,
                messages,
            });
        }
    }

    fn stopped(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let message = "The tools of extensions/ stopped running: what plays goes on as it is, but cards, control loops and new sounds wait until the project opens again";
        session.update(cx, |session, cx| session.report(message, cx));
    }

    /// One frame of the window: the control loops run, and a card whose watches moved draws
    /// again.
    fn frame(&mut self, cx: &mut Context<Self>) {
        self.run_loops(cx);
        self.look_at_watches(cx);
        self.take_samples(cx);
    }

    /// Runs the behaviour of every instance whose sample was read on its thread again, so it
    /// plays it.
    fn take_samples(&mut self, cx: &mut Context<Self>) {
        let read = crate::samples::take_read();
        let Some(session) = self.session.upgrade().filter(|_| !read.is_empty()) else {
            return;
        };
        session.update(cx, |session, cx| {
            let failed = session.background(cx, |project| {
                // One deleted while its sample was read has nothing to play it.
                (read.iter())
                    .filter_map(|id| {
                        project.tool_of(id)?;
                        project.rebind(id).err()
                    })
                    .collect::<Vec<_>>()
            });
            for error in failed {
                session.report(error, cx);
            }
        });
    }

    /// Runs the control loop of every instance whose tool has one, cards or not.
    fn run_loops(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        if let Some(sent) = self.frame_out {
            if sent.elapsed() > ANSWER_TIMEOUT {
                let message = format!("ran a control loop for longer than {ANSWER_TIMEOUT:?}");
                self.bun.stop(&message);
            }
            return;
        }
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();
        let time = engine_time(&session, cx);
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        // An instance that is gone, which an undo may bring back at its defaults.
        self.played.retain(|id, _| project.tool_of(id).is_some());
        let looped: Vec<Looped> = (project.instances())
            .filter(|(_, tool)| tools.iter().any(|info| info.name == *tool && info.tick))
            .map(|(id, tool)| looped(project, id, tool))
            .collect();
        if !looped.is_empty() {
            self.bun.send(&Request::Frame {
                dt,
                time,
                instances: looped,
            });
            self.frame_out = Some(Instant::now());
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
                let ticks = tools
                    .iter()
                    .any(|info| info.name == entry.tool && info.tick);
                if !ticks {
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
            Event::Loaded(loaded) => self.loaded(loaded, cx),
            Event::Framed => self.frame_out = None,
            Event::Failed { tool, message } => {
                let file = (self.tools.borrow().iter())
                    .find(|info| info.name == tool)
                    .map(|info| info.file.clone());
                if let Some(file) = file {
                    self.problems.push(Problem {
                        path: format!("{FOLDER}/{file}"),
                        message: format!("tool {tool}: {message}"),
                    });
                    self.report(cx);
                }
            }
            Event::Tree {
                card,
                tree,
                version,
                error,
            } => {
                let Some(entry) = self.cards.get_mut(&card) else {
                    return;
                };
                entry.asked = false;
                entry.version = version;
                entry.tree = Some(match (tree, error) {
                    (Some(tree), _) => serde_json::from_value(tree)
                        .map(Rc::new)
                        .map_err(|error| error.to_string()),
                    (None, error) => Err(error.unwrap_or_default()),
                });
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
                let id = match InstanceId::new(&instance) {
                    Ok(id) => id,
                    Err(error) => return eprintln!("error: {error}"),
                };
                session.update(cx, |session, cx| {
                    session.edit(cx, |project| {
                        let mut edit = project.begin(&label);
                        project.update_json(&mut edit, &id, |old| *old = state)?;
                        project.finish(edit)
                    })
                });
            }
            Event::Control {
                instance,
                name,
                value,
                at,
            } => match InstanceId::new(&instance) {
                Ok(id) => self.control(&id, &name, value, at, cx),
                Err(error) => eprintln!("error: {error}"),
            },
            Event::Note {
                instance,
                pitch,
                velocity,
                seconds,
                at,
            } => match InstanceId::new(&instance) {
                Ok(id) => self.note(
                    &id,
                    |session| HumUpdate::Note {
                        pitch: Pitch::nearest(i64::from(pitch)),
                        velocity: Velocity::nearest(
                            (velocity.clamp(0.0, 1.0) * 127.0).round() as i64
                        ),
                        at: at.map(|at| frames_of(at, session)),
                        frames: seconds.map(|seconds| frames_of(f64::from(seconds), session)),
                    },
                    cx,
                ),
                Err(error) => eprintln!("error: {error}"),
            },
            Event::Release {
                instance,
                pitch,
                at,
            } => match InstanceId::new(&instance) {
                Ok(id) => self.note(
                    &id,
                    |session| HumUpdate::Release {
                        pitch: Pitch::nearest(i64::from(pitch)),
                        at: at.map(|at| frames_of(at, session)),
                    },
                    cx,
                ),
                Err(error) => eprintln!("error: {error}"),
            },
        }
        cx.notify();
    }

    /// A file of `extensions/` was saved: every tool is defined again, and every card draws
    /// again.
    fn loaded(&mut self, mut loaded: Loaded, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        // Every tool, changed or not: its file was saved, so its `sound` may make other Hum.
        let mut problems = crate::define(&self.bun, &mut loaded, |tools| {
            let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
            let defined = session.update(cx, |session, cx| {
                session.background(cx, |project| project.define_json_tools(tools))
            });
            match defined {
                Ok(refused) => (refused.into_iter())
                    .map(|(name, error)| (name, error.to_string()))
                    .collect(),
                // The folder did not read: no tool of the save plays.
                Err(error) => (names.into_iter())
                    .map(|name| (name, error.to_string()))
                    .collect(),
            }
        });
        let gone: Vec<ToolInfo> = (self.tools.borrow().iter())
            .filter(|info| !loaded.tools.iter().any(|tool| tool.name == info.name))
            .cloned()
            .collect();
        for info in &gone {
            problems.push(Problem {
                path: format!("{FOLDER}/{}", info.file),
                message: format!(
                    "the tool {} is gone from the code; its records play as they did until the project opens again",
                    info.name
                ),
            });
        }
        self.problems = problems;
        self.report(cx);
        *self.tools.borrow_mut() = gone.into_iter().chain(loaded.tools).collect();
        self.generation.set(self.generation.get() + 1);
        let cards: Vec<u64> = self.cards.keys().copied().collect();
        for card in cards {
            self.render(card, cx);
        }
    }

    /// Lists what is wrong with `extensions/` among the problems of the project.
    fn report(&self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let problems = self.problems.clone();
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
