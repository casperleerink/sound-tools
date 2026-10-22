//! MIDI input: a keyboard plays what is selected. The computer keys can be that keyboard too
//! ([`ComputerKeys`]).
//!
//! This extension has no tool and no record. It is a processor in the engine and a device layer
//! over `midir`. It meets the tools only through the note contract ([`sound_notes`]): the window
//! says which ports the notes go to.
//!
//! ```no_run
//! # fn main() -> Result<(), sound_core::GraphError> {
//! # let (mut engine, _) = sound_core::Engine::new(sound_core::EngineConfig::new(48_000, 2));
//! # let notes: Vec<sound_core::InputEndpoint> = Vec::new();
//! let mut keyboard = midi::Keyboard::attach(&mut engine)?;
//! keyboard.play_into(&mut engine, &notes)?;    // `notes` ports, such as an instrument's
//! let mut ports = midi::Ports::new(keyboard.input());
//! # Ok(())
//! # }
//! ```

mod computer_keys;
mod keyboard;
mod keys;
mod ports;

use sound_core::AgentDoc;

pub use computer_keys::ComputerKeys;
pub use keyboard::{Keyboard, Latency, Lost};
pub use keys::{INPUT_CAPACITY, Input, Played};
pub use ports::{PortError, Ports};

/// What an agent needs to know about a raw take. Every project gets it, because every project
/// can be recorded into: the runtime registers it with `runtime_agent_doc`.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "takes",
    when: "A file under `assets/takes/`, or the raw performance of a recorded clip",
    markdown: include_str!("../agent-doc.md"),
};
