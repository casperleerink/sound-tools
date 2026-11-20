//! A file of the composer brought into the project, dropped or chosen: a sound is copied into
//! `assets/audio/`, and a MIDI file becomes the records of a `timeline` for each of its parts.
//! Both on a thread of their own; the caller polls for the result.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use serde_json::Value;
use sound_core::Assets;

use super::Waker;

/// What a file brought into the project became.
#[derive(Debug, PartialEq)]
pub enum Brought {
    /// A sound, copied into `assets/audio/` under this name, for a `file` field.
    Sound(String),
    /// The records of a `timeline` for each part of a MIDI file, in order.
    Parts(Vec<Value>),
}

/// A file on its way into the project.
pub struct Importing(Receiver<Result<Brought, String>>);

impl Importing {
    /// What it became, once it is done, or why it was not brought in, in the composer's words.
    /// A sound that does not play is not copied.
    pub fn poll(&self) -> Option<Result<Brought, String>> {
        self.0.try_recv().ok()
    }
}

/// Brings the file at `path` into the project of `assets` on a thread of its own, which wakes
/// the caller when it is done.
pub fn import(assets: &Assets, path: PathBuf, waker: Waker) -> Importing {
    let (send, done) = mpsc::channel();
    let assets = assets.clone();
    let started = std::thread::Builder::new().name("import".into()).spawn({
        let send = send.clone();
        move || {
            send.send(bring(&assets, &path)).ok();
            waker();
        }
    });
    if let Err(error) = started {
        send.send(Err(format!("The file was not brought in: {error}")))
            .ok();
    }
    Importing(done)
}

fn bring(assets: &Assets, path: &Path) -> Result<Brought, String> {
    if let Some(parts) = timelines_in(path) {
        return parts.map(Brought::Parts);
    }
    let imported = sound_media::import(assets, path).map_err(|error| error.to_string())?;
    Ok(Brought::Sound(imported.asset.to_string()))
}

/// The timelines of the parts of the MIDI file at `path`, or what went wrong: `None` when it is
/// not a MIDI file.
fn timelines_in(path: &Path) -> Option<Result<Vec<Value>, String>> {
    let mut start = [0; 4];
    let mut file = std::fs::File::open(path).ok()?;
    std::io::Read::read_exact(&mut file, &mut start).ok()?;
    if !sound_notes::midi_file::is_midi_file(&start) {
        return None;
    }
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_default();
    let not_imported = |error: String| format!("{name} was not imported. {error}");
    let read = std::fs::read(path).map_err(|error| not_imported(error.to_string()));
    let parts = read.and_then(|bytes| {
        sound_notes::midi_file::read(&bytes, &name).map_err(|error| not_imported(error.to_string()))
    });
    Some(parts.and_then(|parts| {
        (parts.iter())
            .map(|(title, clip)| sound_typescript::timeline_state(clip, title))
            .collect()
    }))
}
