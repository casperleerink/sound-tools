//! The Bun process that runs `host.ts`. It loads the files of `extensions/` once and says what
//! it loaded, then hears what the window tells it and says what the tools do. A change of the
//! code starts a new Bun. One JSON message per line, both ways.
//!
//! Bun does one thing at a time and says which tool's code it runs before it runs it, so a
//! thread here that hears it knows what hangs, without the window: the window may be busy.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
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
use crate::tree::Op;
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

/// The files of `extensions/` Bun runs without, because their code hung or stopped it, with
/// what to tell the composer. A change in the folder clears it.
pub(crate) type Skipped = BTreeMap<String, String>;

/// What the window tells.
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
    /// A view opens: its code makes its nodes once.
    Mount {
        card: u64,
        instance: &'a InstanceId,
        /// The last value of each watch of the instance, by name.
        watches: &'a BTreeMap<String, f32>,
        /// The room the window gives the view: the whole window, or the body of a card.
        size: PageSize,
        #[serde(flatten)]
        clock: Clock,
    },
    /// The view has another room.
    Resize {
        card: u64,
        size: PageSize,
    },
    /// The watches of an instance moved, and a binding of its cards reads them.
    Watches {
        instance: &'a InstanceId,
        watches: &'a BTreeMap<String, f32>,
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
    /// A press, a click, or a drag that ended or moved, on a node: `payload` is what its
    /// handler hears.
    Event {
        card: u64,
        node: u64,
        kind: EventKind,
        payload: Value,
        time: f64,
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
    /// What an instance whose tool hears notes heard since it last did.
    Notes {
        instance: Played<'a>,
        events: &'a [Heard],
        #[serde(flatten)]
        clock: Clock,
    },
    Drop {
        card: u64,
    },
    /// The answer to an `openFile` of a tool: the name under `assets/audio/` of the file the
    /// composer chose and the runtime copied there, or none.
    FileOpened {
        question: u64,
        asset: Option<&'a str>,
    },
    /// What the behaviour of an owner declares for its record and its children, as Bun was
    /// told them.
    Behaviour {
        question: u64,
        tool: &'a str,
        instance: &'a InstanceId,
    },
}

/// What happened on a node, which names the handler that hears it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    Press,
    End,
    Move,
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

/// The room a page has, in whole points.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) struct PageSize {
    pub width: u32,
    pub height: u32,
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
        }
    }
}

impl Transport {
    /// What bindings follow: not the playhead, which moves on every frame.
    pub(crate) fn followed(&self) -> (bool, Option<[u64; 2]>, bool) {
        (self.playing, self.repeat, self.click)
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
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The control loops ran the last frame.
    Framed,
    /// The tick of a tool, or a click on its card, failed: the first time.
    Failed { tool: String, message: String },
    /// What changed on the nodes of a card, or why it failed. Read by [`read_line`].
    #[serde(skip_deserializing)]
    Ops {
        card: u64,
        ops: Result<Vec<Op>, String>,
        /// The ops make all of it again, as after a new Bun.
        mount: bool,
        /// They answer a press or a drag, whose preview lets go with them.
        event: bool,
    },
    /// What the bindings of an instance read now: the window tells Bun at once only of that.
    Reads { instance: InstanceId, reads: Reads },
    /// The code of an instance changed its record or those of its children, as one edit
    /// called `label`.
    Edit {
        instance: InstanceId,
        label: String,
        changes: Vec<RecordEdit>,
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
/// change of that. `state` names the fields they read, or is `None` for all of them.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Reads {
    pub state: Option<Vec<String>>,
    pub children: bool,
    pub watches: bool,
}

/// Before Bun says, its cards read everything.
impl Default for Reads {
    fn default() -> Self {
        Self {
            state: None,
            children: true,
            watches: true,
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
    /// Ended by [`Bun::end`], or when the runtime lets go of it. `None` once ended. The tools
    /// it loaded hold it too, so their records keep asking it outside the window.
    child: Mutex<Option<Child>>,
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

/// What tells one record apart from another: its tool and its state as JSON.
pub(crate) fn record_key(tool: &str, state: &str) -> u64 {
    crate::tools::key(&(tool, state))
}

/// The number of the next Bun of the process.
static NEXT_BUN: AtomicU64 = AtomicU64::new(0);

static NEXT_OPENING: AtomicU64 = AtomicU64::new(0);

static SENT_BYTES: AtomicU64 = AtomicU64::new(0);
static SENT_UPDATES: AtomicU64 = AtomicU64::new(0);
static SENT_EVENTS: AtomicU64 = AtomicU64::new(0);
static HEARD_BATCHES: AtomicU64 = AtomicU64::new(0);
static HEARD_BYTES: AtomicU64 = AtomicU64::new(0);
/// What the last batch of ops cost: Bun's milliseconds and the window's microseconds to apply.
static LAST_BUN_MICROS: AtomicU64 = AtomicU64::new(0);
static LAST_APPLY_MICROS: AtomicU64 = AtomicU64::new(0);

/// How many bytes this process told every Bun, for a test of what a frame costs.
pub fn sent_bytes() -> u64 {
    SENT_BYTES.load(Ordering::Relaxed)
}

/// How many times this process told every Bun something that changes what cards show (records,
/// watches, the transport, a card that opens) and how many presses and drags it told, for a
/// test that a drag does not cross to Bun.
pub fn sent_updates_and_events() -> (u64, u64) {
    (
        SENT_UPDATES.load(Ordering::Relaxed),
        SENT_EVENTS.load(Ordering::Relaxed),
    )
}

/// How many batches of ops the windows of this process applied, for a measurement of how long
/// one takes to come.
pub fn heard_batches() -> u64 {
    HEARD_BATCHES.load(Ordering::Relaxed)
}

/// How many bytes of ops this process heard from every Bun, for a measurement of their size.
pub fn heard_bytes() -> u64 {
    HEARD_BYTES.load(Ordering::Relaxed)
}

/// What the last batch of ops cost, in milliseconds: Bun's work for its request, and the
/// window's to apply it.
pub fn last_batch() -> (f64, f64) {
    let millis = |micros: &AtomicU64| micros.load(Ordering::Relaxed) as f64 / 1000.0;
    (millis(&LAST_BUN_MICROS), millis(&LAST_APPLY_MICROS))
}

pub(crate) fn count_batch(apply: Duration) {
    HEARD_BATCHES.fetch_add(1, Ordering::Relaxed);
    LAST_APPLY_MICROS.store(apply.as_micros() as u64, Ordering::Relaxed);
}

/// A number for one opening of a project, which every Bun of it runs for: the first, and those
/// the window starts again after a change of the code. A project opened again in the same
/// process, as by a test, is another opening.
pub(crate) fn new_opening() -> u64 {
    NEXT_OPENING.fetch_add(1, Ordering::Relaxed)
}

impl Bun {
    /// Starts Bun on `host_script` in `folder`, to load `files` of it for the project's
    /// `opening`, and waits for its load. Blocks for as long as the load takes: never call it
    /// on the thread that draws.
    pub(crate) fn start(
        program: &Path,
        host_script: &Path,
        (folder, opening): (&Path, u64),
        files: &[&str],
    ) -> Result<(Self, Loaded), LoadFailure> {
        let failure = |why: String| LoadFailure { file: None, why };
        #[allow(clippy::disallowed_methods)]
        let mut child = sound_core::process::background_command(program)
            .arg(host_script)
            .args(files)
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| failure(format!("did not start: {error}")))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(failure("started without pipes".to_string()));
        };
        let (events, received) = smol::channel::unbounded();
        let (said, heard) = mpsc::channel();
        let (lines, to_write) = mpsc::sync_channel::<String>(QUEUED_LINES);
        let busy = Arc::new(Mutex::new(Busy {
            pending: 0,
            since: Instant::now(),
            runs: None,
        }));
        // From here a failure drops it, which ends the process.
        let number = NEXT_BUN.fetch_add(1, Ordering::Relaxed);
        let bun = Self {
            number,
            opening,
            lines,
            busy: busy.clone(),
            events: received,
            child: Mutex::new(Some(child)),
            told: Mutex::default(),
        };
        std::thread::Builder::new()
            .name("bun".into())
            .spawn(move || read(stdout, &said))
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
                        .spawn(move || hear(number, &heard, &busy, &events))
                        .map_err(|error| failure(error.to_string()))?;
                    return Ok((bun, loaded));
                }
                // Bun runs no tool and does nothing it is told before its load.
                Ok(Line::Runs(_) | Line::Done | Line::Declared { .. } | Line::Event(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break format!("did not finish loading within {LOAD_TIMEOUT:?}");
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break "stopped Bun".to_string(),
            }
        };
        Err(LoadFailure { file, why })
    }

    /// What Bun says after its load, in order, up to [`Event::Ended`].
    pub(crate) fn events(&self) -> smol::channel::Receiver<Event> {
        self.events.clone()
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
        let length = line.len() as u64;
        let sent = self.lines.try_send(line).is_ok();
        if sent {
            SENT_BYTES.fetch_add(length, Ordering::Relaxed);
            match request {
                Request::Records { .. }
                | Request::Mount { .. }
                | Request::Resize { .. }
                | Request::Watches { .. }
                | Request::Clock { .. } => SENT_UPDATES.fetch_add(1, Ordering::Relaxed),
                Request::Event { .. } => SENT_EVENTS.fetch_add(1, Ordering::Relaxed),
                _ => 0,
            };
        } else {
            let mut busy = lock();
            busy.pending = busy.pending.saturating_sub(1);
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
            .filter(|child| child.parent().as_ref() == Some(id) && !given.contains(child))
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
    pub(crate) fn take_unconfirmed(&self) -> BTreeSet<InstanceId> {
        let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut told.unconfirmed)
    }
}

/// The `type` of a line, which every line has.
#[derive(Deserialize)]
struct Kind<'a> {
    #[serde(rename = "type", borrow)]
    kind: std::borrow::Cow<'a, str>,
}

/// The ops of a card, as [`Event::Ops`].
#[derive(Deserialize)]
struct OpsLine<'a> {
    card: u64,
    #[serde(borrow)]
    ops: Option<&'a serde_json::value::RawValue>,
    error: Option<String>,
    #[serde(default)]
    mount: bool,
    #[serde(default)]
    event: bool,
    #[serde(default)]
    took: f64,
}

/// A line of Bun. Ops are read on their own road, here on the thread that hears Bun, not the
/// window's: the first ops of a page of thousands of notes are megabytes.
fn read_line(line: &str) -> serde_json::Result<Line> {
    if serde_json::from_str::<Kind>(line)?.kind != "ops" {
        return serde_json::from_str(line);
    }
    let OpsLine {
        card,
        ops,
        error,
        mount,
        event,
        took,
    } = serde_json::from_str(line)?;
    HEARD_BYTES.fetch_add(line.len() as u64, Ordering::Relaxed);
    LAST_BUN_MICROS.store((took * 1000.0) as u64, Ordering::Relaxed);
    let ops = match (ops, error) {
        (_, Some(error)) => Err(error),
        (Some(ops), None) => serde_json::from_str(ops.get()).map_err(|error| error.to_string()),
        (None, None) => Ok(Vec::new()),
    };
    Ok(Line::Event(Event::Ops {
        card,
        ops,
        mount,
        event,
    }))
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
        match read_line(&line) {
            Ok(line) => {
                if said.send(line).is_err() {
                    break;
                }
            }
            Err(error) => eprintln!("error: the TypeScript host said something unknown: {error}"),
        }
    }
}

/// Hears what Bun `number` says after its load and passes its events on to `events`, and the
/// answers of behaviours to them, until it hangs or stops, which [`Event::Ended`] says.
fn hear(
    number: u64,
    heard: &mpsc::Receiver<Line>,
    busy: &Mutex<Busy>,
    events: &smol::channel::Sender<Event>,
) {
    let lock = || busy.lock().unwrap_or_else(PoisonError::into_inner);
    let ended = |why: String| {
        behaviours::ended(number, &why);
        let blame = lock().runs.take();
        // Outside the window nobody hears it.
        events.try_send(Event::Ended { blame, why }).ok();
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
                busy.pending = busy.pending.saturating_sub(1);
                busy.runs = None;
            }
            // Code that says things in a loop that never ends says more than the window hears.
            Ok(Line::Event(_)) if events.len() >= MAX_EVENTS => {
                return ended(format!("said more than {MAX_EVENTS} things at once"));
            }
            Ok(Line::Event(event)) => {
                events.try_send(event).ok();
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
            }
            // Bun loads once.
            Ok(Line::Loading { .. } | Line::Loaded(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return ended("stopped Bun".to_string()),
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
    fn a_tool_this_side_cannot_read_is_an_error_of_its_file_and_the_others_load() {
        let tool = |name: &str, unit: &str| {
            serde_json::json!({
                "info": {
                    "name": name, "file": format!("{name}.ts"), "title": name, "when": "w",
                    "doc": "d", "voices": 1, "input": true, "tick": false, "view": false,
                    "keys": false, "notes": false, "behaviour": false, "inputs": {},
                    "fields": { "rate": { "kind": "number", "min": 0, "max": 1, "default": 0, "unit": unit } }
                },
                "sounds": [], "view": { "Err": "no view" }
            })
        };
        let line = serde_json::json!({
            "type": "loaded",
            "tools": [tool("good", "hz"), tool("bad", "seconds")],
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
            message.starts_with("tool bad: unknown variant `seconds`"),
            "{message}"
        );
    }
}
