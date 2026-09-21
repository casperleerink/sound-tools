//! MIDI input: a keyboard plays the instrument of the selected track, and a recording of what
//! was played becomes a clip.
//!
//! This extension has no tool and no record. It is a processor in the engine, a device layer
//! over `midir`, and a recorder. It meets the arrangement only through the note contract
//! ([`sound_notes`]): the window says which port the notes go to and makes the clip.
//!
//! ```no_run
//! # fn main() -> Result<(), sound_core::GraphError> {
//! # let (mut engine, _) = sound_core::Engine::new(sound_core::EngineConfig::new(48_000, 2));
//! # let notes: Option<sound_core::InputEndpoint> = None;
//! let mut keyboard = midi::Keyboard::attach(&mut engine)?;
//! keyboard.play_into(&mut engine, notes)?;     // the `notes` port of an instrument
//! let mut ports = midi::Ports::new(keyboard.input());
//! # Ok(())
//! # }
//! ```
//!
//! `README.md` in this crate is the guide.

mod keyboard;
mod keys;
mod ports;
mod take;

use sound_core::AgentDoc;

pub use keyboard::{Keyboard, Latency, Lost};
pub use keys::{Arrived, INPUT_CAPACITY, Input, Keys, Played, REPORT_CAPACITY, Sounded};
pub use ports::{PortError, Ports};
pub use take::{RawEvent, RawTake, TAKES_FOLDER, Take, TakeEvent, take_asset};

/// What an agent needs to know about a raw take. Every project gets it, because every project
/// can be recorded into: the runtime registers it with `runtime_agent_doc`.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "takes",
    when: "You see a file under assets/takes/, or you want the performance a clip came from",
    markdown: include_str!("../agent-doc.md"),
};
