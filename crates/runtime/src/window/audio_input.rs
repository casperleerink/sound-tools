//! The audio input of the window: open while a track is armed, a take records or `project.json`
//! connects the device input, its level for the meters, and the recorder that writes the takes.
//! The input is opened and the recorder runs on the background executor, never on the thread
//! that draws: opening a device can take a while, and the first time macOS asks the composer
//! whether the app may use the microphone. It closes there too, see
//! [`super::drop_in_background`].

use std::sync::Arc;

use gpui::App;
use sound_core::{
    Assets, CaptureReader, CaptureStatus, DeviceError, InputDevice, InstanceId, LiveInput,
};

use super::drop_in_background;

use crate::recorder::{Recorder, RecorderCommand, RecorderReport};

/// An input the window opened: the stream of the device, which stops when it is dropped, the
/// reader of what it captures, and the live input for the engine. A test gives one with no
/// device and writes into it itself.
pub struct OpenedInput {
    pub stream: Option<Box<dyn Send>>,
    pub reader: CaptureReader,
    pub live: LiveInput,
    /// Says so when this is not the input the composer chose: that one is not there or did not
    /// open.
    pub notice: Option<String>,
}

/// How the window opens its input: an input device of the system, or a simulated one. It is
/// called on a background thread.
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

/// Polls of the window, about 2 s, after which an input that gave nothing but exact silence is
/// likely one macOS does not let the app hear.
const SILENT_POLLS: u32 = 125;

struct Open {
    _stream: Option<Box<dyn Send>>,
    /// Polls in a row whose level was exactly zero on every channel, and whether that was told.
    silent_polls: u32,
    /// The input went away, and that was told.
    gone_told: bool,
    status: CaptureStatus,
    sample_rate: u32,
    /// The recorder, while no background task has it.
    recorder: Option<Recorder>,
    /// Which opening of the input this is, so a recorder that comes back from a run after the
    /// input was closed and opened again is not taken for the new one.
    generation: u64,
}

pub(crate) struct AudioInput {
    open_input: Option<OpenInput>,
    open: Option<Open>,
    /// The opening on its way, while one is. A close forgets it, and what it opens is dropped.
    opening: Option<u64>,
    /// What waits for the recorder's next run. A start waits here while the input opens.
    commands: Vec<RecorderCommand>,
    openings: u64,
    /// The last opening failed, the input went away, or it was given up because it plays
    /// nothing. It is not opened again until something new wants it, see [`Self::wants`].
    failed: bool,
    /// What wanted it at the last [`Self::wants`].
    armed: Vec<InstanceId>,
    heard: bool,
}

impl AudioInput {
    pub(crate) fn new(open_input: Option<OpenInput>) -> Self {
        Self {
            open_input,
            open: None,
            opening: None,
            commands: Vec::new(),
            openings: 0,
            failed: false,
            armed: Vec::new(),
            heard: false,
        }
    }

    /// Whether the input is to be open for the armed tracks and whether a `project.json`
    /// connection hears it. After a failure it stays closed until something new wants it: a
    /// track armed that was not, or a connection that was not there. Else a failure that
    /// disarms every track would open it again, and fail again, at once.
    pub(crate) fn wants(&mut self, armed: Vec<InstanceId>, heard: bool) -> bool {
        let new = (heard && !self.heard) || armed.iter().any(|track| !self.armed.contains(track));
        self.failed &= !new;
        self.heard = heard;
        self.armed = armed;
        (heard || !self.armed.is_empty()) && !self.failed
    }

    /// Closes an input that was opened for a connection alone and plays nothing, because it runs
    /// at another rate than the output. It is not opened again until something new wants it.
    pub(crate) fn give_up(&mut self, cx: &App) {
        self.close(cx);
        self.failed = true;
    }

    /// Whether this window has an input to open at all.
    pub(crate) fn can_open(&self) -> bool {
        self.open_input.is_some()
    }

    /// Whether the input is open or on its way.
    pub(crate) fn is_open_or_opening(&self) -> bool {
        self.open.is_some() || self.opening.is_some()
    }

    /// What opens the input, for a background thread, and which opening this is. `None` when it
    /// is open or on its way, or when this window has no input.
    pub(crate) fn start_opening(&mut self) -> Option<(OpenInput, u64)> {
        if self.is_open_or_opening() {
            return None;
        }
        let opener = self.open_input.clone()?;
        self.openings += 1;
        self.opening = Some(self.openings);
        Some((opener, self.openings))
    }

    /// The input opening `generation` opened, or why not. Gives its channel count and its live
    /// input for the engine, or `None` for an opening that was given up since: its input stops
    /// again.
    pub(crate) fn opened(
        &mut self,
        generation: u64,
        opened: Result<OpenedInput, DeviceError>,
        assets: &Assets,
        cx: &App,
    ) -> Option<Result<LiveInput, DeviceError>> {
        if self.opening != Some(generation) {
            if let Ok(input) = opened {
                drop_in_background(input, cx);
            }
            return None;
        }
        self.opening = None;
        let OpenedInput {
            stream,
            reader,
            live,
            ..
        } = match opened {
            Ok(opened) => opened,
            Err(error) => {
                self.commands.clear();
                self.failed = true;
                return Some(Err(error));
            }
        };
        let status = reader.status();
        self.failed = false;
        self.open = Some(Open {
            generation,
            silent_polls: 0,
            gone_told: false,
            _stream: stream,
            status,
            sample_rate: reader.sample_rate(),
            recorder: Some(Recorder::new(reader, assets.clone())),
        });
        Some(Ok(live))
    }

    /// Whether the open input went away, such as an interface that was unplugged.
    pub(crate) fn is_gone(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.status.is_gone())
    }

    /// Stops the device, or forgets the opening on its way. A recorder out on a run is dropped
    /// when it comes back.
    pub(crate) fn close(&mut self, cx: &App) {
        if let Some(open) = self.open.take() {
            drop_in_background(open, cx);
        }
        self.opening = None;
        self.commands.clear();
    }

    pub(crate) fn sample_rate(&self) -> Option<u32> {
        self.open.as_ref().map(|open| open.sample_rate)
    }

    /// Whether the open input just reached about two seconds of nothing but exact silence, once
    /// per opening. A real input is never exactly silent: macOS gives zeros to an app it does not
    /// let use the microphone.
    pub(crate) fn poll_silence(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let levels = open.status.take_levels();
        match levels.iter().all(|level| *level == 0.0) {
            true => open.silent_polls = open.silent_polls.saturating_add(1),
            false => open.silent_polls = u32::MAX,
        }
        open.silent_polls == SILENT_POLLS
    }

    /// Whether the device just went away or stopped: true once, on the poll that sees it.
    pub(crate) fn went_away(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let now = open.status.is_gone() && !open.gone_told;
        open.gone_told |= now;
        self.failed |= now;
        now
    }

    pub(crate) fn send(&mut self, command: RecorderCommand) {
        self.commands.push(command);
    }

    /// The recorder and what it is to do, for a run on the background executor, when it is
    /// here: at most one run at a time, so the takes are written in order.
    pub(crate) fn start_run(&mut self) -> Option<Run> {
        let open = self.open.as_mut()?;
        let recorder = open.recorder.take()?;
        Some(Run {
            recorder,
            commands: std::mem::take(&mut self.commands),
            generation: open.generation,
        })
    }

    /// The recorder is back from its run. One of an input closed since goes with it.
    pub(crate) fn end_run(&mut self, recorder: Recorder, generation: u64) {
        if let Some(open) = &mut self.open
            && open.generation == generation
        {
            open.recorder = Some(recorder);
        }
    }
}

/// One run of the recorder on the background executor.
pub(crate) struct Run {
    recorder: Recorder,
    commands: Vec<RecorderCommand>,
    generation: u64,
}

impl Run {
    /// Reads and writes files: never on the thread that draws. Gives the recorder back with
    /// what it reported and which opening of the input it belongs to.
    pub(crate) fn run(mut self) -> (Recorder, Vec<RecorderReport>, u64) {
        let reports = self.recorder.run(self.commands);
        (self.recorder, reports, self.generation)
    }
}
