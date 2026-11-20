//! The audio input: open while a recorder is armed, a take records or `project.json` connects
//! the device input, its level for the meters, and the recorder that writes the takes. The
//! input opens, the recorder runs and the device closes on threads of their own, never on the
//! thread that draws: opening a device can take a while, and the first time macOS asks the
//! composer whether the app may use the microphone.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sound_core::{
    Assets, CaptureReader, CaptureStatus, DeviceError, InputDevice, InstanceId, LiveInput,
    Project,
};

use super::recorder::{Recorder, RecorderCommand, RecorderReport};
use super::{Happened, Input, Waker, drop_off_thread};

/// An input that opened: the stream of the device, which stops when it is dropped, the reader
/// of what it captures, and the live input for the engine. A test gives one with no device and
/// writes into it itself.
pub struct OpenedInput {
    pub stream: Option<Box<dyn Send>>,
    pub reader: CaptureReader,
    pub live: LiveInput,
    /// Says so when this is not the input the composer chose: that one is not there or did not
    /// open.
    pub notice: Option<String>,
}

/// How the audio input opens: an input device of the system, or a simulated one. It is called
/// on a thread of its own.
pub type OpenInput = Arc<dyn Fn() -> Result<OpenedInput, DeviceError> + Send + Sync>;

/// The input device the composer chose on this machine, by its id, or the default input of the
/// system when there is no choice or that device is not there or does not open. The default
/// then says so: a take must not come from another microphone unnoticed.
pub fn system_input(choice: Option<String>) -> OpenInput {
    Arc::new(move || {
        let device = InputDevice::open(choice.as_deref())?;
        let name = device.name()?;
        let chosen = choice.is_none() || device.id().ok() == choice;
        let notice = (!chosen).then(|| {
            format!("The chosen audio input is not there or did not open, so {name} is used.")
        });
        let (stream, reader, live) = device.start()?;
        println!(
            "audio in: {name}, {} Hz, {} channels",
            reader.sample_rate(),
            reader.channels()
        );
        Ok(OpenedInput {
            stream: Some(Box::new(stream)),
            reader,
            live,
            notice,
        })
    })
}

/// After this much exact silence an input is likely one macOS does not let the app hear.
const SILENT_FOR: Duration = Duration::from_secs(2);

/// How often the recorder takes what the input captured.
const RECORDER_RUNS_EVERY: Duration = Duration::from_millis(10);

const SILENCE: &str = "The audio input gives nothing but silence. If it is a microphone, allow Sound Tools, or the terminal it runs from, in System Settings, Privacy & Security, Microphone, then open the project again.";

/// Whether an open input gave anything but exact silence.
enum Silence {
    Since(Instant),
    Heard,
    Told,
}

struct Open {
    _stream: Option<Box<dyn Send>>,
    status: CaptureStatus,
    sample_rate: u32,
    silence: Silence,
    /// The loudest sample of each channel at the last poll.
    levels: Vec<f32>,
    /// The input went away, and that was told.
    gone_told: bool,
    /// To the thread of the recorder, which ends when this goes.
    commands: Sender<RecorderCommand>,
}

type Opening = (u64, Result<OpenedInput, DeviceError>);

pub(super) struct AudioInput {
    open_input: Option<OpenInput>,
    open: Option<Open>,
    /// The opening on its way, while one is. A close forgets it, and what it opens is dropped.
    opening: Option<u64>,
    openings: u64,
    /// What waits for the recorder while the input opens.
    commands: Vec<RecorderCommand>,
    /// The last opening failed, the input went away, or it was given up because it plays
    /// nothing. It is not opened again until something new wants it, see [`Self::wants`].
    failed: bool,
    /// What wanted it at the last [`Self::wants`].
    armed: Vec<InstanceId>,
    heard: bool,
    opened: (Sender<Opening>, Receiver<Opening>),
    reports: (Sender<Vec<RecorderReport>>, Receiver<Vec<RecorderReport>>),
    waker: Waker,
}

impl AudioInput {
    pub(super) fn new(open_input: Option<OpenInput>, waker: Waker) -> Self {
        Self {
            open_input,
            open: None,
            opening: None,
            openings: 0,
            commands: Vec::new(),
            failed: false,
            armed: Vec::new(),
            heard: false,
            opened: mpsc::channel(),
            reports: mpsc::channel(),
            waker,
        }
    }

    /// Whether the input is to be open for the armed recorders and whether a `project.json`
    /// connection hears it. After a failure it stays closed until something new wants it: a
    /// recorder armed that was not, or a connection that was not there. Else a failure that
    /// disarms every recorder would open it again, and fail again, at once.
    fn wants(&mut self, armed: Vec<InstanceId>, heard: bool) -> bool {
        let new = (heard && !self.heard) || armed.iter().any(|each| !self.armed.contains(each));
        self.failed &= !new;
        self.heard = heard;
        self.armed = armed;
        (heard || !self.armed.is_empty()) && !self.failed
    }

    /// Whether there is an input to open at all.
    pub(super) fn can_open(&self) -> bool {
        self.open_input.is_some()
    }

    fn is_open_or_opening(&self) -> bool {
        self.open.is_some() || self.opening.is_some()
    }

    /// Opens the input on a thread of its own, when it is not open or on its way.
    pub(super) fn open(&mut self) {
        if self.is_open_or_opening() {
            return;
        }
        let Some(opener) = self.open_input.clone() else {
            return;
        };
        self.openings += 1;
        let generation = self.openings;
        self.opening = Some(generation);
        let (opened, waker) = (self.opened.0.clone(), self.waker.clone());
        let started = std::thread::Builder::new()
            .name("audio-input-open".into())
            .spawn(move || {
                // A project that closed meanwhile takes nothing: the input closes again.
                if let Err(mpsc::SendError((_, Ok(input)))) = opened.send((generation, opener())) {
                    drop(input);
                }
                waker();
            });
        if let Err(error) = started {
            eprintln!("error: no thread to open the audio input on: {error}");
            self.opening = None;
            self.failed = true;
        }
    }

    /// The opening that came back, if one did: its live input for the engine and the notice
    /// that it is not the one chosen, or why it did not open. `None` too for an opening given
    /// up since: its input closes again.
    fn take_opened(&mut self, assets: &Assets) -> Option<Result<(LiveInput, Option<String>), DeviceError>> {
        let (generation, opened) = self.opened.1.try_recv().ok()?;
        if self.opening != Some(generation) {
            if let Ok(input) = opened {
                drop_off_thread(input.stream);
            }
            return None;
        }
        self.opening = None;
        let OpenedInput {
            stream,
            reader,
            live,
            notice,
        } = match opened {
            Ok(opened) => opened,
            Err(error) => {
                self.commands.clear();
                self.failed = true;
                return Some(Err(error));
            }
        };
        let (status, sample_rate) = (reader.status(), reader.sample_rate());
        let commands = self.start_recorder(reader, assets.clone());
        for command in self.commands.drain(..) {
            commands.send(command).ok();
        }
        self.failed = false;
        self.open = Some(Open {
            _stream: stream,
            status,
            sample_rate,
            silence: Silence::Since(Instant::now()),
            levels: Vec::new(),
            gone_told: false,
            commands,
        });
        Some(Ok((live, notice)))
    }

    /// The recorder of an opening, on a thread of its own: it takes what the input captured,
    /// writes the takes in order and says what came of it. While nothing records it still
    /// takes what the input brings, so its ring never fills. It ends, and its files close, when
    /// the input closes.
    fn start_recorder(&self, reader: CaptureReader, assets: Assets) -> Sender<RecorderCommand> {
        let (commands, received) = mpsc::channel();
        let (reports, waker) = (self.reports.0.clone(), self.waker.clone());
        let started = std::thread::Builder::new()
            .name("recorder".into())
            .spawn(move || {
                let mut recorder = Recorder::new(reader, assets);
                loop {
                    let (first, closed) = match received.recv_timeout(RECORDER_RUNS_EVERY) {
                        Ok(command) => (Some(command), false),
                        Err(RecvTimeoutError::Timeout) => (None, false),
                        Err(RecvTimeoutError::Disconnected) => (None, true),
                    };
                    let mut run: Vec<RecorderCommand> = first.into_iter().collect();
                    run.extend(received.try_iter());
                    let said = recorder.run(run);
                    if !said.is_empty() {
                        if reports.send(said).is_err() {
                            return;
                        }
                        waker();
                    }
                    if closed {
                        return;
                    }
                }
            });
        if let Err(error) = started {
            eprintln!("error: no thread to record on: {error}");
            self.reports.0.send(vec![RecorderReport::NoInput]).ok();
        }
        commands
    }

    /// What the recorder said since the last call.
    fn take_reports(&mut self) -> Vec<RecorderReport> {
        self.reports.1.try_iter().flatten().collect()
    }

    /// Stops the device off this thread, or forgets the opening on its way. The recorder writes
    /// what the input brought and ends.
    fn close(&mut self) {
        if let Some(open) = self.open.take() {
            drop_off_thread(open._stream);
        }
        self.opening = None;
        self.commands.clear();
    }

    /// Closes an input that was opened for a connection alone and plays nothing, because it
    /// runs at another rate than the output. It is not opened again until something new wants
    /// it.
    fn give_up(&mut self) {
        self.close();
        self.failed = true;
    }

    pub(super) fn sample_rate(&self) -> Option<u32> {
        self.open.as_ref().map(|open| open.sample_rate)
    }

    /// The loudest sample of each channel at the last poll; none while it is closed.
    pub(super) fn levels(&self) -> &[f32] {
        self.open.as_ref().map_or(&[], |open| &open.levels)
    }

    /// Takes the levels, and says whether the open input just reached two seconds of nothing
    /// but exact silence, once per opening. A real input is never exactly silent: macOS gives
    /// zeros to an app it does not let use the microphone.
    fn poll_silence(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        open.levels = open.status.take_levels();
        if open.levels.iter().any(|level| *level != 0.0) {
            open.silence = Silence::Heard;
        }
        match open.silence {
            Silence::Since(since) if since.elapsed() >= SILENT_FOR => {
                open.silence = Silence::Told;
                true
            }
            _ => false,
        }
    }

    /// Whether the device just went away or stopped: true once, on the poll that sees it.
    fn went_away(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let now = open.status.is_gone() && !open.gone_told;
        open.gone_told |= now;
        self.failed |= now;
        now
    }

    fn is_gone(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.status.is_gone())
    }

    /// Sends a command to the recorder, or keeps it until the input is open.
    pub(super) fn send(&mut self, command: RecorderCommand) {
        match &self.open {
            Some(open) => {
                open.commands.send(command).ok();
            }
            None => self.commands.push(command),
        }
    }
}

impl Input {
    /// One poll of the audio input: the opening that came back, its level for the meters, a
    /// silence that says what is wrong, a device that went away, and what the recorder said.
    pub(super) fn poll_audio(&mut self, project: &mut Project) {
        while let Some(opened) = self.audio.take_opened(project.assets()) {
            self.input_opened(project, opened);
        }
        if self.audio.poll_silence() {
            self.happened.push(Happened::Notice(SILENCE.to_string()));
        }
        if self.audio.went_away() {
            self.input_gone();
        }
        let reports = self.audio.take_reports();
        if !reports.is_empty() {
            self.on_reports(project, reports);
        }
    }

    /// The input opened, or why not. With no input, nothing is armed and no take gets sound.
    fn input_opened(
        &mut self,
        project: &mut Project,
        opened: Result<(LiveInput, Option<String>), DeviceError>,
    ) {
        match opened {
            Ok((live, notice)) => {
                self.happened.extend(notice.map(Happened::Notice));
                let plays = live.sample_rate() == project.sample_rate();
                // The engine reads it while the input is open, whether a connection hears it
                // or not, so it never falls behind.
                project.set_live_input(Some(live));
                // Opened for a connection alone, an input at another rate plays nothing. The
                // problem of the connection says why, and the device closes. A take records at
                // any rate.
                if !plays && self.armed.is_empty() && !self.hold.audio {
                    self.audio.give_up();
                }
            }
            Err(error) => {
                self.happened.push(Happened::Notice(error.to_string()));
                // A take that waited for it ends with no sound: its notes are kept.
                if self.hold.audio {
                    self.on_reports(project, vec![RecorderReport::NoInput]);
                }
                // Armed with no input would show a level that never moves.
                self.armed.clear();
            }
        }
    }

    /// The input is open while something is armed, a take records sound or a `project.json`
    /// connection in the graph hears the device input, and closed otherwise, so the device is
    /// not held and macOS shows no microphone in use. One that failed or went away is closed
    /// and not opened again here until something new wants it: the next arming, or a
    /// connection that comes, opens the input there is then.
    pub(super) fn follow_input(&mut self, project: &mut Project) {
        let armed: Vec<InstanceId> = self.armed.iter().cloned().collect();
        let heard = project.hears_device_input();
        if self.audio.wants(armed, heard) && !self.audio.is_gone() {
            self.audio.open();
        } else if !self.hold.audio && self.audio.is_open_or_opening() {
            self.audio.close();
            project.set_live_input(None);
        }
    }

    /// The input went away, such as an interface that was unplugged. Everything is disarmed:
    /// arming again opens the default input the system has then. A take that records sound
    /// ends and says so itself; the recorder writes the last of it before the input closes.
    fn input_gone(&mut self) {
        if !self.hold.audio {
            self.happened
                .push(Happened::Notice("The audio input went away.".to_string()));
        }
        self.armed.clear();
    }
}
