//! The Bun process that runs `host.ts`, and the two ways to talk to it: a question that waits
//! for its answer, such as the Hum of a tool, and messages that come when they come, which the
//! window reads. One JSON message per line, both ways.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Midi;
use crate::tools::ToolInfo;

/// How long a question, or a step of the control loops, may take before Bun counts as hung
/// and is stopped. Bun answers in well under a millisecond; a tool whose code hangs must not
/// hang the window.
pub(crate) const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// The lines that may wait for Bun to read them. More, and Bun has stopped reading.
const QUEUED_LINES: usize = 1024;

/// How long the first load may take. Bun starts in a few tens of milliseconds.
const FIRST_LOAD_TIMEOUT: Duration = Duration::from_secs(10);

const STOPPED: &str = "the TypeScript host is not running";

/// What the runtime asks or tells.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Request<'a> {
    /// The Hum of a tool with these choices: the answer is its lines.
    Sound {
        id: u64,
        tool: &'a str,
        choices: &'a serde_json::Map<String, serde_json::Value>,
    },
    /// The tree of a tool's card, or its page, at its defaults after one tick: a check that
    /// it draws, without a window.
    Draw {
        id: u64,
        tool: &'a str,
        page: bool,
    },
    Render {
        card: u64,
        instance: &'a str,
        tool: &'a str,
        state: serde_json::Value,
        /// The last value of each watch of the instance, by name.
        watches: &'a BTreeMap<String, f32>,
        /// Whether it is drawn as a page, the whole window, and not as a card.
        page: bool,
    },
    /// A click, or a press or a drag on a canvas at `x` and `y` across and down, 0 to 1.
    Event {
        card: u64,
        /// The version of the tree that was clicked.
        version: u64,
        handler: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        x: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        y: Option<f32>,
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
}

/// An instance whose tool has a control loop, as it is now.
#[derive(Serialize)]
pub(crate) struct Looped<'a> {
    pub instance: &'a str,
    pub tool: &'a str,
    pub state: serde_json::Value,
    pub watches: BTreeMap<String, f32>,
}

/// What Bun says without being asked, or as the answer to a render or a click.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The files of `extensions/` loaded, at the start and after every save.
    Loaded(Loaded),
    /// The control loops ran the last frame.
    Framed,
    /// The tick of a tool, or a click on its card, failed: the first time since the last load.
    Failed { tool: String, message: String },
    /// A card drew, or failed to.
    Tree {
        card: u64,
        #[serde(default)]
        tree: Option<serde_json::Value>,
        /// What a click on it names, so it runs a handler of this tree and not of a newer one.
        #[serde(default)]
        version: u64,
        #[serde(default)]
        error: Option<String>,
    },
    /// A card, a key or the MIDI keyboard changed the record of an instance.
    Edit {
        instance: String,
        label: String,
        state: serde_json::Value,
    },
    /// A card or a control loop moved a live control of an instance, or fired a trigger when
    /// there is no value: at `at`, engine time in seconds, or at once.
    Control {
        instance: String,
        name: String,
        #[serde(default)]
        value: Option<f32>,
        #[serde(default)]
        at: Option<f64>,
    },
    /// A card or a control loop played a note of an instance: a key held for `seconds` from
    /// `at`, engine time in seconds, or from now; `velocity` from 0 to 1. With no `seconds` it
    /// is held until its release.
    Note {
        instance: String,
        pitch: u8,
        velocity: f32,
        #[serde(default)]
        seconds: Option<f32>,
        #[serde(default)]
        at: Option<f64>,
    },
    /// Lets go of a held note of an instance, at `at` or now.
    Release {
        instance: String,
        pitch: u8,
        #[serde(default)]
        at: Option<f64>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(from = "SentLoad")]
pub(crate) struct Loaded {
    pub tools: Vec<ToolInfo>,
    pub errors: Vec<LoadError>,
}

/// A load as `host.ts` sends it. Each tool is read on its own, so one that this side cannot
/// read is an error of its file, and the others load.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SentLoad {
    tools: Vec<serde_json::Value>,
    errors: Vec<LoadError>,
}

impl From<SentLoad> for Loaded {
    fn from(sent: SentLoad) -> Self {
        let SentLoad { tools, mut errors } = sent;
        let tools = tools
            .into_iter()
            .filter_map(|tool| match ToolInfo::deserialize(&tool) {
                Ok(info) => Some(info),
                Err(error) => {
                    let text = |key| tool.get(key).and_then(serde_json::Value::as_str);
                    errors.push(LoadError {
                        file: text("file").unwrap_or_default().to_string(),
                        message: format!("tool {}: {error}", text("name").unwrap_or_default()),
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

/// The answer to a question, see [`Bun::ask`].
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Answer {
    Answer {
        id: u64,
        #[serde(default)]
        value: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Line {
    Answer(Answer),
    Event(Event),
}

/// The questions that wait for their answer, by id. `None` once Bun stopped, so a question
/// fails at once and does not wait for its timeout.
type Waiting = Arc<Mutex<Option<HashMap<u64, mpsc::Sender<Result<serde_json::Value, String>>>>>>;

pub(crate) struct Bun {
    /// Lines on their way to Bun, which a thread of their own writes: a Bun that stops reading
    /// does not block the window.
    lines: mpsc::SyncSender<String>,
    waiting: Waiting,
    next_id: AtomicU64,
    events: smol::channel::Receiver<Event>,
    /// Ended when it hangs, or when the runtime lets go of it.
    child: Mutex<Child>,
    /// It hung and was stopped: once is enough.
    stopped: AtomicBool,
}

impl Bun {
    /// Starts Bun on `host_script` in `folder`, and waits for the first load of its files.
    pub(crate) fn start(
        program: &Path,
        host_script: &Path,
        folder: &Path,
    ) -> Result<(Self, Loaded), String> {
        // Before the project opens and before any window: its tools are registered from what
        // Bun loads first, so there is nothing to draw while it starts.
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(program)
            .arg(host_script)
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("bun did not start: {error}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("bun started without pipes".to_string());
        };
        let waiting: Waiting = Arc::new(Mutex::new(Some(HashMap::new())));
        let (events, received) = smol::channel::unbounded();
        let (first, first_load) = mpsc::channel();
        let (lines, to_write) = mpsc::sync_channel::<String>(QUEUED_LINES);
        // From here a failure drops it, which ends the process.
        let bun = Self {
            lines,
            waiting: waiting.clone(),
            next_id: AtomicU64::new(0),
            events: received,
            child: Mutex::new(child),
            stopped: AtomicBool::new(false),
        };
        std::thread::Builder::new()
            .name("bun".into())
            .spawn(move || read(stdout, &waiting, first, &events))
            .map_err(|error| error.to_string())?;
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
            .map_err(|error| error.to_string())?;
        let loaded = first_load
            .recv_timeout(FIRST_LOAD_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    format!("bun did not load extensions/ within {FIRST_LOAD_TIMEOUT:?}")
                }
                mpsc::RecvTimeoutError::Disconnected => "bun stopped before it loaded".to_string(),
            })?;
        Ok((bun, loaded))
    }

    /// What Bun says on its own after its first load, in order.
    pub(crate) fn events(&self) -> smol::channel::Receiver<Event> {
        self.events.clone()
    }

    /// The Hum of `tool` with these choices. Waits for Bun.
    pub(crate) fn sound(
        &self,
        tool: &str,
        choices: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<String>, String> {
        let lines = self.ask(|id| Request::Sound { id, tool, choices })?;
        serde_json::from_value(lines).map_err(|error| error.to_string())
    }

    /// The tree of the card of `tool`, or of its page, at its defaults. Waits for Bun.
    pub(crate) fn draw(&self, tool: &str, page: bool) -> Result<serde_json::Value, String> {
        self.ask(|id| Request::Draw { id, tool, page })
    }

    /// Asks Bun and waits for the answer, at most [`ANSWER_TIMEOUT`].
    fn ask<'a>(
        &self,
        request: impl FnOnce(u64) -> Request<'a>,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, wait) = mpsc::channel();
        {
            let mut waiting = self.waiting.lock().map_err(|_| STOPPED.to_string())?;
            let waiting = waiting.as_mut().ok_or_else(|| STOPPED.to_string())?;
            waiting.insert(id, answer);
        }
        self.send(&request(id));
        match wait.recv_timeout(ANSWER_TIMEOUT) {
            Ok(answer) => answer,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let message = format!("took longer than {ANSWER_TIMEOUT:?} to answer");
                self.stop(&message);
                Err(format!("the TypeScript host {message}"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(STOPPED.to_string()),
        }
    }

    /// Tells Bun something and does not wait.
    pub(crate) fn send(&self, request: &Request) {
        let line = match serde_json::to_string(request) {
            Ok(json) => json + "\n",
            Err(error) => return eprintln!("error: {error}"),
        };
        // A stopped host reads nothing: its stop was reported by the reader.
        if let Err(mpsc::TrySendError::Full(_)) = self.lines.try_send(line) {
            self.stop("stopped reading");
        }
    }

    /// Stops a Bun that hangs. The reader then hears it end, every question fails at once,
    /// and the window says the tools stopped.
    pub(crate) fn stop(&self, why: &str) {
        if self.stopped.swap(true, Ordering::Relaxed) {
            return;
        }
        eprintln!("error: the TypeScript host {why}, so it is stopped");
        if let Ok(mut child) = self.child.lock() {
            child.kill().ok();
        }
    }
}

/// Reads what Bun says until it stops: an answer goes to the question that waits for it, the
/// first load to `first`, and every other event to `events`.
fn read(
    stdout: ChildStdout,
    waiting: &Waiting,
    first: mpsc::Sender<Loaded>,
    events: &smol::channel::Sender<Event>,
) {
    let mut first = Some(first);
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("error: the TypeScript host stopped: {error}");
                break;
            }
        };
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Answer(Answer::Answer { id, value, error })) => {
                let answer = value.ok_or_else(|| error.unwrap_or_default());
                let waiter =
                    (waiting.lock().ok()).and_then(|mut waiting| waiting.as_mut()?.remove(&id));
                // The question timed out and nobody waits any more.
                if let Some(waiter) = waiter {
                    waiter.send(answer).ok();
                }
            }
            Ok(Line::Event(Event::Loaded(loaded))) if first.is_some() => {
                if let Some(first) = first.take() {
                    first.send(loaded).ok();
                }
            }
            // Outside the window nobody reads them, and Bun sends nothing unasked there but a
            // load after a save.
            Ok(Line::Event(event)) => {
                events.try_send(event).ok();
            }
            Err(error) => {
                eprintln!("error: the TypeScript host said something unknown: {error}");
            }
        }
    }
    // Every question still waiting, and every later one, hears that Bun is gone.
    if let Ok(mut waiting) = waiting.lock() {
        *waiting = None;
    }
}

impl Drop for Bun {
    fn drop(&mut self) {
        // Already ended, or never to be waited for: nothing to report on the way out.
        if let Ok(child) = self.child.get_mut() {
            child.kill().ok();
            child.wait().ok();
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
                "name": name, "file": format!("{name}.ts"), "title": name, "when": "w", "doc": "d",
                "kind": "effect", "tick": false, "page": false, "keys": false, "midi": false,
                "controls": {},
                "fields": { "rate": { "kind": "knob", "min": 0, "max": 1, "default": 0, "unit": unit } }
            })
        };
        let line = serde_json::json!({
            "type": "loaded",
            "tools": [tool("good", "hz"), tool("bad", "seconds")],
            "errors": [{ "file": "broken.ts", "message": "SyntaxError" }]
        });
        let Ok(Line::Event(Event::Loaded(loaded))) = serde_json::from_value(line) else {
            panic!("a load with one bad tool is no load");
        };
        let names: Vec<&str> = loaded.tools.iter().map(|info| info.name.as_str()).collect();
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
