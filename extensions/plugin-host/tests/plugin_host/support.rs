//! A project with one hosted plugin, a test tool that plays notes into it, and the scan
//! folder that holds the repository's own test plugin.

use std::path::{Path, PathBuf};

use plugin_host::{PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand};
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
    pub fn frame(self) -> u64 {
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

/// A steady level in both channels, as the instrument of a rack. It makes the dry signal of an
/// effect a number a test can read in any frame, and it is not a plugin, so a test that tells
/// the plugin of this process to misbehave only tells the effect.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub value: f32,
}

impl State for Level {
    const TOOL: &'static str = "test.level";
}

pub struct LevelProcessor {
    value: f32,
}

impl LevelProcessor {
    const OUTPUT: sound_core::AudioOutput = sound_core::AudioOutput::new(0);
}

impl Processor for LevelProcessor {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _config: &PrepareConfig) {}

    fn update(&mut self, value: &mut f32) {
        self.value = *value;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames;
        for channel in context.audio_outputs.get(Self::OUTPUT) {
            channel[..frames].fill(self.value);
        }
    }
}

fn apply_level(state: &Level, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let level = context.processor("level", || LevelProcessor { value: state.value })?;
    context.update(level, state.value)?;
    context.output(
        sound_notes::AUDIO_OUTPUT,
        OutputEndpoint::new(level, LevelProcessor::OUTPUT),
    );
    Ok(())
}

/// A tiny stand-in for a track: it owns the `instrument` child and plays into it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rack {}

impl State for Rack {
    const TOOL: &'static str = "test.rack";
    const OWNS_CHILDREN: bool = true;
}

/// What a track does: the sender into the instrument, the instrument through the effect it has,
/// and the last of them to the device.
///
/// The chain is the one the arrangement builds, in miniature: one fixed effect slot named
/// `effect`. A slot with no record is left out, as a track leaves one out.
fn apply_rack(_state: &Rack, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    if let Some(played) = context.child_output("keys", PLAYED_OUTPUT)
        && let Some(notes) = context.child_input("instrument", NOTES_INPUT)
    {
        context.connect(played.to(notes))?;
    }
    let mut sound = context.child_output("instrument", sound_notes::AUDIO_OUTPUT);
    let effect = context
        .child_input(EFFECT, sound_notes::AUDIO_INPUT)
        .zip(context.child_output(EFFECT, sound_notes::AUDIO_OUTPUT));
    if let Some((input, output)) = effect {
        if let Some(sound) = sound {
            context.connect(sound.to(input))?;
        }
        sound = Some(output);
    }
    if let Some(sound) = sound {
        context.connect(sound.to_device(0))?;
    }
    Ok(())
}

/// The one effect slot of the test rack, after its instrument.
pub const EFFECT: &str = "effect";

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

/// Makes the VST 3 test plugin say its output is silent and write nothing into it, from its
/// second block on. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_go_silent() {
    // SAFETY: nextest runs one test per process and this is called before any thread but this
    // one exists, so no other thread can be reading the environment.
    unsafe { std::env::set_var("SOUND_TOOLS_TEST_PLUGIN_SILENT", "1") };
}

/// Makes the VST 3 test plugin move the parameter its sustain pedal is mapped to, once, right
/// after the host has looked that mapping up, and tell the host about it: to another
/// parameter, or with `nowhere` to none at all. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_move_its_pedal(to: &str) {
    // SAFETY: nextest runs one test per process and this is called before any thread but this
    // one exists, so no other thread can be reading the environment.
    unsafe { std::env::set_var(test_plugin_support::MOVE_PEDAL_VARIABLE, to) };
}

/// Makes the VST 3 test plugin's controller edit its `Level` to a quarter in the same moment it
/// asks to be started again for a new latency. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_edit_as_it_restarts() {
    // SAFETY: as above.
    unsafe { std::env::set_var(test_plugin_support::EDIT_AT_RESTART_VARIABLE, "1") };
}

/// Makes the VST 3 test plugin keep a state in its edit controller as well as in its component:
/// how loud it plays, which the pedal halves along with the transpose. Same rules as
/// [`tell_the_plugin`].
pub fn tell_the_plugin_to_keep_a_controller_state() {
    // SAFETY: nextest runs one test per process and this is called before any thread but this
    // one exists, so no other thread can be reading the environment.
    unsafe { std::env::set_var(test_plugin_support::CONTROLLER_STATE_VARIABLE, "1") };
}

/// Makes the VST 3 test plugin's edit controller fail to give its state. Same rules as
/// [`tell_the_plugin`].
pub fn tell_the_plugin_that_its_controller_fails() {
    // SAFETY: as above.
    unsafe { std::env::set_var(test_plugin_support::CONTROLLER_FAILS_VARIABLE, "1") };
}

/// Makes the VST 3 test plugin write its state with the header last: room first, then the
/// payload, then back to the start for the header. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_write_its_header_last() {
    // SAFETY: as above.
    unsafe { std::env::set_var(test_plugin_support::HEADER_LAST_VARIABLE, "1") };
}

/// Makes the test plugin close its own window as soon as the host has shown it, which is what
/// a composer does with the title bar of a real plugin's window. CLAP only, because VST 3 has
/// no such call. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_close_its_window() {
    // SAFETY: nextest runs one test per process and this is called before any thread but this
    // one exists, so no other thread can be reading the environment.
    unsafe { std::env::set_var(test_plugin_support::CLOSE_GUI_VARIABLE, "1") };
}

/// Makes the test plugin offer no window of its own at all. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_have_no_window(without: bool) {
    // SAFETY: as above.
    unsafe {
        match without {
            true => std::env::set_var(test_plugin_support::NO_WINDOW_VARIABLE, "1"),
            false => std::env::remove_var(test_plugin_support::NO_WINDOW_VARIABLE),
        }
    }
}

/// Makes the test plugin ask its host for this window size as soon as it has a window, the way
/// a plugin that sizes itself as it opens does. A width of zero stops it asking. Same rules as
/// [`tell_the_plugin`].
pub fn tell_the_plugin_to_ask_for_a_window_size(width: u32, height: u32) {
    // SAFETY: as above.
    unsafe {
        match width == 0 || height == 0 {
            true => std::env::remove_var(test_plugin_support::RESIZE_GUI_VARIABLE),
            false => std::env::set_var(
                test_plugin_support::RESIZE_GUI_VARIABLE,
                format!("{width}x{height}"),
            ),
        }
    }
}

/// Makes the VST 3 test plugin's view ask for another size from inside `onSize`, which is
/// inside the host's answer to a request of its own. A width of zero stops it asking. Same
/// rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_ask_again_from_inside_the_answer(width: u32, height: u32) {
    // SAFETY: as above.
    unsafe {
        match width == 0 || height == 0 {
            true => std::env::remove_var(test_plugin_support::RESIZE_IN_ON_SIZE_VARIABLE),
            false => std::env::set_var(
                test_plugin_support::RESIZE_IN_ON_SIZE_VARIABLE,
                format!("{width}x{height}"),
            ),
        }
    }
}

/// Makes the VST 3 test plugin's controller edit its `Level` parameter through the host as
/// soon as it has a component handler, the way its own window would when the composer turns a
/// knob: one `beginEdit`, `count` values on the way down, one `endEdit`. The last value is
/// `1 / count`. Same rules as [`tell_the_plugin`].
pub fn tell_the_plugin_to_edit_its_level(count: u32) {
    // SAFETY: as above.
    unsafe {
        match count == 0 {
            true => std::env::remove_var(test_plugin_support::EDITS_VARIABLE),
            false => std::env::set_var(test_plugin_support::EDITS_VARIABLE, count.to_string()),
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

/// Every format the repository has a test plugin for. A check that is about the host and not
/// about one format runs once per format, so both backends answer the same list.
pub const FORMATS: [PluginFormat; 2] = [PluginFormat::Clap, PluginFormat::Vst3];

/// The folder a scan looks in, with the repository's own test plugins in it, one per format.
pub fn plugin_folder(root: &Path) -> PathBuf {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    test_vst3_plugin::install_into(&folder);
    folder
}

/// The same folder with only one format's test plugin in it, for a test that must not find
/// the other one.
pub fn plugin_folder_of(root: &Path, format: PluginFormat) -> PathBuf {
    let folder = root.join("plugins");
    match format {
        PluginFormat::Clap => test_clap_plugin::install_into(&folder),
        PluginFormat::Vst3 => test_vst3_plugin::install_into(&folder),
    };
    folder
}

/// The scanner: the `plugin-scan` program of this crate, which the runtime does with its own
/// executable.
pub fn scanner() -> ScanCommand {
    ScanCommand::new(env!("CARGO_BIN_EXE_plugin-scan"), [])
}

/// No test ever reads or writes the cache of this machine.
pub fn no_cache() -> ScanCache {
    ScanCache::none()
}

/// The id of the repository's test plugin of this format.
pub fn plugin_id(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    }
}

pub fn record(format: PluginFormat, state_asset: &str) -> PluginRecord {
    PluginRecord::new(format, plugin_id(format), state_asset).expect("a plugin record")
}

/// What the test plugin saved, out of a state asset.
///
/// A CLAP asset is the plugin's own bytes. A VST 3 asset is the container this host writes,
/// because VST 3 keeps two states: `SVT3`, then the component's state with its length, then
/// the controller's. Reading it here is also what checks that the container is what it says.
pub fn saved_state(format: PluginFormat, bytes: &[u8]) -> test_plugin_support::SavedState {
    let own = match format {
        PluginFormat::Clap => bytes,
        PluginFormat::Vst3 => {
            assert_eq!(&bytes[..4], b"SVT3", "not a VST 3 state asset");
            let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            &bytes[8..8 + length]
        }
    };
    test_plugin_support::load_state(own).expect("a Test Tone state")
}

/// The transpose the test plugin saved.
pub fn saved_transpose(format: PluginFormat, bytes: &[u8]) -> i32 {
    saved_state(format, bytes).semitones
}

/// The level a parameter edit left the plugin on, out of the component part of a VST 3 state
/// asset. It is hundredths, so 100 is the level a plugin nobody edited plays at.
pub fn saved_edit_level(bytes: &[u8]) -> i32 {
    saved_state(PluginFormat::Vst3, bytes).edit_level
}

/// The level the plugin's edit controller saved, out of the controller part of a VST 3 state
/// asset. `None` says the asset holds no controller state at all.
pub fn saved_controller_level(bytes: &[u8]) -> Option<i32> {
    assert_eq!(&bytes[..4], b"SVT3", "not a VST 3 state asset");
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let rest = &bytes[8 + length..];
    let controller = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    test_plugin_support::load_controller_state(&rest[4..4 + controller])
}

/// A VST 3 state asset as this host writes one, for a test that puts a state in the project
/// before any plugin has run.
pub fn vst3_state(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut bytes = b"SVT3".to_vec();
    bytes.extend_from_slice(&(component.len() as u32).to_le_bytes());
    bytes.extend_from_slice(component);
    bytes.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    bytes.extend_from_slice(controller);
    bytes
}

/// The loudest sample of a channel.
pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
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

    /// A project whose engine renders instead of playing on a device, which is what `--render`
    /// opens. Every plugin is told, in the way its own format has for it.
    pub fn rendering_offline() -> Self {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let scan_folder = plugin_folder(folder.path());
        let plugins = Plugins::new(vec![scan_folder], scanner(), no_cache());
        Self::with_plugins_and_engine(
            folder,
            plugins,
            EngineConfig::new(SAMPLE_RATE, 2).rendering_offline(),
        )
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
            Plugins::new(search_paths, scanner(), no_cache())
        } else {
            Plugins::read_only(search_paths, scanner(), no_cache())
        };
        Self::with_plugins(folder, plugins)
    }

    /// A project on `folder` with a host that is already made, for a test that wants to say
    /// how it scans.
    pub fn with_plugins(folder: tempfile::TempDir, plugins: Plugins) -> Self {
        Self::with_plugins_and_engine(folder, plugins, EngineConfig::new(SAMPLE_RATE, 2))
    }

    /// The same, with the engine given: a device run or a render.
    pub fn with_plugins_and_engine(
        folder: tempfile::TempDir,
        plugins: Plugins,
        config: EngineConfig,
    ) -> Self {
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
        registry
            .tool::<Level>("test")
            .expect("the level tool registers")
            .behaviour(apply_level);
        let (control, engine) = Engine::new(config);
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

    /// A rack `track` whose instrument is a steady level, with the plugin of `record` as its
    /// effect. The dry signal is then a number a test can read in any frame.
    pub fn add_level_track(&mut self, value: f32, record: PluginRecord) {
        let mut changes = Changes::new();
        changes.create(id("track"), Rack {});
        changes.create(id("track/instrument"), Level { value });
        changes.create(id(&format!("track/{EFFECT}")), record);
        self.project
            .commit("Add track", changes)
            .expect("the track is added");
    }

    /// Puts a plugin in the effect slot of the rack, after its instrument.
    pub fn add_effect(&mut self, record: PluginRecord) {
        let mut changes = Changes::new();
        changes.create(id(&format!("track/{EFFECT}")), record);
        self.project
            .commit("Add effect", changes)
            .expect("the effect is added");
    }

    /// Writes an offset, in hundredths, into the state asset of an effect, as the host would
    /// have saved it. It is what the effect half of the test plugin adds to every sample.
    pub fn write_offset(&self, format: PluginFormat, name: &str, offset: i32) {
        let own = test_plugin_support::save_state(test_plugin_support::SavedState {
            offset,
            ..Default::default()
        });
        let bytes = match format {
            PluginFormat::Clap => own,
            PluginFormat::Vst3 => vst3_state(&own, b""),
        };
        let path = self.project.assets().path(&state_asset(name));
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the folder");
        std::fs::write(path, bytes).expect("the state asset");
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
            self.plugins.send_restarts(&mut self.project);
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
