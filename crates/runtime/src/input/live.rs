//! The MIDI keyboard and the computer keys: they play into the `notes` of where the composer
//! works, and while a take runs they stay where it began.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use midi::{ComputerKeys, Latency, Lost, PortError, Ports};
use sound_core::{InputEndpoint, InstanceId, Project};
use sound_notes::NOTES_INPUT;

use super::{Happened, Input, Waker};

/// How often the MIDI ports of the machine are looked at again, so a keyboard plugged in
/// later plays without a restart.
const PORTS_EVERY: Duration = Duration::from_secs(1);

/// A key of the computer keyboard that went down.
#[derive(Clone, Copy, Debug)]
pub struct KeyDown<'a> {
    /// As the keymap names it, such as `a`.
    pub key: &'a str,
    /// Cmd, ctrl, alt or fn is down with it.
    pub modified: bool,
    /// The system repeats a key held down.
    pub repeat: bool,
    /// The plain keys are free: no text field and no tool that has the keys takes them.
    pub free: bool,
}

impl Input {
    /// Takes what the engine reported about the MIDI input and wires the live notes to where
    /// they go now, see [`Input::poll`].
    pub(super) fn poll_keyboard(&mut self, project: &mut Project, selected: Option<&InstanceId>) {
        let destinations = notes_inputs(project, selected);
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        // The poll also makes the port of an earlier `play_into` the one the live input
        // reaches: the release of what was held went out in a block before it.
        let polled = keyboard.poll(project.engine(), self.timing.as_deref());
        // A take goes to where it began, so the live input stays there too while it runs.
        // Selecting something else during a take would otherwise split the two.
        let wired = match self.hold.notes {
            true => Ok(()),
            false => keyboard.play_into(project.engine(), &destinations),
        };
        if let Err(error) = polled.and(wired) {
            self.happened.push(Happened::Notice(error.to_string()));
        }
    }

    /// Whether the computer keys play.
    pub fn computer_keys(&self) -> bool {
        self.computer_keys.is_some()
    }

    /// Turns the computer keys on or off, as cmd-K does: not an edit, like the click. Off ends
    /// what they hold. Without a keyboard to play into they stay off. Whether they play now.
    pub fn set_computer_keys(&mut self, on: bool) -> bool {
        match (on, self.keyboard.is_some()) {
            (true, true) => {
                self.computer_keys.get_or_insert_with(ComputerKeys::default);
            }
            _ => {
                self.let_go_of_computer_keys();
                self.computer_keys = None;
            }
        }
        self.computer_keys()
    }

    /// A key went down. Gives whether the computer keys took it, and then nothing else may hear
    /// it. A key they hold stays theirs wherever the focus went since, so its repeats reach
    /// nothing else. Another key is theirs while they play, when it is one of theirs, plain
    /// and the plain keys are free.
    pub fn computer_key_down(&mut self, down: KeyDown<'_>) -> bool {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return false;
        };
        if keys.holds(down.key) {
            return true;
        }
        if !(down.free && !down.modified && ComputerKeys::plays(down.key)) {
            return false;
        }
        // A repeat of a key that went down where they did not hear it, such as on a tool's
        // view, starts nothing. Else it goes where a MIDI keyboard's messages go, so the
        // instrument, a take and the tools hear it as they hear those.
        if !down.repeat
            && let Some(played) = keys.down(down.key)
        {
            keyboard.input().send(played);
        }
        true
    }

    /// A key came up, whatever was held with it. Gives whether it was the computer keys', and
    /// ends its note.
    pub fn computer_key_up(&mut self, key: &str) -> bool {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return false;
        };
        let held = keys.holds(key);
        if let Some(played) = keys.up(key) {
            keyboard.input().send(played);
        }
        held
    }

    /// Ends every note the computer keys hold, for when their key ups will not come: cmd went
    /// down (macOS sends no key up while it is held), or the window lost the keys.
    pub fn let_go_of_computer_keys(&mut self) {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return;
        };
        let input = keyboard.input();
        for played in keys.release() {
            input.send(played);
        }
    }
}

/// The `notes` of `selected` or of the nearest parent of it that has them; with nothing
/// selected, those of every instance at the top that has them, as a parent gives its notes to
/// each of its children that plays them.
fn notes_inputs(project: &Project, selected: Option<&InstanceId>) -> Vec<InputEndpoint> {
    let Some(selected) = selected else {
        let top = project
            .instances()
            .filter(|(id, _)| !id.as_str().contains('/'));
        return top
            .filter_map(|(id, _)| project.input_port(id, NOTES_INPUT))
            .collect();
    };
    let mut at = Some(selected.clone());
    while let Some(id) = at {
        if let Some(port) = project.input_port(&id, NOTES_INPUT) {
            return vec![port];
        }
        at = id.parent();
    }
    Vec::new()
}

/// The thread that reads every MIDI port of the machine into the keyboard. It ends with this.
pub(super) struct PortsThread {
    _stop: Sender<()>,
    notices: Receiver<String>,
}

impl PortsThread {
    pub(super) fn start(input: midi::Input, waker: Waker) -> Option<Self> {
        let (stop, stopped) = mpsc::channel::<()>();
        let (tell, notices) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("midi-ports".into())
            .spawn(move || {
                let mut ports = Ports::new(input);
                loop {
                    match ports.refresh() {
                        Ok(opened) => {
                            for name in opened {
                                println!("midi in: {name}");
                            }
                        }
                        Err(error) => {
                            let unavailable = matches!(error, PortError::Unavailable(_));
                            tell.send(error.to_string()).ok();
                            waker();
                            // The system's MIDI does not start later in the same run.
                            if unavailable {
                                return;
                            }
                        }
                    }
                    if !matches!(
                        stopped.recv_timeout(PORTS_EVERY),
                        Err(RecvTimeoutError::Timeout)
                    ) {
                        return;
                    }
                }
            });
        match started {
            Ok(_) => Some(Self {
                _stop: stop,
                notices,
            }),
            Err(error) => {
                eprintln!("error: no thread to read the MIDI ports on: {error}");
                None
            }
        }
    }

    pub(super) fn notices(&self) -> impl Iterator<Item = String> + '_ {
        self.notices.try_iter()
    }
}

/// What a key press cost, measured over the session. It covers the wait for the next audio
/// block and the output latency the device reports, not the keyboard and its cable.
pub(super) fn print_midi_report(latency: Latency, lost: Lost) {
    if latency.count() == 0 {
        return;
    }
    println!(
        "midi to sound over {} messages: mean {:?}, shortest {:?}, longest {:?}, jitter {:?}",
        latency.count(),
        latency.mean(),
        latency.shortest(),
        latency.longest(),
        latency.spread()
    );
    if lost.any() {
        println!(
            "midi lost: {} messages never played, {} missing from a take",
            lost.input, lost.reports
        );
    }
}
