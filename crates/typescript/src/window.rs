//! The tools of `extensions/` for the window: Bun runs their views of
//! `sdk/view.ts`, a plain function of the records each, and the actions on them. The window
//! calls [`Tools::poll`] at the start of each frame, and [`Tools::run_loops`] every
//! [`FRAME_INTERVAL`]; nothing here waits for Bun but [`Tools::settle`], which a run without a
//! window calls before each frame so the same project always draws the same.
//!
//! Any change in `extensions/`, a crash or a hang of Bun starts a new Bun.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::RecommendedWatcher;
use serde_json::{Map, Value, json};
use sound_core::feeds::{Feed, FeedContext};
use sound_core::{
    Changes, GROUPING_WINDOW, InstanceId, Problem, Project, ProjectEdit, ProjectEvent, Sections,
    TempoMap, Ticks,
};
use sound_core::{EventTap, Timed};
use sound_notes::{NOTES_INPUT, NoteEvent, NoteFeed, NoteFeedUpdate, Performed, Pitch, Velocity};
use sound_signals::{Signals, SignalsUpdate};

/// Opens the own window of an instance, or brings it forward: the app's, for `openWindow` of
/// code. `Err` says why there is none.
pub type WindowOpener = Box<dyn Fn(&Project, &InstanceId) -> Result<(), String>>;

pub use crate::bun::{Answer, Clock, Transport, Waker};
use crate::bun::{
    Bun, Event, Heard, Played, RecordEdit, Request, Runs, Skipped, Telling, TransportOp, record_key,
};
use crate::devices::{Group, Offer, Takes};
use crate::project::{
    changes_of, control_update, every_instance, find, played, told_to_bun, watch,
};
use crate::recorder::{ARM, PROCESSOR as RECORDING, RECORD, RECORDER, Recorder, RecorderUpdate};
use crate::timeline::{self, Timeline, TimelineUpdate};
use crate::tools::{Choice, Field, NOTES_TAP, SOUNDING_TAP, ToolInfo, feed_child, feed_processor};
use crate::{Extensions, FOLDER, Launched, NoteMessage, path};

/// The file of a project's design tokens, in `extensions/`.
pub const THEME: &str = "theme.json";

/// The file of a project that draws the card of a device whose tool has no
/// view, from the fields of its record. Bun loads it with the tools.
pub const GENERIC_CARD: &str = "ui/blocks/generic-card.tsx";

/// The folder of a project's cards of built-in tools: `<tool>.tsx` draws the devices of `tool`
/// in place of the generic card.
const CARDS: &str = "ui/blocks/cards";

/// The project's cards of built-in tools in `extensions`, which Bun loads after the generic card.
pub(crate) fn card_files(extensions: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(extensions.join(CARDS))
        .into_iter()
        .flatten()
        .flatten();
    let mut files: Vec<String> = (entries.filter_map(|entry| entry.file_name().into_string().ok()))
        .filter(|name| name.ends_with(".tsx") && !name.contains(".test."))
        .map(|name| format!("{CARDS}/{name}"))
        .collect();
    files.sort();
    files
}

/// Whether the file `file` of `extensions/` is a card: the generic one or one of a tool.
fn is_card(file: &str) -> bool {
    file == GENERIC_CARD || file.starts_with(&format!("{CARDS}/"))
}

/// What a tool whose file was written for the views of an older Sound Tools says.
pub const OLD_VIEWS: &str = "This was made for an older Sound Tools, whose views are gone. It plays as before; the agent can bring its view back.";

/// What a device shows whose card has a mistake.
const MISTAKE: &str = "This device's card has a mistake, so it shows nothing until it is mended.";

/// How long [`Tools::settle`] waits for Bun at most, so a view in an endless loop cannot stop
/// a run without a window. Bun calls code hung after 2 s itself.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a Bun that is stopped has to say what it said before: a process its code started
/// may hold its pipe.
const LAST_WORDS: Duration = Duration::from_millis(200);

/// How often the control loops run.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Why an action on a view did nothing while no Bun runs.
const NOT_RUNNING: &str = "the code of the project does not run now, so this did nothing";

/// What the window shows, see [`Tools::show`].
#[derive(Default)]
pub struct Shown {
    pub instances: BTreeSet<InstanceId>,
    /// The shown instances whose tools have no view, which the project's card of their tool or
    /// its generic card draws.
    pub cards: BTreeSet<InstanceId>,
    /// The numbers a lane moves of each shown instance, see `holds`.
    pub holds: BTreeMap<InstanceId, Vec<String>>,
    /// The numbers a lane moves that a hand writes into now, as `holds` names them: an update
    /// of one is a [`Want::Write`].
    pub writes: BTreeMap<InstanceId, Vec<String>>,
    /// What the window knows of each shown instance and of each of its children that no record
    /// holds: `Facts` in `sdk/view.ts`.
    pub facts: BTreeMap<InstanceId, Value>,
    pub catalog: Vec<Offer>,
    /// The key each action of a tool has now, by `<tool>.<action>`, or none.
    pub keys: BTreeMap<String, Option<String>>,
}

/// What [`Tools::run_loops`] did with a step of the control loops.
#[derive(Debug, PartialEq)]
pub enum Looped {
    Sent,
    /// Bun still runs the last step: the time keeps for the next.
    Busy,
    /// No Bun runs, or no instance has a control loop: no time keeps.
    Nothing,
}

/// A tree of a view as the window keeps it: what [`Tools::read_trees`] made of the JSON Bun
/// sent, which is dropped then.
pub type Tree = Rc<dyn Any>;

/// What the window shows of an instance: the tree of its view, or why there is none.
pub enum Drawn {
    View {
        root: Tree,
        /// The parts a `memo` built, by key, with the number of the Bun and of the build.
        memos: HashMap<String, ((u64, u64), Tree)>,
    },
    /// Its tool has no view of `sdk/view.ts`, or is no tool of the project.
    None,
    Failed(String),
    /// The generic card does not draw it: why, in plain words.
    NoCard(String),
}

/// A question of code, by the load of the Bun that asked it: each Bun numbers its own, so an
/// answer goes to no other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Question {
    load: u64,
    number: u64,
}

/// What the composer reads who answered code that started again since it asked.
pub const ASKED_BEFORE: &str =
    "The code of the project started again meanwhile, so this did nothing. Do it again.";

/// What code asks of the window that only the window does: the composer's choices that no record
/// holds, and the questions only the composer answers. [`Tools::take_wants`] gives them; the
/// answer to a question goes back with [`Tools::answer`].
#[derive(Debug, PartialEq)]
pub enum Want {
    /// What the composer works on: an instance, or nothing, and a stretch of ticks.
    Select {
        instance: Option<InstanceId>,
        span: Option<Range<Ticks>>,
    },
    /// Show a range of the plane `key` of the view of `instance`, across and down.
    Show {
        instance: InstanceId,
        key: String,
        x: Option<[f32; 2]>,
        y: Option<[f32; 2]>,
    },
    /// Playback repeats over the range, or stops repeating and keeps its range.
    Repeat(Option<Range<Ticks>>),
    Click(bool),
    ComputerKeys(bool),
    WriteAutomation(bool),
    /// An update of `instance`'s code named `label` set numbers a hand writes into a lane now,
    /// by path as `drag.plays` names them: they go to the lane, not to the record.
    Write {
        instance: InstanceId,
        label: String,
        values: Vec<(String, f64)>,
    },
    /// A yes or no, its yes button `confirm` and the other Cancel: [`Answer::Confirmed`].
    Confirm {
        question: Question,
        message: String,
        detail: Option<String>,
        confirm: String,
    },
    /// A file the composer chooses, copied into `assets/audio/`: [`Answer::Asset`].
    OpenFile {
        question: Question,
        title: String,
    },
    /// Where to write the MIDI file `bytes`, offered as `name`: [`Answer::Confirmed`] once written.
    ExportNotes {
        question: Question,
        name: String,
        bytes: Vec<u8>,
    },
    /// The sound of `instance` alone over `span`, with what still sounds after it when `tail`,
    /// into a new file of `assets/audio/` named after `name`: [`Answer::Asset`].
    Render {
        question: Question,
        instance: InstanceId,
        span: Range<Ticks>,
        tail: bool,
        name: String,
    },
}

/// A take a `recorder` made, for the `onTake` of its parent: the record of the child that would
/// keep it, a `timeline` of notes or a `region` of sound, whose files are written.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Take {
    pub recorder: InstanceId,
    /// A name of the files it wrote, for the child that keeps it.
    pub name: String,
    pub tool: String,
    pub state: Value,
}

/// The takes no code kept when the project closed, for its next open.
const WAITING_TAKES: &str = ".sound-tools/takes.json";

pub struct Tools {
    folder: PathBuf,
    opening: u64,
    skipped: Skipped,
    bun: Option<Arc<Bun>>,
    /// A new Bun that loads on a thread of its own, and whether the folder changed meanwhile.
    starting: Option<(mpsc::Receiver<Result<Option<Launched>, String>>, bool)>,
    tools: Vec<Rc<ToolInfo>>,
    problems: Vec<Problem>,
    watch_problem: Option<Problem>,
    /// Whether the system's watcher hears `extensions/`. It watches anew at every start, so a
    /// folder made since is watched too.
    watched: bool,
    _watcher: Option<RecommendedWatcher>,
    changes: mpsc::Receiver<()>,
    /// When the folder last changed, while a restart waits for it to be quiet.
    changed_at: Option<Instant>,
    changed_records: BTreeSet<InstanceId>,
    /// A drag plays numbers: changed records wait for its end, so no view runs as it moves.
    deferred: bool,
    shown: BTreeSet<InstanceId>,
    cards: BTreeSet<InstanceId>,
    /// The numbers a lane moves of each shown instance.
    holds: BTreeMap<InstanceId, Vec<String>>,
    writes: BTreeMap<InstanceId, Vec<String>>,
    told_shown: bool,
    drawn: HashMap<InstanceId, Drawn>,
    /// Why a view does not draw, by instance, until it draws again.
    view_problems: BTreeMap<InstanceId, Problem>,
    /// The edit of the action on a view that runs now: one step of undo, finished once it ran.
    acting: Option<ProjectEdit>,
    /// How many actions went to Bun that it did not say it ran.
    acts: usize,
    /// A gesture over several elements, such as a paint: its actions are one step of undo,
    /// which ends once it is let go and they ran.
    held: Held,
    /// The label of the last update an action on a view made, until the window takes it.
    updated: Option<String>,
    /// Why actions on views did nothing, oldest first, for whoever asked: `--act`, or a notice.
    failed_actions: Vec<String>,
    /// Whether Bun runs a frame of the control loops now.
    ticking: bool,
    told_transport: Option<Transport>,
    /// Where the code of the running Bun played: an instance's own voices and controls, or the
    /// `notes` of its child, by path. Released when that Bun ends.
    performed_on: BTreeSet<(InstanceId, Option<String>)>,
    /// How many times Buns loaded, or none could: the window reads its design tokens again.
    loads: u64,
    /// How many trees came, for a test that a frame runs no view.
    view_runs: u64,
    /// How many times what a view draws came, a tree or not.
    trees_heard: u64,
    read: fn(Value) -> Tree,
    /// What code asked of the window since it last looked.
    wants: Vec<Want>,
    /// The tempo map and the sections as Bun was told them.
    told_song: Option<(TempoMap, Sections)>,
    /// What `addOffer` puts, as the window last showed it, and as Bun was told it.
    catalog: Vec<Offer>,
    told_catalog: Value,
    facts: BTreeMap<InstanceId, Value>,
    keys: BTreeMap<String, Option<String>>,
    /// The clock of the last step of the control loops, which other requests carry too.
    clock: Option<Clock>,
    /// Takes that ended while no Bun ran, or that one never did, for the next one to keep.
    held_takes: Vec<Take>,
    /// Takes given to the running Bun, by the number of what it was told, until it says it
    /// did them.
    taking: Vec<(u64, Take)>,
    /// The tap of what reached the notes of each instance whose tool has `onNotes`, and how
    /// far it was read.
    taps: HashMap<InstanceId, (EventTap<NoteEvent>, u64)>,
    waker: Waker,
    open_window: Option<WindowOpener>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    No,
    Yes,
    Cancelled,
}

impl Tools {
    /// The tools of `extensions` for the window of `project`. `watched`: a change in
    /// `extensions/` starts a new Bun; a test tells it with [`Tools::code_changed`] instead.
    pub fn start(extensions: Extensions, project: &Project, watched: bool, waker: Waker) -> Self {
        let Extensions {
            folder,
            bun,
            opening,
            tools,
            skipped,
            problems,
        } = extensions;
        let (send_changes, changes) = mpsc::channel();
        let (watcher, watch_problem) = match watched {
            true => watching(&folder, send_changes, waker.clone()),
            false => (None, None),
        };
        let mut this = Self {
            folder,
            opening,
            skipped,
            bun: None,
            starting: None,
            tools: tools.into_iter().map(Rc::new).collect(),
            problems,
            watch_problem,
            watched,
            _watcher: watcher,
            changes,
            changed_at: None,
            changed_records: every_instance(project),
            deferred: false,
            shown: BTreeSet::new(),
            cards: BTreeSet::new(),
            holds: BTreeMap::new(),
            writes: BTreeMap::new(),
            told_shown: true,
            drawn: HashMap::new(),
            view_problems: BTreeMap::new(),
            acting: None,
            acts: 0,
            held: Held::No,
            updated: None,
            failed_actions: Vec::new(),
            ticking: false,
            told_transport: None,
            performed_on: BTreeSet::new(),
            loads: 0,
            view_runs: 0,
            trees_heard: 0,
            read: |value| Rc::new(value),
            wants: Vec::new(),
            told_song: None,
            catalog: Vec::new(),
            told_catalog: Value::Null,
            facts: BTreeMap::new(),
            keys: BTreeMap::new(),
            clock: None,
            held_takes: waiting_takes(project),
            taking: Vec::new(),
            taps: HashMap::new(),
            waker,
            open_window: None,
        };
        if let Some(bun) = bun {
            this.listen(bun);
            this.offer_held_takes(project);
        }
        this
    }

    /// Where `openWindow` of code goes. Without one it is refused.
    pub fn open_windows_with(&mut self, opener: WindowOpener) {
        self.open_window = Some(opener);
    }

    /// Hears `bun` from now on: what it says wakes the window.
    fn listen(&mut self, bun: Arc<Bun>) {
        bun.wake_with(self.waker.clone());
        self.bun = Some(bun);
        (self.told_shown, self.told_song) = (false, None);
    }

    /// What code asked of the window since the last call, oldest first.
    pub fn take_wants(&mut self) -> Vec<Want> {
        std::mem::take(&mut self.wants)
    }

    /// A question the running Bun asks.
    fn question(&self, number: u64) -> Question {
        Question {
            load: self.loads,
            number,
        }
    }

    /// Whether the Bun that asked `question` runs still, which an answer goes to.
    pub fn asks(&self, question: Question) -> bool {
        question.load == self.loads && self.bun.is_some()
    }

    /// The window's answer to the question of a [`Want`], to the Bun that asked it. Whether it
    /// went: an answer for a Bun that ended is nobody's.
    pub fn answer(&mut self, question: Question, answer: &Answer) -> bool {
        let Some(bun) = self.bun.as_ref().filter(|_| question.load == self.loads) else {
            return false;
        };
        let question = question.number;
        bun.send(&Request::Answer { question, answer });
        true
    }

    /// What the window shows of `id`, once a Bun said. `None` while one may still say: a Bun
    /// runs or starts.
    pub fn drawn(&self, id: &InstanceId) -> Option<&Drawn> {
        self.drawn.get(id)
    }

    /// Whether a view not drawn yet may still come, see [`Tools::drawn`].
    pub fn drawing(&self) -> bool {
        self.bun.is_some() || self.starting.is_some()
    }

    /// How many times Buns loaded: the design tokens may have changed with the code.
    pub fn loads(&self) -> u64 {
        self.loads
    }

    pub fn view_runs(&self) -> u64 {
        self.view_runs
    }

    /// How many records the running Bun was told.
    pub fn records_told(&self) -> u64 {
        self.bun.as_ref().map_or(0, |bun| bun.records_told())
    }

    /// How many times what a view draws came: a tree, or why there is none.
    pub fn trees_heard(&self) -> u64 {
        self.trees_heard
    }

    /// Keeps each tree that comes as `read` makes it of its JSON: as the JSON without it.
    pub fn read_trees(&mut self, read: fn(Value) -> Tree) {
        self.read = read;
    }

    /// What is wrong with `extensions/`.
    pub fn problems(&self) -> impl Iterator<Item = &Problem> {
        (self.problems.iter())
            .chain(&self.watch_problem)
            .chain(self.view_problems.values())
    }

    /// What the window shows now, with what it knows besides the records. Whether that is news
    /// to Bun.
    pub fn show(&mut self, shown: Shown) -> bool {
        let Shown {
            instances,
            cards,
            holds,
            writes,
            facts,
            catalog,
            keys,
        } = shown;
        // Bun never reads it.
        self.writes = writes;
        let told = serde_json::to_value(&catalog).unwrap_or_default();
        if instances == self.shown
            && cards == self.cards
            && holds == self.holds
            && facts == self.facts
            && told == self.told_catalog
            && keys == self.keys
        {
            return false;
        }
        (self.holds, self.facts, self.keys) = (holds, facts, keys);
        (self.catalog, self.told_catalog) = (catalog, told);
        // A built-in device a card shows is told to Bun, as one inside a tool of the project is.
        let new: Vec<InstanceId> = (cards.iter())
            .filter(|id| !self.cards.contains(*id))
            .cloned()
            .collect();
        self.changed_records.extend(new);
        self.drawn.retain(|id, _| instances.contains(id));
        (self.shown, self.cards) = (instances, cards);
        self.told_shown = false;
        true
    }

    /// Runs `action` of the element `key` of the view of `instance`, as a pointer or an agent
    /// does: what it changes is one step of undo. Why it did nothing comes from
    /// [`Tools::take_failed_actions`] once Bun ran it, or here when no Bun runs.
    pub fn act(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) -> Result<(), String> {
        let request = Request::Act {
            instance,
            key,
            action,
            value,
        };
        match &self.bun {
            Some(bun) if bun.send(&request) => {
                self.acts += 1;
                Ok(())
            }
            _ => Err(NOT_RUNNING.to_string()),
        }
    }

    /// Whether Bun said it ran every action sent to it.
    pub fn answered(&self) -> bool {
        self.acts == 0
    }

    /// While `deferred`, records that change are told to Bun only once it is not.
    pub fn defer_records(&mut self, deferred: bool) {
        self.deferred = deferred;
    }

    /// The label of the last update an action on a view made since the last call, landed or
    /// written into a lane.
    pub fn take_update(&mut self) -> Option<String> {
        self.updated.take()
    }

    /// Holds a gesture over several elements, such as a paint, or lets it go: the actions while
    /// it is held are one step of undo.
    pub fn hold(&mut self, project: &mut Project, held: bool) {
        self.held = if held { Held::Yes } else { Held::No };
        if !held && self.acts == 0 {
            self.finish_action(project);
        }
    }

    /// Lets go of a held gesture and takes back what its actions did, once they all ran.
    pub fn cancel_hold(&mut self, project: &mut Project) {
        self.held = Held::Cancelled;
        if self.acts == 0 {
            self.finish_action(project);
        }
    }

    /// The step of undo of the actions that ran ends, or goes back if their gesture was
    /// cancelled.
    fn finish_action(&mut self, project: &mut Project) {
        let cancelled = std::mem::replace(&mut self.held, Held::No) == Held::Cancelled;
        let Some(edit) = self.acting.take() else {
            return;
        };
        let ended = match cancelled {
            true => project.cancel(edit),
            false => project.finish(edit),
        };
        if let Err(error) = ended {
            self.failed_actions.push(error.to_string());
        }
    }

    /// Why actions on views did nothing since the last call.
    pub fn take_failed_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failed_actions)
    }

    /// The fields of `tool`, a tool of the project, as a JSON Schema of its record: what a card
    /// with no view of its own shows a control for.
    pub fn schema(&self, tool: &str) -> Option<Value> {
        let info = find(&self.tools, tool)?;
        let properties: Map<String, Value> = (info.fields.0.iter())
            .filter_map(|(name, field)| Some((name.clone(), field_schema(field)?)))
            .collect();
        Some(json!({ "x-title": info.title, "properties": properties }))
    }

    /// Whether a new Bun waits for the folder to be quiet: the window polls again soon. One that
    /// loads wakes the window itself.
    pub fn waits(&self) -> bool {
        self.changed_at.is_some()
    }

    /// Whether a tool of an instance of `project` has a control loop: the window runs them every
    /// [`FRAME_INTERVAL`].
    pub fn loops(&self, project: &Project) -> bool {
        self.bun.is_some() && (project.instances()).any(|(_, tool)| self.ticks(tool))
    }

    fn ticks(&self, tool: &str) -> bool {
        find(&self.tools, tool).is_some_and(|info| info.tick)
    }

    /// One step of the control loop of every instance whose tool has one, `dt` seconds after
    /// the last, at `clock`; also tells Bun the clock when what its code follows of it changed.
    /// Not while the last step runs.
    pub fn run_loops(&mut self, project: &Project, clock: Clock, dt: f32) -> Looped {
        let Some(bun) = self.bun.clone() else {
            return Looped::Nothing;
        };
        self.clock = Some(clock);
        self.tell_clock(&bun, clock);
        if self.ticking {
            return Looped::Busy;
        }
        let instances: Vec<Played> = (project.instances())
            .filter(|(_, tool)| self.ticks(tool))
            .map(|(id, _)| played(project, id))
            .collect();
        if instances.is_empty() {
            return Looped::Nothing;
        }
        // Its code reads the records as they are now.
        self.tell(project);
        self.ticking = bun.send(&Request::Frame {
            dt,
            instances,
            clock,
        });
        match self.ticking {
            true => Looped::Sent,
            false => Looped::Nothing,
        }
    }

    /// Tells Bun `clock` when what its code follows of it changed: never for the playhead alone.
    fn tell_clock(&mut self, bun: &Bun, clock: Clock) {
        let told = self.told_transport.map(|told| told.followed());
        if told != Some(clock.transport.followed()) && bun.send(&Request::Clock { clock }) {
            self.told_transport = Some(clock.transport);
        }
    }

    /// The key of `actions` of the tool of `instance` went down: its code runs them in order
    /// until one does it, as one step of undo. Whether Bun got them.
    pub fn action(
        &mut self,
        project: &Project,
        instance: &InstanceId,
        actions: &[String],
        clock: Clock,
    ) -> bool {
        let Some(bun) = self.bun.clone() else {
            return false;
        };
        self.tell(project);
        self.tell_clock(&bun, clock);
        let sent = bun.send(&Request::Action {
            instance: played(project, instance),
            actions,
            clock,
        });
        self.acts += usize::from(sent);
        sent
    }

    /// A key `onKey` of the tool of `instance` takes went down or up, as `name` without
    /// shift: `a`, `cmd-c`. Whether Bun got it.
    pub fn key(
        &mut self,
        project: &Project,
        instance: &InstanceId,
        (name, down): (&str, bool),
        held: crate::shortcuts::Held,
        clock: Clock,
    ) -> bool {
        let Some(bun) = self.bun.clone() else {
            return false;
        };
        self.tell(project);
        self.tell_clock(&bun, clock);
        bun.send(&Request::Key {
            instance: played(project, instance),
            key: name,
            down,
            shift: held.shift,
            alt: held.alt,
            cmd: held.platform,
            ctrl: held.control,
            clock,
        })
    }

    /// The actions of every tool of the project that has some, as the keymap lists them.
    pub fn shortcut_groups(&self) -> Vec<crate::shortcuts::Group> {
        (self.tools.iter())
            .filter_map(|info| info.shortcuts())
            .collect()
    }

    /// Whether the action `action` of `tool` runs wherever the focus is, while its view is the
    /// main view.
    pub fn acts_everywhere(&self, tool: &str, action: &str) -> bool {
        find(&self.tools, tool).is_some_and(|info| info.acts_everywhere(action))
    }

    /// Whether `onKey` of `tool` takes the key `name` while its view has the keys: those its
    /// `keys` name, else every key without cmd, ctrl or alt.
    pub fn takes_key(&self, tool: &str, name: &str) -> bool {
        let plain = !["cmd-", "ctrl-", "alt-", "super-"]
            .iter()
            .any(|held| name.starts_with(held));
        find(&self.tools, tool).is_some_and(|info| {
            info.keys
                && (info.takes.as_ref()).map_or(plain, |takes| takes.iter().any(|key| key == name))
        })
    }

    /// Whether `onKey` of `tool` hears the key `name` wherever the focus is: its `globalKeys`.
    pub fn takes_global_key(&self, tool: &str, name: &str) -> bool {
        find(&self.tools, tool)
            .is_some_and(|info| info.keys && info.global_keys.iter().any(|key| key == name))
    }

    /// A change in `extensions/`, as the watcher says it.
    pub fn code_changed(&mut self) {
        self.changed_at = Some(Instant::now() - GROUPING_WINDOW);
    }

    /// Hears Bun and the folder, applies what code changed, and tells Bun what changed of the
    /// records. `events` are the project's since the last poll. Whether what it shows changed.
    pub fn poll(&mut self, project: &mut Project, events: &[ProjectEvent]) -> bool {
        for event in events {
            if let ProjectEvent::Created(id)
            | ProjectEvent::Changed(id)
            | ProjectEvent::Deleted(id) = event
            {
                self.changed_records.insert(id.clone());
            }
        }
        while self.changes.try_recv().is_ok() {
            self.changed_at = Some(Instant::now());
        }
        let mut changed = self.launched(project);
        if let Some(bun) = self.bun.clone() {
            // Read first: what it said for each thing it did is in the events by then.
            let said_done = bun.said_done();
            let events = bun.events();
            while let Ok(event) = events.try_recv() {
                changed |= self.heard(event, project);
            }
            self.taking.retain(|(sent, _)| *sent > said_done);
        }
        // Until it is quiet: an editor saves in several writes, an agent several files.
        if self
            .changed_at
            .is_some_and(|at| at.elapsed() >= GROUPING_WINDOW)
        {
            self.changed_at = None;
            self.skipped.clear();
            self.restart(project);
        }
        let (shown, problems) = (&self.shown, self.view_problems.len());
        self.view_problems.retain(|id, _| shown.contains(id));
        if self.view_problems.len() != problems {
            self.report(project);
        }
        self.tell(project);
        self.hear_taps(project);
        changed
    }

    /// Gives `take` to the `onTake` of the parent of its recorder. While no Bun runs it waits
    /// for the next one. Gives what to tell the composer: why it was not kept, or that it waits.
    pub fn take(&mut self, project: &Project, take: Take) -> Option<String> {
        let parent = take.recorder.parent();
        let info = (parent.as_ref()).and_then(|parent| self.info_of(project, parent));
        let keeps = info.as_ref().is_some_and(|info| info.on_take);
        let (Some(parent), true) = (parent, keeps) else {
            return Some(format!(
                "The take of {} was not kept: nothing keeps what it records yet. Ask the agent for it.",
                take.recorder
            ));
        };
        self.tell(project);
        let request = Request::Take {
            instance: played(project, &parent),
            recorder: take.recorder.name(),
            name: &take.name,
            tool: &take.tool,
            state: &take.state,
            clock: self.clock_now(project),
        };
        // A Bun that runs without the file of its tool keeps it once the file is fixed.
        let runs = info.is_some_and(|info| !self.skipped.contains_key(&info.file));
        let sent = (self.bun.as_ref())
            .filter(|bun| runs && bun.send(&request))
            .map(|bun| bun.sent());
        if let Some(sent) = sent {
            self.taking.push((sent, take));
            return None;
        }
        let notice = take_waits(&take);
        self.held_takes.push(take);
        Some(notice)
    }

    /// Whether a take went to Bun that did not say it did it yet, or waits for one that starts.
    pub fn keeping_takes(&self) -> bool {
        let starts = self.starting.is_some() || self.changed_at.is_some();
        !self.taking.is_empty() || (starts && !self.held_takes.is_empty())
    }

    /// For the project closing: the takes no code kept yet wait for its next open. Their files
    /// stay in the project either way.
    pub fn close(&mut self, project: &Project) {
        let waiting: Vec<&Take> = (self.taking.iter().map(|(_, take)| take))
            .chain(&self.held_takes)
            .collect();
        if waiting.is_empty() {
            return;
        }
        let written = serde_json::to_vec(&waiting)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                let path = project.root().join(WAITING_TAKES);
                std::fs::create_dir_all(path.parent().unwrap_or(project.root()))?;
                sound_core::write_atomically(&path, &bytes)
            });
        if let Err(error) = written {
            eprintln!("error: the takes no code kept were not written down: {error}");
        }
    }

    /// `bun` ends: what it said since the last poll is heard, then the takes it did not say it
    /// did wait for the next one.
    fn hear_last_of(&mut self, bun: &Bun, project: &mut Project) {
        bun.kill();
        let events = bun.events();
        let start = Instant::now();
        // Until it said all it will: its end comes last.
        loop {
            match events.try_recv() {
                Ok(Event::Ended { .. }) | Err(smol::channel::TryRecvError::Closed) => break,
                Ok(event) => {
                    self.heard(event, project);
                }
                Err(smol::channel::TryRecvError::Empty) if start.elapsed() < LAST_WORDS => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(smol::channel::TryRecvError::Empty) => break,
            }
        }
        self.hold_takes_after(bun.said_done(), project);
    }

    /// The takes numbered after `said_done`, which a Bun that ended never did, wait for the
    /// next one.
    fn hold_takes_after(&mut self, said_done: u64, project: &mut Project) {
        for (sent, take) in std::mem::take(&mut self.taking) {
            if sent > said_done {
                self.problems.push(crate::folder_problem(take_waits(&take)));
                self.held_takes.push(take);
            }
        }
        self.report(project);
    }

    /// The clock of the last step of the control loops, or the start of the song before one.
    fn clock_now(&self, project: &Project) -> Clock {
        self.clock.unwrap_or_else(|| Clock {
            time: 0.0,
            transport: Transport {
                bpm: project
                    .project_file()
                    .tempo_map
                    .change_at(Ticks(0))
                    .bpm
                    .bpm(),
                bar: 1,
                beat: 1,
                ..Transport::default()
            },
        })
    }

    /// Gives what reached the notes of every instance whose tool has `onNotes` since the last
    /// poll to that code. A tap written over before it was read is a problem of its tool. While
    /// no Bun runs it waits, so what plays during a reload is heard once the new Bun is there.
    fn hear_taps(&mut self, project: &mut Project) {
        let Some(bun) = self.bun.clone() else {
            return;
        };
        let taps: Vec<(InstanceId, EventTap<NoteEvent>)> = (project.instances())
            .filter(|(_, tool)| find(&self.tools, tool).is_some_and(|info| info.notes))
            .filter_map(|(id, _)| Some((id.clone(), project.tap::<NoteEvent>(id, NOTES_TAP)?)))
            .collect();
        let mut read = std::mem::take(&mut self.taps);
        let rate = f64::from(project.clock().sample_rate());
        let mut heard = Vec::new();
        for (id, tap) in taps {
            // A new tap, of a new instance or of a tool that hears notes again, from its start.
            let from = (read.remove(&id))
                .filter(|(kept, _)| kept.is(&tap))
                .map_or(0, |(_, from)| from);
            heard.clear();
            let tapped = tap.read(from, &mut heard);
            self.taps.insert(id.clone(), (tap, tapped.next));
            if tapped.missed > 0 {
                let message = format!(
                    "onNotes missed {} note events: the window was too busy to hear them in time",
                    tapped.missed
                );
                if let Some(problem) = self.problem_of(project, &id, &message)
                    && !self.problems.contains(&problem)
                {
                    self.problems.push(problem);
                    self.report(project);
                }
            }
            if heard.is_empty() {
                continue;
            }
            let events: Vec<Heard> = (heard.iter())
                .map(|(frame, event)| Heard {
                    event: NoteMessage::from(*event),
                    at: *frame as f64 / rate,
                })
                .collect();
            self.tell(project);
            bun.send(&Request::Notes {
                instance: played(project, &id),
                events: &events,
                clock: self.clock_now(project),
            });
        }
    }

    /// Waits until Bun did all it was told and the window heard it, and polls, until nothing
    /// is left: what a run without a window does before each frame. Whether what it shows
    /// changed, or that Bun did not finish. The events of the project it took go into
    /// `heard`, for whoever else hears them.
    pub fn settle(
        &mut self,
        project: &mut Project,
        heard: &mut Vec<ProjectEvent>,
    ) -> Result<bool, String> {
        let started = Instant::now();
        let mut changed = false;
        loop {
            let left = SETTLE_TIMEOUT.saturating_sub(started.elapsed());
            let starting = match &mut self.starting {
                Some((launched, _)) => match launched.recv_timeout(left) {
                    Ok(launched) => {
                        self.starting = None;
                        changed |= self.loaded(launched, project);
                        true
                    }
                    Err(_) => false,
                },
                None => false,
            };
            // What it says for what it was told is queued before it says it is done.
            while self.bun.as_ref().is_some_and(|bun| !bun.is_done())
                && started.elapsed() < SETTLE_TIMEOUT
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            let events = project.drain_events();
            let polled = self.poll(project, &events);
            heard.extend(events);
            changed |= polled;
            let idle = self.bun.as_ref().is_none_or(|bun| bun.is_idle());
            if !polled && !starting && idle && self.starting.is_none() {
                return Ok(changed);
            }
            if started.elapsed() >= SETTLE_TIMEOUT {
                return Err(format!(
                    "the code of the project did not finish within {SETTLE_TIMEOUT:?}"
                ));
            }
        }
    }

    /// Ends Bun, if one runs, and starts a new one on the files as they are now.
    fn restart(&mut self, project: &mut Project) {
        if let Some((_, again)) = &mut self.starting {
            *again = true;
            return;
        }
        if self.watched {
            let (send, changes) = mpsc::channel();
            let (watcher, problem) = watching(&self.folder, send, self.waker.clone());
            (self._watcher, self.watch_problem, self.changes) = (watcher, problem, changes);
        }
        let old = self.bun.take();
        if let Some(old) = &old {
            self.hear_last_of(old, project);
        }
        self.performer_gone(project);
        let (folder, opening, skipped) = (self.folder.clone(), self.opening, self.skipped.clone());
        let (send, launched) = mpsc::channel();
        let wake = self.waker.clone();
        let started = std::thread::Builder::new()
            .name("bun-start".into())
            .spawn(move || {
                if let Some(old) = old {
                    old.end();
                }
                send.send(crate::launch((&folder, opening), skipped, true))
                    .ok();
                wake();
            });
        match started {
            Ok(_) => self.starting = Some((launched, false)),
            Err(error) => self.problems.push(crate::folder_problem(error.to_string())),
        }
    }

    /// A new Bun that loaded, if one did.
    fn launched(&mut self, project: &mut Project) -> bool {
        let Some((receiver, again)) = &self.starting else {
            return false;
        };
        let Ok(launched) = receiver.try_recv() else {
            return false;
        };
        let again = *again;
        self.starting = None;
        if again {
            // Its tools are older than the folder.
            drop(launched);
            self.restart(project);
            return false;
        }
        self.loaded(launched, project)
    }

    /// Defines the tools of a new Bun again, and shows its views.
    fn loaded(
        &mut self,
        launched: Result<Option<Launched>, String>,
        project: &mut Project,
    ) -> bool {
        self.changed_records = every_instance(project);
        self.loads += 1;
        self.end_actions(project, "the code changed while it ran");
        let Launched {
            bun,
            sounds,
            skipped,
            mut problems,
        } = match launched {
            Ok(Some(launched)) => launched,
            Ok(None) => {
                self.forget_views();
                self.problems.clear();
                self.report(project);
                return true;
            }
            Err(message) => {
                self.forget_views();
                self.problems = vec![crate::folder_problem(message)];
                self.report(project);
                return true;
            }
        };
        self.skipped = skipped;
        let tools = crate::define(sounds, &mut problems, |tools| {
            let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
            match project.define_json_tools(tools) {
                Ok(refused) => (refused.into_iter())
                    .map(|(name, error)| (name, error.to_string()))
                    .collect(),
                Err(error) => names
                    .into_iter()
                    .map(|name| (name, error.to_string()))
                    .collect(),
            }
        });
        // The project keeps playing a tool the code no longer defines, as it was.
        let gone: Vec<Rc<ToolInfo>> = (self.tools.iter())
            .filter(|info| !tools.iter().any(|tool| tool.name == info.name))
            .cloned()
            .collect();
        // A file Bun runs without says why already.
        let unexplained = (gone.iter()).filter(|info| !self.skipped.contains_key(&info.file));
        problems.extend(unexplained.map(|info| {
            info.problem(
                "it is gone from the code; its records play as they did until the project opens again",
            )
        }));
        self.tools = gone
            .into_iter()
            .chain(tools.into_iter().map(Rc::new))
            .collect();
        self.problems = problems;
        self.listen(bun);
        self.offer_held_takes(project);
        self.report(project);
        true
    }

    /// The takes that waited for a Bun go to the one that runs now.
    fn offer_held_takes(&mut self, project: &Project) {
        for take in std::mem::take(&mut self.held_takes) {
            if let Some(notice) = self.take(project, take) {
                self.problems.push(crate::folder_problem(notice));
            }
        }
    }

    /// The Bun that ran the actions sent is gone: what they changed is one step, and those it
    /// did not say it ran did nothing.
    fn end_actions(&mut self, project: &mut Project, why: &str) {
        self.finish_action(project);
        let unanswered = std::mem::take(&mut self.acts);
        self.failed_actions
            .extend(std::iter::repeat_n(why.to_string(), unanswered));
        (self.ticking, self.told_transport) = (false, None);
    }

    /// No Bun draws the views now.
    fn forget_views(&mut self) {
        self.drawn.clear();
        self.view_problems.clear();
    }

    fn report(&self, project: &mut Project) {
        let problems = self.problems().cloned().collect();
        project.set_problems_in(&format!("{FOLDER}/"), problems);
    }

    /// One thing Bun said. Whether what the window shows changed.
    fn heard(&mut self, event: Event, project: &mut Project) -> bool {
        match event {
            Event::Tree {
                instance,
                tree,
                error,
            } => {
                if !self.shown.contains(&instance) {
                    return false;
                }
                self.trees_heard += 1;
                // Bun draws a card for an instance it got one for, with the generic card.
                let card = self.cards.contains(&instance);
                let before = self.drawn.remove(&instance);
                let problem = (error.as_ref()).and_then(|error| match card {
                    true => Some(Problem {
                        path: format!("{FOLDER}/{}", self.card_file(project, &instance)),
                        message: format!("it fails to draw the card of {instance}: {error}"),
                    }),
                    false => {
                        self.problem_of(project, &instance, &format!("its view failed: {error}"))
                    }
                });
                let reported = match problem {
                    Some(problem) => self
                        .view_problems
                        .insert(instance.clone(), problem)
                        .is_none(),
                    None => self.view_problems.remove(&instance).is_some(),
                };
                if reported {
                    self.report(project);
                }
                let drawn = match (tree, error) {
                    (_, Some(_)) if card => Drawn::NoCard(MISTAKE.to_string()),
                    // Its file is stopped, missing or did not load: the problems say why.
                    (None, None) if card => Drawn::NoCard(self.no_card().to_string()),
                    (_, Some(error)) => Drawn::Failed(error),
                    (Some(tree), None) => {
                        self.view_runs += 1;
                        let bun = self.bun.as_ref().map_or(0, |bun| bun.number());
                        drawn(tree, before, bun, self.read)
                    }
                    (None, None) => Drawn::None,
                };
                self.drawn.insert(instance, drawn);
                true
            }
            Event::Acted { error } => {
                self.acts = self.acts.saturating_sub(1);
                if self.held == Held::No || (self.held == Held::Cancelled && self.acts == 0) {
                    self.finish_action(project);
                }
                self.failed_actions.extend(error);
                false
            }
            Event::Framed => {
                self.ticking = false;
                false
            }
            Event::Edit {
                instance,
                label,
                changes,
                gesture,
            } => {
                self.edit(project, &instance, &label, changes, gesture);
                false
            }
            Event::Failed { tool, message } => {
                if let Some(info) = find(&self.tools, &tool) {
                    self.problems.push(info.problem(message));
                    self.report(project);
                }
                false
            }
            Event::Ended { blame, why } => {
                // It says its end last: what it did before is heard.
                if let Some(said_done) = self.bun.as_ref().map(|bun| bun.said_done()) {
                    self.hold_takes_after(said_done, project);
                }
                self.failed(blame, &why, project);
                true
            }
            Event::Control {
                instance,
                child,
                name,
                value,
                at,
            } => {
                let played = self.played_on(project, &instance, child.as_deref());
                let command = played
                    .as_ref()
                    .ok()
                    .filter(|id| self.info_of(project, id).is_none());
                if let Some(id) = command.cloned() {
                    match project.command(&id, &name, value) {
                        Ok(true) => return false,
                        Ok(false) => {}
                        Err(error) => {
                            let message = format!("its send({name:?}) was refused: {error}");
                            self.tool_problem(project, &instance, &message);
                            return false;
                        }
                    }
                }
                let value = value.map(|value| value as f32);
                let performed = match &played {
                    Ok(id) if project.tool_of(id) == Some(RECORDER) => {
                        Some(record_on(project, id, &name, value, at))
                    }
                    Ok(id) if project.tool_of(id) == Some(timeline::NAME) => {
                        Some(launch_on(project, id, &name, value, at))
                    }
                    _ => None,
                };
                if let Some(sent) = performed {
                    self.performed(project, &instance, child.as_deref(), sent);
                    return false;
                }
                let at = at.map(|at| frame_of(project, at));
                if let Ok(id) = &played {
                    self.performed_on.insert((id.clone(), None));
                }
                let sent = played.and_then(|id| {
                    let info = self.info_of(project, &id).ok_or_else(|| {
                        let tool = project.tool_of(&id).unwrap_or("nothing");
                        format!("has no live controls or triggers: it is {tool}")
                    })?;
                    let update = control_update(&info, &name, value, at)?;
                    (project.send::<Signals>(&id, info.processor(), update))
                        .map_err(|error| error.to_string())
                });
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Note {
                instance,
                child,
                pitch,
                velocity,
                seconds,
                at,
            } => {
                let performed = Performed::Play {
                    pitch: Pitch::nearest(i64::from(pitch)),
                    velocity: Velocity::nearest((velocity.clamp(0.0, 1.0) * 127.0).round() as i64),
                    at: performed_frame(project, at),
                    frames: frame_of(project, f64::from(seconds)),
                };
                let sent = self.play_note(project, &instance, child.as_deref(), performed);
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Send {
                instance,
                child,
                event,
                at,
            } => {
                // The SDK sends no controller from 120 on, MIDI's channel mode messages.
                let sent = match event.event() {
                    Some(event) => {
                        let at = performed_frame(project, at);
                        let performed = Performed::Send { event, at };
                        self.play_note(project, &instance, child.as_deref(), performed)
                    }
                    None => Err(format!("sends {event:?}, which is no note event")),
                };
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Transport { op } => {
                self.transport(project, op);
                false
            }
            Event::Select {
                instance,
                path,
                span,
            } => {
                let selected = match path {
                    None => Ok(None),
                    Some(path) => self.child_of(project, &instance, &path).map(Some),
                };
                match selected {
                    Ok(instance) => self.wants.push(Want::Select {
                        instance,
                        span: span.map(|[from, to]| Ticks(from)..Ticks(to)),
                    }),
                    Err(error) => {
                        self.tool_problem(project, &instance, &format!("it selects {error}"))
                    }
                }
                false
            }
            Event::SongEdit {
                instance,
                label,
                tempo_map,
                sections,
                gesture,
            } => {
                self.edit_song(project, &instance, &label, (tempo_map, sections), gesture);
                false
            }
            Event::Confirm {
                question,
                message,
                detail,
                confirm,
            } => {
                let question = self.question(question);
                self.wants.push(Want::Confirm {
                    question,
                    message,
                    detail,
                    confirm,
                });
                false
            }
            Event::OpenFile { question, title } => {
                let question = self.question(question);
                self.wants.push(Want::OpenFile { question, title });
                false
            }
            Event::ExportNotes {
                question,
                instance,
                path,
                name,
            } => {
                let question = self.question(question);
                match self.notes_of(project, &instance, &path) {
                    Ok(bytes) => self.wants.push(Want::ExportNotes {
                        question,
                        name,
                        bytes,
                    }),
                    Err(error) => {
                        let message = format!("it exports the notes of {error}");
                        self.tool_problem(project, &instance, &message);
                        self.answer(question, &Answer::Confirmed(false));
                    }
                }
                false
            }
            Event::Render {
                question,
                instance,
                path,
                span: [from, to],
                tail,
                name,
            } => {
                let question = self.question(question);
                let child =
                    self.child_of(project, &instance, &path)
                        .and_then(|child| match from < to {
                            true => Ok(child),
                            false => Err(format!("{path:?} from {from} to {to}, which is no span")),
                        });
                match child {
                    Ok(child) => self.wants.push(Want::Render {
                        question,
                        instance: child,
                        span: Ticks(from)..Ticks(to),
                        tail,
                        name,
                    }),
                    Err(error) => {
                        self.tool_problem(project, &instance, &format!("it renders {error}"));
                        self.answer(question, &Answer::Asset(None));
                    }
                }
                false
            }
            Event::AddOffer {
                instance,
                path,
                key,
                label,
                gesture,
            } => {
                self.add_offer(project, &instance, (&path, &key), (&label, gesture));
                false
            }
            Event::ShowRange {
                instance,
                key,
                x,
                y,
            } => {
                self.wants.push(Want::Show {
                    instance,
                    key,
                    x,
                    y,
                });
                false
            }
            Event::OpenWindow { instance, path } => {
                // A card opens its own device's window with the path "".
                let child = match path.is_empty() {
                    true => Ok(instance.clone()),
                    false => self.child_of(project, &instance, &path),
                };
                let opened = child.and_then(|child| match &self.open_window {
                    Some(open) => open(project, &child),
                    None => Err("this window opens no other".to_string()),
                });
                if let Err(why) = opened {
                    let message = format!("its openWindow({path:?}) was refused: {why}");
                    self.tool_problem(project, &instance, &message);
                }
                false
            }
        }
    }

    /// A number field of `instance`, or of its child at the path `child`, as it plays at `tick`:
    /// what a lane plays into it, else its record, at its default where the record leaves it out.
    pub fn field_now(
        &self,
        project: &Project,
        instance: &InstanceId,
        child: Option<&str>,
        name: &str,
        tick: Ticks,
    ) -> f64 {
        let path = match child {
            Some(child) => format!("{child}.{name}"),
            None => name.to_string(),
        };
        let (id, name) = crate::path::number_at(project, instance, &path);
        let at = crate::feeds::Moment::at(tick);
        if let Some((value, _)) = crate::feeds::lane_value(project, &id, name, at) {
            return f64::from(value);
        }
        let state = project.state_json(&id);
        let state = state.and_then(|state| serde_json::from_str::<Value>(&state).ok());
        let state = match (state, self.info_of(project, &id)) {
            (Some(state), Some(info)) => Some(info.with_defaults(state)),
            (state, _) => state,
        };
        let value = (state.as_ref())
            .and_then(|state| crate::path::at(state, name).and_then(crate::path::number_of));
        value.map_or(0.0, f64::from)
    }

    fn info_of(&self, project: &Project, id: &InstanceId) -> Option<Rc<ToolInfo>> {
        (project.tool_of(id))
            .and_then(|tool| find(&self.tools, tool))
            .cloned()
    }

    /// The instance a performer of `instance` plays: itself, or its child at the path `child`,
    /// never one outside it.
    fn played_on(
        &self,
        project: &Project,
        instance: &InstanceId,
        child: Option<&str>,
    ) -> Result<InstanceId, String> {
        let Some(child) = child else {
            return Ok(instance.clone());
        };
        let played = InstanceId::new(&format!("{instance}/{child}"))
            .ok()
            .filter(|id| id.is_inside(instance) && project.tool_of(id).is_some());
        played.ok_or_else(|| {
            let children: Vec<&str> = (project.instances())
                .filter(|(id, _)| id.parent().as_ref() == Some(instance))
                .map(|(id, _)| id.name())
                .collect();
            match children.is_empty() {
                true => format!("{instance} does not own: it owns none"),
                false => format!(
                    "{instance} does not own: its children are {}",
                    children.join(", ")
                ),
            }
        })
    }

    /// Plays a key of `instance`, or lets go of one, as the keyboard would: on its own voices,
    /// or into the `notes` of its child `child` through the feed it declares.
    fn play_note(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        child: Option<&str>,
        performed: Performed,
    ) -> Result<(), String> {
        let error = |error: sound_core::ProjectError| error.to_string();
        let Some(child) = child else {
            if let Some(info) = self.info_of(project, instance) {
                self.performed_on.insert((instance.clone(), None));
                let update = SignalsUpdate::Note(performed);
                return (project.send::<Signals>(instance, info.processor(), update))
                    .map_err(error);
            }
            // A built-in device, played by its card: through the feed of the nearest tool of
            // the project that owns it, as that tool would play its child.
            let owner = (instance.ancestors())
                .find(|owner| self.info_of(project, owner).is_some())
                .ok_or("has no voices to play")?;
            let path = (instance.as_str().strip_prefix(&format!("{owner}/")))
                .ok_or("has no voices to play")?
                .to_string();
            return self.play_note(project, &owner, Some(&path), performed);
        };
        (self.performed_on).insert((instance.clone(), Some(child.to_string())));
        let played = self.played_on(project, instance, Some(child))?;
        if project.input_port(&played, NOTES_INPUT).is_none() {
            return Err("has no `notes` input".to_string());
        }
        if feed_child(project.assets(), instance, child) {
            project.rebind(instance).map_err(error)?;
        }
        let (feed, update) = (feed_processor(child), NoteFeedUpdate::Note(performed));
        (project.send::<NoteFeed>(instance, &feed, update)).map_err(error)
    }

    /// Tells each place the code of the Bun that ended played on that it is gone, so what it
    /// held lets go. A place deleted since has nothing left to release.
    fn performer_gone(&mut self, project: &mut Project) {
        for (id, child) in std::mem::take(&mut self.performed_on) {
            let gone = match &child {
                None => match self.info_of(project, &id) {
                    Some(info) => {
                        project.send::<Signals>(&id, info.processor(), SignalsUpdate::PerformerGone)
                    }
                    None => Ok(()),
                },
                Some(child) => project.send::<NoteFeed>(
                    &id,
                    &feed_processor(child),
                    NoteFeedUpdate::PerformerGone,
                ),
            };
            match gone {
                Ok(()) | Err(sound_core::ProjectError::MissingProcessor { .. }) => {}
                Err(error) => self.problems.push(crate::folder_problem(error.to_string())),
            }
        }
    }

    /// What code of `instance` played could not be played: a problem of its tool, once, as its
    /// types do not check it.
    fn performed(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        child: Option<&str>,
        sent: Result<(), String>,
    ) {
        let Err(error) = sent else {
            return;
        };
        let message = match child {
            Some(child) => format!("a performer plays the child {child:?}, which {error}"),
            None => format!("a performer plays it, but it {error}"),
        };
        if let Some(problem) = self.problem_of(project, instance, &message)
            && !self.problems.contains(&problem)
        {
            self.problems.push(problem);
            self.report(project);
        }
    }

    /// Code played, stopped or moved the clock, or turned one of its switches.
    fn transport(&mut self, project: &mut Project, op: TransportOp) {
        let engine = project.engine();
        match op {
            TransportOp::Play => engine.play(),
            TransportOp::Pause => engine.pause(),
            TransportOp::Stop => engine.stop(),
            TransportOp::Jump { to, at: None } => engine.seek(Ticks(to)),
            TransportOp::Jump { to, at: Some(at) } => engine.jump_at(Ticks(at), Ticks(to)),
            TransportOp::Repeat { range } => {
                let range = range.map(|[from, to]| Ticks(from)..Ticks(to));
                self.wants.push(Want::Repeat(range));
            }
            TransportOp::Click { on } => self.wants.push(Want::Click(on)),
            TransportOp::Keys { on } => self.wants.push(Want::ComputerKeys(on)),
            TransportOp::Automation { on } => self.wants.push(Want::WriteAutomation(on)),
        }
    }

    /// The instance at the child path `path` of `instance`, or what is wrong with it in words.
    fn child_of(
        &self,
        project: &Project,
        instance: &InstanceId,
        path: &str,
    ) -> Result<InstanceId, String> {
        (instance.child(path).ok())
            .filter(|id| project.tool_of(id).is_some())
            .ok_or_else(|| format!("{path:?}, which {instance} does not own"))
    }

    /// The notes of the timeline child `path` of `instance` as a MIDI file, with the tempo and
    /// the time signature where it starts.
    fn notes_of(
        &self,
        project: &Project,
        instance: &InstanceId,
        path: &str,
    ) -> Result<Vec<u8>, String> {
        let child = self.child_of(project, instance, path)?;
        let state = (project.tool_of(&child) == Some(timeline::NAME))
            .then(|| crate::project::state_of(project, &child))
            .flatten()
            .ok_or_else(|| format!("{path:?}, which is no timeline of {instance}"))?;
        let (clip, title) = timeline::timeline_clip(&state)
            .map_err(|error| format!("{path:?}, which cannot play: {error}"))?;
        let tempo_map = &project.project_file().tempo_map;
        let bpm = tempo_map.change_at(clip.start).bpm.bpm();
        let signature = tempo_map.time_signatures().bar_at(clip.start).signature;
        let beats = |number: u32| u8::try_from(number).unwrap_or(u8::MAX);
        let signature = (beats(signature.numerator()), beats(signature.denominator()));
        sound_notes::midi_file::write(&clip, &title, bpm, signature)
            .map_err(|error| format!("{path:?}, which does not write: {error}"))
    }

    /// Code changed the tempo map and the sections, as one edit: with the others of the action
    /// that runs, or into a run of its label. What the project refuses is a problem of its tool.
    /// Bun is told the song again either way: its own copy went ahead.
    fn edit_song(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        label: &str,
        (tempo_map, sections): (Value, Value),
        gesture: bool,
    ) {
        self.told_song = None;
        let read = serde_json::from_value::<TempoMap>(tempo_map)
            .map_err(|error| format!("tempo_map: {error}"))
            .and_then(|tempo_map| {
                let sections = serde_json::from_value::<Sections>(sections)
                    .map_err(|error| format!("sections: {error}"))?;
                Ok((tempo_map, sections))
            });
        let published = read.and_then(|(tempo_map, sections)| {
            let mut changes = Changes::new();
            changes.set_tempo_map(tempo_map);
            changes.set_sections(sections);
            let published = match gesture {
                true => {
                    let edit = self.acting.get_or_insert_with(|| project.begin(label));
                    project.publish(edit, changes)
                }
                false => project.publish_run(instance, label, changes),
            };
            published.map_err(|error| error.to_string())
        });
        if let Err(refused) = published {
            let message = format!("its clock.update {label:?} was refused: {refused}");
            self.tool_problem(project, instance, &message);
        }
    }

    /// Puts the offer `key` of the catalog at the child `path` of `instance`, as one edit
    /// `label`: with the others of the action that runs, or into a run of its label.
    fn add_offer(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        (path, key): (&str, &str),
        (label, gesture): (&str, bool),
    ) {
        let offer = self.catalog.iter().find(|offer| offer.key == key);
        let refused = match (offer, instance.child(path)) {
            (None, _) => Some(format!("nothing is offered as {key:?}")),
            (_, Err(error)) => Some(error.to_string()),
            (Some(offer), Ok(slot)) => {
                let mut changes = Changes::new();
                match offer.write(project, &slot, &mut changes) {
                    Err(refused) => Some(refused),
                    Ok(()) => {
                        self.changed_records.insert(slot);
                        let published = match gesture {
                            true => {
                                let edit = self.acting.get_or_insert_with(|| project.begin(label));
                                project.publish(edit, changes)
                            }
                            false => project.publish_run(instance, label, changes),
                        };
                        published.err().map(|error| error.to_string())
                    }
                }
            }
        };
        if let Some(refused) = refused {
            let message = format!("its addOffer {label:?} was refused: {refused}");
            self.tool_problem(project, instance, &message);
        }
    }

    /// What a picker offers of the tools of the project: each at its defaults. Not an owner,
    /// whose place is the top of the project, nor a tool whose sound is silence.
    pub fn offers(&self) -> Vec<Offer> {
        let tools = (self.tools.iter()).filter(|info| !info.behaviour && !info.silent);
        tools
            .map(|info| {
                let takes = match info.tool_kind() {
                    crate::tools::ToolKind::Effect => Takes::Audio,
                    crate::tools::ToolKind::Instrument => Takes::Notes,
                };
                Offer::of_tool(&info.name, &info.title, Group::Project, takes)
            })
            .collect()
    }

    /// A problem of the code that draws or plays `instance`, once: its tool's, its card's, or
    /// else the folder's.
    fn tool_problem(&mut self, project: &mut Project, instance: &InstanceId, message: &str) {
        let problem = (self.problem_of(project, instance, message))
            .unwrap_or_else(|| crate::folder_problem(format!("{instance}: {message}")));
        if !self.problems.contains(&problem) {
            self.problems.push(problem);
            self.report(project);
        }
    }

    /// Why a device shows no card though its file is fine to draw it, in plain words.
    fn no_card(&self) -> &'static str {
        match (
            self.skipped.contains_key(GENERIC_CARD),
            self.folder.join(GENERIC_CARD).is_file(),
        ) {
            (true, _) => {
                "This device's card stopped working, so it shows nothing until it is mended."
            }
            (false, false) => "This project has no card for this device.",
            (false, true) => MISTAKE,
        }
    }

    /// The key of each action of a tool, as Bun was told last.
    pub fn keys(&self) -> &BTreeMap<String, Option<String>> {
        &self.keys
    }

    /// The numbers a lane moves of each shown instance, as Bun was told last.
    pub fn holds(&self) -> &BTreeMap<InstanceId, Vec<String>> {
        &self.holds
    }

    pub fn writes(&self) -> &BTreeMap<InstanceId, Vec<String>> {
        &self.writes
    }

    /// Whether a card draws `id`, as the window last showed it.
    pub fn draws_card(&self, id: &InstanceId) -> bool {
        self.cards.contains(id)
    }

    /// Whether `tool` is a tool of the project that loaded.
    pub fn knows(&self, tool: &str) -> bool {
        find(&self.tools, tool).is_some()
    }

    /// Whether the file of `tool` was written for the views of an older Sound Tools.
    pub fn old_views(&self, tool: &str) -> bool {
        find(&self.tools, tool).is_some_and(|info| info.old_views)
    }

    /// Whether the tool `tool` draws a view of its own; without one, the generic card draws it.
    pub fn has_view(&self, tool: &str) -> bool {
        find(&self.tools, tool).is_some_and(|info| info.view)
    }

    /// The file of the card that draws `instance`: the project's card of its tool, else the
    /// generic card.
    fn card_file(&self, project: &Project, instance: &InstanceId) -> String {
        let tool = project.tool_of(instance).unwrap_or_default();
        let own = format!("{CARDS}/{tool}.tsx");
        match self.folder.join(&own).is_file() && !self.skipped.contains_key(&own) {
            true => own,
            false => GENERIC_CARD.to_string(),
        }
    }

    /// A problem of the code of `instance`: its tool's, if it is one of the project's, or the
    /// card's that draws it.
    pub fn problem_of(
        &self,
        project: &Project,
        instance: &InstanceId,
        message: &str,
    ) -> Option<Problem> {
        if self.cards.contains(instance) {
            return Some(Problem {
                path: format!("{FOLDER}/{}", self.card_file(project, instance)),
                message: message.to_string(),
            });
        }
        let info = project
            .tool_of(instance)
            .and_then(|tool| find(&self.tools, tool))?;
        Some(info.problem(message.to_string()))
    }

    /// Code changed records, as one edit: with the others of the action on a view that runs, as
    /// one step of undo, or else into a run of its label.
    fn edit(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        label: &str,
        edits: Vec<RecordEdit>,
        gesture: bool,
    ) {
        let edits = self.written(project, instance, label, edits);
        if edits.is_empty() {
            // All of it went to a lane.
            if let Some(bun) = &self.bun {
                bun.heard_edit();
            }
            if gesture {
                self.updated = Some(label.to_string());
            }
            return;
        }
        // A built-in tool, such as one a generic card shows, owns no children to change.
        let behaviour = (project.tool_of(instance))
            .map(|tool| find(&self.tools, tool).is_some_and(|info| info.behaviour));
        let refused =
            match behaviour.map(|behaviour| changes_of(project, behaviour, instance, edits)) {
                None => Some(format!("{instance} is gone")),
                Some(Err(refused)) => Some(refused),
                Some(Ok((changes, touched))) => {
                    self.changed_records.extend(touched);
                    let published = match gesture {
                        true => {
                            let edit = self.acting.get_or_insert_with(|| project.begin(label));
                            project.publish(edit, changes)
                        }
                        false => project.publish_run(instance, label, changes),
                    };
                    published.err().map(|error| error.to_string())
                }
            };
        if let Some(bun) = &self.bun {
            bun.heard_edit();
            if refused.is_some() {
                bun.tell([]);
            }
        }
        let Some(refused) = refused else {
            if gesture {
                self.updated = Some(label.to_string());
            }
            return;
        };
        let problem =
            format!("its update {label:?} was refused, and none of it applied: {refused}");
        if gesture {
            self.failed_actions.push(problem);
        } else {
            let problem = (self.problem_of(project, instance, &problem))
                .unwrap_or_else(|| crate::folder_problem(format!("{instance}: {problem}")));
            if !self.problems.contains(&problem) {
                self.problems.push(problem);
                self.report(project);
            }
        }
    }

    /// `edits` without the numbers they set that a hand writes into a lane now, which go to the
    /// window as one [`Want::Write`]. An edit left with nothing to do goes.
    fn written(
        &mut self,
        project: &Project,
        instance: &InstanceId,
        label: &str,
        mut edits: Vec<RecordEdit>,
    ) -> Vec<RecordEdit> {
        let Some(writes) = self.writes.get(instance) else {
            return edits;
        };
        let mut values = Vec::new();
        edits.retain_mut(|edit| {
            let RecordEdit::Fields {
                child,
                set,
                removed,
            } = edit
            else {
                return true;
            };
            let at = child
                .as_ref()
                .map_or(String::new(), |child| format!("{child}."));
            let id = match child {
                Some(child) => instance.child(child).ok(),
                None => Some(instance.clone()),
            };
            let record: Option<Value> = (id.and_then(|id| project.state_json(&id)))
                .and_then(|json| serde_json::from_str(&json).ok());
            set.retain(|name, value| {
                let field = format!("{at}{name}");
                // A number of the field, or one inside it, such as `bands[0].gain_db`.
                for path in writes.iter().filter(|path| inside(path, &field)) {
                    let inner = &path[field.len()..];
                    let Some(number) = path::at(value, inner).and_then(Value::as_f64) else {
                        continue;
                    };
                    values.push((path.clone(), number));
                    // The record keeps what it had there.
                    let kept = record
                        .as_ref()
                        .and_then(|record| path::at(record, &path[at.len()..]));
                    match (inner.is_empty(), kept) {
                        (true, _) => return false,
                        (false, Some(kept)) => path::set(value, inner, Some(kept.clone())),
                        (false, None) => {}
                    }
                }
                record.as_ref().and_then(|record| record.get(name.as_str())) != Some(value)
            });
            !(set.is_empty() && removed.is_empty())
        });
        if !values.is_empty() {
            self.wants.push(Want::Write {
                instance: instance.clone(),
                label: label.to_string(),
                values,
            });
        }
        edits
    }

    /// Bun hung or stopped: the file of the tool whose code ran then runs no more until the
    /// folder changes. With nothing to blame, no Bun runs until then.
    fn failed(&mut self, blame: Option<Runs>, why: &str, project: &mut Project) {
        let info = blame
            .as_ref()
            .and_then(|runs| find(&self.tools, &runs.tool).cloned());
        self.end_actions(project, &format!("the code of the project {why}"));
        if let Some(runs) = blame.as_ref().filter(|runs| is_card(&runs.tool)) {
            let shown = match runs.tool == GENERIC_CARD {
                true => "devices show no card",
                false => "its devices show the generic card",
            };
            let message = format!("its {} {why}, so {shown}: fix it and save", runs.what);
            self.skipped.insert(runs.tool.clone(), message);
            self.restart(project);
            return;
        }
        match (info, blame) {
            (Some(info), Some(runs)) => {
                let problem = info.problem(format!(
                    "its {} {why}, so the tools run without this file: fix it and save",
                    runs.what
                ));
                self.skipped.insert(info.file.clone(), problem.message);
                self.restart(project);
            }
            _ => {
                self.bun = None;
                self.performer_gone(project);
                self.forget_views();
                let message = format!(
                    "the code of the project {why} with no tool to blame, so the tools do not run: fix them and save"
                );
                self.problems.push(crate::folder_problem(message));
                self.report(project);
            }
        }
    }

    /// Tells Bun the records that changed since it was last told, and what the window shows.
    fn tell(&mut self, project: &Project) {
        let Some(bun) = (self.bun.as_ref()).filter(|_| !self.deferred) else {
            return;
        };
        let ids = std::mem::take(&mut self.changed_records);
        let records: Vec<(&InstanceId, Option<(&str, String)>)> = (ids.iter())
            .map(|id| {
                let tool = project
                    .tool_of(id)
                    .filter(|_| told_to_bun(project, &self.tools, id) || self.cards.contains(id));
                (
                    id,
                    tool.and_then(|tool| Some((tool, project.state_json(id)?))),
                )
            })
            .collect();
        let telling = records.iter().map(|(id, record)| -> Telling {
            (
                id,
                (record.as_ref())
                    .map(|(tool, state)| (*tool, record_key(tool, state), state.as_str())),
            )
        });
        if !bun.tell(telling) {
            drop(records);
            self.changed_records = ids;
            return;
        }
        let file = project.project_file();
        let song = (&file.tempo_map, &file.sections);
        if self
            .told_song
            .as_ref()
            .is_none_or(|(tempo_map, sections)| (tempo_map, sections) != song)
            && bun.send(&Request::Song {
                tempo_map: song.0,
                sections: song.1,
            })
        {
            self.told_song = Some((song.0.clone(), song.1.clone()));
        }
        if !self.told_shown {
            self.told_shown = bun.send(&Request::Show {
                instances: self.shown.iter().collect(),
                cards: &self.cards,
                held: &self.holds,
                facts: &self.facts,
                catalog: &self.told_catalog,
                keys: &self.keys,
            });
        }
    }
}

/// The keys down on the sound of a tool of the project, followed from what its keys did.
#[derive(Default)]
struct Keys {
    tap: Option<EventTap<NoteEvent>>,
    next: u64,
    down: u128,
}

impl Keys {
    /// The keys down now on the sound whose keys `tap` hears.
    fn follow_tap(&mut self, tap: Option<EventTap<NoteEvent>>) -> u128 {
        let Some(tap) = tap else {
            *self = Self::default();
            return 0;
        };
        if !(self.tap.as_ref()).is_some_and(|kept| kept.is(&tap)) {
            *self = Self {
                tap: Some(tap.clone()),
                ..Self::default()
            };
        }
        let mut events = Vec::new();
        self.next = tap.read(self.next, &mut events).next;
        for (_, event) in events {
            match event {
                NoteEvent::On { pitch, .. } => self.down |= 1 << pitch.number(),
                NoteEvent::Off { pitch } => self.down &= !(1 << pitch.number()),
                NoteEvent::AllOff => self.down = 0,
                _ => {}
            }
        }
        self.down
    }
}

/// The feed `sounding` of every instance, see [`crate::register_feeds`]: the keys down on what
/// it plays, or on one that plays none of its own, such as a track, on what its children play.
pub(crate) fn sounding(context: &FeedContext<'_>) -> Vec<f32> {
    let Some(tap) = context.tap::<NoteEvent>(SOUNDING_TAP) else {
        let mut keys = vec![0.0_f32; 128];
        for feed in context.of_children("sounding") {
            if let Feed::List(down) = feed {
                (keys.iter_mut().zip(down)).for_each(|(key, down)| *key = (*key).max(down));
            }
        }
        return keys;
    };
    let down = context.kept("sounding", |keys: &mut Keys| keys.follow_tap(Some(tap)));
    (0..128)
        .map(|pitch| f32::from(u8::from(down & (1 << pitch) != 0)))
        .collect()
}

/// Writes into `sounding` the notes of `events` that move a key. Realtime safe.
pub(crate) fn sounds(sounding: &EventTap<NoteEvent>, events: &[Timed<NoteEvent>]) {
    for timed in events {
        if let NoteEvent::On { .. } | NoteEvent::Off { .. } | NoteEvent::AllOff = timed.event {
            sounding.write(std::iter::once((0, timed.event)));
        }
    }
}

/// A tree as Bun `bun` sends it, with the memos the window kept from `before` that it still
/// uses. A memo's version holds the number of its Bun, whose builds count from one again: an
/// older Bun's memos are not this one's, which sends every memo it uses.
fn drawn(mut tree: Value, before: Option<Drawn>, bun: u64, read: fn(Value) -> Tree) -> Drawn {
    let mut memos = match before {
        Some(Drawn::View { memos, .. }) => memos,
        _ => HashMap::new(),
    };
    memos.retain(|_, ((number, _), _)| *number == bun);
    if let Some(Value::Array(dropped)) = tree.get_mut("drop").map(Value::take) {
        for key in dropped.iter().filter_map(Value::as_str) {
            memos.remove(key);
        }
    }
    if let Some(Value::Object(fresh)) = tree.get_mut("memos").map(Value::take) {
        for (key, mut memo) in fresh {
            let version = memo.get("v").and_then(Value::as_u64).unwrap_or_default();
            memos.insert(key, ((bun, version), read(memo["node"].take())));
        }
    }
    let root = read(tree.get_mut("root").map(Value::take).unwrap_or_default());
    Drawn::View { root, memos }
}

/// Starts or stops the timeline `id` as a number into its input `launch` does, now or at the
/// engine time `at`: nothing is saved.
fn launch_on(
    project: &mut Project,
    id: &InstanceId,
    name: &str,
    value: Option<f32>,
    at: Option<f64>,
) -> Result<(), String> {
    let Some(value) = value.filter(|_| name == timeline::LAUNCH) else {
        return Err(format!(
            "is a timeline: it takes only the number input `launch`, not {name:?}"
        ));
    };
    let at = at.map(|at| performed_frame(project, Some(at)));
    let update = TimelineUpdate::Launch(timeline::launched(value), at);
    (project.send::<Timeline>(id, timeline::PROCESSOR, update)).map_err(|error| error.to_string())
}

/// Turns the input `record` or `arm` of the recorder `id` on or off as `value` says, as a
/// performer does: nothing is saved.
fn record_on(
    project: &mut Project,
    id: &InstanceId,
    name: &str,
    value: Option<f32>,
    at: Option<f64>,
) -> Result<(), String> {
    let update = match (name, value, at) {
        (RECORD, Some(value), at) => {
            let at = at.map(|at| performed_frame(project, Some(at)));
            RecorderUpdate::Record(value > 0.0, at)
        }
        (ARM, Some(value), None) => RecorderUpdate::Arm(value > 0.0),
        (ARM, Some(_), Some(_)) => {
            return Err(
                "is a recorder armed with an `at`: arming opens the input at once, so send `arm` without one".to_string(),
            );
        }
        _ => {
            return Err(format!(
                "is a recorder: it takes only the number inputs `record` and `arm`, not {name:?}"
            ));
        }
    };
    (project.send::<Recorder>(id, RECORDING, update)).map_err(|error| error.to_string())
}

/// The frame of engine time at `seconds` of it.
fn frame_of(project: &Project, seconds: f64) -> u64 {
    let rate = f64::from(project.clock().sample_rate());
    (seconds.max(0.0) * rate).round() as u64
}

/// The frame of engine time at `at`, or now when that is gone or left out.
fn performed_frame(project: &mut Project, at: Option<f64>) -> u64 {
    let rate = f64::from(project.clock().sample_rate());
    let now = project.engine().poll().map_or(0, |status| status.frames) as f64 / rate;
    frame_of(project, at.map_or(now, |at| at.max(now)))
}

/// A field of a tool of the project as a JSON Schema says it: what a card with no view shows.
fn field_schema(field: &Field) -> Option<Value> {
    let choice = |choice: &Choice| match choice {
        Choice::Number(number) => json!(number),
        Choice::Word(word) => json!(word),
    };
    Some(match field {
        Field::Number {
            min,
            max,
            integer,
            default,
            label,
            unit,
        } => {
            let kind = if *integer { "integer" } else { "number" };
            let scale = match crate::tools::knob_range(*min as f32, *max as f32, *unit).scale {
                sound_core::Scale::Logarithmic => "logarithmic",
                _ => "linear",
            };
            // Its unit as declared, none for a plain number: a card reads it as it is.
            json!({ "type": kind, "minimum": min, "maximum": max, "default": default.unwrap_or(*min), "title": label, "x-unit": unit, "x-scale": scale })
        }
        Field::Boolean { default, label } => {
            json!({ "type": "boolean", "default": default, "title": label })
        }
        Field::Choice {
            options,
            default,
            label,
        } => {
            json!({ "enum": options.iter().map(choice).collect::<Vec<_>>(), "default": choice(default), "title": label })
        }
        Field::List {
            item,
            max,
            label,
            default,
            pattern: true,
        } => {
            json!({ "type": "array", "items": field_schema(item)?, "maxItems": max, "default": default, "title": label, "x-pattern": true })
        }
        Field::Text { .. } | Field::File { .. } | Field::List { .. } | Field::Object { .. } => {
            return None;
        }
    })
}

/// Watches `extensions/` and says on `changed` when code changed, waking the window.
fn watching(
    folder: &Path,
    changed: mpsc::Sender<()>,
    wake: Waker,
) -> (Option<RecommendedWatcher>, Option<Problem>) {
    let (send, heard) = smol::channel::unbounded();
    let passed = std::thread::Builder::new()
        .name("views-watch".into())
        .spawn(move || {
            while heard.recv_blocking().is_ok() {
                if changed.send(()).is_err() {
                    return;
                }
                wake();
            }
        });
    if let Err(error) = passed {
        return (None, Some(crate::folder_problem(error.to_string())));
    }
    watch(folder, send)
}

/// Whether the number at `path` is the field `field` or inside it: `chain/eq.bands[0].gain_db`
/// is inside `chain/eq.bands`.
fn inside(path: &str, field: &str) -> bool {
    path.strip_prefix(field)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
}

#[cfg(test)]
mod card_file_tests {
    #![allow(clippy::unwrap_used)]

    #[test]
    fn a_test_beside_the_cards_is_no_card() {
        let extensions = tempfile::tempdir().unwrap();
        let cards = extensions.path().join(super::CARDS);
        std::fs::create_dir_all(&cards).unwrap();
        for name in ["eq.tsx", "eq.test.tsx"] {
            std::fs::write(cards.join(name), "").unwrap();
        }
        let files = super::card_files(extensions.path());
        assert_eq!(files, vec![format!("{}/eq.tsx", super::CARDS)]);
    }
}

/// What the composer reads of a take no code runs to keep yet.
/// The takes no code kept as the project last closed, which the first Bun is given. The file
/// goes: the next close writes those still waiting.
fn waiting_takes(project: &Project) -> Vec<Take> {
    let path = project.root().join(WAITING_TAKES);
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    if let Err(error) = std::fs::remove_file(&path) {
        eprintln!("error: {}: {error}", path.display());
    }
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        eprintln!("error: {}: {error}", path.display());
        Vec::new()
    })
}

fn take_waits(take: &Take) -> String {
    format!(
        "The take of {} waits for the code of the project to run again.",
        take.recorder
    )
}
