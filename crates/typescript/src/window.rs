//! The tools of `extensions/` while the window is open: Bun runs their cards and control loops,
//! the pickers offer them, and the cards play their live controls and triggers and follow their
//! watches.
//!
//! Any change in `extensions/`, a crash or a hang of Bun starts a new Bun, which loads every
//! file again, so a helper a tool imports is read again too, and every `memory` starts again.
//! A tool whose code hung or stopped Bun is left out of the new one until the folder changes.
//! Nothing here waits for Bun: what it says comes as events.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Global, Task, WeakEntity};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde_json::Value;
use sound_core::{
    Changes, EventTap, GROUPING_WINDOW, InstanceId, Problem, Project, ProjectEvent, ToolSchema,
};
use sound_notes::{NOTES_INPUT, NoteEvent, NoteFeed, NoteFeedUpdate, Performed, Pitch, Velocity};
use sound_signals::{Signals, SignalsUpdate};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Playhead, Session, Views};

use crate::bun::{
    Bun, Clock, Event, Heard, PageSize, Played, RecordEdit, Request, Runs, Skipped, Telling,
    Transport, record_key,
};
use crate::card::{Page, TypeScriptCard};
use crate::sequencer::{self, Sequencer, SequencerUpdate};
use crate::tools::{Control, NOTES_TAP, Sounds, ToolInfo, ToolKind, feed_processor};
use crate::tree::Node;
use crate::{Extensions, FOLDER, Launched, NoteMessage, OWNED};

/// How often the cards look at their watches and the control loops run: often enough for a
/// step light, a meter or a moving drawing.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    /// The `extensions/` folder of the project.
    folder: PathBuf,
    session: WeakEntity<Session>,
    host: Host,
    /// The opening of the project every Bun of the window runs for.
    opening: u64,
    schemas: Arc<[ToolSchema]>,
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
    /// The records that changed since Bun was last told, see [`Live::tell`].
    changed_records: BTreeSet<InstanceId>,
    /// The tap of what reaches the notes of each instance whose tool hears them, and how far
    /// it was read.
    taps: HashMap<InstanceId, (EventTap<NoteEvent>, u64)>,
    /// When the control loops last ran.
    last_frame: Instant,
    /// What is wrong with `extensions/`: the last load, and what failed since.
    problems: Vec<Problem>,
    /// Hears the changes in `extensions/`.
    _watcher: Option<RecommendedWatcher>,
    /// What it cannot hear, a problem of its own.
    watch_problem: Option<Problem>,
    changed: smol::channel::Sender<()>,
    _tasks: [Task<()>; 2],
}

/// The Bun of the window.
enum Host {
    Running {
        bun: Arc<Bun>,
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
    /// Whether it went.
    fn send(&mut self, request: &Request) -> bool {
        match self {
            Self::Running { bun, .. } => bun.send(request),
            _ => false,
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
    /// The last tree has a meter or steps, which show the watches as they are.
    shows_watches: bool,
    /// The transport the last tree was asked with.
    transport: Option<Transport>,
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

/// The tools of the window, for what the window asks of them.
struct WindowTools(Entity<Live>);

impl Global for WindowTools {}

/// Whether the main view of `id`, the page of a tool of the project, shows the transport.
pub fn page_shows_transport(project: &Project, id: &InstanceId, cx: &App) -> bool {
    let Some(live) = cx.try_global::<WindowTools>() else {
        return false;
    };
    let tools = live.0.read(cx).tools.borrow();
    (project.tool_of(id))
        .is_some_and(|tool| find(&tools, tool).is_some_and(|info| info.page && info.transport))
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
            Some(cx.new(|_| Page(page)).into())
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
/// that may be imported changes, or a folder. Not hidden files, such as an editor's. With
/// what it cannot watch, which is a problem.
fn watch(
    folder: &Path,
    changed: smol::channel::Sender<()>,
) -> (Option<RecommendedWatcher>, Option<Problem>) {
    let not_heard = |path: &Path, error: notify::Error| {
        let message = format!("changes in {} are not heard: {error}", path.display());
        Some(crate::folder_problem(message))
    };
    let skipped = |name: &str| name.starts_with('.') || name == "node_modules";
    // FSEvents gives the real path of a change, Windows the path as it was watched, which may
    // have short names (`RUNNER~1`). Only what is inside counts: the folder itself may be in a
    // hidden one, such as a temporary folder.
    let roots = [
        dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()),
        folder.to_path_buf(),
    ];
    let counts = move |path: &Path, removed: bool| {
        let inside = (roots.iter())
            .find_map(|root| path.strip_prefix(root).ok())
            .unwrap_or_else(|| Path::new(path.file_name().unwrap_or_default()));
        let hidden = (inside.components()).any(|part| skipped(&part.as_os_str().to_string_lossy()));
        let importable = (path.extension()).is_some_and(|extension| {
            ["ts", "tsx", "mts", "js", "jsx", "mjs", "cjs", "json"]
                .iter()
                .any(|known| extension == *known)
        });
        // A folder that is gone is no folder any more.
        let folder = path.is_dir() || (removed && path.extension().is_none());
        let owned = OWNED.iter().any(|owned| inside == Path::new(owned));
        !hidden && !owned && (importable || folder)
    };
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            Ok(event) => event,
            Err(error) => return eprintln!("error: watching {FOLDER}/: {error}"),
        };
        // An event with no paths asks to look again at everything, as after a lost event.
        let (paths, removed) = (&event.paths, event.kind.is_remove());
        if !event.kind.is_access()
            && (paths.is_empty() || paths.iter().any(|path| counts(path, removed)))
        {
            changed.try_send(()).ok();
        }
    });
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => return (None, not_heard(folder, error)),
    };
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    let folders = (entries.map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| skipped(&name.to_string_lossy()))
        })
        .map(|path| (path, RecursiveMode::Recursive));
    // The first that fails says so; the others are watched all the same.
    let mut problem = None;
    for (path, mode) in
        std::iter::once((folder.to_path_buf(), RecursiveMode::NonRecursive)).chain(folders)
    {
        if let Err(error) = watcher.watch(&path, mode) {
            problem = problem.or_else(|| not_heard(&path, error));
        }
    }
    (Some(watcher), problem)
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
            opening,
            schemas,
            tools: loaded,
            skipped,
            problems,
        } = extensions;
        *tools.borrow_mut() = loaded;
        cx.new(|cx: &mut Context<Live>| {
            let (changed, changes) = smol::channel::unbounded();
            let (watcher, watch_problem) = watch(&folder, changed.clone());
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
            cx.subscribe(session, |live, _, event, _| match event {
                ProjectEvent::Created(id)
                | ProjectEvent::Changed(id)
                | ProjectEvent::Deleted(id) => {
                    live.changed_records.insert(id.clone());
                }
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged => {}
            })
            .detach();
            let live = Live {
                folder,
                session: session.downgrade(),
                host,
                opening,
                schemas,
                skipped,
                tools: tools.clone(),
                generation: generation.clone(),
                cards: HashMap::new(),
                next_card: 0,
                played: HashMap::new(),
                changed_records: every_instance(session.read(cx).project()),
                taps: HashMap::new(),
                last_frame: Instant::now(),
                problems,
                _watcher: watcher,
                watch_problem,
                changed,
                _tasks: [hearing_changes, framing],
            };
            // Only the watcher's problem is new.
            if live.watch_problem.is_some() {
                live.report(cx);
            }
            live
        })
    }

    /// `bun` as the Bun of the window, heard until it ends.
    fn running(bun: Arc<Bun>, cx: &mut Context<Self>) -> Host {
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

/// An instance as its code gets it, with its watches as they are now.
fn played<'a>(project: &Project, id: &'a InstanceId) -> Played<'a> {
    Played {
        instance: id,
        watches: watches_of(project, id),
    }
}

/// Every instance of `project`, as changed records when a Bun starts: it holds none.
fn every_instance(project: &Project) -> BTreeSet<InstanceId> {
    project.instances().map(|(id, _)| id.clone()).collect()
}

/// Whether Bun holds the record of `id`: one of a tool of the project, or of a child of one,
/// which the code of its parent reads.
fn told_to_bun(project: &Project, tools: &[ToolInfo], id: &InstanceId) -> bool {
    let ours =
        |id: &InstanceId| (project.tool_of(id)).is_some_and(|tool| find(tools, tool).is_some());
    ours(id) || id.parent().is_some_and(|parent| ours(&parent))
}

/// The watches of `id` as its code reads them: its own, and those of each child as
/// `<child>.<watch>`, such as what a sequencer plays.
fn watches_of(project: &Project, id: &InstanceId) -> BTreeMap<String, f32> {
    let children = (project.instances()).filter(|(child, _)| child.parent().as_ref() == Some(id));
    let children = children.flat_map(|(child, _)| {
        let watches = project.watches(child).into_iter();
        watches.map(|(name, watch)| (format!("{}.{name}", child.name()), watch))
    });
    let watches = project.watches(id).into_iter().chain(children);
    watches.map(|(name, watch)| (name, watch.get())).collect()
}

/// Where the engine is now, in seconds: the clock `at` counts on.
fn engine_time(session: &Entity<Session>, cx: &mut App) -> f64 {
    session.update(cx, |session, _| {
        let rate = f64::from(session.project().clock().sample_rate());
        let frames = session.engine().poll().map_or(0, |status| status.frames);
        frames as f64 / rate
    })
}

/// Where the sound and the piece are now.
fn clock_of(session: &Entity<Session>, cx: &mut App) -> Clock {
    Clock {
        time: engine_time(session, cx),
        transport: transport_of(session.read(cx), cx),
    }
}

/// Where the project plays now, as the playhead of the window shows it.
fn transport_of(session: &Session, cx: &App) -> Transport {
    let Playhead { playing, tick, .. } = *session.playhead().read(cx);
    let tempo_map = &session.project().project_file().tempo_map;
    let position = tempo_map.time_signatures().bar_beat_of(tick);
    Transport {
        playing,
        tick: tick.0,
        bar: position.bar,
        beat: position.beat,
        bpm: tempo_map.change_at(tick).bpm.bpm(),
    }
}

/// What a picker offers of the tools of `kind`: each one's record at its defaults. Not an owner,
/// whose place is the top of the project.
fn offers(tools: &[ToolInfo], kind: impl Fn(ToolKind) -> bool) -> Vec<DeviceOffer> {
    let tools = tools
        .iter()
        .filter(|info| kind(info.kind) && !info.behaviour);
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
            shows_watches: false,
            transport: None,
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
        self.tell(cx);
        let clock = clock_of(&session, cx);
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        let request = Request::Render {
            card,
            instance: &entry.id,
            watches: &entry.watches,
            page,
            clock,
        };
        // A render that did not go is not waited for: the next change asks again.
        if self.host.send(&request) {
            entry.asked = true;
            entry.transport = Some(clock.transport);
        }
    }

    /// Tells Bun the records that changed since it was last told, before it runs code that
    /// reads them.
    fn tell(&mut self, cx: &App) {
        let (Some(session), Host::Running { bun, .. }) = (self.session.upgrade(), &self.host)
        else {
            return;
        };
        let mut ids = std::mem::take(&mut self.changed_records);
        ids.extend(bun.take_unconfirmed());
        let project = session.read(cx).project();
        let tools = self.tools.borrow();
        let records: Vec<(&InstanceId, Option<(&str, String)>)> = (ids.iter())
            .map(|id| {
                let tool = (project.tool_of(id)).filter(|_| told_to_bun(project, &tools, id));
                (
                    id,
                    tool.and_then(|tool| Some((tool, project.state_json(id)?))),
                )
            })
            .collect();
        let telling = records.iter().map(|(id, record)| -> Telling {
            let record = (record.as_ref())
                .map(|(tool, state)| (*tool, record_key(tool, state), state.as_str()));
            (id, record)
        });
        if !bun.tell(telling) {
            drop(tools);
            self.changed_records = ids;
        }
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
        cx: &mut App,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        if self.cards.contains_key(&card) {
            self.tell(cx);
            let request = Request::Event {
                card,
                version,
                handler,
                at,
                time: engine_time(&session, cx),
            };
            self.host.send(&request);
        }
    }

    /// Sends what `update` makes for the tool of instance `id` to its processor, as the
    /// keyboard or a knob of a built-in tool would, or says why it does not.
    fn send_update(
        &self,
        id: &InstanceId,
        update: impl FnOnce(&ToolInfo, &Session) -> Result<SignalsUpdate, String>,
        cx: &mut App,
    ) -> Result<(), String> {
        let Some(session) = self.session.upgrade() else {
            return Err("plays in no window".to_string());
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let Some(info) = tool.and_then(|tool| find(&tools, tool)) else {
            let tool = tool.unwrap_or("nothing");
            return Err(format!("has no live controls or triggers: it is {tool}"));
        };
        let update = update(info, session.read(cx))?;
        let processor = info.processor();
        drop(tools);
        session.update(cx, |session, cx| {
            let sent =
                session.background(cx, |project| project.send::<Signals>(id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
        Ok(())
    }

    /// Moves the live control `name` of instance `id` to `value`, or fires the trigger `name`
    /// when `value` is `None`, or says why it does not. Not an edit: nothing is saved.
    pub(crate) fn control(
        &mut self,
        id: &InstanceId,
        name: &str,
        value: Option<f32>,
        at: Option<f64>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let update = |info: &ToolInfo, session: &Session| match (info.control(name), value) {
            (Some((index, Control::Live { .. })), Some(value)) => {
                Ok(SignalsUpdate::Live { index, value })
            }
            (Some((index, Control::Trigger { .. })), None) => Ok(SignalsUpdate::Trigger {
                index,
                at: at.map(|at| frames_of(at, session)),
            }),
            _ => {
                let kind = if value.is_some() {
                    "live control"
                } else {
                    "trigger"
                };
                let names: Vec<&str> = (info.controls.0.iter())
                    .filter(|(_, control)| {
                        matches!(control, Control::Live { .. }) == value.is_some()
                    })
                    .map(|(name, _)| name.as_str())
                    .collect();
                let names = match names.is_empty() {
                    true => "none".to_string(),
                    false => names.join(", "),
                };
                Err(format!("has no {kind} {name}: its {kind}s are {names}"))
            }
        };
        self.send_update(id, update, cx)?;
        if let Some(value) = value {
            self.played
                .entry(id.clone())
                .or_default()
                .insert(name.to_string(), value);
            // A pad or a knob of the card shows where it is, also while nothing else redraws.
            cx.notify();
        }
        Ok(())
    }

    /// The child `child` of `parent`, which a performer of `parent` plays. One it does not own
    /// is a problem of its tool.
    fn child_of(
        &mut self,
        parent: &InstanceId,
        child: &str,
        cx: &mut Context<Self>,
    ) -> Option<InstanceId> {
        let session = self.session.upgrade()?;
        let project = session.read(cx).project();
        // Not a grandchild, as a name with a `/` would give.
        let id = (parent.child(child).ok())
            .filter(|id| id.parent().as_ref() == Some(parent) && project.tool_of(id).is_some());
        if id.is_none() {
            let children: Vec<&str> = (project.instances())
                .filter(|(id, _)| id.parent().as_ref() == Some(parent))
                .map(|(id, _)| id.name())
                .collect();
            let children = match children.is_empty() {
                true => "it owns none".to_string(),
                false => format!("its children are {}", children.join(", ")),
            };
            let problem = format!(
                "a performer plays the child {child:?}, which {parent} does not own: {children}"
            );
            self.tool_problem(parent, problem, cx);
        }
        id
    }

    /// Moves a live control of instance `id`, or of its child `child`, or fires a trigger. What
    /// a child does not have is a problem of the tool, as its types do not check it.
    fn perform_control(
        &mut self,
        id: &InstanceId,
        child: Option<&str>,
        name: &str,
        value: Option<f32>,
        at: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let Some(child) = child else {
            if let Err(error) = self.control(id, name, value, at, cx) {
                eprintln!("error: {id} {error}");
            }
            return;
        };
        let Some(played) = self.child_of(id, child, cx) else {
            return;
        };
        if let Some(session) = self.session.upgrade()
            && session.read(cx).project().tool_of(&played) == Some(sequencer::NAME)
        {
            return self.launch_on(id, &played, (name, value), &session, cx);
        }
        if let Err(error) = self.control(&played, name, value, at, cx) {
            let problem = format!("a performer plays the child {child:?}, which {error}");
            self.tool_problem(id, problem, cx);
        }
    }

    /// Launches what `value` says on the sequencer `played`, a child of `id`, as a performer
    /// moves its input `launch` like a live control: nothing is saved.
    fn launch_on(
        &mut self,
        id: &InstanceId,
        played: &InstanceId,
        (name, value): (&str, Option<f32>),
        session: &Entity<Session>,
        cx: &mut Context<Self>,
    ) {
        let Some(value) = value.filter(|_| name == sequencer::LAUNCH) else {
            let problem = format!(
                "a performer plays {name:?} of the sequencer {:?}, which takes only the live control `launch`",
                played.name()
            );
            return self.tool_problem(id, problem, cx);
        };
        let update = SequencerUpdate::Launch(sequencer::launched(value));
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| {
                project.send::<Sequencer>(played, sequencer::PROCESSOR, update)
            });
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// Plays a key of instance `id`, or lets go of one, as the keyboard would: on its own
    /// voices, or into the `notes` of its child `child` through the feed it declares, so a
    /// built-in instrument plays it too.
    fn perform_note(
        &mut self,
        id: &InstanceId,
        child: Option<&str>,
        performed: Performed,
        cx: &mut Context<Self>,
    ) {
        let Some(child) = child else {
            let update = |_: &ToolInfo, _: &Session| Ok(SignalsUpdate::Note(performed));
            if let Err(error) = self.send_update(id, update, cx) {
                eprintln!("error: {id} {error}");
            }
            return;
        };
        let (Some(session), Some(played)) = (self.session.upgrade(), self.child_of(id, child, cx))
        else {
            return;
        };
        if (session.read(cx).project().input_port(&played, NOTES_INPUT)).is_none() {
            let problem = format!(
                "a performer plays notes on the child {child:?}, which has no `notes` input"
            );
            return self.tool_problem(id, problem, cx);
        }
        session.update(cx, |session, cx| {
            if crate::tools::feed_child(session.project().assets(), &played) {
                session.rebind(std::slice::from_ref(id), cx);
            }
            let update = NoteFeedUpdate::Note(performed);
            let feed = feed_processor(child);
            let sent =
                session.background(cx, |project| project.send::<NoteFeed>(id, &feed, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// `message` as a problem of the tool of `id`, once.
    fn tool_problem(&mut self, id: &InstanceId, message: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let problem = tool
            .and_then(|tool| find(&tools, tool))
            .map(|info| info.problem(message));
        drop(tools);
        if let Some(problem) = problem
            && !self.problems.contains(&problem)
        {
            self.problems.push(problem);
            self.report(cx);
        }
    }

    /// A key of the computer keyboard went down or up on a card that has the keys.
    pub(crate) fn key(&mut self, card: u64, key: &str, down: bool, cx: &mut Context<Self>) {
        let (Some(session), Some(entry)) = (self.session.upgrade(), self.cards.get(&card)) else {
            return;
        };
        let id = entry.id.clone();
        let clock = clock_of(&session, cx);
        self.tell(cx);
        let request = Request::Key {
            instance: played(session.read(cx).project(), &id),
            key,
            down,
            clock,
        };
        self.host.send(&request);
    }

    /// What reached the notes of every instance whose tool hears them since the last frame. A
    /// tap that was written over before it was read is a problem of its tool. While no Bun runs
    /// it waits, so what plays during a reload is heard once the new Bun is there.
    fn hear_taps(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Host::Running { .. }) = (self.session.upgrade(), &self.host) else {
            return;
        };
        let project = session.read(cx).project();
        let tools = self.tools.borrow();
        let taps: Vec<(InstanceId, String, EventTap<NoteEvent>)> = (project.instances())
            .filter(|(_, tool)| find(&tools, tool).is_some_and(|info| info.notes))
            .filter_map(|(id, tool)| {
                let tap = project.tap::<NoteEvent>(id, NOTES_TAP)?;
                Some((id.clone(), tool.to_string(), tap))
            })
            .collect();
        drop(tools);
        let mut read = std::mem::take(&mut self.taps);
        let mut heard = Vec::new();
        for (id, tool, tap) in taps {
            // A new tap, of a new instance or of a tool that hears notes again, from its start.
            let from = (read.remove(&id))
                .filter(|(kept, _)| kept.is(&tap))
                .map_or(0, |(_, from)| from);
            heard.clear();
            let tapped = tap.read(from, &mut heard);
            if tapped.missed > 0 {
                let missed = tapped.missed;
                let problem = format!(
                    "onNotes missed {missed} note events: the window was too busy to hear them in time"
                );
                self.tool_problem(&id, problem, cx);
            }
            self.hear(&id, &tool, &heard, &session, cx);
            self.taps.insert(id, (tap, tapped.next));
        }
    }

    /// Gives `heard` to the `onNotes` of the instance `id` of `tool`, when it has one.
    fn hear(
        &mut self,
        id: &InstanceId,
        tool: &str,
        heard: &[(u64, NoteEvent)],
        session: &Entity<Session>,
        cx: &mut Context<Self>,
    ) {
        let hears = find(&self.tools.borrow(), tool).is_some_and(|info| info.notes);
        if heard.is_empty() || !hears {
            return;
        }
        let clock = clock_of(session, cx);
        self.tell(cx);
        let session = session.read(cx);
        let rate = f64::from(session.project().clock().sample_rate());
        let events: Vec<Heard> = (heard.iter())
            .map(|(frame, event)| Heard {
                event: NoteMessage::from(*event),
                at: *frame as f64 / rate,
            })
            .collect();
        let instance = played(session.project(), id);
        let events = &events;
        self.host.send(&Request::Notes {
            instance,
            events,
            clock,
        });
    }

    /// One frame of the window: the control loops run, a card whose watches moved draws again,
    /// and every instance whose sample was read on its thread runs again, so it plays it.
    fn frame(&mut self, cx: &mut Context<Self>) {
        self.hear_taps(cx);
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
        let clock = clock_of(&session, cx);
        self.tell(cx);
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        // An instance that is gone, which an undo may bring back at its defaults.
        self.played.retain(|id, _| project.tool_of(id).is_some());
        let instances: Vec<Played> = (project.instances())
            .filter(|(_, tool)| find(&tools, tool).is_some_and(|info| info.tick))
            .map(|(id, _)| played(project, id))
            .collect();
        if !instances.is_empty() {
            let request = Request::Frame {
                dt,
                instances,
                clock,
            };
            let sent = self.host.send(&request);
            if let Host::Running { ticking, .. } = &mut self.host {
                *ticking = sent;
            }
        }
    }

    /// Reads the watches of every card; a card whose watches moved draws again, and so does a
    /// card of a tool with the transport when it moved.
    fn look_at_watches(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let mut moved = Vec::new();
        let mut shown_watches_moved = false;
        let tools = self.tools.borrow();
        let transport = transport_of(session.read(cx), cx);
        for (card, entry) in &mut self.cards {
            let info = find(&tools, &entry.tool);
            let watches = watches_of(session.read(cx).project(), &entry.id);
            // The control loop draws the card of its tool after every step anyway, but with
            // the transport of its last draw.
            let watches_changed = watches != entry.watches;
            let watches_moved = watches_changed && !info.is_some_and(|info| info.tick);
            shown_watches_moved |= watches_changed && entry.shows_watches;
            entry.watches = watches;
            let transport_moved =
                info.is_some_and(|info| info.transport) && entry.transport != Some(transport);
            if watches_moved || transport_moved {
                moved.push(*card);
            }
        }
        drop(tools);
        for card in &moved {
            self.render(*card, cx);
        }
        // What else the watches and the transport move comes with a tree.
        if shown_watches_moved {
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
                    let node: Node =
                        serde_json::from_value(tree).map_err(|error| error.to_string())?;
                    Ok((version, Rc::new(node)))
                }));
                entry.shows_watches =
                    matches!(&entry.tree, Some(Ok((_, node))) if node.shows_watches());
                if std::mem::take(&mut entry.stale) {
                    self.render(card, cx);
                }
            }
            Event::Unchanged { card } => {
                if let Some(entry) = self.cards.get_mut(&card) {
                    entry.asked = false;
                    if std::mem::take(&mut entry.stale) {
                        self.render(card, cx);
                    }
                }
                return;
            }
            Event::Edit {
                instance,
                label,
                changes,
            } => self.edit(&instance, &label, changes, cx),
            Event::Control {
                instance,
                child,
                name,
                value,
                at,
            } => self.perform_control(&instance, child.as_deref(), &name, value, at, cx),
            Event::Note {
                instance,
                child,
                pitch,
                velocity,
                seconds,
                at,
            } => {
                let Some(session) = self.session.upgrade() else {
                    return;
                };
                let at = performed_frame(at, &session, cx);
                let performed = {
                    let session = session.read(cx);
                    Performed::Play {
                        pitch: Pitch::nearest(i64::from(pitch)),
                        velocity: Velocity::nearest(
                            (velocity.clamp(0.0, 1.0) * 127.0).round() as i64
                        ),
                        at,
                        frames: frames_of(f64::from(seconds), session),
                    }
                };
                self.perform_note(&instance, child.as_deref(), performed, cx);
            }
            Event::Send {
                instance,
                child,
                event,
                at,
            } => {
                let Some(session) = self.session.upgrade() else {
                    return;
                };
                // The SDK sends no controller from 120 on, MIDI's channel mode messages.
                let Some(event) = event.event() else {
                    return eprintln!("error: {instance} sends {event:?}, which is no note event");
                };
                let at = performed_frame(at, &session, cx);
                self.perform_note(
                    &instance,
                    child.as_deref(),
                    Performed::Send { event, at },
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// What the code of `instance` changed, as one edit called `label`, on the records as they
    /// are now, in the run of `instance` (see [`Project::publish_run`]). What the records refuse
    /// is a problem of its tool, and nothing of the edit applies. Nothing for an instance that is
    /// gone, which code may edit after an `await`.
    fn edit(
        &mut self,
        instance: &InstanceId,
        label: &str,
        edits: Vec<RecordEdit>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let project = session.read(cx).project();
        let tools = self.tools.borrow();
        let info = (project.tool_of(instance)).and_then(|tool| find(&tools, tool));
        let changes = info.map(|info| changes_of(project, info, instance, edits));
        drop(tools);
        let refused = match changes {
            None => None,
            Some(Err(refused)) => Some(refused),
            Some(Ok((changes, touched))) => {
                self.changed_records.extend(touched);
                // Not an edit of the composer's: its notice stays, and a refusal is the tool's.
                let published = session.update(cx, |session, cx| {
                    session.background(cx, |project| project.publish_run(instance, label, changes))
                });
                published.err().map(|error| error.to_string())
            }
        };
        if let Some(refused) = refused {
            let problem =
                format!("its update {label:?} was refused, and none of it applied: {refused}");
            self.tool_problem(instance, problem, cx);
        }
        if let Host::Running { bun, .. } = &self.host {
            bun.heard_edit();
        }
    }

    /// Something changed in `extensions/`: maybe a fix, so every file runs again.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.skipped.clear();
        // A new folder is watched too.
        (self._watcher, self.watch_problem) = watch(&self.folder, self.changed.clone());
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
            // Ending a Bun waits for it: not on the thread that draws.
            if let Host::Running { bun, .. } = std::mem::replace(&mut self.host, Host::Stopped) {
                cx.background_executor()
                    .spawn(async move { bun.end() })
                    .detach();
            }
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
    fn launch(&mut self, old: Option<Arc<Bun>>, cx: &mut Context<Self>) {
        let (folder, opening, skipped) = (self.folder.clone(), self.opening, self.skipped.clone());
        let schemas = self.schemas.clone();
        cx.spawn(async move |live, cx| {
            // Ending a Bun waits for it, and a load up to a timeout of each step: not on the
            // thread that draws.
            let launched = (cx.background_executor())
                .spawn(async move {
                    if let Some(old) = old {
                        old.end();
                    }
                    crate::launch((&folder, opening, &schemas), skipped)
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
        // The new Bun holds no record, and its first frame is from now.
        if let Some(session) = self.session.upgrade() {
            self.changed_records = every_instance(session.read(cx).project());
        }
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
        let watched = self.watch_problem.clone();
        let problems = self.problems.iter().cloned().chain(watched).collect();
        session.update(cx, |session, cx| {
            session.background(cx, |project| {
                project.set_problems_in(&format!("{FOLDER}/"), problems);
            });
        });
    }
}

/// The changes of `edits` that the code of `instance` of the tool `info` made, on the records as
/// they are now, with each record they touch. Or what is wrong with one of them.
fn changes_of(
    project: &Project,
    info: &ToolInfo,
    instance: &InstanceId,
    edits: Vec<RecordEdit>,
) -> Result<(Changes, Vec<InstanceId>), String> {
    let child = |name: &str| match instance.child(name) {
        // A path would reach a grandchild.
        Ok(_) if name.contains('/') => Err(format!("the child {name:?} is named with a `/`")),
        Ok(id) => Ok(id),
        Err(error) => Err(error.to_string()),
    };
    let mut changes = Changes::new();
    let mut touched = Vec::new();
    for edit in edits {
        let id = match edit {
            RecordEdit::Fields {
                child: name,
                set,
                removed,
            } => {
                let id = name
                    .as_deref()
                    .map_or_else(|| Ok(instance.clone()), child)?;
                let (Some(tool), Some(Value::Object(mut fields))) =
                    (project.tool_of(&id), state_of(project, &id))
                else {
                    return Err(format!("{id} is gone"));
                };
                for name in removed {
                    fields.remove(&name);
                }
                fields.extend(set);
                project
                    .set_json(&mut changes, id.clone(), tool, Value::Object(fields))
                    .map_err(|error| error.to_string())?;
                id
            }
            RecordEdit::Record {
                child: name,
                tool,
                state,
            } => {
                if !info.behaviour {
                    return Err(format!(
                        "it adds the child {name:?}, but only a tool with a behaviour has children"
                    ));
                }
                let id = child(&name)?;
                project
                    .set_json(&mut changes, id.clone(), &tool, state)
                    .map_err(|error| error.to_string())?;
                id
            }
            RecordEdit::Delete { child: name } => {
                let id = child(&name)?;
                changes.delete(&id);
                id
            }
        };
        touched.push(id);
    }
    Ok((changes, touched))
}

/// The frame of what a performer plays at `at`, engine time in seconds, or now: never before
/// now, as a processor made since the last block does not know where engine time is.
fn performed_frame(at: Option<f64>, session: &Entity<Session>, cx: &mut App) -> u64 {
    let now = engine_time(session, cx);
    frames_of(at.map_or(now, |at| at.max(now)), session.read(cx))
}

/// The frame of engine time at `seconds` of it.
fn frames_of(seconds: f64, session: &Session) -> u64 {
    let rate = f64::from(session.project().clock().sample_rate());
    (seconds.max(0.0) * rate).round() as u64
}
