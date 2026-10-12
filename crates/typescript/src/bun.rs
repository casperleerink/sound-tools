//! The Bun process that runs `host.ts`. It loads the files of `extensions/` once and says what
//! it loaded, then hears what the window tells it and says what the tools do. A change of the
//! code starts a new Bun. One JSON message per line, both ways.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use sound_core::InstanceId;
use sound_signals::Graph;

use crate::Midi;
use crate::tools::ToolInfo;

/// How long one step of a load, the start of Bun or a file, may take before Bun counts as
/// hung. Bun starts in a few tens of milliseconds and loads a file in less.
const LOAD_TIMEOUT: Duration = Duration::from_secs(5);

/// The lines that may wait for Bun to read them. More, and Bun has stopped reading.
const QUEUED_LINES: usize = 1024;

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
    },
    /// A click, or a press or a drag on a canvas `at` across and down, 0 to 1.
    Event {
        card: u64,
        /// The version of the tree that was clicked.
        version: u64,
        handler: usize,
        at: Option<(f32, f32)>,
    },
    /// One step of the control loop of instances of one tool.
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
    /// What the MIDI keyboard played, for the instances of one tool that hear it.
    Midi {
        time: f64,
        instances: Vec<Looped<'a>>,
        messages: &'a [Midi],
    },
    Drop {
        card: u64,
    },
}

/// The room a page has, in whole points.
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) struct PageSize {
    pub width: u32,
    pub height: u32,
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

/// What Bun says after its load.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// It finished the oldest request it had not finished.
    Done,
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
    #[serde(untagged)]
    Event(Event),
}

/// Why Bun did not load, and the file it was loading then, which is to blame.
pub(crate) struct LoadFailure {
    pub file: Option<String>,
    pub why: String,
}

pub(crate) struct Bun {
    /// Lines on their way to Bun, which a thread of their own writes: a Bun that stops reading
    /// does not block the window.
    lines: mpsc::SyncSender<String>,
    events: smol::channel::Receiver<Event>,
    /// Ended when the runtime lets go of it.
    child: Child,
}

impl Bun {
    /// Starts Bun on `host_script` in `folder`, to load `files` of it, and waits for its load.
    /// Blocks for as long as the load takes: never call it on the thread that draws.
    pub(crate) fn start(
        program: &Path,
        host_script: &Path,
        folder: &Path,
        files: &[&String],
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
        let (steps, load) = mpsc::channel();
        let (lines, to_write) = mpsc::sync_channel::<String>(QUEUED_LINES);
        // From here a failure drops it, which ends the process.
        let bun = Self {
            lines,
            events: received,
            child,
        };
        std::thread::Builder::new()
            .name("bun".into())
            .spawn(move || read(stdout, steps, &events))
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
        loop {
            let why = match load.recv_timeout(LOAD_TIMEOUT) {
                Ok(Line::Loading { file: next }) => {
                    file = Some(next);
                    continue;
                }
                Ok(Line::Loaded(loaded)) => return Ok((bun, loaded)),
                Ok(Line::Event(_)) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    format!("did not finish loading within {LOAD_TIMEOUT:?}")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => "stopped Bun".to_string(),
            };
            return Err(LoadFailure { file, why });
        }
    }

    /// What Bun says after its load, in order. It ends when Bun does.
    pub(crate) fn events(&self) -> smol::channel::Receiver<Event> {
        self.events.clone()
    }

    /// Tells Bun something and does not wait. Bun says [`Event::Done`] when it is done.
    pub(crate) fn send(&self, request: &Request) {
        let line = match serde_json::to_string(request) {
            Ok(json) => json + "\n",
            Err(error) => return eprintln!("error: {error}"),
        };
        // A Bun that stopped reading never says done, so the window finds it hung; one that
        // stopped says so with the end of its events.
        self.lines.try_send(line).ok();
    }
}

/// Reads what Bun says until it stops: the steps of its load to `steps`, which nobody hears
/// after it, and every event to `events`.
fn read(stdout: ChildStdout, steps: mpsc::Sender<Line>, events: &smol::channel::Sender<Event>) {
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("error: the TypeScript host stopped: {error}");
                break;
            }
        };
        match serde_json::from_str::<Line>(&line) {
            // Outside the window nobody reads them, and Bun says nothing unasked.
            Ok(Line::Event(event)) => {
                events.try_send(event).ok();
            }
            Ok(step) => {
                steps.send(step).ok();
            }
            Err(error) => eprintln!("error: the TypeScript host said something unknown: {error}"),
        }
    }
}

impl Drop for Bun {
    fn drop(&mut self) {
        // Already ended, or never to be waited for: nothing to report on the way out.
        self.child.kill().ok();
        self.child.wait().ok();
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
                    "doc": "d", "kind": "effect", "tick": false, "page": false, "keys": false,
                    "midi": false, "controls": {},
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
