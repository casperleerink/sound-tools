//! The device layer: open every MIDI input port and read from it. As thin as it can be.
//!
//! Everything behind it is tested with messages that a test sends itself. This file is the
//! part no test can run: a machine without a MIDI device has nothing to open.

use std::collections::{BTreeMap, BTreeSet};

use midir::{MidiInput, MidiInputConnection};

use crate::keys::Input;

/// The name this application shows in the MIDI system of the machine.
const CLIENT: &str = "Sound Tools";

#[derive(Debug, thiserror::Error)]
pub enum PortError {
    #[error("MIDI is not available on this machine: {0}")]
    Unavailable(#[from] midir::InitError),
    #[error("the MIDI port {name:?} did not open: {message}")]
    NotOpened { name: String, message: String },
}

/// Every MIDI input port of the machine, open and read into one [`Input`].
///
/// All ports, all channels, merged. There is no picker and no filter: a composer plugs a
/// keyboard in and plays. Call [`Self::refresh`] about once a second, so a keyboard plugged in
/// later works without a restart.
pub struct Ports {
    input: Input,
    /// By the stable id the system gives a port, with the name for people.
    open: BTreeMap<String, (String, MidiInputConnection<Input>)>,
    /// Ports that did not open. They are not tried again until they come back, so one broken
    /// port does not report itself every second.
    failed: BTreeSet<String>,
}

impl Ports {
    pub fn new(input: Input) -> Self {
        Self {
            input,
            open: BTreeMap::new(),
            failed: BTreeSet::new(),
        }
    }

    /// The names of the ports that are open, for people.
    pub fn open_names(&self) -> Vec<&str> {
        self.open.values().map(|(name, _)| name.as_str()).collect()
    }

    /// Opens ports that appeared and drops ones that went away. Gives the names that opened
    /// now, and the first port that would not open.
    pub fn refresh(&mut self) -> Result<Vec<String>, PortError> {
        let lister = MidiInput::new(CLIENT)?;
        let ports = lister.ports();
        let present: BTreeSet<String> = ports.iter().map(midir::MidiInputPort::id).collect();
        let open_before = self.open.len();
        self.open.retain(|id, _| present.contains(id));
        self.failed.retain(|id| present.contains(id));
        if self.open.len() < open_before {
            // A keyboard that is unplugged while it holds keys sends no note off, ever. What
            // the live input holds goes, through the one release path.
            self.input.release_held();
        }

        let mut opened = Vec::new();
        let mut error = None;
        for port in &ports {
            let id = port.id();
            if self.open.contains_key(&id) || self.failed.contains(&id) {
                continue;
            }
            // `connect` takes the `MidiInput`, so each connection needs one of its own.
            let client = MidiInput::new(CLIENT)?;
            let name = client.port_name(port).unwrap_or_else(|_| id.clone());
            let read = |_timestamp: u64, bytes: &[u8], input: &mut Input| {
                // The time a message arrived is taken here and not from the port, because
                // several ports would each count from a starting point of their own.
                input.read(bytes);
            };
            match client.connect(port, CLIENT, read, self.input.clone()) {
                Ok(connection) => {
                    self.open.insert(id, (name.clone(), connection));
                    opened.push(name);
                }
                Err(failure) => {
                    self.failed.insert(id);
                    error.get_or_insert(PortError::NotOpened {
                        name,
                        message: failure.to_string(),
                    });
                }
            }
        }
        match error {
            Some(error) => Err(error),
            None => Ok(opened),
        }
    }
}
