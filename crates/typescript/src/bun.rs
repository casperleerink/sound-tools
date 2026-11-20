//! The Bun process that runs `host.ts`. It loads the files of `extensions/` once and says what
//! it loaded, then hears what the window tells it and says what the tools do. A change of the
//! code starts a new Bun. One JSON message per line, both ways.
//!
//! Bun does one thing at a time and says which tool's code it runs before it runs it, so a
//! thread here that hears it knows what hangs, without the window: the window may be busy.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use sound_core::{InstanceId, Sections, TempoMap};
use sound_signals::Graph;

use crate::tools::ToolInfo;
use crate::{NoteMessage, behaviours};

/// How long one step of a load, the start of Bun or a file, may take before Bun counts as
/// hung. A file's step is its import, the sound of every combination of its choices and the
/// first draw of its cards: all of it takes well under a second.
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// How long Bun may say nothing while it has something to do before it counts as hung. It
/// answers in well under a millisecond.
const HANG_TIMEOUT: Duration = Duration::from_secs(2);

/// The lines that may wait for Bun to read them. More, and Bun has stopped reading.
const QUEUED_LINES: usize = 1024;

/// What Bun said that may wait for the window to hear it. More, and the code of a tool says
/// things in a loop that never ends: the window would not catch up.
const MAX_EVENTS: usize = 4096;

/// What the code of the tools printed since Bun started, in `extensions/`, for an agent that
/// debugs a tool it wrote.
pub(crate) const LOG_FILE: &str = "log.txt";

/// The lines the log keeps: the last ones, between half of these and all.
const LOG_LINES: usize = 200;

/// The files of `extensions/` Bun runs without, because their code hung or stopped it, with
/// what to tell the composer. A change in the folder clears it.
pub(crate) type Skipped = BTreeMap<String, String>;

/// What the window tells.
#[allow(
    dead_code,
    reason = "the window uses it once it does all the old app did"
)]
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Request<'a> {
    /// The records Bun did not have as they are, and those that are gone. The other requests
    /// read the records from what it was told.
    Records {
        records: Vec<ToldRecord<'a>>,
        gone: Vec<&'a InstanceId>,
        /// How many edits of this Bun the runtime heard: Bun's own view of a record told here
        /// has the edits it sent after those.
        edits: u64,
    },
    /// The clock started or stopped, or its repeat or click changed: what bindings follow. Also
    /// before an event, so its handler reads where the playhead is.
    Clock {
        #[serde(flatten)]
        clock: Clock,
    },
    /// The tempo map and the sections, as `project.json` holds them: at the start and when they
    /// change.
    Song {
        tempo_map: &'a TempoMap,
        sections: &'a Sections,
    },
    /// One step of the control loop of every instance whose tool has one.
    Frame {
        /// Seconds since the last frame.
        dt: f32,
        instances: Vec<Played<'a>>,
        #[serde(flatten)]
        clock: Clock,
    },
    /// A key of the computer keyboard went down or up on a card or a page that has the keys.
    Key {
        instance: Played<'a>,
        key: &'a str,
        down: bool,
        shift: bool,
        alt: bool,
        cmd: bool,
        ctrl: bool,
        #[serde(flatten)]
        clock: Clock,
    },
    /// The key of some actions of its tool went down: its code runs them in this order until
    /// one does it.
    Action {
        instance: Played<'a>,
        actions: &'a [String],
        #[serde(flatten)]
        clock: Clock,
    },
    /// A take one of its `recorder` children made, for its `onTake` to keep.
    Take {
        instance: Played<'a>,
        recorder: &'a str,
        name: &'a str,
        tool: &'a str,
        state: &'a serde_json::Value,
        #[serde(flatten)]
        clock: Clock,
    },
    /// What an instance whose tool hears notes heard since it last did.
    Notes {
        instance: Played<'a>,
        events: &'a [Heard],
        #[serde(flatten)]
        clock: Clock,
    },
    /// The answer to a question of a tool: `openFile`, `render`, `openMenu`, `confirm`, `offers`
    /// or `numbers`.
    Answer { question: u64, answer: Answer<'a> },
    /// What the behaviour of an owner declares for its record and its children, as Bun was
    /// told them.
    Behaviour {
        question: u64,
        tool: &'a str,
        instance: &'a InstanceId,
    },
    /// The instances whose views the new window shows: each new one sends its tree, and each
    /// again when a record of it or inside it changes.
    Show {
        instances: Vec<&'a InstanceId>,
        /// The card of each shown instance whose tool has no view: its `Facts` in
        /// `sdk/view.ts`.
        cards: &'a BTreeMap<InstanceId, Value>,
        /// The numbers a lane moves of each shown instance, as its `held` names them.
        held: &'a BTreeMap<InstanceId, Vec<String>>,
    },
    /// An action on the element `key` of the view of `instance`: `press`, `drag` with `{dx, dy}`,
    /// or one the element names, such as `toggle` or `set` with a number.
    Act {
        instance: &'a InstanceId,
        key: &'a str,
        action: &'a str,
        value: Option<Value>,
    },
}

/// A number a lane or a cable moves, as code gets it: its range, and how a knob travels it.
#[derive(Debug, Serialize)]
pub(crate) struct Moved {
    pub name: String,
    /// What a person reads for it: as its device names it, or its field in plain words.
    pub label: String,
    pub min: f32,
    pub max: f32,
    /// `"linear"`, `"log"` or `"fader"`, as the knob of the number travels.
    pub scale: &'static str,
    /// How a value of it reads, see `unit_of` of the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    /// What its record holds now.
    pub value: Option<f32>,
}

/// What the window answers a tool that asked.
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum Answer<'a> {
    /// The name under `assets/audio/` of the file the composer chose and the runtime copied
    /// there, or that a render wrote, or none.
    Asset(Option<&'a str>),
    /// The place of the item the composer picked in a menu, or none.
    Item(Option<usize>),
    /// What the window offers that takes what was asked for, and what its pickers say under
    /// them.
    Offers {
        offers: Vec<Offered>,
        notes: Vec<String>,
        /// The key of the offer at the child it was asked about, as a picker ticks it.
        here: Option<String>,
    },
    /// What a lane or a cable can move of the child asked about.
    Numbers(Vec<Moved>),
    /// Whether the composer said yes to a question, or a file was written.
    Confirmed(bool),
}

/// A device the window offers, as code gets it: what `addOffer` names it by.
#[derive(Debug, Serialize)]
pub(crate) struct Offered {
    pub key: String,
    pub name: String,
    pub detail: Option<String>,
    pub group: String,
    /// The icon a picker shows beside it.
    pub icon: String,
    /// Why it cannot be picked here, or `None` when it can.
    pub unavailable: Option<String>,
}

/// An item of a menu code opens.
#[allow(
    dead_code,
    reason = "the window uses it once it does all the old app did"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MenuItemSpec {
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub checked: bool,
    pub icon: Option<String>,
    /// A quiet second line.
    pub detail: Option<String>,
    /// The heading it goes under, with the items before and after it of the same group.
    pub group: Option<String>,
    /// A line above it.
    #[serde(default)]
    pub separator: bool,
    /// The keys that do it too, as `cmd-c`.
    pub shortcut: Option<String>,
}

/// Where the sound and the piece are, which all code of a tool gets.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Clock {
    /// Engine time in seconds: the clock `at` of an event counts on.
    pub time: f64,
    pub transport: Transport,
}

/// A record as Bun is told it: its `state` as the file holds it.
#[derive(Serialize)]
pub(crate) struct ToldRecord<'a> {
    pub id: &'a InstanceId,
    pub tool: &'a str,
    pub state: Value,
}

/// Where the project plays, as the window's playhead shows it, and how the clock plays.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) struct Transport {
    pub playing: bool,
    pub tick: u64,
    pub bar: u64,
    pub beat: u32,
    /// The tempo at `tick`.
    pub bpm: f64,
    /// The ticks playback goes back from the end to the start of, while it repeats.
    pub repeat: Option<[u64; 2]>,
    pub click: bool,
    /// Whether the computer keys play notes.
    pub keys: bool,
    /// Whether a hand on a number a lane moves writes into the lane while the song plays.
    pub automation: bool,
}

/// Stopped at the start, at 120 bpm.
impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            tick: 0,
            bar: 1,
            beat: 1,
            bpm: 120.0,
            repeat: None,
            click: false,
            keys: false,
            automation: false,
        }
    }
}

impl Transport {
    /// What bindings follow: not the playhead, which moves on every frame.
    pub(crate) fn followed(&self) -> (bool, Option<[u64; 2]>, bool, bool, bool) {
        (
            self.playing,
            self.repeat,
            self.click,
            self.keys,
            self.automation,
        )
    }
}

/// What code does to the clock.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum TransportOp {
    Play,
    Pause,
    Stop,
    /// Playback goes on from `to`: at once, or once it reaches `at`, on its frame.
    Jump {
        to: u64,
        at: Option<u64>,
    },
    /// Playback goes back from the end to the start of `range` while it is set.
    Repeat {
        range: Option<[u64; 2]>,
    },
    Click {
        on: bool,
    },
    Keys {
        on: bool,
    },
    Automation {
        on: bool,
    },
}

/// A note event a tool heard, at `at`: engine time in seconds, the clock of `time` and of the
/// `at` of what it plays.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct Heard {
    #[serde(flatten)]
    pub event: NoteMessage,
    pub at: f64,
}

/// An instance whose code runs, with its watches as they are now. Its record is what Bun was
/// told.
#[derive(Serialize)]
pub(crate) struct Played<'a> {
    pub instance: &'a InstanceId,
    pub watches: BTreeMap<String, f32>,
}

/// One record an `update` of a tool changed: its own, or one of a child, by name.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RecordEdit {
    /// These fields of the record of the instance, or of the one at the path `child` inside it
    /// (`track` or `track/clip`), are set and those of `removed` left out, on the record as it
    /// is when the edit lands.
    Fields {
        child: Option<String>,
        set: Map<String, Value>,
        removed: Vec<String>,
    },
    /// The record at the path `child` is new, or one of another tool.
    Record {
        child: String,
        tool: String,
        state: Value,
    },
    Delete {
        child: String,
    },
}

/// What the window hears of Bun after its load.
#[allow(
    dead_code,
    reason = "the window uses it once it does all the old app did"
)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The control loops ran the last frame.
    Framed,
    /// The tick of a tool, or a click on its card, failed: the first time.
    Failed { tool: String, message: String },
    /// What the bindings of an instance read now: the window tells Bun at once only of that.
    Reads { instance: InstanceId, reads: Reads },
    /// The code of an instance changed its record or those of its children, as one edit
    /// called `label`.
    Edit {
        instance: InstanceId,
        label: String,
        changes: Vec<RecordEdit>,
        /// An action on a view of the new window made it: one step of undo with the others of
        /// that action, up to its [`Event::Acted`].
        #[serde(default)]
        gesture: bool,
    },
    /// An action on a view of the new window ran, or why it did not.
    Acted {
        #[serde(default)]
        error: Option<String>,
    },
    /// A view of the new window ran: its tree, or none for a tool without one, or why it failed.
    Tree {
        instance: InstanceId,
        #[serde(default)]
        tree: Option<Value>,
        #[serde(default)]
        error: Option<String>,
    },
    /// Code moved a number input of an instance, or of its child `child`, or sent a bang when
    /// there is no value: at `at`, engine time in seconds, or at once.
    Control {
        instance: InstanceId,
        child: Option<String>,
        name: String,
        value: Option<f32>,
        at: Option<f64>,
    },
    /// A card or a control loop played a note of an instance, or of its child `child`: a key
    /// held for `seconds` from `at`, engine time in seconds, or from now; `velocity` from 0 to
    /// 1.
    Note {
        instance: InstanceId,
        child: Option<String>,
        pitch: u8,
        velocity: f32,
        seconds: f32,
        at: Option<f64>,
    },
    /// A card or a control loop sent a note event to an instance, or to its child `child`, at
    /// `at` or now.
    Send {
        instance: InstanceId,
        child: Option<String>,
        event: NoteMessage,
        at: Option<f64>,
    },
    /// Code asks the composer for a file, with `title`: the answer goes to its `question`.
    OpenFile { question: u64, title: String },
    /// Code renders the sound of the child `path` of `instance` over `span`, in ticks, and
    /// with `tail` what still sounds after it, into a new file of `assets/audio/` named after
    /// `name`: the answer goes to its `question`.
    Render {
        question: u64,
        instance: InstanceId,
        path: String,
        span: [u64; 2],
        tail: bool,
        name: String,
    },
    /// Code exports the notes of the timeline child `path` of `instance` as a MIDI file the
    /// composer names, offered as `name`: whether it was written goes to its `question`.
    ExportNotes {
        question: u64,
        instance: InstanceId,
        path: String,
        name: String,
    },
    /// Code asks the composer `message` with the system's question, its buttons `confirm` and
    /// Cancel: the answer goes to its `question`.
    Confirm {
        question: u64,
        message: String,
        #[serde(default)]
        detail: Option<String>,
        confirm: String,
    },
    /// Code opens a menu of `items` where the pointer is: the answer goes to its `question`.
    OpenMenu {
        question: u64,
        items: Vec<MenuItemSpec>,
        /// Quiet lines under the items.
        #[serde(default)]
        notes: Vec<String>,
        /// How wide it is, and how tall its items go before they scroll, in points.
        #[serde(default)]
        width: Option<f32>,
        #[serde(default, rename = "maxHeight")]
        max_height: Option<f32>,
        /// A field above the items that keeps those whose label holds what is typed.
        #[serde(default)]
        search: bool,
    },
    /// Code asks what the window offers that takes `takes`, `"notes"` or `"audio"`.
    /// With `at`, the answer says which offer is at that child of `instance` now.
    Offers {
        question: u64,
        takes: String,
        instance: InstanceId,
        #[serde(default)]
        at: Option<String>,
    },
    /// Code asks what a lane or a cable can move of the child of `instance` at `path`.
    Numbers {
        question: u64,
        instance: InstanceId,
        path: String,
    },
    /// The code of `instance` puts the offer `key` at its child `path`, as one edit `label`.
    AddOffer {
        instance: InstanceId,
        path: String,
        key: String,
        label: String,
    },
    /// The code of `instance` opens the window of its own of its child at `path`.
    OpenWindow { instance: InstanceId, path: String },
    /// The code of `instance` selected its child at `path`, or nothing, and the ticks `span`.
    Select {
        instance: InstanceId,
        path: Option<String>,
        span: Option<[u64; 2]>,
    },
    /// Code played, stopped or moved the clock.
    Transport {
        #[serde(flatten)]
        op: TransportOp,
    },
    /// The code of an instance changed the tempo map and the sections, as one edit called
    /// `label`: both as `project.json` holds them.
    SongEdit {
        instance: InstanceId,
        label: String,
        tempo_map: Value,
        sections: Value,
    },
    /// Bun hung or stopped, the last thing it says: `why`, and the tool and the code of it that
    /// it ran then, which are to blame. Nothing to blame when it ran no tool's code, such as
    /// code a timer of a tool ran.
    #[serde(skip_deserializing)]
    Ended { blame: Option<Runs>, why: String },
}

/// What the bindings of the cards of an instance read, so the window tells Bun at once only of a
/// change of that. `state` and `watches` name what they read, or are `None` for all of it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Reads {
    pub state: Option<Vec<String>>,
    pub children: bool,
    pub watches: Option<Vec<String>>,
}

/// Before Bun says, its cards read everything.
impl Default for Reads {
    fn default() -> Self {
        Self {
            state: None,
            children: true,
            watches: None,
        }
    }
}

/// The code of a tool Bun runs: its tool, and which code, such as `tick`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Runs {
    pub tool: String,
    pub what: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(from = "SentLoad")]
pub(crate) struct Loaded {
    pub tools: Vec<LoadedTool>,
    pub errors: Vec<LoadError>,
}

/// A tool as Bun loaded it: everything the runtime needs of its code but its cards and loop.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoadedTool {
    pub info: ToolInfo,
    /// The sound for every combination of its choices.
    pub sounds: Vec<(Map<String, Value>, Result<Graph, String>)>,
    /// The ops of its view at its defaults, after one tick.
    pub view: Result<Value, String>,
}

/// A load as `host.ts` sends it. Each tool is read on its own, so one that this side cannot
/// read is an error of its file, and the others load.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SentLoad {
    tools: Vec<Value>,
    errors: Vec<LoadError>,
}

impl From<SentLoad> for Loaded {
    fn from(sent: SentLoad) -> Self {
        let SentLoad { tools, mut errors } = sent;
        let tools = tools
            .into_iter()
            .filter_map(|tool| match LoadedTool::deserialize(&tool) {
                Ok(tool) => Some(tool),
                Err(error) => {
                    let text = |key| tool["info"][key].as_str().unwrap_or_default();
                    errors.push(LoadError {
                        file: text("file").to_string(),
                        message: format!("tool {}: {error}", text("name")),
                    });
                    None
                }
            })
            .collect();
        Self { tools, errors }
    }
}

/// What is wrong with a file of `extensions/`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoadError {
    pub file: String,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Line {
    /// It loads this file now.
    Loading {
        file: String,
    },
    Loaded(Loaded),
    /// It runs this code of a tool now.
    Runs(Runs),
    /// It did the oldest thing it was told and had not done.
    Done,
    /// The behaviour of an owner answered a question: its declaration, or why there is none.
    Declared {
        question: u64,
        declaration: Result<Value, String>,
        #[serde(default)]
        reads: Option<behaviours::Reads>,
    },
    #[serde(untagged)]
    Event(Event),
}

/// Why Bun did not load, and the file it was loading then, which is to blame.
pub(crate) struct LoadFailure {
    pub file: Option<String>,
    pub why: String,
}

/// What Bun has to do, for the thread that hears it.
struct Busy {
    /// How many things it was told and has not done.
    pending: usize,
    /// When it last took a step: started the code of a tool, finished what it was told, or was
    /// told something when it had nothing to do.
    since: Instant,
    /// The code it runs now.
    runs: Option<Runs>,
}

pub(crate) struct Bun {
    /// Which Bun of the process this is, counted from its start.
    number: u64,
    /// Which opening of a project it runs for, see [`new_opening`].
    opening: u64,
    /// Lines on their way to Bun, which a thread of their own writes: a Bun that stops reading
    /// does not block the window.
    lines: mpsc::SyncSender<String>,
    busy: Arc<Mutex<Busy>>,
    events: smol::channel::Receiver<Event>,
    /// Called once Bun said something, from the thread that hears it.
    waker: Arc<Mutex<Option<Waker>>>,
    /// Ended by [`Bun::end`], or when the runtime lets go of it. `None` once ended. The tools
    /// it loaded hold it too, so their records keep asking it outside the window.
    child: Mutex<Option<Child>>,
    /// Disconnected once the thread that passes on what Bun prints is done.
    printed: Mutex<mpsc::Receiver<()>>,
    told: Mutex<Told>,
}

/// What Bun was told of the records, so it is told only what changed: a control loop of a
/// parent of hundreds of children costs no more than one of none.
#[derive(Default)]
struct Told {
    /// The key of each record as it was told.
    records: BTreeMap<InstanceId, u64>,
    /// How many edits of Bun the runtime heard, and how many of them Bun was told of.
    edits: u64,
    edits_told: u64,
    /// The records a behaviour told it since the window last looked: one of a group that did
    /// not apply is not what the project holds.
    unconfirmed: BTreeSet<InstanceId>,
}

/// A record to tell Bun: its id, and its tool, its key (see [`record_key`]) and `state` as
/// JSON, or `None` when it is gone.
pub(crate) type Telling<'a> = (&'a InstanceId, Option<(&'a str, u64, &'a str)>);

/// What wakes a window that waits for nothing else: Bun said something.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What tells one record apart from another: its tool and its state as JSON.
pub(crate) fn record_key(tool: &str, state: &str) -> u64 {
    crate::tools::key(&(tool, state))
}

static NEXT_BUN: AtomicU64 = AtomicU64::new(0);

static NEXT_OPENING: AtomicU64 = AtomicU64::new(0);

/// A number for one opening of a project, which every Bun of it runs for: the first, and those
/// the window starts again after a change of the code. A project opened again in the same
/// process, as by a test, is another opening.
pub(crate) fn new_opening() -> u64 {
    NEXT_OPENING.fetch_add(1, Ordering::Relaxed)
}

impl Bun {
    /// Starts Bun on `host_script` in `folder`, to load `files` of it for the project's
    /// `opening`, and waits for its load. What its code prints goes to [`LOG_FILE`] too when
    /// it `logs`: only a runtime that holds the project writes it. Blocks for as long as the
    /// load takes: never call it on the thread that draws.
    pub(crate) fn start(
        program: &Path,
        host_script: &Path,
        (folder, opening): (&Path, u64),
        files: &[&str],
        logs: bool,
    ) -> Result<(Self, Loaded), LoadFailure> {
        let failure = |why: String| LoadFailure { file: None, why };
        let number = NEXT_BUN.fetch_add(1, Ordering::Relaxed);
        #[allow(clippy::disallowed_methods)]
        let mut child = sound_core::process::background_command(program)
            .arg(host_script)
            // Its questions are numbered from its own: an answer meant for a Bun before it
            // settles none of them.
            .arg(number.to_string())
            .args(files)
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| failure(format!("did not start: {error}")))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(failure("started without pipes".to_string()));
        };
        // The tools run without a log they cannot write.
        let log = match logs.then(|| std::fs::File::create(folder.join(LOG_FILE))) {
            Some(Ok(file)) => Some(file),
            Some(Err(error)) => {
                eprintln!("error: could not write {LOG_FILE}: {error}");
                None
            }
            None => None,
        };
        let (events, received) = smol::channel::unbounded();
        let (said, heard) = mpsc::channel();
        let (lines, to_write) = mpsc::sync_channel::<String>(QUEUED_LINES);
        let (printing, printed) = mpsc::channel::<()>();
        let busy = Arc::new(Mutex::new(Busy {
            pending: 0,
            since: Instant::now(),
            runs: None,
        }));
        // From here a failure drops it, which ends the process.
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let bun = Self {
            number,
            opening,
            lines,
            busy: busy.clone(),
            events: received,
            waker: waker.clone(),
            child: Mutex::new(Some(child)),
            printed: Mutex::new(printed),
            told: Mutex::default(),
        };
        std::thread::Builder::new()
            .name("bun".into())
            .spawn(move || read(stdout, &said))
            .map_err(|error| failure(error.to_string()))?;
        std::thread::Builder::new()
            .name("bun-log".into())
            .spawn(move || {
                let _printing = printing;
                print(stderr, log);
            })
            .map_err(|error| failure(error.to_string()))?;
        std::thread::Builder::new()
            .name("bun-input".into())
            .spawn(move || {
                let mut stdin = stdin;
                // Until Bun or the runtime is gone.
                for line in to_write {
                    if stdin
                        .write_all(line.as_bytes())
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| failure(error.to_string()))?;
        let mut file = None;
        let why = loop {
            match heard.recv_timeout(LOAD_TIMEOUT) {
                Ok(Line::Loading { file: next }) => file = Some(next),
                Ok(Line::Loaded(loaded)) => {
                    std::thread::Builder::new()
                        .name("bun-events".into())
                        .spawn(move || {
                            let wake = || {
                                let waker = waker.lock().unwrap_or_else(PoisonError::into_inner);
                                if let Some(wake) = waker.as_ref() {
                                    wake();
                                }
                            };
                            hear(number, &heard, &busy, &events, wake);
                        })
                        .map_err(|error| failure(error.to_string()))?;
                    return Ok((bun, loaded));
                }
                // Bun runs no tool and does nothing it is told before its load.
                Ok(Line::Runs(_) | Line::Done | Line::Declared { .. } | Line::Event(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break format!("did not finish loading within {LOAD_TIMEOUT:?}");
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break "stopped".to_string(),
            }
        };
        Err(LoadFailure { file, why })
    }

    /// Whether it did all it was told, and the window heard all it said.
    pub(crate) fn is_idle(&self) -> bool {
        self.is_done() && self.events.is_empty()
    }

    /// Whether it did all it was told: what it said for it waits in [`Bun::events`].
    pub(crate) fn is_done(&self) -> bool {
        self.busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            == 0
    }

    /// What Bun says after its load, in order, up to [`Event::Ended`].
    pub(crate) fn events(&self) -> smol::channel::Receiver<Event> {
        self.events.clone()
    }

    /// Calls `waker` each time Bun says something from now on: an event or a behaviour's answer.
    pub(crate) fn wake_with(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    pub(crate) fn number(&self) -> u64 {
        self.number
    }

    pub(crate) fn opening(&self) -> u64 {
        self.opening
    }

    /// Tells Bun something and does not wait. Whether it went: not when Bun stopped reading,
    /// which the thread that hears it finds hung, or stopped.
    pub(crate) fn send(&self, request: &Request) -> bool {
        let line = match serde_json::to_string(request) {
            Ok(json) => json + "\n",
            Err(error) => {
                eprintln!("error: {error}");
                return false;
            }
        };
        let lock = || self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        // Counted before it goes, so its done never comes before it is counted.
        {
            let mut busy = lock();
            if busy.pending == 0 {
                busy.since = Instant::now();
            }
            busy.pending += 1;
        }
        let sent = self.lines.try_send(line).is_ok();
        if !sent {
            done(&mut lock());
        }
        sent
    }

    /// Tells Bun each of `records` it does not hold as it is, and the edits the runtime heard.
    /// Whether it went.
    pub(crate) fn tell<'a>(&self, records: impl IntoIterator<Item = Telling<'a>>) -> bool {
        let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        let edits = told.edits;
        self.tell_in(&mut told, records, edits).is_some()
    }

    /// Tells Bun the record of `owner` and of each of its `children` as its behaviour gets
    /// them, and that each other child it holds is gone. Whether it went. Not the edits heard
    /// since the window last told it: the records those changed may be others, still to tell.
    pub(crate) fn tell_owner<'a>(
        &self,
        owner: (&'a InstanceId, &'a str, &'a str),
        children: impl IntoIterator<Item = (&'a InstanceId, &'a str, u64, &'a str)>,
    ) -> bool {
        let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        let (id, tool, state) = owner;
        let mut records = vec![(id, Some((tool, record_key(tool, state), state)))];
        records.extend(
            (children.into_iter()).map(|(id, tool, key, state)| (id, Some((tool, key, state)))),
        );
        let given: BTreeSet<&InstanceId> = records.iter().map(|(id, _)| *id).collect();
        let held: Vec<InstanceId> = (told.records.keys())
            .filter(|child| child.is_inside(id) && !given.contains(child))
            .cloned()
            .collect();
        records.extend(held.iter().map(|child| (child, None)));
        let edits = told.edits_told;
        let Some(sent) = self.tell_in(&mut told, records, edits) else {
            return false;
        };
        told.unconfirmed.extend(sent);
        true
    }

    /// The ids it told, or `None` when it did not go: what did not go is told again next time.
    fn tell_in<'a>(
        &self,
        told: &mut Told,
        records: impl IntoIterator<Item = Telling<'a>>,
        edits: u64,
    ) -> Option<Vec<InstanceId>> {
        let mut new = Vec::new();
        let mut keys = Vec::new();
        let mut gone = Vec::new();
        for (id, record) in records {
            let Some((tool, key, state)) = record else {
                if told.records.contains_key(id) {
                    gone.push(id);
                }
                continue;
            };
            if told.records.get(id) == Some(&key) {
                continue;
            }
            match serde_json::from_str(state) {
                Ok(state) => {
                    keys.push((id.clone(), key));
                    new.push(ToldRecord { id, tool, state });
                }
                Err(error) => eprintln!("error: {id}: {error}"),
            }
        }
        if new.is_empty() && gone.is_empty() && edits == told.edits_told {
            return Some(Vec::new());
        }
        let request = Request::Records {
            records: new,
            gone: gone.clone(),
            edits,
        };
        if !self.send(&request) {
            return None;
        }
        told.edits_told = edits;
        for id in &gone {
            told.records.remove(*id);
        }
        let mut sent: Vec<InstanceId> = gone.into_iter().cloned().collect();
        sent.extend(keys.iter().map(|(id, _)| id.clone()));
        told.records.extend(keys);
        Some(sent)
    }

    /// The runtime heard an edit Bun sent, applied or not.
    pub(crate) fn heard_edit(&self) {
        let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        told.edits += 1;
    }

    /// The records a behaviour told Bun since the last call, to tell it again as they are.
    #[allow(
        dead_code,
        reason = "the window uses it once it does all the old app did"
    )]
    pub(crate) fn take_unconfirmed(&self) -> BTreeSet<InstanceId> {
        let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut told.unconfirmed)
    }
}

/// Reads what Bun says until it stops, and passes it on to `said`.
fn read(stdout: ChildStdout, said: &mpsc::Sender<Line>) {
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("error: the TypeScript host stopped: {error}");
                break;
            }
        };
        match serde_json::from_str(&line) {
            Ok(line) => {
                if said.send(line).is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("error: the TypeScript host said something unknown: {error}"),
        }
    }
}

/// Passes what the code of the tools prints on to the runtime's own stderr, and keeps its
/// last lines in `log`, until Bun stops.
fn print(printed: impl Read, mut log: Option<std::fs::File>) {
    let mut printed = BufReader::new(printed);
    let mut kept: VecDeque<Vec<u8>> = VecDeque::new();
    loop {
        let mut line = Vec::new();
        match printed.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        std::io::stderr().write_all(&line).ok();
        let Some(file) = &mut log else {
            continue;
        };
        let written = if kept.len() < LOG_LINES {
            let written = file.write_all(&line);
            kept.push_back(line);
            written
        } else {
            // Cut to the last half at once: a tool that prints on every frame does not write
            // the whole log again for each line.
            kept.drain(..LOG_LINES / 2);
            kept.push_back(line);
            (file.set_len(0))
                .and_then(|()| file.rewind())
                .and_then(|()| kept.iter().try_for_each(|line| file.write_all(line)))
        };
        if let Err(error) = written {
            eprintln!("error: could not write {LOG_FILE}: {error}");
            log = None;
        }
    }
}

/// One thing Bun was told is done, or will never be.
fn done(busy: &mut Busy) {
    if busy.pending > 0 {
        busy.pending -= 1;
    }
}

/// Hears what Bun `number` says after its load and passes its events on to `events`, and the
/// answers of behaviours to them, until it hangs or stops, which [`Event::Ended`] says.
fn hear(
    number: u64,
    heard: &mpsc::Receiver<Line>,
    busy: &Mutex<Busy>,
    events: &smol::channel::Sender<Event>,
    wake: impl Fn(),
) {
    let lock = || busy.lock().unwrap_or_else(PoisonError::into_inner);
    let ended = |why: String| {
        behaviours::ended(number, &why);
        let mut busy = lock();
        while busy.pending > 0 {
            done(&mut busy);
        }
        let blame = busy.runs.take();
        drop(busy);
        // Outside the window nobody hears it.
        events.try_send(Event::Ended { blame, why }).ok();
        wake();
    };
    loop {
        match heard.recv_timeout(HANG_TIMEOUT / 4) {
            // A step: code of a tool starts, or what it was told is done.
            Ok(Line::Runs(runs)) => {
                let mut busy = lock();
                busy.since = Instant::now();
                busy.runs = Some(runs);
            }
            Ok(Line::Done) => {
                let mut busy = lock();
                busy.since = Instant::now();
                done(&mut busy);
                busy.runs = None;
            }
            // Code that says things in a loop that never ends says more than the window hears.
            Ok(Line::Event(_)) if events.len() >= MAX_EVENTS => {
                return ended(format!("said more than {MAX_EVENTS} things at once"));
            }
            Ok(Line::Event(event)) => {
                events.try_send(event).ok();
                wake();
            }
            Ok(Line::Declared {
                question,
                declaration,
                reads,
            }) => {
                let answer = declaration.map_err(|error| format!("its behaviour failed: {error}"));
                let answer = answer.and_then(|declaration| {
                    serde_path_to_error::deserialize(declaration).map_err(|error| {
                        format!(
                            "its behaviour returned what is no declaration: {}: {}",
                            error.path(),
                            error.inner()
                        )
                    })
                });
                behaviours::answered(question, answer, reads);
                wake();
            }
            // Bun loads once.
            Ok(Line::Loading { .. } | Line::Loaded(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return ended("stopped".to_string()),
        }
        // Also while it says things: a loop that never ends may.
        let busy = lock();
        if busy.pending > 0 && busy.since.elapsed() > HANG_TIMEOUT {
            drop(busy);
            return ended(format!("took longer than {HANG_TIMEOUT:?}"));
        }
    }
}

impl Bun {
    /// Ends the process, also while tools it loaded still hold it: they find it gone. Waits for
    /// it to end: not on the thread that draws.
    pub(crate) fn end(&self) {
        let child = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut child) = child {
            // Already ended, or never to be waited for: nothing to report on the way out.
            child.kill().ok();
            child.wait().ok();
        }
        // So its last lines are in the log before a new Bun empties it. Not for ever: a
        // process its code started may still hold the pipe.
        (self.printed.lock().unwrap_or_else(PoisonError::into_inner))
            .recv_timeout(Duration::from_secs(1))
            .ok();
    }
}

impl Drop for Bun {
    fn drop(&mut self) {
        let child = self
            .child
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(mut child) = child else {
            return;
        };
        child.kill().ok();
        // The last tool that held it may go on the thread that draws, which does not wait.
        let waited = std::thread::Builder::new()
            .name("bun-end".into())
            .spawn(move || child.wait().ok());
        if let Err(error) = waited {
            eprintln!("error: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_of_a_tool_that_prints_on_and_on_keeps_its_last_lines() {
        let folder = tempfile::tempdir().unwrap();
        let printed: String = (0..1000).map(|line| format!("line {line}\n")).collect();
        let path = folder.path().join(LOG_FILE);
        print(
            printed.as_bytes(),
            Some(std::fs::File::create(&path).unwrap()),
        );
        let log = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(
            (LOG_LINES / 2..=LOG_LINES).contains(&lines.len()),
            "{} lines",
            lines.len()
        );
        let last: Vec<String> = (1000 - lines.len()..1000)
            .map(|line| format!("line {line}"))
            .collect();
        assert_eq!(lines, last);
    }

    #[test]
    fn a_tool_this_side_cannot_read_is_an_error_of_its_file_and_the_others_load() {
        let tool = |name: &str, unit: &str| {
            serde_json::json!({
                "info": {
                    "name": name, "file": format!("{name}.ts"), "title": name, "when": "w",
                    "doc": "d", "voices": 1, "input": true, "sound": true, "tick": false, "view": false,
                    "keys": false, "notes": false, "on_take": false, "behaviour": false, "inputs": {},
                    "fields": { "rate": { "kind": "number", "min": 0, "max": 1, "default": 0, "unit": unit } }
                },
                "sounds": [], "view": { "Err": "no view" }
            })
        };
        let line = serde_json::json!({
            "type": "loaded",
            "tools": [tool("good", "hz"), tool("bad", "furlongs")],
            "errors": [{ "file": "broken.ts", "message": "SyntaxError" }]
        });
        let Ok(Line::Loaded(loaded)) = serde_json::from_value(line) else {
            panic!("a load with one bad tool is no load");
        };
        let names: Vec<&str> = (loaded.tools.iter())
            .map(|tool| tool.info.name.as_str())
            .collect();
        assert_eq!(names, ["good"]);
        let files: Vec<&str> = loaded
            .errors
            .iter()
            .map(|error| error.file.as_str())
            .collect();
        assert_eq!(files, ["broken.ts", "bad.ts"]);
        let message = &loaded.errors[1].message;
        assert!(
            message.starts_with("tool bad: unknown variant `furlongs`"),
            "{message}"
        );
    }
}
