//! The threads of each project on this machine, so the sidebar shows the last one again when
//! the window opens, and its agent resumes it.
//!
//! Kept in the support folder of the machine, never in the project: the log is interface
//! state of this machine, it would be noise in git, and the agent would read its own chat.
//! Each project has one folder, `agent/threads/<project key>/`:
//!
//! - `index.json` lists the threads ([`SavedThread`]), the one used last at the end.
//! - `<thread id>.jsonl` is what the sidebar showed: one [`Line`] per line, the composer's
//!   messages and the [`AgentEvent`]s with the time each came. Replayed through
//!   [`Conversation::apply`] it gives the same conversation back, so the display needs no
//!   provider at all.
//!
//! The conversation itself is the provider's to keep: a thread resumes with its session id.
//! A project is open in one window at a time, so nothing else writes these files meanwhile.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::conversation::{Conversation, request_label};
use crate::{AgentEvent, Provider, Session, TurnOutcome};

const INDEX: &str = "index.json";

/// One thread in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedThread {
    /// Also the name of its log.
    pub id: String,
    pub provider: Provider,
    /// The session [`AgentEvent::Started`] gave, which the next process resumes. `None`
    /// until the agent has started.
    pub session_id: Option<String>,
    /// The first message, cut short.
    pub title: String,
    /// When the composer last sent to it.
    pub updated: SystemTime,
}

impl SavedThread {
    /// The thread that `message` starts.
    pub fn new(provider: Provider, message: &str, now: SystemTime) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            provider,
            session_id: None,
            title: request_label(message),
            updated: now,
        }
    }

    /// What the next process of the thread continues.
    pub fn session(&self) -> Session {
        self.session_id
            .clone()
            .map_or(Session::New, Session::Resume)
    }
}

/// One line of a thread's log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    /// The composer's message.
    Sent {
        at: SystemTime,
        message: String,
    },
    Event {
        at: SystemTime,
        event: AgentEvent,
    },
}

impl Line {
    /// The line of `event`, or `None` when a replay does not need it: a text delta, because
    /// [`AgentEvent::TextDone`] brings the whole text, and the start, whose session id goes
    /// to the index.
    pub fn of(event: &AgentEvent, at: SystemTime) -> Option<Line> {
        let needed = !matches!(
            event,
            AgentEvent::TextDelta { .. } | AgentEvent::Started { .. }
        );
        needed.then(|| Line::Event {
            at,
            event: event.clone(),
        })
    }
}

/// What the sidebar keeps, in the order it happened.
#[derive(Debug)]
pub enum Write {
    /// Adds the thread to the index, or updates it there, as the one used last.
    Thread(SavedThread),
    /// Adds to the log of the thread with this id.
    Lines { thread: String, lines: Vec<Line> },
}

/// The threads of one project. Every method reads or writes files: call them on the
/// background executor.
#[derive(Clone, Debug)]
pub struct ThreadStore {
    folder: PathBuf,
}

impl ThreadStore {
    /// The threads of the project in the folder `project`, kept in `threads`, which is
    /// `agent/threads` in the support folder.
    pub fn new(threads: &Path, project: &Path) -> Self {
        Self {
            folder: threads.join(key(project)),
        }
    }

    /// The thread used last, and what it showed. A line that does not read is left out, with
    /// a notice in its place.
    pub fn last(&self) -> Result<Option<(SavedThread, Conversation)>, String> {
        let Some(thread) = self.index()?.pop() else {
            return Ok(None);
        };
        let path = self.log(&thread.id);
        let text = match fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        Ok(Some((thread, replay(&text))))
    }

    pub fn write(&self, write: &Write) -> Result<(), String> {
        fs::create_dir_all(&self.folder)
            .map_err(|error| format!("{} could not be made: {error}", self.folder.display()))?;
        match write {
            Write::Thread(thread) => self.save(thread),
            Write::Lines { thread, lines } => self.append(thread, lines),
        }
    }

    fn save(&self, thread: &SavedThread) -> Result<(), String> {
        // An index that does not read is written over, or no thread would ever be saved
        // again. `last` has said so already.
        let mut threads = self.index().unwrap_or_default();
        threads.retain(|saved| saved.id != thread.id);
        threads.push(thread.clone());
        let path = self.folder.join(INDEX);
        let failed = |error: String| format!("{} was not written: {error}", path.display());
        let mut text =
            serde_json::to_string_pretty(&threads).map_err(|error| failed(error.to_string()))?;
        text.push('\n');
        // Through a file of its own and a rename, so a crash never leaves half an index.
        let temporary = self.folder.join(format!("{INDEX}.tmp"));
        fs::write(&temporary, text)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| failed(error.to_string()))
    }

    fn append(&self, thread: &str, lines: &[Line]) -> Result<(), String> {
        let path = self.log(thread);
        let failed = |error: String| format!("{} was not written: {error}", path.display());
        let mut text = String::new();
        for line in lines {
            text.push_str(&serde_json::to_string(line).map_err(|error| failed(error.to_string()))?);
            text.push('\n');
        }
        // Whole lines in one write, so a crash damages at most the last line, which a replay
        // leaves out.
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .map_err(|error| failed(error.to_string()))
    }

    fn index(&self) -> Result<Vec<SavedThread>, String> {
        let path = self.folder.join(INDEX);
        let failed = |error: String| format!("{} could not be read: {error}", path.display());
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| failed(error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(failed(error.to_string())),
        }
    }

    fn log(&self, thread: &str) -> PathBuf {
        self.folder.join(format!("{thread}.jsonl"))
    }
}

/// The folder name of a project: the path of its folder, the same however it was opened,
/// with `%` and `/` escaped, so it is one name and two projects never share one.
fn key(project: &Path) -> String {
    let path = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    path.to_string_lossy()
        .replace('%', "%25")
        .replace('/', "%2F")
}

/// The conversation a log gives. A turn still open is one the app quit during: it ends as
/// stopped, at the time of the last line.
fn replay(text: &str) -> Conversation {
    let mut conversation = Conversation::default();
    let mut last = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Line>(line) {
            Ok(Line::Sent { at, message }) => {
                conversation.send(&message, at);
                last = Some(at);
            }
            Ok(Line::Event { at, event }) => {
                conversation.apply(event, at);
                last = Some(at);
            }
            Err(_) => conversation.notice("Part of this thread could not be read."),
        }
    }
    if let Some(at) = last.filter(|_| conversation.is_working()) {
        let stopped = AgentEvent::TurnEnded {
            outcome: TurnOutcome::Interrupted,
        };
        conversation.apply(stopped, at);
    }
    conversation
}

#[cfg(test)]
mod tests;
