//! The Bun process that runs `host.ts`, and the two ways to talk to it: a question that waits
//! for its answer, which a behaviour asks for the Hum of a tool, and messages that come when
//! they come, which the window reads. One JSON message per line, both ways.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::tools::ToolInfo;

/// How long a question may take before the behaviour that asked fails. Bun answers in well
/// under a millisecond; a tool whose `sound` hangs must not hang the window for good.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// What the runtime asks or tells.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Request<'a> {
    Sound {
        id: u64,
        tool: &'a str,
        choices: &'a serde_json::Map<String, serde_json::Value>,
    },
    Render {
        card: u64,
        tool: &'a str,
        state: serde_json::Value,
    },
    Event {
        card: u64,
        handler: usize,
    },
    Drop {
        card: u64,
    },
}

/// What Bun says without being asked, or as the answer to a render or a click.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event {
    /// The files of `extensions/` loaded, at the start and after every save.
    Loaded(Loaded),
    /// A card drew, or failed to.
    Tree {
        card: u64,
        #[serde(default)]
        tree: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
    /// A click changed the record of a card.
    Edit {
        card: u64,
        label: String,
        state: serde_json::Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Loaded {
    pub tools: Vec<ToolInfo>,
    /// Every tool that has a card in `extensions/`: the project's own and built-in ones.
    pub cards: BTreeSet<String>,
    pub errors: Vec<LoadError>,
}

/// What is wrong with a file of `extensions/`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoadError {
    pub file: String,
    pub message: String,
}

/// The answer to a question, see [`Bun::sound`].
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Answer {
    Sound {
        id: u64,
        #[serde(default)]
        code: Option<Vec<String>>,
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

type Waiting = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Vec<String>, String>>>>>;

pub(crate) struct Bun {
    stdin: Mutex<ChildStdin>,
    waiting: Waiting,
    next_id: AtomicU64,
    events: smol::channel::Receiver<Event>,
    /// See [`Self::first_load`].
    first_load: Mutex<mpsc::Receiver<Loaded>>,
    /// Ended when the runtime lets go of it, and the process with it.
    child: Mutex<Child>,
    /// What [`Bun::send`] and [`Bun::sound`] wrote, for a measurement.
    pub(crate) sent: Sent,
}

/// Counts of what the runtime sent Bun.
#[derive(Default)]
pub(crate) struct Sent {
    pub sounds: AtomicU64,
    pub renders: AtomicU64,
}

impl Bun {
    pub(crate) fn start(bun: &Path, host_script: &Path, folder: &Path) -> std::io::Result<Self> {
        // Before the project opens and before any window: its tools are registered from what
        // Bun loads first, so there is nothing to draw while it starts.
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(bun)
            .arg(host_script)
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(std::io::Error::other("bun started without pipes"));
        };
        let waiting = Waiting::default();
        let (events, received) = smol::channel::unbounded();
        let (first, first_load) = mpsc::channel();
        let mut first = Some(first);
        let reader_waiting = waiting.clone();
        std::thread::Builder::new()
            .name("bun".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let line = match line {
                        Ok(line) => line,
                        Err(error) => {
                            eprintln!("error: the TypeScript host stopped: {error}");
                            break;
                        }
                    };
                    match serde_json::from_str::<Line>(&line) {
                        Ok(Line::Answer(Answer::Sound { id, code, error })) => {
                            let answer = code.ok_or_else(|| error.unwrap_or_default());
                            let waiter = reader_waiting.lock().ok().and_then(|mut w| w.remove(&id));
                            // The question timed out and nobody waits any more.
                            if let Some(waiter) = waiter {
                                waiter.send(answer).ok();
                            }
                        }
                        // The first load goes to whoever opens the project; the rest to the window.
                        Ok(Line::Event(Event::Loaded(loaded))) if first.is_some() => {
                            if let Some(first) = first.take() {
                                first.send(loaded).ok();
                            }
                        }
                        // Nobody reads them outside the window; they are dropped then.
                        Ok(Line::Event(event)) => {
                            events.try_send(event).ok();
                        }
                        Err(error) => {
                            eprintln!("error: the TypeScript host said something unknown: {error}");
                        }
                    }
                }
                // Every question still waiting hears that Bun is gone.
                if let Ok(mut waiting) = reader_waiting.lock() {
                    waiting.clear();
                }
            })?;
        Ok(Self {
            stdin: Mutex::new(stdin),
            waiting,
            next_id: AtomicU64::new(0),
            events: received,
            first_load: Mutex::new(first_load),
            child: Mutex::new(child),
            sent: Sent::default(),
        })
    }

    /// What Bun says on its own, in order. Every clone gets every message once, between them.
    pub(crate) fn events(&self) -> smol::channel::Receiver<Event> {
        self.events.clone()
    }

    /// The first [`Event::Loaded`], waiting at most `timeout`.
    pub(crate) fn first_load(&self, timeout: Duration) -> Result<Loaded, String> {
        let first_load = self
            .first_load
            .lock()
            .map_err(|_| "bun broke".to_string())?;
        first_load
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    format!("bun did not load extensions/ within {timeout:?}")
                }
                mpsc::RecvTimeoutError::Disconnected => "bun stopped before it loaded".to_string(),
            })
    }

    /// The Hum of `tool` with these choices. Waits for Bun.
    pub(crate) fn sound(
        &self,
        tool: &str,
        choices: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Vec<String>, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (answer, wait) = mpsc::channel();
        self.waiting
            .lock()
            .map_err(|_| "the TypeScript host broke".to_string())?
            .insert(id, answer);
        self.sent.sounds.fetch_add(1, Ordering::Relaxed);
        self.send(&Request::Sound { id, tool, choices });
        match wait.recv_timeout(ANSWER_TIMEOUT) {
            Ok(answer) => answer,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(mut waiting) = self.waiting.lock() {
                    waiting.remove(&id);
                }
                Err(format!(
                    "the sound of {tool} took longer than {ANSWER_TIMEOUT:?}"
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("the TypeScript host is not running".to_string())
            }
        }
    }

    /// Tells Bun something and does not wait.
    pub(crate) fn send(&self, request: &Request) {
        if matches!(request, Request::Render { .. }) {
            self.sent.renders.fetch_add(1, Ordering::Relaxed);
        }
        let line = match serde_json::to_string(request) {
            Ok(json) => json + "\n",
            Err(error) => return eprintln!("error: {error}"),
        };
        // A pipe of a stopped host fails. Its stop was reported by the reader.
        let Ok(mut stdin) = self.stdin.lock() else {
            return;
        };
        if let Err(error) = stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
        {
            eprintln!("error: could not reach the TypeScript host: {error}");
        }
    }
}

impl Drop for Bun {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            // Already ended, or never to be waited for: nothing to report on the way out.
            child.kill().ok();
            child.wait().ok();
        }
    }
}
