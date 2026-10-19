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

use gpui::{App, AppContext, Context, Entity, PathPromptOptions, Task, WeakEntity};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use serde_json::Value;
use sound_core::{
    Changes, EventTap, GROUPING_WINDOW, InstanceId, Problem, Project, ProjectEvent, Scale,
    Sections, TempoMap, Ticks,
};
use sound_notes::{NOTES_INPUT, NoteEvent, NoteFeed, NoteFeedUpdate, Performed, Pitch, Velocity};
use sound_signals::{Signals, SignalsUpdate};
use sound_ui::components::device_card::{
    CARD_HEIGHT, CARD_PADDING, HEADER_HEIGHT, PLAIN_CARD_WIDTH,
};
use sound_ui::{
    DeviceLabel, DeviceOffer, Devices, FinishedTake, GlobalKeys, Loop, OfferGroup, Playhead,
    Session, Takes, Views,
};

use crate::bun::{
    Answer, Bun, Clock, Event, EventKind, Heard, MenuItemSpec, Moved, Offered, PageSize, Played,
    Reads, RecordEdit, Request, Runs, Skipped, Telling, Transport, TransportOp, record_key,
};
use crate::card::{Page, TypeScriptCard};
use crate::feeds::played_name;
use crate::recorder::{self, Recorder, RecorderUpdate};
use crate::timeline::{self, Timeline, TimelineUpdate};
use crate::tools::{Input, NOTES_TAP, Sounds, ToolInfo, ToolKind, feed_processor};
use crate::tree::{NodeTable, Uses};
use crate::{Extensions, FOLDER, Launched, NoteMessage, OWNED};

/// How often the cards look at their watches and the control loops run: often enough for a
/// step light, a meter or a moving drawing.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// The room a card in a rack gives its view: the body of a card, under its header. It grows
/// sideways as wide as its view needs.
const CARD_ROOM: PageSize = PageSize {
    width: (PLAIN_CARD_WIDTH - 2. * CARD_PADDING) as u32,
    height: (CARD_HEIGHT - HEADER_HEIGHT - CARD_PADDING) as u32,
};

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    /// The `extensions/` folder of the project.
    folder: PathBuf,
    session: WeakEntity<Session>,
    host: Host,
    /// Takes that ended while no Bun ran, for the next one to keep.
    held_takes: Vec<FinishedTake>,
    /// The opening of the project every Bun of the window runs for.
    opening: u64,
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
    /// A tell is due soon: a record changed that a binding reads.
    telling: bool,
    /// What the bindings of the cards of each instance read, as Bun said last.
    reads: HashMap<InstanceId, Reads>,
    /// The transport, the tempo map and the sections, and the watches of each instance Bun was
    /// told last.
    told_transport: Option<Transport>,
    told_song: Option<(TempoMap, Sections)>,
    told_watches: HashMap<InstanceId, BTreeMap<String, f32>>,
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
    /// The card that heard the last press or drag: a menu its code opens shows there.
    last_event_card: Option<u64>,
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
    /// Its nodes, or why there are none. `None` until its first ops arrive.
    tree: Option<Result<Rc<NodeTable>, String>>,
    /// The watches its live values read.
    watches: BTreeMap<String, f32>,
    /// Its code made its nodes, or is asked to.
    mounted: bool,
    /// Its nodes are an earlier Bun's: a press on them reaches nothing until the new ones come.
    stale: bool,
    /// The presses and drags of the card told to its code, and how many of them its code
    /// answered: a drag's preview stays until its answer.
    events_sent: u64,
    events_answered: u64,
    /// A menu its code opened and the card did not show yet, with its question.
    menu: Option<(u64, Vec<MenuItemSpec>, Vec<String>)>,
}

/// What a tree is drawn on.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Surface {
    /// A card in a rack.
    Card,
    /// The whole window: its size once laid out, which the page draws from.
    Page(Option<PageSize>),
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
    // The keys the tool of the main view takes wherever the focus is.
    GlobalKeys::set(
        {
            let (live, session, tools) = (live.downgrade(), session.downgrade(), tools.clone());
            move |key, cx| {
                let (Some(live), Some(session)) = (live.upgrade(), session.upgrade()) else {
                    return false;
                };
                let Some(main) = Views::main_instance(&session, cx) else {
                    return false;
                };
                let tool = session
                    .read(cx)
                    .project()
                    .tool_of(&main)
                    .map(str::to_string);
                let takes = tool.is_some_and(|tool| {
                    find(&tools.borrow(), &tool)
                        .is_some_and(|info| info.global_keys.contains(&key.name))
                });
                // A key it took comes up to it whatever it takes now.
                if !takes && key.down {
                    return false;
                }
                let (name, modifiers) = (key.keystroke.key.clone(), key.keystroke.modifiers);
                live.update(cx, |live, cx| {
                    live.key(&main, &name, key.down, modifiers, cx)
                });
                true
            }
        },
        cx,
    );

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
            move |tool| find(&tools.borrow(), tool).is_some_and(|info| info.view)
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
            let recording = session.read(cx).recording().clone();
            cx.subscribe(&recording, |live, _, take: &FinishedTake, cx| {
                live.take(take, cx)
            })
            .detach();
            cx.subscribe(session, |live, _, event, cx| match event {
                ProjectEvent::Created(id)
                | ProjectEvent::Changed(id)
                | ProjectEvent::Deleted(id) => {
                    live.changed_records.insert(id.clone());
                    if live.is_read(id) {
                        live.tell_soon(cx);
                    }
                }
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged => {}
            })
            .detach();
            let live = Live {
                folder,
                session: session.downgrade(),
                host,
                held_takes: Vec::new(),
                opening,
                skipped,
                tools: tools.clone(),
                generation: generation.clone(),
                cards: HashMap::new(),
                next_card: 0,
                played: HashMap::new(),
                changed_records: every_instance(session.read(cx).project()),
                telling: false,
                reads: HashMap::new(),
                told_transport: None,
                told_song: None,
                told_watches: HashMap::new(),
                taps: HashMap::new(),
                last_frame: Instant::now(),
                problems,
                _watcher: watcher,
                watch_problem,
                changed,
                _tasks: [hearing_changes, framing],
                last_event_card: None,
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

/// Whether Bun holds the record of `id`: one of a tool of the project, or one inside one at
/// any depth, which the code of that tool reads.
fn told_to_bun(project: &Project, tools: &[ToolInfo], id: &InstanceId) -> bool {
    let ours =
        |id: &InstanceId| (project.tool_of(id)).is_some_and(|tool| find(tools, tool).is_some());
    ours(id) || id.ancestors().any(|owner| ours(&owner))
}

/// The watches of `id` as its code reads them: its own, and those of each instance inside it
/// by its path from `id`, as `<child>.<watch>` or `<child>/<grandchild>.<watch>`, such as what
/// a looping timeline plays.
fn watches_of(project: &Project, id: &InstanceId) -> BTreeMap<String, f32> {
    let inside = (project.instances()).filter(|(inner, _)| inner.is_inside(id));
    let children = inside.flat_map(|(inner, _)| {
        let path = &inner.as_str()[id.as_str().len() + 1..];
        let watches = project.watches(inner).into_iter();
        watches.map(move |(name, watch)| (format!("{path}.{name}"), watch))
    });
    let watches = project.watches(id).into_iter().chain(children);
    // What a cable plays is read as it plays, by live values, and tells Bun nothing.
    let (played, inside) = (played_name(""), format!(".{}", played_name("")));
    let watches = watches.filter(|(name, _)| !name.starts_with(&played) && !name.contains(&inside));
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

/// Where the project plays now, as the playhead of the window shows it, and how it plays.
fn transport_of(session: &Session, cx: &App) -> Transport {
    let Playhead { playing, tick, .. } = *session.playhead().read(cx);
    let tempo_map = &session.project().project_file().tempo_map;
    let position = tempo_map.time_signatures().bar_beat_of(tick);
    let looped = session.looped().read(cx);
    Transport {
        playing,
        tick: tick.0,
        bar: position.bar,
        beat: position.beat,
        bpm: tempo_map.change_at(tick).bpm.bpm(),
        repeat: looped
            .on
            .then(|| [looped.range.start.0, looped.range.end.0]),
        click: session.click_on(),
        keys: session.computer_keys_on(),
    }
}

/// What a picker offers of the tools of `kind`: each one's record at its defaults. Not an owner,
/// whose place is the top of the project.
fn offers(tools: &[ToolInfo], kind: impl Fn(ToolKind) -> bool) -> Vec<DeviceOffer> {
    let tools = tools
        .iter()
        .filter(|info| kind(info.tool_kind()) && !info.behaviour);
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
    /// The nodes of a card. `None` until its first ops came.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<Rc<NodeTable>, String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// The watches the live values of a card read.
    pub(crate) fn watches(&self, card: u64) -> Option<&BTreeMap<String, f32>> {
        self.cards.get(&card).map(|card| &card.watches)
    }

    /// The tool of a card, as it last loaded.
    /// Goes up each time the tools load.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    pub(crate) fn info(&self, card: u64) -> Option<ToolInfo> {
        find(&self.tools.borrow(), &self.cards.get(&card)?.tool).cloned()
    }

    /// A new card of the instance, whose code makes its nodes at once.
    pub(crate) fn add(
        &mut self,
        id: InstanceId,
        tool: String,
        surface: Surface,
        cx: &mut Context<Self>,
    ) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        let watches = self
            .session
            .upgrade()
            .map(|session| watches_of(session.read(cx).project(), &id))
            .unwrap_or_default();
        let entry = Card {
            id,
            tool,
            surface,
            tree: None,
            watches,
            mounted: false,
            stale: false,
            events_sent: 0,
            events_answered: 0,
            menu: None,
        };
        self.cards.insert(card, entry);
        self.mount(card, cx);
        card
    }

    pub(crate) fn remove(&mut self, card: u64) {
        self.cards.remove(&card);
        self.host.send(&Request::Drop { card });
    }

    /// Asks the code of the card to make its nodes, from the records as they are now.
    fn mount(&mut self, card: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Some(entry) = self.cards.get(&card) else {
            return;
        };
        let size = match entry.surface {
            Surface::Card => CARD_ROOM,
            Surface::Page(Some(size)) => size,
            // It is made once it is laid out and has a size.
            Surface::Page(None) => return,
        };
        self.tell(cx);
        let clock = clock_of(&session, cx);
        self.told_transport = Some(clock.transport);
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        let request = Request::Mount {
            card,
            instance: &entry.id,
            watches: &entry.watches,
            size,
            clock,
        };
        // One that did not go is asked again by the next Bun.
        entry.mounted = self.host.send(&request);
        if entry.mounted {
            self.told_watches
                .insert(entry.id.clone(), entry.watches.clone());
        }
    }

    /// Makes again the cards of each instance whose record is of another tool now: its code is
    /// another's.
    fn mount_replaced(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let project = session.read(cx).project();
        let replaced: Vec<(u64, String)> = (self.cards.iter())
            .filter_map(|(card, entry)| {
                let tool = project.tool_of(&entry.id)?;
                (tool != entry.tool).then(|| (*card, tool.to_string()))
            })
            .collect();
        for (card, tool) in replaced {
            if let Some(entry) = self.cards.get_mut(&card) {
                entry.tool = tool;
                entry.tree = None;
            }
            self.host.send(&Request::Drop { card });
            self.mount(card, cx);
        }
    }

    /// Tells Bun soon the records that changed: once for all that change together.
    fn tell_soon(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.telling, true) {
            return;
        }
        cx.spawn(async move |live, cx| {
            live.update(cx, |live, cx| {
                live.telling = false;
                live.tell(cx);
                live.mount_replaced(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Whether a binding of a card reads the record of `id`: of its instance, or of one inside
    /// it at any depth.
    fn is_read(&self, id: &InstanceId) -> bool {
        let reads = |instance: &InstanceId| {
            let shown = self.cards.values().any(|card| card.id == *instance);
            (shown).then(|| self.reads.get(instance).cloned().unwrap_or_default())
        };
        let own = reads(id)
            .is_some_and(|reads| reads.state.as_ref().is_none_or(|fields| !fields.is_empty()));
        own || (id.ancestors()).any(|owner| reads(&owner).is_some_and(|reads| reads.children))
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

    /// The page was laid out at `size`: its code hears it when that is new.
    pub(crate) fn resize(&mut self, card: u64, size: PageSize, cx: &mut Context<Self>) {
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        if entry.surface == Surface::Page(Some(size)) {
            return;
        }
        entry.surface = Surface::Page(Some(size));
        if entry.mounted {
            self.host.send(&Request::Resize { card, size });
        } else {
            self.mount(card, cx);
        }
    }

    /// A press, a click or a drag on the node `node` of the card. The number of the event.
    pub(crate) fn event(
        &mut self,
        card: u64,
        node: u64,
        kind: EventKind,
        payload: serde_json::Value,
        cx: &mut App,
    ) -> u64 {
        let Some(session) = self.session.upgrade() else {
            return 0;
        };
        let Some(instance) = (self.cards.get(&card))
            .filter(|entry| !entry.stale)
            .map(|entry| entry.id.clone())
        else {
            return 0;
        };
        self.tell(cx);
        // Its handler reads the watches and the transport as they are now.
        let watches = watches_of(session.read(cx).project(), &instance);
        if self.told_watches.get(&instance) != Some(&watches)
            && self.host.send(&Request::Watches {
                instance: &instance,
                watches: &watches,
            })
        {
            self.told_watches.insert(instance, watches);
        }
        let clock = clock_of(&session, cx);
        if self.told_transport != Some(clock.transport) && self.host.send(&Request::Clock { clock })
        {
            self.told_transport = Some(clock.transport);
        }
        self.last_event_card = Some(card);
        let request = Request::Event {
            card,
            node,
            kind,
            payload,
            time: engine_time(&session, cx),
        };
        let sent = self.host.send(&request);
        let Some(entry) = self.cards.get_mut(&card) else {
            return 0;
        };
        entry.events_sent += u64::from(sent);
        entry.events_sent
    }

    /// The menu the code of the card opened, which the card shows now.
    pub(crate) fn take_menu(&mut self, card: u64) -> Option<(u64, Vec<MenuItemSpec>, Vec<String>)> {
        self.cards.get_mut(&card)?.menu.take()
    }

    /// Answers an `openMenu` with the place of the item picked, or none.
    pub(crate) fn menu_picked(&mut self, question: u64, item: Option<usize>) {
        self.host.send(&Request::Answer {
            question,
            answer: Answer::Item(item),
        });
    }

    /// Puts the offer `key` at the child `path` of `instance`, as one edit `label`: a device a
    /// view of the project picked.
    fn add_offer(
        &mut self,
        instance: &InstanceId,
        path: &str,
        key: &str,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let offer = [Takes::Notes, Takes::Audio]
            .into_iter()
            .flat_map(|takes| Devices::offered(takes, cx))
            .find(|offer| offer.key == key);
        let project = session.read(cx).project();
        let slot = path
            .split('/')
            .try_fold(instance.clone(), |id, name| id.child(name));
        let refused = match (offer, slot) {
            (None, _) => Some(format!("nothing is offered as {key:?}")),
            (_, Err(error)) => Some(error.to_string()),
            (Some(offer), Ok(slot)) => match offer.unavailable_in(project) {
                Some(reason) => Some(reason.to_string()),
                None => {
                    let mut changes = Changes::new();
                    if project.tool_of(&slot).is_some() {
                        changes.delete(&slot);
                    }
                    match offer.write(project, &slot, &mut changes) {
                        Err(error) => Some(error.to_string()),
                        Ok(()) => {
                            self.changed_records.insert(slot);
                            let published = session.update(cx, |session, cx| {
                                session.background(cx, |project| {
                                    project.publish_run(instance, label, changes)
                                })
                            });
                            published.err().map(|error| error.to_string())
                        }
                    }
                }
            },
        };
        if let Some(refused) = refused {
            let problem = format!("its addOffer {label:?} was refused: {refused}");
            self.tool_problem(instance, problem, cx);
        }
    }

    /// How many presses and drags of the card its code answered.
    pub(crate) fn events_answered(&self, card: u64) -> u64 {
        self.cards
            .get(&card)
            .map_or(0, |entry| entry.events_answered)
    }

    /// What the live values of the card's nodes move with.
    pub(crate) fn uses(&self, card: u64) -> Uses {
        match self.cards.get(&card).and_then(|entry| entry.tree.as_ref()) {
            Some(Ok(table)) => table.uses().clone(),
            _ => Uses::default(),
        }
    }

    /// Where each live control of the instance was put last.
    pub(crate) fn played_of(&self, id: &InstanceId) -> Option<&BTreeMap<String, f32>> {
        self.played.get(id)
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

    /// Moves the number input `name` of instance `id` to `value`, or sends the bang `name` when
    /// `value` is `None`, or says why it does not. Not an edit: nothing is saved.
    pub(crate) fn control(
        &mut self,
        id: &InstanceId,
        name: &str,
        value: Option<f32>,
        at: Option<f64>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let update = |info: &ToolInfo, session: &Session| match (info.input(name), value) {
            (Some((index, Input::Number { .. })), Some(value)) => Ok(SignalsUpdate::Live {
                index,
                value,
                at: at.map(|at| frames_of(at, session)),
            }),
            (Some((index, Input::Bang { .. })), None) => Ok(SignalsUpdate::Trigger {
                index,
                at: at.map(|at| frames_of(at, session)),
            }),
            _ => {
                let kind = if value.is_some() {
                    "number input"
                } else {
                    "bang input"
                };
                let names: Vec<&str> = (info.inputs.0.iter())
                    .filter(|(_, input)| matches!(input, Input::Number { .. }) == value.is_some())
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
        // A path such as `voice/rec` reaches deeper, but never out of the parent.
        let id = InstanceId::new(&format!("{}/{child}", parent.as_str()))
            .ok()
            .filter(|id| id.is_inside(parent) && project.tool_of(id).is_some());
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
        if let Some(session) = self.session.upgrade() {
            let tool = session.read(cx).project().tool_of(&played);
            if tool == Some(timeline::NAME) {
                return self.launch_on(id, &played, (name, value, at), &session, cx);
            }
            if tool == Some(recorder::RECORDER) {
                return self.record_on(id, &played, (name, value, at), &session, cx);
            }
        }
        if let Err(error) = self.control(&played, name, value, at, cx) {
            let problem = format!("a performer plays the child {child:?}, which {error}");
            self.tool_problem(id, problem, cx);
        }
    }

    /// Starts or stops the timeline `played`, a child of `id`, as `value` says, as a performer
    /// plays its input `launch`: nothing is saved.
    fn launch_on(
        &mut self,
        id: &InstanceId,
        played: &InstanceId,
        (name, value, at): (&str, Option<f32>, Option<f64>),
        session: &Entity<Session>,
        cx: &mut Context<Self>,
    ) {
        let Some(value) = value.filter(|_| name == timeline::LAUNCH) else {
            let problem = format!(
                "a performer plays {name:?} of the timeline {:?}, which takes only the number input `launch`",
                played.name()
            );
            return self.tool_problem(id, problem, cx);
        };
        let at = at.map(|at| performed_frame(Some(at), session, cx));
        let update = TimelineUpdate::Launch(timeline::launched(value), at);
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| {
                project.send::<Timeline>(played, timeline::PROCESSOR, update)
            });
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// Turns the input `record` or `arm` of the recorder `played`, a child of `id`, on or off as
    /// `value` says, as a performer does: nothing is saved.
    fn record_on(
        &mut self,
        id: &InstanceId,
        played: &InstanceId,
        (name, value, at): (&str, Option<f32>, Option<f64>),
        session: &Entity<Session>,
        cx: &mut Context<Self>,
    ) {
        let update = match (name, value, at) {
            (recorder::RECORD, Some(value), at) => {
                let at = at.map(|at| performed_frame(Some(at), session, cx));
                RecorderUpdate::Record(value > 0.0, at)
            }
            (recorder::ARM, Some(value), None) => RecorderUpdate::Arm(value > 0.0),
            (recorder::ARM, Some(_), Some(_)) => {
                let problem = format!(
                    "a performer arms the recorder {:?} with an `at`: arming opens the input at once, so send `arm` without one",
                    played.name()
                );
                return self.tool_problem(id, problem, cx);
            }
            _ => {
                let problem = format!(
                    "a performer plays {name:?} of the recorder {:?}, which takes only the number inputs `record` and `arm`",
                    played.name()
                );
                return self.tool_problem(id, problem, cx);
            }
        };
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| {
                project.send::<Recorder>(played, recorder::PROCESSOR, update)
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
            if crate::tools::feed_child(session.project().assets(), id, child) {
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

    /// A key of the computer keyboard went down or up on a card of `id` that has the keys, or
    /// wherever the focus is for the main view of `id`.
    pub(crate) fn key(
        &mut self,
        id: &InstanceId,
        key: &str,
        down: bool,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let clock = clock_of(&session, cx);
        self.told_transport = Some(clock.transport);
        self.tell(cx);
        let request = Request::Key {
            instance: played(session.read(cx).project(), id),
            key,
            down,
            shift: modifiers.shift,
            alt: modifiers.alt,
            cmd: modifiers.platform,
            ctrl: modifiers.control,
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

    /// Gives a take to the `onTake` of the parent of its recorder. One with none keeps nothing
    /// but the files, and says so.
    fn take(&mut self, take: &FinishedTake, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let parent = take.recorder.parent();
        let project = session.read(cx).project();
        let tool = parent.as_ref().and_then(|parent| project.tool_of(parent));
        let keeps = tool
            .is_some_and(|tool| find(&self.tools.borrow(), tool).is_some_and(|info| info.on_take));
        let (Some(parent), true) = (parent, keeps) else {
            let message = format!(
                "The take of {} was not kept: its parent has no `onTake`. Its files are under assets/.",
                take.recorder
            );
            return session.update(cx, |session, cx| session.report(message, cx));
        };
        let clock = clock_of(&session, cx);
        self.tell(cx);
        let instance = played(session.read(cx).project(), &parent);
        let sent = self.host.send(&Request::Take {
            instance,
            recorder: take.recorder.name(),
            name: &take.name,
            tool: take.tool,
            state: &take.state,
            clock,
        });
        if !sent {
            let message = format!(
                "The take of {} waits for the code of the project to run again.",
                take.recorder
            );
            session.update(cx, |session, cx| session.report(message, cx));
            self.held_takes.push(take.clone());
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
    pub(crate) fn frame(&mut self, cx: &mut Context<Self>) {
        self.hear_taps(cx);
        self.run_loops(cx);
        self.look_at_watches(cx);
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let assets = session.read(cx).project().assets();
        let mut read = crate::samples::take_read(assets);
        read.extend(crate::region::take_read(assets));
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
        let ticks =
            |tools: &[ToolInfo], tool: &str| find(tools, tool).is_some_and(|info| info.tick);
        // Only for code that runs now: a knob turned on a card tells Bun nothing.
        if (session.read(cx).project().instances())
            .any(|(_, tool)| ticks(&self.tools.borrow(), tool))
        {
            self.tell(cx);
        }
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        // An instance that is gone, which an undo may bring back at its defaults.
        self.played.retain(|id, _| project.tool_of(id).is_some());
        let instances: Vec<Played> = (project.instances())
            .filter(|(_, tool)| ticks(&tools, tool))
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

    /// Reads the watches of every card, which its live values follow, and tells Bun those its
    /// bindings read as they move, and the clock and the song when they changed.
    fn look_at_watches(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let mut moved = BTreeMap::new();
        let mut shown_watches_moved = false;
        let tools = self.tools.borrow();
        for entry in self.cards.values_mut() {
            let watches = watches_of(session.read(cx).project(), &entry.id);
            if watches == entry.watches {
                continue;
            }
            let reads = self.reads.get(&entry.id).cloned().unwrap_or_default();
            shown_watches_moved |= entry
                .tree
                .as_ref()
                .is_some_and(|tree| tree.as_ref().is_ok_and(|table| table.uses().watches));
            let read_moved = match &reads.watches {
                None => true,
                Some(names) => {
                    (names.iter()).any(|name| watches.get(name) != entry.watches.get(name))
                }
            };
            if read_moved {
                moved.insert(entry.id.clone(), watches.clone());
            }
            entry.watches = watches;
        }
        drop(tools);
        for (instance, watches) in moved {
            if self.host.send(&Request::Watches {
                instance: &instance,
                watches: &watches,
            }) {
                self.told_watches.insert(instance, watches);
            }
        }
        // What bindings follow of the clock, when it changed: never the playhead alone, which
        // moves on every frame.
        let transport = transport_of(session.read(cx), cx);
        let told = self.told_transport.map(|told| told.followed());
        if told != Some(transport.followed()) {
            let clock = clock_of(&session, cx);
            if self.host.send(&Request::Clock { clock }) {
                self.told_transport = Some(clock.transport);
            }
        }
        self.tell_song(cx);
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
            Event::Ops {
                card,
                ops,
                mount,
                event,
            } => {
                let Some(entry) = self.cards.get_mut(&card) else {
                    return;
                };
                if event {
                    entry.events_answered += 1;
                }
                let started = Instant::now();
                // Taken out, so a change applies in place rather than to a copy.
                let base = match entry.tree.take() {
                    _ if mount => Some(Rc::default()),
                    Some(Ok(table)) => Some(table),
                    // Failed, or not made yet: what comes before its nodes changes nothing.
                    other => {
                        entry.tree = other;
                        None
                    }
                };
                match (base, ops) {
                    (Some(mut table), ops) => {
                        let applied = ops.and_then(|ops| {
                            Rc::make_mut(&mut table).apply(ops)?;
                            Ok(table)
                        });
                        entry.stale &= !(mount && applied.is_ok());
                        entry.tree = Some(applied);
                    }
                    (None, Err(error)) => entry.tree = Some(Err(error)),
                    (None, Ok(_)) => {}
                }
                crate::bun::count_batch(started.elapsed());
            }
            Event::Reads { instance, reads } => {
                self.reads.insert(instance, reads);
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
            Event::OpenFile { question, title } => self.open_file(question, title, cx),
            Event::OpenMenu {
                question,
                items,
                notes,
            } => match (self.last_event_card).and_then(|card| self.cards.get_mut(&card)) {
                Some(card) => {
                    card.menu = Some((question, items, notes));
                    cx.notify();
                }
                None => self.menu_picked(question, None),
            },
            Event::Offers { question, takes } => {
                let offers = offered(takes_of(&takes), self.session.upgrade(), cx);
                self.host.send(&Request::Answer {
                    question,
                    answer: Answer::Offers {
                        offers,
                        notes: (Devices::notes_of(cx).iter())
                            .map(ToString::to_string)
                            .collect(),
                    },
                });
            }
            Event::AddOffer {
                instance,
                path,
                key,
                label,
            } => self.add_offer(&instance, &path, &key, &label, cx),
            Event::Numbers {
                question,
                instance,
                path,
            } => {
                let numbers = (self.session.upgrade())
                    .map(|session| moved_numbers(session.read(cx).project(), &instance, &path))
                    .unwrap_or_default();
                self.host.send(&Request::Answer {
                    question,
                    answer: Answer::Numbers(numbers),
                });
            }
            Event::Transport { op } => self.transport(op, cx),
            Event::Select {
                instance,
                path,
                span,
            } => self.select(&instance, path.as_deref(), span, cx),
            Event::SongEdit {
                instance,
                label,
                tempo_map,
                sections,
            } => self.edit_song(&instance, &label, tempo_map, sections, cx),
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

    /// Tells Bun the tempo map and the sections when they are not what it was told.
    fn tell_song(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let file = session.read(cx).project().project_file();
        let told = self.told_song.as_ref();
        if told.is_some_and(|(tempo_map, sections)| {
            *tempo_map == file.tempo_map && *sections == file.sections
        }) {
            return;
        }
        let (tempo_map, sections) = (file.tempo_map.clone(), file.sections.clone());
        if self.host.send(&Request::Song {
            tempo_map: &tempo_map,
            sections: &sections,
        }) {
            self.told_song = Some((tempo_map, sections));
        }
    }

    /// The code of `instance` selected its child at `path`, or nothing, and a stretch of ticks.
    /// A path it does not own is a problem of the tool, as its types do not check it.
    fn select(
        &mut self,
        instance: &InstanceId,
        path: Option<&str>,
        span: Option<[u64; 2]>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let selected = match path {
            None => None,
            Some(path) => {
                let project = session.read(cx).project();
                let id = InstanceId::new(&format!("{instance}/{path}")).ok();
                match id.filter(|id| project.tool_of(id).is_some()) {
                    Some(id) => Some(id),
                    None => {
                        let problem = format!("it selects {path:?}, which {instance} does not own");
                        self.tool_problem(instance, problem, cx);
                        return;
                    }
                }
            }
        };
        let span = span.map(|[from, to]| -> sound_ui::SpanOf {
            std::rc::Rc::new(move |_: &Project| Some((Ticks(from), Ticks(to))))
        });
        session.update(cx, |session, cx| {
            session.select(selected, cx);
            session.select_span(span, cx);
        });
    }

    /// Code played, stopped or moved the clock. What it changed reaches Bun with the next frame.
    fn transport(&mut self, op: TransportOp, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        session.update(cx, |session, cx| match op {
            TransportOp::Play => session.engine().play(),
            TransportOp::Pause => session.engine().pause(),
            TransportOp::Stop => session.engine().stop(),
            TransportOp::Jump { to, at: None } => session.engine().seek(Ticks(to)),
            TransportOp::Jump { to, at: Some(at) } => {
                session.engine().jump_at(Ticks(at), Ticks(to))
            }
            TransportOp::Repeat { range } => {
                let looped = match range {
                    Some([from, to]) => Loop {
                        range: Ticks(from)..Ticks(to),
                        on: true,
                    },
                    None => Loop {
                        on: false,
                        ..session.looped().read(cx).clone()
                    },
                };
                session.set_loop(looped, cx);
            }
            TransportOp::Click { on } => session.set_click(on, cx),
            TransportOp::Keys { on } => session.set_computer_keys(on, cx),
        });
    }

    /// The code of `instance` changed the tempo map and the sections: one edit, which lands as
    /// its record edits do. Bun hears the song again either way, as its own copy went ahead.
    fn edit_song(
        &mut self,
        instance: &InstanceId,
        label: &str,
        tempo_map: serde_json::Value,
        sections: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        self.told_song = None;
        let read = serde_json::from_value::<TempoMap>(tempo_map)
            .map_err(|error| format!("tempo_map: {error}"))
            .and_then(|tempo_map| {
                let sections = serde_json::from_value::<Sections>(sections)
                    .map_err(|error| format!("sections: {error}"))?;
                Ok((tempo_map, sections))
            });
        let refused = match read {
            Err(refused) => Some(refused),
            Ok((tempo_map, sections)) => {
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                changes.set_sections(sections);
                let published = session.update(cx, |session, cx| {
                    session.background(cx, |project| project.publish_run(instance, label, changes))
                });
                published.err().map(|error| error.to_string())
            }
        };
        if let Some(refused) = refused {
            let problem = format!("its clock.update {label:?} was refused: {refused}");
            self.tool_problem(instance, problem, cx);
        }
        self.tell_song(cx);
    }

    /// Opens a file the composer chooses, copies it into `assets/audio/`, and answers the
    /// `openFile` `question` of Bun with its name there, or with none.
    fn open_file(&mut self, question: u64, title: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            let answer = Answer::Asset(None);
            self.host.send(&Request::Answer { question, answer });
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(title.into()),
        });
        let assets = session.read(cx).project().assets().clone();
        cx.spawn(async move |live, cx| {
            let path = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(error)) => {
                    let error = format!("The file picker did not open: {error}");
                    let reported = session.clone();
                    cx.update(|cx| reported.update(cx, |session, cx| session.report(error, cx)));
                    None
                }
                _ => None,
            };
            let imported = match path {
                Some(path) => Some(
                    cx.background_spawn(async move { sound_media::import(&assets, &path) })
                        .await,
                ),
                None => None,
            };
            // Gone with the window: there is nothing left to answer.
            live.update(cx, |live, cx| {
                let asset = match imported {
                    Some(Ok(file)) => Some(file.asset.to_string()),
                    Some(Err(error)) => {
                        session.update(cx, |session, cx| session.report(error, cx));
                        None
                    }
                    None => None,
                };
                let asset = asset.as_deref();
                live.host.send(&Request::Answer {
                    question,
                    answer: Answer::Asset(asset),
                });
            })
            .ok();
        })
        .detach();
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
        if let Host::Running { bun, .. } = &self.host {
            bun.heard_edit();
            // No record changes to tell: Bun hears that the edit is done with, and shows the
            // records as they are again.
            if refused.is_some() {
                bun.tell([]);
            }
        }
        if let Some(refused) = refused {
            let problem =
                format!("its update {label:?} was refused, and none of it applied: {refused}");
            self.tool_problem(instance, problem, cx);
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
        cx.spawn(async move |live, cx| {
            // Ending a Bun waits for it, and a load up to a timeout of each step: not on the
            // thread that draws.
            let launched = (cx.background_executor())
                .spawn(async move {
                    if let Some(old) = old {
                        old.end();
                    }
                    crate::launch((&folder, opening), skipped)
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
        // The new Bun knows no card and has read nothing yet.
        self.reads.clear();
        self.told_transport = None;
        self.told_song = None;
        self.told_watches.clear();
        let cards: Vec<u64> = self.cards.keys().copied().collect();
        for card in cards {
            if let Some(entry) = self.cards.get_mut(&card) {
                entry.mounted = false;
                entry.stale = entry.tree.is_some();
            }
            self.mount(card, cx);
        }
        if matches!(self.host, Host::Running { .. }) {
            for take in std::mem::take(&mut self.held_takes) {
                self.take(&take, cx);
            }
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
    // A path from the instance: `track` or `track/clip`, at any depth.
    let child = |path: &str| instance.child(path).map_err(|error| error.to_string());
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
                // The core refuses one whose parent owns no children.
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

/// What a lane or a cable can move of the child of `instance` at `path`, in the order its
/// behaviour names them: none for a path that is no instance.
fn moved_numbers(project: &Project, instance: &InstanceId, path: &str) -> Vec<Moved> {
    let child = path
        .split('/')
        .filter(|name| !name.is_empty())
        .try_fold(instance.clone(), |id, name| id.child(name));
    let Ok(child) = child else {
        return Vec::new();
    };
    (project.automatable(&child))
        .filter_map(|name| {
            let number = project.automation(&child, name)?;
            let range = number.range;
            let scale = match range.scale {
                Scale::Linear => "linear",
                Scale::Logarithmic => "log",
                _ => "fader",
            };
            Some(Moved {
                name: name.to_string(),
                min: range.min,
                max: range.max,
                scale,
                value: number.record,
            })
        })
        .collect()
}

/// What a view of the project asks offers for: `"notes"` takes notes, anything else sound.
fn takes_of(takes: &str) -> Takes {
    match takes {
        "notes" => Takes::Notes,
        _ => Takes::Audio,
    }
}

/// What the window offers that takes `takes`, as code gets it. Commands, such as a scan, are
/// the window's own pickers'.
fn offered(takes: Takes, session: Option<Entity<Session>>, cx: &App) -> Vec<Offered> {
    let project = session.as_ref().map(|session| session.read(cx).project());
    (Devices::offered(takes, cx).into_iter())
        .filter(|offer| !offer.is_command())
        .map(|offer| Offered {
            unavailable: project
                .and_then(|project| offer.unavailable_in(project))
                .map(|reason| reason.to_string()),
            key: offer.key.to_string(),
            name: offer.name.to_string(),
            detail: offer.detail.as_ref().map(ToString::to_string),
            group: offer.group.label().to_string(),
            icon: offer.icon.to_string(),
        })
        .collect()
}
