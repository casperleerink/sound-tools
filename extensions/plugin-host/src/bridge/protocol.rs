//! What the host and a plugin's process say on the control socket: one request, then its
//! answer. Each is a frame, JSON or raw bytes, with its length in front, and the length is
//! checked before anything is allocated for it.

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};

use serde::{Deserialize, Serialize};

use crate::backend::{Hand, ParameterChange, Requests};
use crate::parameters::Parameter;
use crate::scan::ScannedPlugin;
use crate::window::Placement;
use crate::{Pin, PluginProblem};

/// The most a message may be. Nothing the host asks or a plugin answers comes near it.
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// The most a plugin's state may be: VST 3's limit for each of its two parts
/// (`vst3::MAX_STATE`) and their header. A CLAP state above it is not saved, see `child.rs`.
pub(crate) const MAX_STATE: usize = 2 * crate::vst3::MAX_STATE + 64;

const MESSAGE: u8 = b'm';
const BYTES: u8 = b'b';

/// One request, with the values sent to the plugin since the last one, which the child gives
/// the plugin first, in order.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Message {
    pub sends: Vec<ParameterChange>,
    pub request: Request,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Request {
    /// Followed by the saved state as bytes, when `state` says there is one.
    Load {
        found: ScannedPlugin,
        sample_rate: u32,
        offline: bool,
        pins: BTreeMap<u32, Pin>,
        state: bool,
    },
    Poll,
    ReadPins(Vec<u32>),
    Value(u32),
    Text {
        id: u32,
        value: f64,
    },
    Parameters,
    Show(ParameterChange),
    SaveState,
    Restart {
        sample_rate: u32,
        offline: bool,
        pins: Vec<ParameterChange>,
    },
    Released,
    Quit,
    /// The plugin's own window, in this process's own window, where `placement` says.
    OpenWindow {
        title: String,
        placement: Option<Placement>,
        focus: bool,
    },
    CloseWindow,
}

impl Request {
    /// Whether the plugin may keep its main thread long for it.
    pub(crate) fn is_loading(&self) -> bool {
        matches!(
            self,
            Self::Load { .. } | Self::Restart { .. } | Self::SaveState
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Answer {
    Loaded {
        latency: u32,
        notes: Vec<PluginProblem>,
        /// Whether the plugin has a window this process can open.
        has_window: bool,
    },
    DidNotLoad(PluginProblem),
    /// What the plugin asked for, and the keys its window did not use.
    Polled(Requests, Vec<SpareKey>),
    Pins {
        values: Option<Vec<(u32, f64)>>,
        hand: Hand,
    },
    Value(Option<f64>),
    Text(Option<String>),
    Parameters(Vec<Parameter>),
    Shown(bool),
    /// Followed by the state as bytes when it is `Ok`.
    State(Result<(), String>),
    Restarted(Option<Result<u32, PluginProblem>>),
    Released(bool),
    Quit,
    /// A request for a plugin this process does not have.
    NotLoaded,
    /// Where the window opened, or why it did not.
    WindowOpened(Result<Option<Placement>, PluginProblem>),
    WindowClosed,
}

/// A key the plugin's window did not use, for the computer keys of the host, as a keystroke's
/// text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum SpareKey {
    Down { keystroke: String, held: bool },
    Up { keystroke: String },
    LetGo,
}

/// Why a frame could not be read.
#[derive(Debug)]
pub(crate) enum Broken {
    /// The other side closed, or wrote what is no frame of ours: the plugin's process is gone or
    /// not to be trusted.
    Gone,
    /// `check` said to stop waiting.
    GaveUp,
}

pub(crate) fn write_message(
    stream: &mut impl Write,
    value: &impl Serialize,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    write_frame(stream, MESSAGE, &bytes)
}

pub(crate) fn write_bytes(stream: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    write_frame(stream, BYTES, bytes)
}

fn write_frame(stream: &mut impl Write, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let length = u32::try_from(payload.len()).map_err(std::io::Error::other)?;
    let mut header = [0; 5];
    header[0] = kind;
    header[1..].copy_from_slice(&length.to_le_bytes());
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

/// Reads a message. `check` runs whenever a read waited for its timeout, and stops the wait
/// with an error.
pub(crate) fn read_message<T: for<'a> Deserialize<'a>>(
    stream: &mut impl Read,
    check: &mut impl FnMut() -> Result<(), Broken>,
) -> Result<T, Broken> {
    let bytes = read_frame(stream, MESSAGE, MAX_MESSAGE, check)?;
    serde_json::from_slice(&bytes).map_err(|_| Broken::Gone)
}

pub(crate) fn read_bytes(
    stream: &mut impl Read,
    check: &mut impl FnMut() -> Result<(), Broken>,
) -> Result<Vec<u8>, Broken> {
    read_frame(stream, BYTES, MAX_STATE, check)
}

fn read_frame(
    stream: &mut impl Read,
    kind: u8,
    most: usize,
    check: &mut impl FnMut() -> Result<(), Broken>,
) -> Result<Vec<u8>, Broken> {
    let mut header = [0; 5];
    read_exactly(stream, &mut header, check)?;
    let mut length = [0; 4];
    length.copy_from_slice(&header[1..]);
    let length = u32::from_le_bytes(length) as usize;
    if header[0] != kind || length > most {
        return Err(Broken::Gone);
    }
    let mut payload = vec![0; length];
    read_exactly(stream, &mut payload, check)?;
    Ok(payload)
}

fn read_exactly(
    stream: &mut impl Read,
    mut buffer: &mut [u8],
    check: &mut impl FnMut() -> Result<(), Broken>,
) -> Result<(), Broken> {
    while !buffer.is_empty() {
        match stream.read(buffer) {
            Ok(0) => return Err(Broken::Gone),
            Ok(read) => buffer = &mut buffer[read..],
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                check()?;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(Broken::Gone),
        }
    }
    Ok(())
}
