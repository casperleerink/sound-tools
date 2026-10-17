//! The Bun process that runs `host.ts`. It loads the files of `extensions/` once and says what
//! it loaded, then hears what the window tells it and says what the tools do. A change of the
//! code starts a new Bun. One JSON message per line, both ways.
//!
//! Bun does one thing at a time and says which tool's code it runs before it runs it, so a
//! thread here that hears it knows what hangs, without the window: the window may be busy.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use sound_core::InstanceId;
use sound_notes::Quantize;
use sound_signals::Graph;

use crate::tools::ToolInfo;
use crate::{Midi, behaviours};

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
    Render {
        card: u64,
        instance: &'a InstanceId,
        tool: &'a str,
        state: Value,
        /// The last value of each watch of the instance, by name.
        watches: &'a BTreeMap<String, f32>,
        /// The size of the page, the whole window; `None` for a card.
        page: Option<PageSize>,
        transport: Transport,
    },
    /// A click, or a press or a drag on a canvas `at` across and down, 0 to 1.
    Event {
        card: u64,
        /// The version of the tree that was clicked.
        version: u64,
        handler: usize,
        at: Option<(f32, f32)>,
    },
    /// One step of the control loop of every instance whose tool has one.
    Frame {
        /// Seconds since the last frame.
        dt: f32,
        /// Engine time in seconds: the clock `at` of an event counts on.
        time: f64,
        instances: Vec<Looped<'a>>,
    },
    /// A key of the computer keyboard went down or up on a card or a page that has the keys.
    Key {
        time: f64,
        instance: Looped<'a>,
        key: &'a str,
        down: bool,
    },
    /// What the MIDI keyboard played, for the instances that hear it.
    Midi {
        time: f64,
        instances: Vec<Looped<'a>>,
        messages: &'a [Midi],
    },
    Drop {
        card: u64,
    },
    /// What the behaviour of an owner declares for its record, as it plays, and its children.
    Behaviour {
        question: u64,
        tool: &'a str,
        state: &'a Value,
        /// Each as `{ name, tool, state }`.
        children: &'a Value,
    },
}

/// The room a page has, in whole points.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) struct PageSize {
    pub width: u32,
    pub height: u32,
}

/// Where the project plays, as the window's playhead shows it.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) struct Transport {
    pub playing: bool,
    pub tick: u64,
    pub bar: u64,
    pub beat: u32,
    /// The tempo at `tick`.
    pub bpm: f64,
}

/// An instance whose tool has a control loop, as it is now.
#[derive(Serialize)]
pub(crate) struct Looped<'a> {
    pub instance: &'a InstanceId,
    pub tool: &'a str,
    pub state: Value,
    pub watches: BTreeMap<String, f32>,
    /// How many edits of its record from Bun `state` has: fewer than Bun sent, and Bun starts
    /// from the last record it sent instead.
    pub edits: u64,
}

/// What the window hears of Bun after its load.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The control loops ran the last frame.
    Framed,
    /// The tick of a tool, or a click on its card, failed: the first time.
    Failed { tool: String, message: String },
    /// A card drew, with the version of its tree, which a click on it names, so it runs a
    /// handler of this tree and not of a newer one. Or why it did not.
    Tree {
        card: u64,
        tree: Result<(u64, Value), String>,
    },
    /// A card, a key or the MIDI keyboard changed the record of an instance.
    Edit {
        instance: InstanceId,
        label: String,
        state: Value,
    },
    /// A card or a control loop moved a live control of an instance, or fired a trigger when
    /// there is no value: at `at`, engine time in seconds, or at once.
    Control {
        instance: InstanceId,
        name: String,
        value: Option<f32>,
        at: Option<f64>,
    },
    /// A card or a control loop played a note of an instance: a key held for `seconds` from
    /// `at`, engine time in seconds, or from now; `velocity` from 0 to 1. With no `seconds` it
    /// is held until its release.
    Note {
        instance: InstanceId,
        pitch: u8,
        velocity: f32,
        seconds: Option<f32>,
        at: Option<f64>,
    },
    /// Lets go of a held note of an instance, at `at` or now.
    Release {
        instance: InstanceId,
        pitch: u8,
        at: Option<f64>,
    },
    /// Launches the pattern of index `pattern` on the player `player` of an owner, or stops it
    /// with none, from the next point of `quantize`.
    Launch {
        instance: InstanceId,
        player: String,
        pattern: Option<usize>,
        quantize: Quantize,
    },
    /// Bun hung or stopped, the last thing it says: `why`, and the tool and the code of it that
    /// it ran then, which are to blame. Nothing to blame when it ran no tool's code, such as
    /// code a timer of a tool ran.
    #[serde(skip_deserializing)]
    Ended { blame: Option<Runs>, why: String },
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
    /// The tree of its card at its defaults, after one tick.
    pub card: Result<Value, String>,
    /// The same of its page, when it has one.
    pub page: Option<Result<Value, String>>,
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
    /// Lines on their way to Bun, which a thread of their own writes: a Bun that stops reading
    /// does not block the window.
    lines: mpsc::SyncSender<String>,
    busy: Arc<Mutex<Busy>>,
    events: smol::channel::Receiver<Event>,
    /// Ended by [`Bun::end`], or when the runtime lets go of it. `None` once ended. The tools
    /// it loaded hold it too, so their records keep asking it outside the window.
    child: Mutex<Option<Child>>,
}

/// The number of the next Bun of the process.
static NEXT_BUN: AtomicU64 = AtomicU64::new(0);

impl Bun {
    /// Starts Bun on `host_script` in `folder`, to load `files` of it, and waits for its load.
    /// Blocks for as long as the load takes: never call it on the thread that draws.
    pub(crate) fn start(
        program: &Path,
        host_script: &Path,
        folder: &Path,
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
            lines,
            busy: busy.clone(),
            events: received,
            child: Mutex::new(Some(child)),
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

    /// Which Bun of the process this is.
    pub(crate) fn number(&self) -> u64 {
        self.number
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
            let mut busy = lock();
            busy.pending = busy.pending.saturating_sub(1);
        }
        sent
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
        match serde_json::from_str::<Line>(&line) {
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
                behaviours::answered(question, answer);
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
                    "doc": "d", "kind": "effect", "tick": false, "page": false, "transport": false, "keys": false,
                    "midi": false, "behaviour": false, "controls": {},
                    "fields": { "rate": { "kind": "knob", "min": 0, "max": 1, "default": 0, "unit": unit } }
                },
                "sounds": [], "card": { "Err": "no card" }, "page": null
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
