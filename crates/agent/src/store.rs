//! The threads of each project on this machine, so the sidebar shows the current one again
//! when the window opens, and its agent resumes it.
//!
//! Kept in the support folder of the machine, never in the project: the log is interface
//! state of this machine, it would be noise in git, and the agent would read its own chat.
//! Each project has one folder, `agent/threads/<project key>/`:
//!
//! - `index.json` lists the threads ([`SavedThread`]), the one a message last went to last,
//!   and names the current one, if any.
//! - `<thread id>.jsonl` is what the sidebar showed: one [`Line`] per line, the composer's
//!   messages and the [`AgentEvent`]s with the time each came. Replayed through
//!   [`Conversation::apply`] it gives the same conversation back, so the display needs no
//!   provider at all.
//!
//! The conversation itself is the provider's to keep: a thread resumes with its session id.
//! A project is open in one window at a time, so nothing else writes these files meanwhile.

use std::fs;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::conversation::{Conversation, request_label};
use crate::{AgentEvent, TurnOutcome};

const INDEX: &str = "index.json";

/// Where an index that does not read is put aside, so nothing in it is written over.
const BAD_INDEX: &str = "index.json.bad";

/// One thread in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedThread {
    /// Also the name of its log.
    pub id: String,
    /// The session the agent works in, from [`crate::Thread::session_id`]. `None` until an
    /// agent started for the thread.
    pub session_id: Option<String>,
}

impl SavedThread {
    /// A thread no message has gone to yet.
    pub(crate) fn fresh() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: None,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// The id of the thread the sidebar shows. `None` after **+**: the next message starts
    /// a thread.
    current: Option<String>,
    threads: Vec<SavedThread>,
}

/// A thread that is not the current one, for **Open recent**.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentThread {
    pub id: String,
    /// Its first message, on one line and cut short.
    pub title: String,
}

/// One line of a thread's log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Line {
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
    /// [`AgentEvent::TextDone`] brings the whole text, and the start, which shows nothing.
    pub(crate) fn of(event: &AgentEvent, at: SystemTime) -> Option<Line> {
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
pub(crate) enum Write {
    /// The thread the sidebar shows, added to the index or updated there, or none after **+**.
    Current(Option<SavedThread>),
    /// Adds to the log of the thread with this id.
    Lines { thread: String, lines: Vec<Line> },
}

/// The threads of one project. Every method reads or writes files: call them on the
/// background executor.
#[derive(Clone, Debug)]
pub(crate) struct ThreadStore {
    folder: PathBuf,
}

impl ThreadStore {
    /// The threads of the project in the folder `project`, kept in `threads`, which is
    /// `agent/threads` in the support folder.
    pub(crate) fn new(threads: &Path, project: &Path) -> Self {
        Self {
            folder: threads.join(key(project)),
        }
    }

    /// The current thread, and what it showed. A line that does not read is left out, with a
    /// notice in its place.
    pub(crate) fn current(&self) -> Result<Option<(SavedThread, Conversation)>, String> {
        match self.index()?.current {
            Some(current) => self.thread(&current),
            None => Ok(None),
        }
    }

    /// The thread with this id, and what it showed, or `None` when the index has no such
    /// thread.
    pub(crate) fn thread(&self, id: &str) -> Result<Option<(SavedThread, Conversation)>, String> {
        let mut index = self.index()?;
        let Some(position) = index.threads.iter().position(|saved| saved.id == id) else {
            return Ok(None);
        };
        let thread = index.threads.swap_remove(position);
        let text = self.read_log(&thread.id)?;
        Ok(Some((thread, replay(&text))))
    }

    /// The threads other than the current one, the one a message last went to first, at most
    /// `limit`. A thread whose log does not read, or holds no message, is left out.
    pub(crate) fn recent(&self, limit: usize) -> Result<Vec<RecentThread>, String> {
        let index = self.index()?;
        let others = (index.threads.iter().rev())
            .filter(|thread| index.current.as_ref() != Some(&thread.id));
        let recent = others
            .filter_map(|thread| {
                let text = self.read_log(&thread.id).ok()?;
                let message = text
                    .lines()
                    .find_map(|line| match serde_json::from_str(line) {
                        Ok(Line::Sent { message, .. }) => Some(message),
                        _ => None,
                    })?;
                Some(RecentThread {
                    id: thread.id.clone(),
                    title: request_label(&message),
                })
            })
            .take(limit)
            .collect();
        Ok(recent)
    }

    /// The text of a thread's log, empty when it has none yet.
    fn read_log(&self, thread: &str) -> Result<String, String> {
        let path = self.log(thread);
        match fs::read(&path) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(format!("{} could not be read: {error}", path.display())),
        }
    }

    pub(crate) fn write(&self, write: &Write) -> Result<(), String> {
        fs::create_dir_all(&self.folder)
            .map_err(|error| format!("{} could not be made: {error}", self.folder.display()))?;
        match write {
            Write::Current(thread) => self.save(thread.as_ref()),
            Write::Lines { thread, lines } => self.append(thread, lines),
        }
    }

    fn save(&self, current: Option<&SavedThread>) -> Result<(), String> {
        let mut index = self.index()?;
        index.current = current.map(|thread| thread.id.clone());
        // Last in the list, so the threads are in the order a message last went to them.
        if let Some(thread) = current {
            index.threads.retain(|saved| saved.id != thread.id);
            index.threads.push(thread.clone());
        }
        let path = self.folder.join(INDEX);
        let failed = |error: String| format!("{} was not written: {error}", path.display());
        let mut text =
            serde_json::to_string_pretty(&index).map_err(|error| failed(error.to_string()))?;
        text.push('\n');
        write_whole(&path, &text).map_err(|error| failed(error.to_string()))
    }

    fn append(&self, thread: &str, lines: &[Line]) -> Result<(), String> {
        let path = self.log(thread);
        let failed = |error: String| format!("{} was not written: {error}", path.display());
        let mut text = String::new();
        for line in lines {
            text.push_str(&serde_json::to_string(line).map_err(|error| failed(error.to_string()))?);
            text.push('\n');
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|error| failed(error.to_string()))?;
        // A write a crash cut off has no end of line: end it, or the next line would join it
        // and be lost with it.
        let length = file
            .metadata()
            .map_err(|error| failed(error.to_string()))?
            .len();
        if let Some(last) = length.checked_sub(1) {
            let mut byte = [0];
            // Appending writes at the end whatever the position, so the seek moves only the
            // read.
            (&file)
                .seek(SeekFrom::Start(last))
                .and_then(|_| (&file).read_exact(&mut byte))
                .map_err(|error| failed(error.to_string()))?;
            if byte != *b"\n" {
                text.insert(0, '\n');
            }
        }
        // Whole lines in one write, so a crash damages at most the last line, which a replay
        // leaves out.
        (&file)
            .write_all(text.as_bytes())
            .map_err(|error| failed(error.to_string()))
    }

    /// The index, or an empty one when there is none yet. One that does not read is put
    /// aside as `index.json.bad`, never written over, and the error says so once; the next
    /// read starts a new index.
    fn index(&self) -> Result<Index, String> {
        let path = self.folder.join(INDEX);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Index::default()),
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        serde_json::from_str(&text).or_else(|error| {
            let bad = self.folder.join(BAD_INDEX);
            fs::rename(&path, &bad).map_err(|rename| {
                format!(
                    "{} could not be read ({error}) or put aside: {rename}",
                    path.display()
                )
            })?;
            Err(format!(
                "{} could not be read, and is kept as {}: {error}",
                path.display(),
                bad.display()
            ))
        })
    }

    fn log(&self, thread: &str) -> PathBuf {
        self.folder.join(format!("{thread}.jsonl"))
    }
}

/// Writes `text` as the whole of the file at `path`: through a file of its own and a rename,
/// so a crash never leaves half a file. The other file's name is new each time, so no other
/// write can be halfway through it.
pub(crate) fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

/// The folder name of a project: a name-based UUID of the path of its folder, the same
/// however it was opened. Any path gives a name of the same short length.
fn key(project: &Path) -> String {
    let path = dunce::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    Uuid::new_v5(&Uuid::NAMESPACE_URL, path.as_os_str().as_encoded_bytes()).to_string()
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
