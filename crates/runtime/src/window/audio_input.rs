//! The audio input of the window: open while a track is armed or a take records, its level for
//! the meters, and the recorder that writes the takes, which runs on the background executor
//! and never on the thread that draws.

use std::rc::Rc;

use sound_core::{Assets, CaptureReader, CaptureStatus, DeviceError, InputDevice, InputStream};

use crate::recorder::{Recorder, RecorderCommand, RecorderReport};

/// An input the window opened: the stream of the device, which stops when it is dropped, and
/// the reader of what it captures. A test gives one with no device and writes into it itself.
pub struct OpenedInput {
    pub stream: Option<InputStream>,
    pub reader: CaptureReader,
}

/// How the window opens its input: the default input of the system, or a simulated one.
pub type OpenInput = Rc<dyn Fn() -> Result<OpenedInput, DeviceError>>;

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

struct Open {
    _stream: Option<InputStream>,
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
    /// What waits for the recorder's next run.
    commands: Vec<RecorderCommand>,
    openings: u64,
}

impl AudioInput {
    pub(super) fn new(open_input: Option<OpenInput>) -> Self {
        Self {
            open_input,
            open: None,
            commands: Vec::new(),
            openings: 0,
        }
    }

    /// Whether this window has an input to open at all.
    pub(super) fn can_open(&self) -> bool {
        self.open_input.is_some()
    }

    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the input, when it is not open. Gives its channel count.
    pub(super) fn open(&mut self, assets: &Assets) -> Result<usize, DeviceError> {
        if let Some(open) = &self.open {
            return Ok(open.status.channels());
        }
        let opener = self.open_input.as_ref().ok_or(DeviceError::NoInputDevice)?;
        let opened = opener()?;
        let status = opened.reader.status();
        let channels = status.channels();
        self.openings += 1;
        self.open = Some(Open {
            generation: self.openings,
            _stream: opened.stream,
            status,
            sample_rate: opened.reader.sample_rate(),
            recorder: Some(Recorder::new(opened.reader, assets.clone())),
        });
        Ok(channels)
    }

    /// Stops the device. A recorder out on a run is dropped when it comes back.
    pub(super) fn close(&mut self) {
        self.open = None;
        self.commands.clear();
    }

    pub(super) fn sample_rate(&self) -> Option<u32> {
        self.open.as_ref().map(|open| open.sample_rate)
    }

    /// The loudest sample of each channel since the last call, while the input is open.
    pub(super) fn take_levels(&self) -> Option<Vec<f32>> {
        Some(self.open.as_ref()?.status.take_levels())
    }

    /// The device went away, or stopped.
    pub(super) fn is_gone(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.status.is_gone())
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
