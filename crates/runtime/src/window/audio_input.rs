//! The audio input of the window: open while a track is armed or a take records, its level for
//! the meters, and the recorder that writes the takes. The input is opened and the recorder
//! runs on the background executor, never on the thread that draws: opening a device can take
//! a while, and the first time macOS asks the composer whether the app may use the microphone.

use std::sync::Arc;

use sound_core::{Assets, CaptureReader, CaptureStatus, DeviceError, InputDevice, InputStream};

use crate::recorder::{Recorder, RecorderCommand, RecorderReport};

/// An input the window opened: the stream of the device, which stops when it is dropped, and
/// the reader of what it captures. A test gives one with no device and writes into it itself.
pub struct OpenedInput {
    pub stream: Option<InputStream>,
    pub reader: CaptureReader,
}

/// How the window opens its input: the default input of the system, or a simulated one. It is
/// called on a background thread.
pub type OpenInput = Arc<dyn Fn() -> Result<OpenedInput, DeviceError> + Send + Sync>;

/// The default input of the system, as set in macOS. No choice of device in the window.
pub fn default_input() -> Result<OpenedInput, DeviceError> {
    let device = InputDevice::default_input()?;
    let name = device.name()?;
    let (stream, reader) = device.start()?;
    println!(
        "audio in: {name}, {} Hz, {} channels",
        reader.sample_rate(),
        reader.channels()
    );
    Ok(OpenedInput {
        stream: Some(stream),
        reader,
    })
}

/// Polls of the window, about 2 s, after which an input that gave nothing but exact silence is
/// likely one macOS does not let the app hear.
const SILENT_POLLS: u32 = 125;

struct Open {
    _stream: Option<InputStream>,
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

pub(super) struct AudioInput {
    open_input: Option<OpenInput>,
    open: Option<Open>,
    /// The opening on its way, while one is. A close forgets it, and what it opens is dropped.
    opening: Option<u64>,
    /// What waits for the recorder's next run. A start waits here while the input opens.
    commands: Vec<RecorderCommand>,
    openings: u64,
}

impl AudioInput {
    pub(super) fn new(open_input: Option<OpenInput>) -> Self {
        Self {
            open_input,
            open: None,
            opening: None,
            commands: Vec::new(),
            openings: 0,
        }
    }

    /// Whether this window has an input to open at all.
    pub(super) fn can_open(&self) -> bool {
        self.open_input.is_some()
    }

    /// Whether the input is open or on its way.
    pub(super) fn is_open_or_opening(&self) -> bool {
        self.open.is_some() || self.opening.is_some()
    }

    /// What opens the input, for a background thread, and which opening this is. `None` when it
    /// is open or on its way, or when this window has no input.
    pub(super) fn start_opening(&mut self) -> Option<(OpenInput, u64)> {
        if self.is_open_or_opening() {
            return None;
        }
        let opener = self.open_input.clone()?;
        self.openings += 1;
        self.opening = Some(self.openings);
        Some((opener, self.openings))
    }

    /// The input opening `generation` opened, or why not. Gives its channel count, or `None`
    /// for an opening that was given up since: its input stops again.
    pub(super) fn opened(
        &mut self,
        generation: u64,
        opened: Result<OpenedInput, DeviceError>,
        assets: &Assets,
    ) -> Option<Result<usize, DeviceError>> {
        if self.opening != Some(generation) {
            return None;
        }
        self.opening = None;
        let opened = match opened {
            Ok(opened) => opened,
            Err(error) => {
                self.commands.clear();
                return Some(Err(error));
            }
        };
        let status = opened.reader.status();
        let channels = status.channels();
        self.open = Some(Open {
            generation,
            silent_polls: 0,
            gone_told: false,
            _stream: opened.stream,
            status,
            sample_rate: opened.reader.sample_rate(),
            recorder: Some(Recorder::new(opened.reader, assets.clone())),
        });
        Some(Ok(channels))
    }

    /// Stops the device, or forgets the opening on its way. A recorder out on a run is dropped
    /// when it comes back.
    pub(super) fn close(&mut self) {
        self.open = None;
        self.opening = None;
        self.commands.clear();
    }

    pub(super) fn sample_rate(&self) -> Option<u32> {
        self.open.as_ref().map(|open| open.sample_rate)
    }

    /// The loudest sample of each channel since the last call, while the input is open, and
    /// whether it just reached about two seconds of nothing but exact silence, once per
    /// opening. A real input is never exactly silent: macOS gives zeros to an app it does not
    /// let use the microphone.
    pub(super) fn poll_levels(&mut self) -> Option<(Vec<f32>, bool)> {
        let open = self.open.as_mut()?;
        let levels = open.status.take_levels();
        match levels.iter().all(|level| *level == 0.0) {
            true => open.silent_polls = open.silent_polls.saturating_add(1),
            false => open.silent_polls = u32::MAX,
        }
        Some((levels, open.silent_polls == SILENT_POLLS))
    }

    /// Whether the device just went away or stopped: true once, on the poll that sees it.
    pub(super) fn went_away(&mut self) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let now = open.status.is_gone() && !open.gone_told;
        open.gone_told |= now;
        now
    }

    pub(super) fn send(&mut self, command: RecorderCommand) {
        self.commands.push(command);
    }

    /// The recorder and what it is to do, for a run on the background executor, when it is
    /// here: at most one run at a time, so the takes are written in order.
    pub(super) fn start_run(&mut self) -> Option<Run> {
        let open = self.open.as_mut()?;
        let recorder = open.recorder.take()?;
        Some(Run {
            recorder,
            commands: std::mem::take(&mut self.commands),
            generation: open.generation,
        })
    }

    /// The recorder is back from its run. One of an input closed since goes with it.
    pub(super) fn end_run(&mut self, recorder: Recorder, generation: u64) {
        if let Some(open) = &mut self.open
            && open.generation == generation
        {
            open.recorder = Some(recorder);
        }
    }
}

/// One run of the recorder on the background executor.
pub(super) struct Run {
    recorder: Recorder,
    commands: Vec<RecorderCommand>,
    generation: u64,
}

impl Run {
    /// Reads and writes files: never on the thread that draws. Gives the recorder back with
    /// what it reported and which opening of the input it belongs to.
    pub(super) fn run(mut self) -> (Recorder, Vec<RecorderReport>, u64) {
        let reports = self.recorder.run(self.commands);
        (self.recorder, reports, self.generation)
    }
}
