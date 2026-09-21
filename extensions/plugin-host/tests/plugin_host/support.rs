//! A project with one hosted plugin, a test tool that plays notes into it, and the scan
//! folder that holds the repository's own test plugin.

use std::path::{Path, PathBuf};

use plugin_host::{PluginFormat, PluginRecord, Plugins, ScanCommand};
use serde::{Deserialize, Serialize};
use sound_core::{
    AssetName, BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EventOutput,
    InstanceId, OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry,
    State,
};
use sound_notes::{NOTES_INPUT, NoteEvent, Pedal, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;

/// Counts allocations while it is armed, so a test can say that a block of audio made none.
/// The realtime sanitizer cannot see inside a plugin's own call, and that is exactly where a
/// buffer the host handed the plugin would grow.
pub struct CountingAllocator;

static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// SAFETY: every call is handed to the system allocator unchanged. The counter only counts.
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if ARMED.load(std::sync::atomic::Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: the caller keeps the contract of `GlobalAlloc::alloc`.
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: the caller keeps the contract of `GlobalAlloc::dealloc`.
        unsafe { std::alloc::System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: std::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        if ARMED.load(std::sync::atomic::Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: the caller keeps the contract of `GlobalAlloc::realloc`.
        unsafe { std::alloc::System.realloc(pointer, layout, new_size) }
    }
}

/// How many allocations happened anywhere in this process while `work` ran.
pub fn allocations_during<T>(work: impl FnOnce() -> T) -> (T, u64) {
    ALLOCATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
    let value = work();
    ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
    (
        value,
        ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// One thing to play, at an engine frame counted from the first block of the render.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "lowercase")]
pub enum Played {
    On { frame: u64, pitch: u8, velocity: u8 },
    Off { frame: u64, pitch: u8 },
    Pedal { frame: u64, value: u8 },
    AllOff { frame: u64 },
}

impl Played {
    fn frame(self) -> u64 {
        match self {
            Self::On { frame, .. }
            | Self::Off { frame, .. }
            | Self::Pedal { frame, .. }
            | Self::AllOff { frame } => frame,
        }
    }

    fn event(self) -> NoteEvent {
        match self {
            Self::On {
                pitch, velocity, ..
            } => NoteEvent::On {
                pitch: Pitch::new(pitch).expect("a pitch"),
                velocity: Velocity::new(velocity).expect("a velocity"),
            },
            Self::Off { pitch, .. } => NoteEvent::Off {
                pitch: Pitch::new(pitch).expect("a pitch"),
            },
            Self::Pedal { value, .. } => NoteEvent::Pedal(Pedal::new(value).expect("a pedal")),
            Self::AllOff { .. } => NoteEvent::AllOff,
        }
    }
}

/// A tool that plays a fixed list of note events at engine frames. It stands in for a track's
/// sequencer, so these tests need no arrangement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keys {
    pub played: Vec<Played>,
}

impl State for Keys {
    const TOOL: &'static str = "test.keys";
}

pub struct KeysProcessor {
    played: Vec<Played>,
    next: usize,
}

impl KeysProcessor {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for KeysProcessor {
    type Update = Vec<Played>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _config: &PrepareConfig) {}

    fn update(&mut self, update: &mut Vec<Played>) {
        std::mem::swap(&mut self.played, update);
        self.next = 0;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let start = context.start_frame;
        let end = start + context.frames as u64;
        while let Some(played) = self.played.get(self.next) {
            if played.frame() >= end {
                break;
            }
            let offset = played.frame().saturating_sub(start) as usize;
            context
                .event_outputs
                .push(Self::NOTES, offset, played.event());
            self.next += 1;
        }
    }
}

fn apply_keys(state: &Keys, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let keys = context.processor("keys", || KeysProcessor {
        played: Vec::new(),
        next: 0,
    })?;
    context.update(keys, state.played.clone())?;
    context.output(
        PLAYED_OUTPUT,
        OutputEndpoint::new(keys, KeysProcessor::NOTES),
    );
    Ok(())
}

/// The port the keys sender exposes, which the rack wires to the instrument.
const PLAYED_OUTPUT: &str = "played";

/// A tiny stand-in for a track: it owns the `instrument` child and plays into it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rack {}

impl State for Rack {
    const TOOL: &'static str = "test.rack";
    const OWNS_CHILDREN: bool = true;
}

/// What a track does: the sender into the instrument, and the instrument to the device.
fn apply_rack(_state: &Rack, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    if let Some(played) = context.child_output("keys", PLAYED_OUTPUT)
        && let Some(notes) = context.child_input("instrument", NOTES_INPUT)
    {
        context.connect(played.to(notes))?;
    }
    if let Some(audio) = context.child_output("instrument", sound_notes::AUDIO_OUTPUT) {
        context.connect(audio.to_device(0))?;
    }
    Ok(())
}

/// A tool whose behaviour refuses when it is told to, so a test can make the project reject a
/// whole edit group the way another extension or a bad connection would.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Picky {
    pub refuses: bool,
}

impl State for Picky {
    const TOOL: &'static str = "test.picky";
}

fn apply_picky(state: &Picky, _context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    if state.refuses {
        return Err(BehaviourError::Other("this record refuses".into()));
    }
    Ok(())
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).expect("an instance id")
}

/// Makes the test plugin write a line for every lifecycle call it gets into `path`, and makes
/// it send `events` events out of every process call.
///
/// The plugin runs inside this process, so it reads this process's environment. Nextest gives
/// every test its own process, so setting it here changes nothing for any other test.
pub fn tell_the_plugin(log: Option<&Path>, events: Option<u32>) {
    // SAFETY: nextest runs one test per process and this is called before any thread but this
    // one exists, so no other thread can be reading the environment.
    unsafe {
        match log {
            Some(path) => std::env::set_var("SOUND_TOOLS_TEST_PLUGIN_LOG", path),
            None => std::env::remove_var("SOUND_TOOLS_TEST_PLUGIN_LOG"),
        }
        match events {
            Some(count) => std::env::set_var("SOUND_TOOLS_TEST_PLUGIN_EVENTS", count.to_string()),
            None => std::env::remove_var("SOUND_TOOLS_TEST_PLUGIN_EVENTS"),
        }
    }
}

/// One line of the plugin's lifecycle log: the call, which plugin of the library it was about,
/// the thread it came in on, and how many process calls that plugin had had by then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedCall {
    pub call: String,
    pub plugin: u64,
    pub thread: String,
    pub processed: u64,
}

pub fn lifecycle(path: &Path) -> Vec<LoggedCall> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(' ');
            Some(LoggedCall {
                call: parts.next()?.to_string(),
                plugin: parts.next()?.strip_prefix("plugin=")?.parse().ok()?,
                thread: parts.next()?.strip_prefix("thread=")?.to_string(),
                processed: parts.next()?.strip_prefix("processed=")?.parse().ok()?,
            })
        })
        .collect()
}

/// The folder a scan looks in, with the repository's own test plugin in it.
pub fn plugin_folder(root: &Path) -> PathBuf {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    folder
}

/// The scanner: the `clap-scan` program of this crate, which the runtime does with its own
/// executable.
pub fn scanner() -> ScanCommand {
    ScanCommand::new(env!("CARGO_BIN_EXE_clap-scan"), [])
}

pub fn record(state_asset: &str) -> PluginRecord {
    PluginRecord::new(PluginFormat::Clap, test_clap_plugin::PLUGIN_ID, state_asset)
        .expect("a plugin record")
}

pub fn state_asset(name: &str) -> AssetName {
    AssetName::new("plugin-state", name, "bin").expect("an asset name")
}

/// An open project on a temporary folder with an offline engine, a plugin host that looks in a
/// folder of its own, and a rack with a `keys` sender.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    pub plugins: Plugins,
    pub folder: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::open(tempfile::tempdir().expect("a temporary folder"), true)
    }

    /// Opens a folder again, as closing and reopening a project does.
    pub fn reopen(self) -> Self {
        let Self {
            project, folder, ..
        } = self;
        drop(project);
        Self::open(folder, true)
    }

    pub fn open(folder: tempfile::TempDir, writes_state: bool) -> Self {
        let scan_folder = plugin_folder(folder.path());
        Self::open_with_paths(folder, vec![scan_folder], writes_state)
    }

    pub fn open_with_paths(
        folder: tempfile::TempDir,
        search_paths: Vec<PathBuf>,
        writes_state: bool,
    ) -> Self {
        let plugins = if writes_state {
            Plugins::new(search_paths, scanner())
        } else {
            Plugins::read_only(search_paths, scanner())
        };
        let mut registry = Registry::new();
        plugin_host::register(&mut registry, plugins.clone()).expect("the plugin host registers");
        registry
            .tool::<Keys>("test")
            .expect("the keys tool registers")
            .behaviour(apply_keys);
        registry
            .tool::<Rack>("test")
            .expect("the rack tool registers")
            .behaviour(apply_rack);
        registry
            .tool::<Picky>("test")
            .expect("the picky tool registers")
            .behaviour(apply_picky);
        let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let project = Project::open(folder.path(), registry, control).expect("an open project");
        Self {
            project,
            engine,
            plugins,
            folder,
        }
    }

    /// A rack `track` with the plugin of `record` as its `instrument`, playing `played`.
    pub fn add_track(&mut self, record: PluginRecord, played: Vec<Played>) {
        let mut changes = Changes::new();
        changes.create(id("track"), Rack {});
        changes.create(id("track/keys"), Keys { played });
        changes.create(id("track/instrument"), record);
        self.project
            .commit("Add track", changes)
            .expect("the track is added");
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the folder");
        }
        std::fs::write(&path, contents).expect("the file is written");
        self.project
            .apply_outside_changes(&[path])
            .expect("the change applies")
    }

    pub fn problems(&self) -> Vec<String> {
        let problems = self.project.problems().into_iter();
        problems
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect()
    }

    /// Renders `frames` frames with nothing but the engine, and counts what was allocated
    /// anywhere in this process while it did.
    pub fn render_counting_allocations(&mut self, frames: usize) -> (Render, u64) {
        let mut output = vec![0.0_f32; frames * 2];
        let ((), allocations) = allocations_during(|| {
            for buffer in output.chunks_mut(512 * 2) {
                self.engine.process_block(buffer);
            }
        });
        (Render { output }, allocations)
    }

    /// Renders without polling the host, so a test decides itself when the host does its
    /// main-thread work and at what time.
    pub fn render_without_polling(&mut self, frames: usize) -> Render {
        let mut output = vec![0.0_f32; frames * 2];
        for buffer in output.chunks_mut(512 * 2) {
            self.engine.process_block(buffer);
        }
        self.project.engine().poll().expect("the engine polls");
        Render { output }
    }

    /// Renders `frames` frames in device buffers of 512, interleaved, and polls the host after
    /// every buffer as the runtime does.
    pub fn render(&mut self, frames: usize) -> Render {
        let mut output = vec![0.0_f32; frames * 2];
        for buffer in output.chunks_mut(512 * 2) {
            self.engine.process_block(buffer);
            self.project.engine().poll().expect("the engine polls");
            self.plugins.poll(&self.project);
        }
        Render { output }
    }

    pub fn play(&mut self, frames: usize) -> Render {
        self.project.engine().play();
        self.render(frames)
    }
}

/// One render, as two channels.
pub struct Render {
    output: Vec<f32>,
}

impl Render {
    pub fn left(&self) -> Vec<f32> {
        self.output.iter().step_by(2).copied().collect()
    }

    pub fn right(&self) -> Vec<f32> {
        self.output.iter().skip(1).step_by(2).copied().collect()
    }

    pub fn samples(&self) -> &[f32] {
        &self.output
    }

    /// The first frame where the left channel is not silent, which is the frame the plugin
    /// started a note on.
    pub fn first_sound(&self) -> Option<usize> {
        self.left().iter().position(|sample| *sample != 0.0)
    }
}
