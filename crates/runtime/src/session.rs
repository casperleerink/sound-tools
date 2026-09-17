#[path = "midi.rs"]
mod midi;

use crate::{open_or_create, register_tools};
use sound_core::{
    Error, Result,
    clock::Transport,
    device::DeviceOutput,
    project::Project,
    registry::{Registry, Tool},
};
use sound_daw::{
    arrangement::{Arrangement, validate_arrangement},
    engine::{BLOCK_FRAMES, Engine, MixerState},
    sample_asset::SampleAsset,
};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const COMMAND_CAPACITY: usize = 32;
const WORK_INTERVAL: Duration = Duration::from_millis(2);
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEVICE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub name: String,
    pub revision: u64,
    pub arrangement: Arrangement,
    pub master_gain: f32,
    pub frame: u64,
    pub playing: bool,
    pub peak: f32,
    pub underruns: u64,
    pub audio_available: bool,
    pub midi_ports: Vec<String>,
    pub midi_input: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Command {
    TogglePlay,
    Stop,
    Seek(u64),
    SetArrangement {
        arrangement: Arrangement,
        expected_revision: u64,
    },
    SetMasterGain(f32),
    RefreshMidiPorts,
    ConnectMidi {
        port: usize,
        track: String,
    },
    DisconnectMidi,
    Undo,
    Redo,
}

struct Envelope {
    command: Command,
    reply: SyncSender<Result<()>>,
}

pub struct Session {
    commands: SyncSender<Envelope>,
    shutdown: Option<SyncSender<()>>,
    worker: Option<JoinHandle<()>>,
    snapshot: Arc<Mutex<Snapshot>>,
}

impl Session {
    pub fn open(root: &str) -> Result<Self> {
        let mut backend = Backend::open(root)?;
        let _ = backend.command(Command::RefreshMidiPorts);
        Self::start(backend, Output::open)
    }

    fn start(
        mut backend: Backend,
        open_output: impl FnOnce() -> Result<Output> + Send + 'static,
    ) -> Result<Self> {
        let snapshot = Arc::new(Mutex::new(backend.snapshot()));
        let shared = Arc::clone(&snapshot);
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (shutdown, stopped) = mpsc::sync_channel(0);
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("sound-session".into())
            .spawn(move || {
                let output = match open_output() {
                    Ok(output) => {
                        backend.view.audio_available = true;
                        Some(output)
                    }
                    Err(error) => {
                        backend.audio_failed(error);
                        None
                    }
                };
                backend.publish(&shared);
                if ready.send(()).is_ok() {
                    run_worker(backend, output, receiver, stopped, shared);
                }
            })?;
        let session = Self {
            commands,
            shutdown: Some(shutdown),
            worker: Some(worker),
            snapshot,
        };
        started
            .recv()
            .map_err(|_| Error("Session worker failed during startup".into()))?;
        Ok(session)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn send(&self, command: Command) -> Result<Receiver<Result<()>>> {
        let (reply, receipt) = mpsc::sync_channel(1);
        self.commands
            .try_send(Envelope { command, reply })
            .map_err(|error| {
                Error(
                    match error {
                        TrySendError::Full(_) => "Session command queue is full",
                        TrySendError::Disconnected(_) => "Session worker has stopped",
                    }
                    .into(),
                )
            })?;
        Ok(receipt)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Backend {
    project: Project,
    engine: Engine,
    transport: Transport,
    arrangement_tool: Tool<Arrangement>,
    mixer_tool: Tool<MixerState>,
    view: Snapshot,
    audio_error: Option<String>,
    midi: midi::Input,
}

impl Backend {
    fn open(root: &str) -> Result<Self> {
        let mut registry = Registry::default();
        let (arrangement_tool, mixer_tool) = register_tools(&mut registry)?;
        let registry = Arc::new(registry);
        let project = open_or_create(root, Arc::clone(&registry), arrangement_tool, mixer_tool)?;
        let mut project = if project.revision() > 0 {
            Project::open(root, registry)?
        } else {
            project
        };
        let asset_root = project.root().to_path_buf();
        project.set_validator(move |records| {
            let record = records
                .get("arrangement")
                .ok_or_else(|| Error("Missing arrangement record".into()))?;
            if record.tool != arrangement_tool.name() {
                return Err(Error("Arrangement has the wrong tool".into()));
            }
            if records
                .get("mixer")
                .is_some_and(|record| record.tool != mixer_tool.name())
            {
                return Err(Error("Mixer has the wrong tool".into()));
            }
            let arrangement: Arrangement = serde_json::from_value(record.state.clone())?;
            validate_timeline(&arrangement)?;
            for track in &arrangement.tracks {
                if track.instrument == "daw.sampler" {
                    let config = track.sampler.as_ref().ok_or_else(|| {
                        Error("Sampler track needs a sample configuration".into())
                    })?;
                    SampleAsset::load(&asset_root, &config.asset)?;
                }
            }
            Ok(())
        })?;
        let arrangement = project.read("arrangement", arrangement_tool)?;
        validate_timeline(&arrangement)?;
        let mixer = if project.records().contains_key("mixer") {
            project.read("mixer", mixer_tool)?
        } else {
            MixerState::default()
        };
        let engine = Engine::build(&project)?;
        let view = Snapshot {
            name: project.manifest().name.clone(),
            revision: project.revision(),
            arrangement,
            master_gain: mixer.master_gain,
            frame: 0,
            playing: false,
            peak: 0.0,
            underruns: 0,
            audio_available: false,
            midi_ports: Vec::new(),
            midi_input: None,
            error: None,
        };
        Ok(Self {
            project,
            engine,
            transport: Transport::default(),
            arrangement_tool,
            mixer_tool,
            view,
            audio_error: None,
            midi: midi::Input::default(),
        })
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            frame: self.transport.project_frame,
            playing: self.transport.playing && self.view.audio_available,
            error: self.view.error.clone().or_else(|| self.audio_error.clone()),
            ..self.view.clone()
        }
    }

    fn publish(&mut self, shared: &Mutex<Snapshot>) {
        *shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = self.snapshot();
        self.view.peak = 0.0;
    }

    fn command(&mut self, command: Command) -> Result<()> {
        let result = self.apply_command(command);
        self.view.error = result.as_ref().err().map(ToString::to_string);
        result
    }

    fn apply_command(&mut self, command: Command) -> Result<()> {
        if matches!(
            command,
            Command::SetArrangement { .. }
                | Command::SetMasterGain(_)
                | Command::Undo
                | Command::Redo
        ) && self.poll_project()?
        {
            return Err(Error(
                "Project changed externally. Review the live state and retry.".into(),
            ));
        }
        let revision = self.project.revision();
        let transport = self.transport;
        match command {
            Command::TogglePlay if self.transport.playing => {
                self.transport.pause();
                self.view.peak = 0.0;
            }
            Command::TogglePlay => {
                if !self.view.audio_available {
                    return Err(Error(
                        self.audio_error
                            .clone()
                            .unwrap_or_else(|| "Audio output is unavailable".into()),
                    ));
                }
                self.transport.play();
            }
            Command::Stop => {
                self.transport.stop();
                self.view.peak = 0.0;
            }
            Command::Seek(frame) => {
                if frame.checked_add(BLOCK_FRAMES as u64).is_none() {
                    return Err(Error("Seek frame is too large".into()));
                }
                self.transport.seek(frame);
                self.view.peak = 0.0;
            }
            Command::SetArrangement {
                arrangement,
                expected_revision,
            } => {
                if expected_revision != revision {
                    return Err(Error(
                        "Project revision changed. Review the live state and retry.".into(),
                    ));
                }
                validate_timeline(&arrangement)?;
                validate_arrangement(&arrangement)?;
                self.project
                    .replace("arrangement", self.arrangement_tool, &arrangement)?;
            }
            Command::SetMasterGain(master_gain) => {
                if !(-2.0..=2.0).contains(&master_gain) {
                    return Err(Error("Master gain must be -2..2".into()));
                }
                let mixer = MixerState { master_gain };
                if self.project.records().contains_key("mixer") {
                    self.project.replace("mixer", self.mixer_tool, &mixer)?;
                } else {
                    self.project
                        .insert("mixer", self.mixer_tool, &mixer, None)?;
                }
            }
            Command::RefreshMidiPorts => {
                self.view.midi_ports.clear();
                let result = self.midi.refresh();
                if self.midi.port_missing() {
                    self.disconnect_midi();
                }
                self.view.midi_ports = result?;
            }
            Command::ConnectMidi { port, track } => {
                if !self
                    .view
                    .arrangement
                    .tracks
                    .iter()
                    .any(|candidate| candidate.name == track)
                {
                    return Err(Error(format!("MIDI target track does not exist: {track}")));
                }
                self.disconnect_midi();
                self.view.midi_input = Some(self.midi.connect(port, track)?);
            }
            Command::DisconnectMidi => self.disconnect_midi(),
            Command::Undo => {
                self.project.undo()?;
            }
            Command::Redo => {
                self.project.redo()?;
            }
        }
        if transport != self.transport {
            if let Some(queue) = &self.midi.queue {
                queue.drain(|_| {});
            }
            self.engine.all_notes_off();
        }
        if revision != self.project.revision() {
            self.refresh()?;
        }
        Ok(())
    }

    fn disconnect_midi(&mut self) {
        self.midi.disconnect();
        self.view.midi_input = None;
        self.engine.all_notes_off();
    }

    fn drain_midi(&mut self) {
        if let (Some(queue), Some(track)) = (&self.midi.queue, &self.midi.track) {
            queue.drain(|event| match event {
                midi::Event::Note { key, velocity, on } if self.view.audio_available => {
                    self.engine.queue_note(track, key, velocity, on);
                }
                midi::Event::Note { .. } => {}
                midi::Event::AllNotesOff => self.engine.all_notes_off(),
            });
        }
    }

    fn refresh(&mut self) -> Result<()> {
        let arrangement = self.project.read("arrangement", self.arrangement_tool)?;
        validate_timeline(&arrangement)?;
        let mixer = if self.project.records().contains_key("mixer") {
            self.project.read("mixer", self.mixer_tool)?
        } else {
            MixerState::default()
        };
        let rebuilt = self.engine.apply_project(&self.project)?;
        let notes_changed = arrangement
            .tracks
            .iter()
            .zip(&self.view.arrangement.tracks)
            .any(|(next, old)| {
                next.clips != old.clips || next.muted != old.muted || next.soloed != old.soloed
            });
        if (rebuilt || notes_changed)
            && let Some(queue) = &self.midi.queue
        {
            queue.drain(|_| {});
            self.engine.all_notes_off();
        }
        if self
            .midi
            .track
            .as_ref()
            .is_some_and(|target| !arrangement.tracks.iter().any(|track| &track.name == target))
        {
            self.disconnect_midi();
        }
        self.view.name = self.project.manifest().name.clone();
        self.view.revision = self.project.revision();
        self.view.arrangement = arrangement;
        self.view.master_gain = mixer.master_gain;
        Ok(())
    }

    fn poll_project(&mut self) -> Result<bool> {
        let changed = self.project.poll()?;
        if changed {
            self.refresh()?;
        }
        Ok(changed)
    }

    fn poll(&mut self) -> bool {
        match self.poll_project() {
            Ok(true) => {
                self.view.error = None;
                true
            }
            Ok(false) => false,
            Err(error) => {
                self.view.error = Some(error.to_string());
                true
            }
        }
    }

    fn audio_failed(&mut self, error: Error) {
        self.engine.all_notes_off();
        self.transport.pause();
        self.view.audio_available = false;
        self.view.peak = 0.0;
        self.audio_error = Some(error.to_string());
    }

    fn render(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) -> Result<()> {
        if self.transport.playing
            && self
                .transport
                .project_frame
                .checked_add(BLOCK_FRAMES as u64)
                .is_none()
        {
            return Err(Error("Playback reached the maximum frame".into()));
        }
        left.clear();
        right.clear();
        self.engine
            .render_block_into(&mut self.transport, &self.view.arrangement, left, right)?;
        self.view.peak = left
            .iter()
            .chain(right.iter())
            .fold(self.view.peak, |peak, sample| peak.max(sample.abs()));
        Ok(())
    }
}

fn validate_timeline(arrangement: &Arrangement) -> Result<()> {
    for track in &arrangement.tracks {
        for clip in &track.clips {
            if clip.start.checked_add(clip.length).is_none()
                || clip.notes.iter().any(|note| {
                    note.start
                        .checked_add(note.length)
                        .and_then(|end| clip.start.checked_add(end))
                        .is_none()
                })
            {
                return Err(Error("Arrangement frame range is too large".into()));
            }
        }
    }
    Ok(())
}

struct Output {
    device: DeviceOutput,
    served: u64,
    last_progress: Instant,
}

impl Output {
    fn open() -> Result<Self> {
        let mut device = DeviceOutput::open()?;
        let capacity = device.free_frames();
        let silence = vec![0.0; capacity];
        device.write(&silence, &silence)?;
        device.start()?;
        Ok(Self {
            device,
            served: 0,
            last_progress: Instant::now(),
        })
    }

    fn service(
        &mut self,
        backend: &mut Backend,
        left: &mut Vec<f32>,
        right: &mut Vec<f32>,
    ) -> Result<()> {
        let served = self.device.frames_served();
        backend.view.underruns = self.device.underruns();
        if served != self.served {
            self.served = served;
            self.last_progress = Instant::now();
        } else if self.last_progress.elapsed() >= DEVICE_TIMEOUT {
            return Err(Error("Audio output stopped draining its buffer".into()));
        }
        let free = self.device.free_frames();
        let blocks = free / BLOCK_FRAMES;
        for _ in 0..blocks {
            backend.render(left, right)?;
            self.device.write(left, right)?;
        }
        Ok(())
    }
}

fn run_worker(
    mut backend: Backend,
    mut output: Option<Output>,
    commands: Receiver<Envelope>,
    stopped: Receiver<()>,
    shared: Arc<Mutex<Snapshot>>,
) {
    let mut last_poll = Instant::now();
    let mut last_snapshot = Instant::now();
    let mut left = Vec::with_capacity(BLOCK_FRAMES);
    let mut right = Vec::with_capacity(BLOCK_FRAMES);
    loop {
        if matches!(stopped.try_recv(), Err(TryRecvError::Disconnected)) {
            break;
        }
        let mut changed = false;
        match commands.recv_timeout(WORK_INTERVAL) {
            Ok(envelope) => {
                let result = backend.command(envelope.command);
                backend.publish(&shared);
                last_snapshot = Instant::now();
                let _ = envelope.reply.send(result);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_poll.elapsed() >= POLL_INTERVAL {
            changed |= backend.poll();
            last_poll = Instant::now();
        }
        backend.drain_midi();
        if let Some(device) = output.as_mut()
            && let Err(error) = device.service(&mut backend, &mut left, &mut right)
        {
            backend.audio_failed(error);
            output = None;
            changed = true;
        }
        if changed || last_snapshot.elapsed() >= SNAPSHOT_INTERVAL {
            backend.publish(&shared);
            last_snapshot = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sound_core::project::atomic_write;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct TestProject(PathBuf);

    impl TestProject {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "sound-session-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            )))
        }

        fn open(&self) -> Backend {
            Backend::open(self.0.to_str().unwrap()).unwrap()
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn attach_midi(backend: &mut Backend) -> midi::Sender {
        let (sender, queue) = midi::Queue::new();
        backend.midi.queue = Some(queue);
        backend.midi.track = Some("Lead".into());
        backend.view.midi_input = Some("Test input".into());
        backend.view.audio_available = true;
        sender
    }

    fn render_peak(backend: &mut Backend) -> f32 {
        let mut left = Vec::new();
        let mut right = Vec::new();
        backend.render(&mut left, &mut right).unwrap();
        left.iter()
            .chain(&right)
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    #[test]
    fn live_midi_is_audible_stopped_and_disconnect_discards_pending_notes() {
        let root = TestProject::new();
        let mut backend = root.open();
        let mut sender = attach_midi(&mut backend);
        sender.receive(&[0x90, 60, 100]);
        backend.drain_midi();
        assert!(render_peak(&mut backend) > 0.01);
        assert_eq!(backend.snapshot().frame, 0);
        assert!(!backend.snapshot().playing);
        assert!(backend.snapshot().peak > 0.01);
        assert!(backend.command(Command::Seek(u64::MAX)).is_err());
        assert!(render_peak(&mut backend) > 0.01);
        let mut arrangement = backend.view.arrangement.clone();
        arrangement.tracks[0].gain = 0.5;
        backend
            .command(Command::SetArrangement {
                arrangement,
                expected_revision: backend.snapshot().revision,
            })
            .unwrap();
        assert!(render_peak(&mut backend) > 0.01);
        sender.receive(&[0x90, 62, 100]);
        backend.command(Command::DisconnectMidi).unwrap();
        sender.receive(&[0x90, 64, 100]);
        backend.drain_midi();
        assert_eq!(render_peak(&mut backend), 0.0);
        assert!(backend.midi.queue.is_none());
        assert!(backend.midi.track.is_none());
        assert!(backend.snapshot().midi_input.is_none());
    }

    #[test]
    fn overflow_and_channel_all_notes_off_clear_live_voices() {
        let root = TestProject::new();
        let mut backend = root.open();
        let mut sender = attach_midi(&mut backend);
        for reset in [false, true] {
            sender.receive(&[0x9f, 60, 100]);
            backend.drain_midi();
            assert!(render_peak(&mut backend) > 0.01);
            if reset {
                sender.receive(&[0xbf, 123, 0]);
            } else {
                for _ in 0..midi::CAPACITY {
                    sender.receive(&[0x90, 62, 100]);
                }
                sender.receive(&[0x80, 60, 0]);
            }
            backend.drain_midi();
            assert_eq!(render_peak(&mut backend), 0.0);
        }
    }

    #[test]
    fn track_deletion_disconnects_and_mute_does_not_revive_held_notes() {
        let root = TestProject::new();
        let mut backend = root.open();
        let mut sender = attach_midi(&mut backend);
        sender.receive(&[0x90, 60, 100]);
        backend.drain_midi();
        assert!(render_peak(&mut backend) > 0.01);
        for muted in [true, false] {
            let mut arrangement = backend.view.arrangement.clone();
            arrangement.tracks[0].muted = muted;
            backend
                .command(Command::SetArrangement {
                    arrangement,
                    expected_revision: backend.snapshot().revision,
                })
                .unwrap();
            if muted {
                sender.receive(&[0x90, 62, 100]);
                backend.drain_midi();
            }
            assert_eq!(render_peak(&mut backend), 0.0);
        }
        sender.receive(&[0x90, 60, 100]);
        backend.drain_midi();
        assert!(render_peak(&mut backend) > 0.01);
        backend
            .command(Command::SetArrangement {
                arrangement: Arrangement { tracks: Vec::new() },
                expected_revision: backend.snapshot().revision,
            })
            .unwrap();
        assert!(backend.snapshot().midi_input.is_none());
        assert!(backend.midi.queue.is_none());
        assert_eq!(render_peak(&mut backend), 0.0);
    }

    #[test]
    fn midi_discovery_failure_is_nonfatal_and_explicit() {
        let root = TestProject::new();
        let mut backend = root.open();
        let result = backend.command(Command::RefreshMidiPorts);
        if let Err(error) = result {
            assert!(error.0.contains("MIDI"));
            assert_eq!(backend.snapshot().error.as_deref(), Some(error.0.as_str()));
            assert!(backend.snapshot().midi_ports.is_empty());
        }
        assert!(backend.snapshot().midi_input.is_none());
        backend.command(Command::SetMasterGain(0.5)).unwrap();
        assert_eq!(backend.snapshot().master_gain, 0.5);
    }

    #[test]
    fn invalid_midi_selection_reports_error_without_breaking_session() {
        let root = TestProject::new();
        let mut backend = root.open();
        assert!(
            backend
                .command(Command::ConnectMidi {
                    port: 0,
                    track: "missing".into()
                })
                .is_err()
        );
        assert!(
            backend
                .command(Command::ConnectMidi {
                    port: usize::MAX,
                    track: "Lead".into()
                })
                .is_err()
        );
        assert!(backend.snapshot().midi_input.is_none());
        backend.command(Command::SetMasterGain(0.5)).unwrap();
        backend.command(Command::DisconnectMidi).unwrap();
        assert_eq!(backend.snapshot().master_gain, 0.5);
    }

    #[test]
    fn edits_and_history_are_persisted_and_applied_to_engine() {
        let root = TestProject::new();
        let mut backend = root.open();
        let original = backend.snapshot().arrangement;
        let mut edited = original.clone();
        edited.tracks[0].name = "Changed lead".into();
        backend
            .command(Command::SetArrangement {
                arrangement: edited.clone(),
                expected_revision: backend.snapshot().revision,
            })
            .unwrap();
        backend.command(Command::SetMasterGain(0.0)).unwrap();
        let reopened = root.open().snapshot();
        assert_eq!(reopened.arrangement, edited);
        assert_eq!(reopened.master_gain, 0.0);
        backend.view.audio_available = true;
        backend.command(Command::TogglePlay).unwrap();
        let mut left = Vec::new();
        let mut right = Vec::new();
        backend.render(&mut left, &mut right).unwrap();
        backend.render(&mut left, &mut right).unwrap();
        assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
        backend.view.peak = 0.0;
        backend.command(Command::Undo).unwrap();
        assert_eq!(backend.snapshot().master_gain, 1.0);
        assert_eq!(root.open().snapshot().master_gain, 1.0);
        backend.render(&mut left, &mut right).unwrap();
        assert!(backend.snapshot().peak > 0.0);
        backend.command(Command::Undo).unwrap();
        assert_eq!(root.open().snapshot().arrangement, original);
        backend.command(Command::Redo).unwrap();
        backend.command(Command::Redo).unwrap();
        assert_eq!(root.open().snapshot().arrangement, edited);
        assert_eq!(root.open().snapshot().master_gain, 0.0);
    }

    #[test]
    fn starter_records_are_not_undoable_and_failed_audio_does_not_stop_edits() {
        let root = TestProject::new();
        let mut backend = root.open();
        let original = backend.snapshot().arrangement;
        backend.command(Command::Undo).unwrap();
        backend.command(Command::Undo).unwrap();
        assert_eq!(root.open().snapshot().arrangement, original);
        assert_eq!(root.open().snapshot().master_gain, 1.0);
        backend.view.audio_available = true;
        backend.command(Command::TogglePlay).unwrap();
        backend.audio_failed(Error("Device stalled".into()));
        assert!(!backend.snapshot().playing);
        assert!(!backend.snapshot().audio_available);
        assert_eq!(backend.snapshot().peak, 0.0);
        backend.command(Command::SetMasterGain(0.5)).unwrap();
        assert_eq!(root.open().snapshot().master_gain, 0.5);
        assert_eq!(backend.snapshot().error.as_deref(), Some("Device stalled"));
        assert!(backend.command(Command::TogglePlay).is_err());
        assert!(!backend.snapshot().playing);
    }

    #[test]
    fn transport_renders_only_when_available_and_preserves_pause_and_seek() {
        let root = TestProject::new();
        let mut backend = root.open();
        assert!(backend.command(Command::TogglePlay).is_err());
        assert!(!backend.snapshot().playing);
        assert_eq!(backend.snapshot().frame, 0);
        backend.view.audio_available = true;
        backend.command(Command::TogglePlay).unwrap();
        let mut left = Vec::new();
        let mut right = Vec::new();
        backend.render(&mut left, &mut right).unwrap();
        assert_eq!(backend.snapshot().frame, BLOCK_FRAMES as u64);
        assert!(backend.snapshot().playing);
        assert!(backend.snapshot().peak > 0.0);
        backend.command(Command::TogglePlay).unwrap();
        backend.render(&mut left, &mut right).unwrap();
        assert_eq!(backend.snapshot().frame, BLOCK_FRAMES as u64);
        assert_eq!(backend.snapshot().peak, 0.0);
        assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
        backend.command(Command::Seek(48_000)).unwrap();
        assert!(!backend.snapshot().playing);
        backend.command(Command::TogglePlay).unwrap();
        backend.command(Command::Seek(24_000)).unwrap();
        backend.render(&mut left, &mut right).unwrap();
        assert_eq!(backend.snapshot().frame, 24_000 + BLOCK_FRAMES as u64);
        backend.command(Command::Stop).unwrap();
        assert_eq!(backend.snapshot().frame, 0);
        assert!(!backend.snapshot().playing);
    }

    #[test]
    fn invalid_commands_preserve_live_and_persisted_state() {
        let root = TestProject::new();
        let mut backend = root.open();
        let original = backend.snapshot();
        for gain in [f32::NAN, f32::INFINITY, -3.0, 3.0] {
            assert!(backend.command(Command::SetMasterGain(gain)).is_err());
        }
        let mut invalid = original.arrangement.clone();
        invalid.tracks[0].pan = 2.0;
        assert!(
            backend
                .command(Command::SetArrangement {
                    arrangement: invalid,
                    expected_revision: original.revision,
                })
                .is_err()
        );
        let mut overflow = original.arrangement.clone();
        overflow.tracks[0].clips[0].notes[0].length = u64::MAX;
        overflow.tracks[0].clips[0].notes[0].start = 1;
        assert!(
            backend
                .command(Command::SetArrangement {
                    arrangement: overflow,
                    expected_revision: original.revision,
                })
                .is_err()
        );
        assert!(backend.command(Command::Seek(u64::MAX)).is_err());
        assert!(backend.snapshot().error.is_some());
        assert_eq!(backend.snapshot().frame, 0);
        assert_eq!(backend.snapshot().arrangement, original.arrangement);
        assert_eq!(root.open().snapshot().arrangement, original.arrangement);
        assert_eq!(root.open().snapshot().master_gain, original.master_gain);
        backend.command(Command::SetMasterGain(0.5)).unwrap();
        assert!(backend.snapshot().error.is_none());
        assert_eq!(root.open().snapshot().master_gain, 0.5);
    }

    fn sampler_arrangement(backend: &Backend) -> Arrangement {
        let mut arrangement = backend.view.arrangement.clone();
        arrangement.tracks[0].instrument = "daw.sampler".into();
        arrangement.tracks[0].sampler = Some(sound_daw::sampler::SamplerConfig {
            asset: "test.wav".into(),
            root_key: 60,
        });
        arrangement
    }

    fn restore_sample(root: &TestProject) {
        let bytes = [
            b"RIFF".as_slice(),
            &38u32.to_le_bytes(),
            b"WAVEfmt ",
            &16u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &1u16.to_le_bytes(),
            &48_000u32.to_le_bytes(),
            &96_000u32.to_le_bytes(),
            &2u16.to_le_bytes(),
            &16u16.to_le_bytes(),
            b"data",
            &2u32.to_le_bytes(),
            &16_384i16.to_le_bytes(),
        ]
        .concat();
        fs::write(root.0.join("assets/test.wav"), bytes).unwrap();
    }

    fn assert_unchanged(backend: &Backend, before: &Snapshot) {
        let snapshot = backend.snapshot();
        assert_eq!(snapshot.revision, before.revision);
        assert_eq!(backend.project.revision(), before.revision);
        assert_eq!(snapshot.arrangement, before.arrangement);
        assert_eq!(snapshot.master_gain, before.master_gain);
        assert_eq!(
            backend
                .project
                .read("arrangement", backend.arrangement_tool)
                .unwrap(),
            before.arrangement
        );
    }

    #[test]
    fn missing_sampler_edit_preserves_disk_revision_and_history() {
        let root = TestProject::new();
        let mut backend = root.open();
        backend.command(Command::SetMasterGain(0.5)).unwrap();
        backend.command(Command::Undo).unwrap();
        let before = backend.snapshot();
        let path = root.0.join("state/arrangement.json");
        let bytes = fs::read(&path).unwrap();
        let manifest = fs::read(root.0.join("project.json")).unwrap();
        assert!(
            backend
                .command(Command::SetArrangement {
                    arrangement: sampler_arrangement(&backend),
                    expected_revision: before.revision,
                })
                .is_err()
        );
        assert_unchanged(&backend, &before);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(root.0.join("project.json")).unwrap(), manifest);
        backend.command(Command::Redo).unwrap();
        assert_eq!(backend.snapshot().master_gain, 0.5);
        backend.command(Command::Undo).unwrap();
        let revision = backend.snapshot().revision;
        backend.command(Command::Undo).unwrap();
        assert_eq!(backend.snapshot().revision, revision);
    }

    #[test]
    fn external_missing_sampler_is_retried_after_asset_restoration() {
        let root = TestProject::new();
        let mut backend = root.open();
        let before = backend.snapshot();
        let arrangement = sampler_arrangement(&backend);
        let record = sound_core::project::Record {
            tool: backend.arrangement_tool.name().into(),
            state: serde_json::to_value(&arrangement).unwrap(),
        };
        let path = root.0.join("state/arrangement.json");
        let bytes = serde_json::to_vec(&record).unwrap();
        atomic_write(&path, &bytes).unwrap();
        for _ in 0..2 {
            assert!(backend.poll());
            assert!(backend.snapshot().error.is_some());
            assert_unchanged(&backend, &before);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        restore_sample(&root);
        assert!(backend.poll());
        assert!(backend.snapshot().error.is_none());
        assert_eq!(backend.snapshot().arrangement, arrangement);
        assert_eq!(backend.snapshot().revision, before.revision + 1);
        assert!(!backend.poll());
        backend.command(Command::Undo).unwrap();
        assert_eq!(backend.snapshot().arrangement, before.arrangement);
        let revision = backend.snapshot().revision;
        backend.command(Command::Undo).unwrap();
        assert_eq!(backend.snapshot().revision, revision);
        backend.command(Command::Redo).unwrap();
        assert_eq!(backend.snapshot().arrangement, arrangement);
    }

    #[test]
    fn missing_sampler_history_targets_fail_atomically_and_remain_retryable() {
        for redo in [false, true] {
            let root = TestProject::new();
            let mut backend = root.open();
            let original = backend.snapshot().arrangement;
            restore_sample(&root);
            let arrangement = sampler_arrangement(&backend);
            backend
                .command(Command::SetArrangement {
                    arrangement: arrangement.clone(),
                    expected_revision: backend.snapshot().revision,
                })
                .unwrap();
            if redo {
                backend.command(Command::Undo).unwrap();
            } else {
                backend
                    .command(Command::SetArrangement {
                        arrangement: original.clone(),
                        expected_revision: backend.snapshot().revision,
                    })
                    .unwrap();
            }
            fs::remove_file(root.0.join("assets/test.wav")).unwrap();
            let before = backend.snapshot();
            let path = root.0.join("state/arrangement.json");
            let bytes = fs::read(&path).unwrap();
            let manifest = fs::read(root.0.join("project.json")).unwrap();
            let command = if redo { Command::Redo } else { Command::Undo };
            for _ in 0..2 {
                assert!(backend.command(command.clone()).is_err());
                assert_unchanged(&backend, &before);
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert_eq!(fs::read(root.0.join("project.json")).unwrap(), manifest);
            }
            restore_sample(&root);
            backend.command(command).unwrap();
            assert_eq!(backend.snapshot().arrangement, arrangement);
            assert_eq!(backend.snapshot().revision, before.revision + 1);
            assert_eq!(root.open().snapshot().arrangement, arrangement);
            backend
                .command(if redo { Command::Undo } else { Command::Redo })
                .unwrap();
            assert_eq!(backend.snapshot().arrangement, original);
        }
    }

    #[test]
    fn external_edits_share_history_and_invalid_files_keep_live_state() {
        let root = TestProject::new();
        let mut backend = root.open();
        let path = root.0.join("state/mixer.json");
        atomic_write(
            &path,
            br#"{"tool":"daw.mixer","state":{"master_gain":0.25}}"#,
        )
        .unwrap();
        assert!(backend.poll());
        assert_eq!(backend.snapshot().master_gain, 0.25);
        assert!(!backend.poll());
        backend.command(Command::Undo).unwrap();
        assert_eq!(root.open().snapshot().master_gain, 1.0);
        backend.command(Command::Redo).unwrap();
        atomic_write(&path, br#"{"tool":"daw.mixer","state":{"master_gain":8}}"#).unwrap();
        assert!(backend.poll());
        assert_eq!(backend.snapshot().master_gain, 0.25);
        assert!(backend.snapshot().error.is_some());
        assert!(backend.command(Command::SetMasterGain(0.75)).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            br#"{"tool":"daw.mixer","state":{"master_gain":8}}"#
        );
        atomic_write(
            &path,
            br#"{"tool":"daw.mixer","state":{"master_gain":0.25}}"#,
        )
        .unwrap();
        backend.command(Command::SetMasterGain(0.75)).unwrap();
        assert!(!backend.poll());
        assert_eq!(root.open().snapshot().master_gain, 0.75);
        assert!(backend.snapshot().error.is_none());
    }

    #[test]
    fn persisted_commands_poll_external_edits_before_writing() {
        for command in [Command::SetMasterGain(0.75), Command::Undo, Command::Redo] {
            let root = TestProject::new();
            let mut backend = root.open();
            let original_revision = backend.snapshot().revision;
            let path = root.0.join("state/mixer.json");
            let external = br#"{"tool":"daw.mixer","state":{"master_gain":0.25}}"#;
            atomic_write(&path, external).unwrap();
            assert!(
                backend
                    .command(command)
                    .unwrap_err()
                    .0
                    .contains("externally")
            );
            assert_eq!(backend.snapshot().master_gain, 0.25);
            assert_eq!(backend.snapshot().revision, original_revision + 1);
            assert_eq!(fs::read(path).unwrap(), external);
        }
    }

    #[test]
    fn arrangement_revisions_reject_external_and_already_observed_changes() {
        let root = TestProject::new();
        let mut backend = root.open();
        let original = backend.snapshot();
        let mut edited = original.arrangement.clone();
        edited.tracks[0].name = "UI edit".into();
        let command = Command::SetArrangement {
            arrangement: edited.clone(),
            expected_revision: original.revision,
        };
        let mut external = root.open();
        let mut arrangement = original.arrangement.clone();
        arrangement.tracks[0].name = "External edit".into();
        external
            .command(Command::SetArrangement {
                arrangement: arrangement.clone(),
                expected_revision: external.snapshot().revision,
            })
            .unwrap();

        assert!(
            backend
                .command(command.clone())
                .unwrap_err()
                .0
                .contains("externally")
        );
        assert_eq!(backend.snapshot().arrangement, arrangement);
        assert_eq!(backend.snapshot().revision, original.revision + 1);
        assert!(backend.command(command).unwrap_err().0.contains("revision"));
        assert_eq!(root.open().snapshot().arrangement, arrangement);
        backend
            .command(Command::SetArrangement {
                arrangement: edited.clone(),
                expected_revision: backend.snapshot().revision,
            })
            .unwrap();
        assert_eq!(root.open().snapshot().arrangement, edited);
    }

    #[test]
    fn worker_receipts_complete_noop_history_and_rejected_edits() {
        let root = TestProject::new();
        let session = Session::start(root.open(), || Err(Error("No test device".into()))).unwrap();
        let original = session.snapshot();
        session
            .send(Command::Undo)
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(session.snapshot().revision, original.revision);
        assert_eq!(session.snapshot().arrangement, original.arrangement);

        let mut edited = original.arrangement.clone();
        edited.tracks[0].name = "UI edit".into();
        let error = session
            .send(Command::SetArrangement {
                arrangement: edited.clone(),
                expected_revision: original.revision.wrapping_add(1),
            })
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap_err();
        assert!(error.0.contains("revision"));
        assert_eq!(session.snapshot().error.as_deref(), Some(error.0.as_str()));
        assert_eq!(session.snapshot().revision, original.revision);
        assert_eq!(session.snapshot().arrangement, original.arrangement);
        assert_eq!(root.open().snapshot().arrangement, original.arrangement);

        session
            .send(Command::SetArrangement {
                arrangement: edited.clone(),
                expected_revision: original.revision,
            })
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(session.snapshot().revision, original.revision + 1);
        assert_eq!(session.snapshot().arrangement, edited);
        assert_eq!(root.open().snapshot().arrangement, edited);
    }

    #[test]
    fn imported_sampler_survives_session_history_and_renders_without_source() {
        use sound_daw::{
            arrangement::{Clip, Note, Track},
            sample_asset::import_sample,
            sampler::SamplerConfig,
        };

        let root = TestProject::new();
        let external = TestProject::new();
        fs::create_dir(&external.0).unwrap();
        let source = external.0.join("source.wav");
        let left: Vec<_> = (0..9_600)
            .map(|frame| 0.25 + (frame % 97) as f32 / 512.0)
            .collect();
        let right: Vec<_> = (0..9_600)
            .map(|frame| -0.5 + (frame % 71) as f32 / 512.0)
            .collect();
        crate::write_wav(&source, &left, &right, 48_000).unwrap();
        let backend = root.open();
        let asset = import_sample(&root.0, &source).unwrap();
        let imported = root.0.join("assets").join(&asset);
        assert!(!source.starts_with(&root.0));
        assert_eq!(fs::read(&imported).unwrap(), fs::read(&source).unwrap());
        let config = SamplerConfig {
            asset,
            root_key: 67,
        };
        let arrangement = Arrangement {
            tracks: vec![Track {
                name: "Imported stereo sample".into(),
                instrument: "daw.sampler".into(),
                sampler: Some(config.clone()),
                gain: 1.0,
                clips: vec![Clip {
                    start: 0,
                    length: 9_600,
                    notes: vec![Note {
                        start: 0,
                        length: 9_600,
                        key: config.root_key,
                        velocity: 127,
                    }],
                }],
                ..Track::default()
            }],
        };
        let session = Session::start(backend, || Err(Error("No test device".into()))).unwrap();
        let original = session.snapshot();
        assert!(!original.audio_available);
        let receive = |command| {
            session
                .send(command)
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
        };
        receive(Command::SetArrangement {
            arrangement: arrangement.clone(),
            expected_revision: original.revision,
        })
        .unwrap();
        assert_eq!(session.snapshot().revision, original.revision + 1);
        assert_eq!(session.snapshot().arrangement, arrangement);
        let reopened = root.open().snapshot();
        assert_eq!(reopened.arrangement, arrangement);
        assert_eq!(
            reopened.arrangement.tracks[0].sampler.as_ref(),
            Some(&config)
        );
        fs::remove_file(&source).unwrap();
        assert!(!source.exists());
        assert!(imported.is_file());

        receive(Command::Undo).unwrap();
        let undone = session.snapshot();
        assert_eq!(undone.revision, original.revision + 2);
        assert_eq!(undone.arrangement, original.arrangement);
        assert_eq!(root.open().snapshot().arrangement, original.arrangement);
        let state_path = root.0.join("state/arrangement.json");
        let manifest_path = root.0.join("project.json");
        let state = fs::read(&state_path).unwrap();
        let manifest = fs::read(&manifest_path).unwrap();
        let error = receive(Command::SetArrangement {
            arrangement: Arrangement { tracks: Vec::new() },
            expected_revision: original.revision,
        })
        .unwrap_err();
        assert!(error.0.contains("revision"));
        assert_eq!(session.snapshot().error.as_deref(), Some(error.0.as_str()));
        assert_eq!(session.snapshot().revision, undone.revision);
        assert_eq!(session.snapshot().arrangement, undone.arrangement);
        assert_eq!(fs::read(&state_path).unwrap(), state);
        assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
        let reopened = root.open().snapshot();
        assert_eq!(reopened.arrangement, undone.arrangement);
        assert_eq!(reopened.master_gain, undone.master_gain);
        receive(Command::Redo).unwrap();
        assert_eq!(session.snapshot().revision, undone.revision + 1);
        assert_eq!(session.snapshot().arrangement, arrangement);
        assert!(!session.snapshot().audio_available);
        assert!(!session.snapshot().playing);
        drop(session);

        let reopened = root.open();
        assert_eq!(reopened.snapshot().arrangement, arrangement);
        let output = external.0.join("render.wav");
        crate::render_wav(&reopened.project, 0.1, output.to_str().unwrap()).unwrap();
        let wav = fs::read(output).unwrap();
        let u16_at = |offset| u16::from_le_bytes(wav[offset..offset + 2].try_into().unwrap());
        let u32_at = |offset| u32::from_le_bytes(wav[offset..offset + 4].try_into().unwrap());
        assert_eq!(wav.len(), 44 + 4_800 * 8);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32_at(4), 36 + 4_800 * 8);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32_at(16), 16);
        assert_eq!(u16_at(20), 3);
        assert_eq!(u16_at(22), 2);
        assert_eq!(u32_at(24), 48_000);
        assert_eq!(u32_at(28), 48_000 * 8);
        assert_eq!(u16_at(32), 8);
        assert_eq!(u16_at(34), 32);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32_at(40), 4_800 * 8);
        let gain = std::f32::consts::FRAC_1_SQRT_2.powi(3);
        for (index, frame) in wav[44..].as_chunks::<8>().0.iter().enumerate() {
            let rendered_left = f32::from_le_bytes(frame[..4].try_into().unwrap());
            let rendered_right = f32::from_le_bytes(frame[4..].try_into().unwrap());
            assert!(rendered_left.is_finite() && rendered_left > 0.0);
            assert!(rendered_right.is_finite() && rendered_right < 0.0);
            assert!((rendered_left - left[index] * gain).abs() < 1e-6);
            assert!((rendered_right - right[index] * gain).abs() < 1e-6);
        }
    }

    #[test]
    fn headless_worker_reports_errors_accepts_edits_and_joins() {
        let root = TestProject::new();
        let session = Session::start(root.open(), || Err(Error("No test device".into()))).unwrap();
        assert!(!session.snapshot().audio_available);
        assert_eq!(session.snapshot().error.as_deref(), Some("No test device"));
        let failed = session.send(Command::TogglePlay).unwrap();
        let edited = session.send(Command::SetMasterGain(0.4)).unwrap();
        edited
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            failed
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap_err()
                .0,
            "No test device"
        );
        assert_eq!(session.snapshot().master_gain, 0.4);
        assert!(!session.snapshot().playing);
        assert_eq!(session.snapshot().frame, 0);
        assert_eq!(session.snapshot().peak, 0.0);
        assert_eq!(session.snapshot().error.as_deref(), Some("No test device"));
        let worker = session.worker.as_ref().unwrap().thread().clone();
        drop(session);
        assert_eq!(root.open().snapshot().master_gain, 0.4);
        assert_ne!(worker.id(), thread::current().id());
    }

    #[test]
    fn command_queue_reports_full_and_disconnected_without_blocking() {
        let root = TestProject::new();
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let session = Session {
            commands,
            shutdown: None,
            worker: None,
            snapshot: Arc::new(Mutex::new(root.open().snapshot())),
        };
        for _ in 0..COMMAND_CAPACITY {
            session.send(Command::Stop).unwrap();
        }
        assert!(session.send(Command::Stop).unwrap_err().0.contains("full"));
        drop(receiver);
        assert!(
            session
                .send(Command::Stop)
                .unwrap_err()
                .0
                .contains("stopped")
        );
    }

    #[test]
    fn real_audio_session_plays_five_seconds_without_underruns() {
        if !sound_core::device::audio_available() {
            eprintln!("skipped: no default audio output device");
            return;
        }
        let root = TestProject::new();
        let session = Session::open(root.0.to_str().unwrap()).unwrap();
        let start = session.snapshot();
        assert!(start.audio_available);
        session
            .send(Command::TogglePlay)
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        std::thread::sleep(Duration::from_secs(5));
        let end = session.snapshot();
        assert!(end.playing);
        assert_eq!(end.underruns, 0, "session underruns after 5s playback");
        let served = end.frame;
        assert!(served >= 48_000 * 4, "frames served: {served}");
    }
}
