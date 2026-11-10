//! A plugin in a process of its own, so a plugin that crashes or hangs costs its own slot and
//! not the app.
//!
//! One process per plugin a record loads: the same program, started with [`PLAY_ARGUMENT`]. It
//! loads the plugin with the backend of its format (`child.rs`). On this side the plugin is a
//! [`crate::backend::LoadedPlugin`] and a [`crate::processor::PluginAudio`] like any other
//! (`parent.rs`), so the table in `host.rs` knows no difference but the crash.
//!
//! Two sockets: control (one request and its answer at a time, `protocol.rs`) and audio (one
//! byte each way per sub-block, the block itself in shared memory, `shared.rs`). A dead child
//! closes its ends of both, so either side notices at once.

mod child;
mod parent;
mod protocol;
mod shared;

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub use child::serve;
pub(crate) use parent::open;

use crate::Isolation;

/// The argument the host starts a plugin's process with.
pub const PLAY_ARGUMENT: &str = "--play-plugin";

/// What a child says first on each of its sockets.
#[derive(Debug, Serialize, Deserialize)]
struct Hello {
    token: String,
    channel: Channel,
    /// Whether the child is the build that started it, see [`Identity`].
    same_build: bool,
}

#[derive(Debug, Serialize, Deserialize)]
enum Channel {
    Control,
    Audio,
}

/// Which file a program is, as the file system tells: an update or a rebuild that replaces it
/// makes another. A child of another build than its host would read the shared memory with
/// another layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    modified: i64,
    modified_nanoseconds: i64,
}

impl Identity {
    fn of(program: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(program).ok()?;
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            modified: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }

    fn of_this_program() -> Option<Self> {
        Self::of(&std::env::current_exe().ok()?)
    }

    fn text(self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.device, self.inode, self.modified, self.modified_nanoseconds
        )
    }

    /// The identity `program` had when this process first looked at it, which for this
    /// program itself is when it started ([`remember_this_program`]).
    fn remembered(program: &Path) -> Option<Self> {
        static SEEN: Mutex<BTreeMap<PathBuf, Identity>> = Mutex::new(BTreeMap::new());
        let mut seen = SEEN.lock().ok()?;
        if let Some(identity) = seen.get(program) {
            return Some(*identity);
        }
        let identity = Self::of(program)?;
        seen.insert(program.to_path_buf(), identity);
        Some(identity)
    }
}

/// Notes which file this program is, as early as the process can: a plugin's process started
/// later must be this same file.
pub fn remember_this_program() {
    if let Ok(program) = std::env::current_exe() {
        let _seen = Identity::remembered(&program);
    }
}
