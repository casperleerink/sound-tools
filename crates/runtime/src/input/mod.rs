//! What brings sound and notes into a project: the MIDI keyboard, the computer keys as a
//! keyboard, the audio input, the takes of the project's `recorder`s, the sound of other apps,
//! and files the composer brings in. Plain structs whose slow work runs on threads of their
//! own: each thread calls the [`Waker`] it was given when it has news, and the owner takes it
//! with a `poll` that never waits. None of it is project state.

mod audio;
mod import;
mod live;
mod other_apps;
pub mod recorder;
mod takes;

use std::collections::BTreeSet;
use std::sync::Arc;

use midi::{ComputerKeys, Keyboard};
use sound_core::{InstanceId, Project, ProjectEvent, StreamTiming};
use sound_typescript::next::Take;

pub use audio::{OpenInput, OpenedInput, system_input};
pub use import::{Brought, Importing, import};
pub use live::KeyDown;
pub use other_apps::{AppSounds, OpenedTap, OtherApps};
pub use takes::{TakeNote, TakeNotes, TakeSound};

use audio::AudioInput;
use takes::Recorders;

/// How a thread tells the owner it has news: the owner polls soon after.
pub type Waker = sound_typescript::next::Waker;

/// What happened since the last [`Input::poll`].
#[derive(Debug)]
pub enum Happened {
    /// What the composer is to read, in their words.
    Notice(String),
    /// A take ended: for the `onTake` of the parent of its recorder, see
    /// `sound_typescript::next::Tools::take`.
    Take(Take),
}

/// What this machine gives [`Input`].
#[derive(Default)]
pub struct Devices {
    /// Opens the audio input, on a thread. `None`: there is none to open.
    pub open_input: Option<OpenInput>,
    /// When the sound of an engine frame reaches the device. `None` without a device, so an
    /// offline engine measures no latency instead of guessing.
    pub timing: Option<Arc<StreamTiming>>,
    /// Reads every MIDI port of the machine, and looks for new ones every second.
    pub midi_ports: bool,
}

/// The live input of one open project: the MIDI keyboard and the computer keys, which play
/// where the composer works, and the audio input with the takes of the recorders.
pub struct Input {
    /// The MIDI input in the engine. `None` only when the engine refused it, which was told.
    keyboard: Option<Keyboard>,
    /// The computer keys while they play as a MIDI keyboard.
    computer_keys: Option<ComputerKeys>,
    timing: Option<Arc<StreamTiming>>,
    audio: AudioInput,
    recorders: Recorders,
    /// The recorders whose `armed` watch is on: the input is open for them.
    armed: BTreeSet<InstanceId>,
    /// What the takes that run hold, see [`Hold`].
    hold: Hold,
    /// Says why a MIDI port did not open, while the thread that reads them runs.
    ports: Option<live::PortsThread>,
    happened: Vec<Happened>,
}

/// What the takes of the recorders hold while they run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Hold {
    /// The live notes stay where they went when a take began.
    notes: bool,
    /// A take records sound: the input stays open, whatever else wants it.
    audio: bool,
}

impl Input {
    /// The live input of `project`: the MIDI input joins its engine, and plays nowhere until
    /// the first [`Input::poll`].
    pub fn new(project: &mut Project, devices: Devices, waker: Waker) -> Self {
        let mut happened = Vec::new();
        let keyboard = match Keyboard::attach(project.engine()) {
            Ok(keyboard) => Some(keyboard),
            Err(error) => {
                happened.push(Happened::Notice(error.to_string()));
                None
            }
        };
        let ports = (keyboard.as_ref())
            .filter(|_| devices.midi_ports)
            .and_then(|keyboard| live::PortsThread::start(keyboard.input(), waker.clone()));
        Self {
            keyboard,
            computer_keys: None,
            timing: devices.timing,
            audio: AudioInput::new(devices.open_input, waker),
            recorders: Recorders::default(),
            armed: BTreeSet::new(),
            hold: Hold::default(),
            ports,
            happened,
        }
    }

    /// Takes what the threads brought, follows the project and the recorders, and wires the
    /// live notes into the `notes` of `selected` or of its nearest parent that has them; with
    /// nothing selected, of every instance at the top that has them. `events` are the project's
    /// since the last poll. Never waits.
    ///
    /// Nothing here is on the way from a key to its sound: that path is the input ring and the
    /// audio thread, and it runs whether this is called or not.
    pub fn poll(
        &mut self,
        project: &mut Project,
        events: &[ProjectEvent],
        selected: Option<&InstanceId>,
    ) -> Vec<Happened> {
        if let Some(ports) = &self.ports {
            self.happened.extend(ports.notices().map(Happened::Notice));
        }
        // A deleted recorder is no longer armed.
        if events.iter().any(|event| matches!(event, ProjectEvent::Deleted(_))) {
            self.armed.retain(|armed| project.tool_of(armed).is_some());
        }
        self.poll_audio(project);
        self.poll_keyboard(project, selected);
        self.poll_recorders(project);
        self.hold = self.recorders.hold();
        self.follow_input(project);
        std::mem::take(&mut self.happened)
    }

    /// Whether a take records: undo waits until it ends.
    pub fn is_recording(&self) -> bool {
        self.recorders.is_recording()
    }

    /// The recorders whose audio input is open for them: they show its level.
    pub fn armed(&self) -> &BTreeSet<InstanceId> {
        &self.armed
    }

    /// The loudest sample of each channel of the audio input since the last poll; none while
    /// it is closed. For a meter, once per frame.
    pub fn input_levels(&self) -> &[f32] {
        self.audio.levels()
    }

    /// The notes the take of `recorder` that runs heard so far.
    pub fn take_notes(&self, recorder: &InstanceId) -> Option<&TakeNotes> {
        self.recorders.notes_of(recorder)
    }

    /// The sound of the newest take of `recorder` while it records, once its first frames came.
    pub fn take_sound(&self, recorder: &InstanceId) -> Option<&TakeSound> {
        self.recorders.sound_of(recorder)
    }

    /// Where a device layer, or a test, puts the MIDI messages it reads. `None` when the
    /// engine refused the MIDI input.
    pub fn midi_input(&self) -> Option<midi::Input> {
        self.keyboard.as_ref().map(Keyboard::input)
    }
}

impl Drop for Input {
    /// What a key press cost, once, as the project closes. A session with no MIDI message
    /// prints nothing.
    fn drop(&mut self) {
        if let Some(keyboard) = &self.keyboard {
            live::print_midi_report(keyboard.latency(), keyboard.lost());
        }
    }
}

/// Drops `value` on a thread of its own: closing a device can take a while, and never stops
/// the thread that draws.
fn drop_off_thread(value: impl Send + 'static) {
    let dropped = std::thread::Builder::new()
        .name("close-device".into())
        .spawn(move || drop(value));
    if let Err(error) = dropped {
        eprintln!("error: no thread to close a device on: {error}");
    }
}
