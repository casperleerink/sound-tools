//! What the machine plays into the project live: the MIDI keyboard, the computer keys and the
//! audio input. None of it is project state, and every window has it, whatever its main view.
//!
//! A take reaches the keyboard and the input through here: while one runs, the live notes stay
//! where it began and the input stays open, see [`LiveInput::hold`].

use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Context, Entity, EventEmitter, KeyDownEvent, Task, prelude::*};
use midi::{ComputerKeys, Input, Keyboard, Latency, Lost};
use sound_core::{InputEndpoint, InstanceId, Project, ProjectEvent, StreamTiming};
use sound_notes::NOTES_INPUT;
use sound_ui::{Recording, Session, every_poll};

use super::audio_input::{AudioInput, OpenInput, OpenedInput};
use crate::recorder::{RecorderCommand, RecorderReport};

/// What a take of a recorder holds while it runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hold {
    /// The live notes stay where they went when it began.
    pub notes: bool,
    /// It records audio: the input stays open, whatever else wants it.
    pub audio: bool,
}

/// What the recorder of the input reported in one run, for every take that listens: each
/// keeps what is about the takes it started.
pub struct Recorded(pub Rc<[RecorderReport]>);

pub struct LiveInput {
    session: Entity<Session>,
    recording: Entity<Recording>,
    /// The MIDI input in the engine. `None` only when the engine refused it, which is reported.
    keyboard: Option<Keyboard>,
    /// The computer keys while they play as a MIDI keyboard. Not project state, like the click.
    computer_keys: Option<ComputerKeys>,
    /// When the sound of an engine frame reaches the device, for the latency. `None` without a
    /// device, so an offline window measures nothing instead of guessing.
    timing: Option<Arc<StreamTiming>>,
    /// The audio input, open while an instance is armed, a take records or `project.json`
    /// connects it, and its recorder.
    audio: AudioInput,
    /// What the takes of the recorders of the project hold, see [`Hold`].
    hold: Hold,
    /// Drains what the engine reports about the MIDI input.
    _polling: Task<()>,
}

impl EventEmitter<Recorded> for LiveInput {}

impl LiveInput {
    pub fn new(
        session: Entity<Session>,
        timing: Option<Arc<StreamTiming>>,
        open_input: Option<OpenInput>,
        cx: &mut Context<Self>,
    ) -> Self {
        // An agent may connect the device input in `project.json`, or take it away, while the
        // window runs. A connection that comes into the graph or leaves it, as its instance
        // comes or goes, changes the problems.
        cx.subscribe(&session, |live, _, event, cx| {
            if matches!(
                event,
                ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged
            ) {
                live.follow_input(cx);
            }
            // A deleted instance is no longer armed.
            if matches!(event, ProjectEvent::Deleted(_)) {
                let project = live.session.read(cx).project();
                let exists: Vec<InstanceId> = (live.recording.read(cx).armed())
                    .filter(|armed| project.tool_of(armed).is_some())
                    .cloned()
                    .collect();
                live.recording.update(cx, |recording, cx| {
                    recording.retain_armed(|armed| exists.contains(armed), cx);
                });
            }
        })
        .detach();
        // The input is open while something is armed: that is how a composer sees the level
        // before a take. A project that connects the device input opens it with the window.
        let recording = session.read(cx).recording().clone();
        cx.observe(&recording, |live, _, cx| live.follow_input(cx))
            .detach();
        let live = cx.weak_entity();
        cx.defer(move |cx| {
            // A window that went in the meantime opens nothing.
            live.update(cx, |live, cx| live.follow_input(cx)).ok();
        });
        // The MIDI input is a processor in the engine too. It plays nowhere until the first
        // poll wires it.
        let keyboard = session.update(cx, |session, cx| match Keyboard::attach(session.engine()) {
            Ok(keyboard) => Some(keyboard),
            Err(error) => {
                session.report(error, cx);
                None
            }
        });
        // What a key press cost, once, when the window goes away. macOS ends the process
        // without unwinding, and GPUI drops the views first, so this is the last moment the
        // numbers exist. A session with no MIDI message prints nothing.
        cx.on_release(|live, _| print_midi_report(live.latency(), live.lost_messages()))
            .detach();
        // What the engine reports about the MIDI input must be drained whether the project
        // plays or not.
        let polling = every_poll(cx, |live: &mut Self, cx| live.poll_input(cx));
        // The computer keys go on and off where the session says: cmd-K, or a tool's button.
        cx.observe(&session, |live, _, cx| live.follow_computer_keys(cx))
            .detach();
        Self {
            recording,
            session,
            keyboard,
            computer_keys: None,
            timing,
            audio: AudioInput::new(open_input),
            hold: Hold::default(),
            _polling: polling,
        }
    }

    /// Takes what the engine reported about the MIDI input, wires the live input to where it
    /// goes now, and polls the audio input. The timer calls it; tests call it to skip the wait.
    ///
    /// Nothing here is on the way from a key to its sound: that path is the input ring and the
    /// audio thread, and it runs whether this is called or not.
    pub fn poll_input(&mut self, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let destinations = self.live_notes(cx);
        self.poll_audio(cx);
        let timing = self.timing.clone();
        let held = self.hold.notes;
        let Some(keyboard) = self.keyboard.as_mut() else {
            return;
        };
        // The poll also makes the port of an earlier `play_into` the one the live input
        // reaches: the release of what was held went out in a block before it.
        let polled = session.update(cx, |session, _| {
            keyboard.poll(session.engine(), timing.as_deref())
        });
        // A take goes to where it began, so the live input stays there too while it runs.
        // Selecting something else during a take would otherwise split the two.
        let wired = match held {
            true => Ok(()),
            false => session.update(cx, |session, _| {
                keyboard.play_into(session.engine(), &destinations)
            }),
        };
        if let Err(error) = polled.and(wired) {
            session.update(cx, |session, cx| session.report(error, cx));
        }
    }

    /// Where live notes go now: into the `notes` of the selected instance or of the nearest
    /// parent of it that has them, and with nothing selected into every instance at the top
    /// that has them. Read it after every change: a port moves when an instance is built again.
    fn live_notes(&self, cx: &App) -> Vec<InputEndpoint> {
        let session = self.session.read(cx);
        notes_inputs(session.project(), session.selected())
    }

    /// Where a device layer puts the messages it reads. `None` when the engine refused the
    /// MIDI input.
    pub fn midi_input(&self) -> Option<Input> {
        self.keyboard.as_ref().map(Keyboard::input)
    }

    pub fn timing(&self) -> Option<Arc<StreamTiming>> {
        self.timing.clone()
    }

    /// How long a MIDI message took to reach the device, over this session.
    pub fn latency(&self) -> Latency {
        (self.keyboard.as_ref())
            .map(Keyboard::latency)
            .unwrap_or_default()
    }

    /// Messages that were never played, and messages that are missing from a take.
    pub fn lost_messages(&self) -> Lost {
        (self.keyboard.as_ref())
            .map(Keyboard::lost)
            .unwrap_or_default()
    }

    /// What the takes of the recorders of the project hold while they run. The input follows
    /// at once.
    pub fn hold(&mut self, hold: Hold, cx: &mut Context<Self>) {
        if self.hold != hold {
            self.hold = hold;
            self.follow_input(cx);
        }
    }

    /// Whether the computer keys play. For tests.
    pub fn computer_keys_are_on(&self) -> bool {
        self.computer_keys.is_some()
    }

    /// Turns the computer keys on or off, as cmd-K does. Not an edit, like the click.
    pub fn toggle_computer_keys(&mut self, cx: &mut Context<Self>) {
        let on = self.computer_keys.is_none();
        (self.session).update(cx, |session, cx| session.set_computer_keys(on, cx));
    }

    /// Plays the computer keys while the session wants them, when there is a keyboard to play
    /// into. Off ends what they hold.
    fn follow_computer_keys(&mut self, cx: &mut Context<Self>) {
        let wanted = self.session.read(cx).computer_keys_on();
        if wanted == self.computer_keys.is_some() {
            return;
        }
        if wanted && self.keyboard.is_none() {
            (self.session).update(cx, |session, cx| session.set_computer_keys(false, cx));
            return;
        }
        match wanted {
            true => self.computer_keys = Some(ComputerKeys::default()),
            false => {
                self.let_go_of_computer_keys(cx);
                self.computer_keys = None;
            }
        }
        cx.notify();
    }

    /// A key went down. Gives whether the computer keys took it, and then nothing else may
    /// hear it. A key they hold stays theirs wherever the focus went since, so its repeats
    /// reach nothing else. Another key is theirs while they play, when it is one of theirs
    /// with no cmd, ctrl, alt or fn and the plain keys are `free`.
    pub fn computer_key_down(&mut self, event: &KeyDownEvent, free: bool) -> bool {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return false;
        };
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if keys.holds(key) {
            return true;
        }
        let modifiers = keystroke.modifiers;
        let plain =
            !(modifiers.platform || modifiers.control || modifiers.alt || modifiers.function);
        if !(free && plain && ComputerKeys::plays(key)) {
            return false;
        }
        // A repeat of a key that went down where they did not hear it, such as on a tool's
        // card, starts nothing. Else it goes where a MIDI keyboard's messages go, so the
        // instrument, a take and the tools hear it as they hear those.
        if !event.is_held
            && let Some(played) = keys.down(key)
        {
            keyboard.input().send(played);
        }
        true
    }

    /// A key came up. Gives whether it was the computer keys', and ends its note.
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

    /// Ends every note the computer keys hold, for when their key ups will not come: cmd is
    /// down, or the window lost the keys.
    pub fn let_go_of_computer_keys(&mut self, _: &mut Context<Self>) {
        let (Some(keys), Some(keyboard)) = (&mut self.computer_keys, &self.keyboard) else {
            return;
        };
        let input = keyboard.input();
        for played in keys.release() {
            input.send(played);
        }
    }

    /// Whether this window has an audio input to open at all.
    pub fn can_open_input(&self) -> bool {
        self.audio.can_open()
    }

    /// The rate of the open input, while one is open.
    pub fn input_rate(&self) -> Option<u32> {
        self.audio.sample_rate()
    }

    /// Sends a command to the recorder of the input, for its next run.
    pub(crate) fn record(&mut self, command: RecorderCommand) {
        self.audio.send(command);
    }

    /// Opens the input on the background executor, when it is not open or on its way.
    pub fn open_input(&mut self, cx: &mut Context<Self>) {
        let Some((opener, generation)) = self.audio.start_opening() else {
            return;
        };
        let work = cx.background_spawn(async move { opener() });
        cx.spawn(async move |live, cx| {
            let opened = work.await;
            match live.update(cx, |live, cx| live.input_opened(generation, opened, cx)) {
                Ok(()) => {}
                // A window that went away takes the input with it.
                Err(_) => {}
            }
        })
        .detach();
    }

    /// The input opened, or why not. With no input, nothing is armed and no audio records.
    fn input_opened(
        &mut self,
        generation: u64,
        mut opened: Result<OpenedInput, sound_core::DeviceError>,
        cx: &mut Context<Self>,
    ) {
        let assets = self.session.read(cx).project().assets().clone();
        let notice = opened.as_mut().ok().and_then(|input| input.notice.take());
        match self.audio.opened(generation, opened, &assets, cx) {
            None => {}
            Some(Ok(live)) => {
                if let Some(notice) = notice {
                    self.session
                        .update(cx, |session, cx| session.report(notice, cx));
                }
                let plays = live.sample_rate() == self.session.read(cx).project().sample_rate();
                // The engine reads it while the input is open, whether a connection hears it
                // or not, so it never falls behind.
                self.session.update(cx, |session, cx| {
                    session.background(cx, |project| project.set_live_input(Some(live)))
                });
                // Opened for a connection alone, an input at another rate plays nothing. The
                // problem of the connection says why, and the device closes. A take records at
                // any rate.
                let armed = self.recording.read(cx).armed().next().is_some();
                if !plays && !armed && !self.hold.audio {
                    self.audio.give_up(cx);
                    return;
                }
            }
            Some(Err(error)) => {
                self.session
                    .update(cx, |session, cx| session.report(error, cx));
                // A take that waited for it ends with no sound: its notes are kept.
                if self.hold.audio {
                    cx.emit(Recorded(vec![RecorderReport::NoInput].into()));
                }
                // Armed with no input would show a level that never moves.
                self.recording.update(cx, |recording, cx| {
                    recording.retain_armed(|_| false, cx);
                    recording.set_takes(Vec::new(), cx);
                });
            }
        }
    }

    /// The input is open while something is armed, a take records audio or a `project.json`
    /// connection in the graph hears the device input, and closed otherwise, so the device is
    /// not held and macOS shows no microphone in use. One that failed or went away is closed
    /// and not opened again here until something new wants it: the next arming, or a
    /// connection that comes, opens the input there is then.
    pub fn follow_input(&mut self, cx: &mut Context<Self>) {
        let armed: Vec<InstanceId> = self.recording.read(cx).armed().cloned().collect();
        let heard = self.session.read(cx).project().hears_device_input();
        if self.audio.wants(armed, heard) && !self.audio.is_gone() {
            self.open_input(cx);
        } else if !self.hold.audio && self.audio.is_open_or_opening() {
            self.audio.close(cx);
            self.session.update(cx, |session, cx| {
                session.background(cx, |project| project.set_live_input(None))
            });
        }
    }

    /// One poll of the audio input: its level for the views, a silence that says what is wrong,
    /// and a device that went away.
    fn poll_audio(&mut self, cx: &mut Context<Self>) {
        if self.audio.poll_silence() {
            let notice = "The audio input gives nothing but silence. If it is a microphone, allow Sound Tools, or the terminal it runs from, in System Settings, Privacy & Security, Microphone, then open the project again.";
            self.session
                .update(cx, |session, cx| session.report(notice, cx));
        }
        let levels = self.audio.levels().to_vec();
        if !(levels.is_empty() && self.recording.read(cx).input_levels().is_empty()) {
            (self.recording).update(cx, |recording, _| recording.set_input_levels(levels));
        }
        if self.audio.went_away() {
            self.input_gone(cx);
        }
        self.run_recorder(cx);
    }

    /// Runs the recorder once on the background executor, when it is here: it takes what the
    /// input captured, writes the takes and says what came of it, see [`Recorded`]. While
    /// nothing records it still takes what the input brings, so its ring never fills.
    fn run_recorder(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.audio.start_run() else {
            return;
        };
        let work = cx.background_spawn(async move { run.run() });
        cx.spawn(async move |live, cx| {
            let (recorder, reports, generation) = work.await;
            let ended = live.update(cx, |live, cx| {
                live.audio.end_run(recorder, generation);
                if !reports.is_empty() {
                    cx.emit(Recorded(reports.into()));
                }
            });
            match ended {
                Ok(()) => {}
                // A window that went away takes the recorder with it, and its files close.
                Err(_) => {}
            }
        })
        .detach();
    }

    /// The input went away, such as an interface that was unplugged. Everything is disarmed:
    /// arming again opens the default input that macOS has then. A take that records audio ends
    /// and says so itself; the recorder writes the last of it before the input closes.
    fn input_gone(&mut self, cx: &mut Context<Self>) {
        if !self.hold.audio {
            self.session.update(cx, |session, cx| {
                session.report("The audio input went away.", cx)
            });
        }
        self.recording
            .update(cx, |recording, cx| recording.retain_armed(|_| false, cx));
        if !self.hold.audio {
            self.follow_input(cx);
        }
    }
}

/// The `notes` of `selected` or of the nearest parent of it that has them; with nothing
/// selected, those of every instance at the top that has them, as a parent gives its notes to
/// each of its children that plays them.
pub fn notes_inputs(project: &Project, selected: Option<&InstanceId>) -> Vec<InputEndpoint> {
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

/// What a key press cost, measured over the session. It covers the wait for the next audio
/// block and the output latency the device reports, not the keyboard and its cable.
fn print_midi_report(latency: Latency, lost: Lost) {
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
