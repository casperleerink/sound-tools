//! A project with one hosted plugin, a test tool that plays notes into it, and the scan
//! folder that holds the repository's own test plugin.

use std::path::{Path, PathBuf};

use plugin_host::{PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand};
use serde::{Deserialize, Serialize};
use sound_core::{
    AssetName, Automation, BehaviourContext, BehaviourError, Changes, Engine, EngineConfig,
    EventOutput, InstanceId, OutputEndpoint, PlayedLanes, Ports, PrepareConfig, ProcessContext,
    Processor, Project, Registry, State, Ticks,
};
use sound_notes::{Amount, Bend, Controller, NOTES_INPUT, NoteEvent, Pedal, Pitch, Velocity};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// Counts allocations while it is armed. The realtime sanitizer cannot see inside a plugin's
/// own call, which is where a buffer the host handed the plugin would grow.
///
/// Armed per thread: other threads, of the test runner or of a plugin, may allocate any time.
pub(crate) struct CountingAllocator;

thread_local! {
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A thread that is ending has no flag left.
fn armed() -> bool {
    ARMED.try_with(std::cell::Cell::get).unwrap_or(false)
}

// SAFETY: every call is handed to the system allocator unchanged.
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if armed() {
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
        if armed() {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // SAFETY: the caller keeps the contract of `GlobalAlloc::realloc`.
        unsafe { std::alloc::System.realloc(pointer, layout, new_size) }
    }
}

pub(crate) fn allocations_during<T>(work: impl FnOnce() -> T) -> (T, u64) {
    ALLOCATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    ARMED.set(true);
    let value = work();
    ARMED.set(false);
    (
        value,
        ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// One thing to play, at an engine frame counted from the first block of the render.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "lowercase")]
pub(crate) enum Played {
    On {
        frame: u64,
        pitch: u8,
        velocity: u8,
    },
    Off {
        frame: u64,
        pitch: u8,
    },
    Pedal {
        frame: u64,
        value: u8,
    },
    Bend {
        frame: u64,
        value: i16,
    },
    ModWheel {
        frame: u64,
        value: u8,
    },
    Pressure {
        frame: u64,
        value: u8,
    },
    Control {
        frame: u64,
        controller: u8,
        value: u8,
    },
    AllOff {
        frame: u64,
    },
}

impl Played {
    pub(crate) fn frame(self) -> u64 {
        match self {
            Self::On { frame, .. }
            | Self::Off { frame, .. }
            | Self::Pedal { frame, .. }
            | Self::Bend { frame, .. }
            | Self::ModWheel { frame, .. }
            | Self::Pressure { frame, .. }
            | Self::Control { frame, .. }
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
            Self::Bend { value, .. } => NoteEvent::Bend(Bend::new(value).expect("a bend")),
            Self::ModWheel { value, .. } => {
                NoteEvent::ModWheel(Amount::new(value).expect("an amount"))
            }
            Self::Pressure { value, .. } => {
                NoteEvent::Pressure(Amount::new(value).expect("an amount"))
            }
            Self::Control {
                controller, value, ..
            } => NoteEvent::Control {
                controller: Controller::new(controller).expect("a controller"),
                value: Amount::new(value).expect("an amount"),
            },
            Self::AllOff { .. } => NoteEvent::AllOff,
        }
    }
}

/// A tool that plays a fixed list of note events at engine frames. It stands in for a
/// `timeline`, so these tests need no TypeScript.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Keys {
    pub played: Vec<Played>,
}

impl State for Keys {
    const TOOL: &'static str = "test.keys";
}

pub(crate) struct KeysProcessor {
    played: Vec<Played>,
    next: usize,
}

impl KeysProcessor {
    pub(crate) const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
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

/// A steady level in both channels, as the instrument of a rack: the dry signal of an effect is
/// then a number a test can read in any frame. Not a plugin, so a test that tells the plugin
/// to misbehave only tells the effect.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Level {
    pub value: f32,
}

impl State for Level {
    const TOOL: &'static str = "test.level";
}

pub(crate) struct LevelProcessor {
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

/// A tiny stand-in for a track: it owns the `instrument` child and plays into it, with a lane
/// that holds one value for each number of its instrument named in `lanes`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rack {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lanes: Vec<(String, f32)>,
    /// A frame of every sub-block and a value every lane bends to there, as a lane that moves
    /// inside a block does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<(usize, f32)>,
}

/// Plays steady lanes, as the lane player of a track does: every lane every block, at offset
/// 0, also while the project does not play. With a bend, every lane again at its frame.
pub(crate) struct LanePlayer {
    lanes: Vec<Automation>,
    bend: Option<(usize, f32)>,
}

impl LanePlayer {
    const OUTPUT: EventOutput<Automation> = EventOutput::new(0);
}

impl Processor for LanePlayer {
    type Update = (Vec<Automation>, Option<(usize, f32)>);

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _config: &PrepareConfig) {}

    fn update(&mut self, update: &mut Self::Update) {
        std::mem::swap(&mut self.lanes, &mut update.0);
        self.bend = update.1;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for lane in &self.lanes {
            context.event_outputs.push(Self::OUTPUT, 0, *lane);
        }
        if let Some((frame, value)) = self.bend {
            for lane in &self.lanes {
                let bent = Automation { value, ..*lane };
                context.event_outputs.push(Self::OUTPUT, frame, bent);
            }
        }
    }
}

struct SteadyLanes(Vec<(std::sync::Arc<str>, f32)>);

impl PlayedLanes for SteadyLanes {
    fn values_at(&self, _tick: Ticks, values: &mut Vec<(std::sync::Arc<str>, f32)>) {
        values.extend(self.0.iter().cloned());
    }
}

/// Plays and shows the rack's lanes as a track does. A name the instrument takes no lane for is
/// reported, and the rest play.
fn play_lanes(state: &Rack, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    if state.lanes.is_empty() {
        return Ok(());
    }
    let Some((input, numbers)) = context.child_automation("instrument") else {
        context.problem("the instrument takes no automation".to_string());
        return Ok(());
    };
    let (mut lanes, mut shown, mut missing) = (Vec::new(), Vec::new(), Vec::new());
    for (name, value) in &state.lanes {
        let place = numbers.iter().position(|number| *number.field == **name);
        match place.and_then(|place| Some((place, u16::try_from(place).ok()?))) {
            Some((place, parameter)) => {
                lanes.push(Automation {
                    parameter,
                    value: *value,
                });
                shown.push((numbers[place].field.clone(), *value));
            }
            None => missing.push(name.clone()),
        }
    }
    context.show_lanes(Some("instrument"), std::sync::Arc::new(SteadyLanes(shown)));
    for name in missing {
        context.problem(format!("no lane for {name}"));
    }
    let player = context.processor("lanes", || LanePlayer {
        lanes: Vec::new(),
        bend: None,
    })?;
    context.update(player, (lanes, state.bend))?;
    context.connect(OutputEndpoint::new(player, LanePlayer::OUTPUT).to(input))?;
    Ok(())
}

impl State for Rack {
    const TOOL: &'static str = "test.rack";
    const OWNS_CHILDREN: bool = true;
}

/// The sender into the instrument, the instrument through the `effect` child when there is
/// one, and the last of them to the device.
fn apply_rack(state: &Rack, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    play_lanes(state, context)?;
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

pub(crate) const EFFECT: &str = "effect";

/// A tool whose behaviour refuses when it is told to, so a test can make the project reject a
/// whole edit group the way another extension or a bad connection would.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Picky {
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

pub(crate) fn id(id: &str) -> InstanceId {
    InstanceId::new(id).expect("an instance id")
}

/// Sets what the test plugin reads from the environment: see `test_plugin_support`. Nextest
/// gives every test its own process, so this changes nothing for any other test.
pub(crate) fn tell_the_plugin(log: Option<&Path>, events: Option<u32>) {
    // SAFETY: called before any other thread exists, so none can be reading the environment.
    unsafe {
        match log {
            Some(path) => std::env::set_var(test_plugin_support::LOG_VARIABLE, path),
            None => std::env::remove_var(test_plugin_support::LOG_VARIABLE),
        }
        match events {
            Some(count) => {
                std::env::set_var(test_plugin_support::EVENTS_VARIABLE, count.to_string())
            }
            None => std::env::remove_var(test_plugin_support::EVENTS_VARIABLE),
        }
    }
}

pub(crate) fn tell_the_plugin_to_smooth() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::SMOOTH_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_go_silent() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::SILENT_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_not_to_implement_set_processing() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::NO_SET_PROCESSING_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_ask_for_a_reload_as_it_loads() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::RELOAD_ON_STATE_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_list_its_level_late() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::LATE_LEVEL_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_edit_as_it_restarts() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::EDIT_AT_RESTART_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_keep_a_controller_state() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::CONTROLLER_STATE_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_that_its_controller_fails() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::CONTROLLER_FAILS_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_write_its_header_last() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::HEADER_LAST_VARIABLE, "1") };
}

/// `bend`, `mod_wheel` or `pressure`.
pub(crate) fn tell_the_plugin_to_show(wheel: &str) {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::SHOW_VARIABLE, wheel) };
}

pub(crate) fn tell_the_plugin_to_map_no_wheels() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::NO_WHEELS_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_close_its_window() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::CLOSE_GUI_VARIABLE, "1") };
}

pub(crate) fn tell_the_plugin_to_have_no_window(without: bool) {
    // SAFETY: as in `tell_the_plugin`.
    unsafe {
        match without {
            true => std::env::set_var(test_plugin_support::NO_WINDOW_VARIABLE, "1"),
            false => std::env::remove_var(test_plugin_support::NO_WINDOW_VARIABLE),
        }
    }
}

/// A width of zero stops it asking.
pub(crate) fn tell_the_plugin_to_ask_for_a_window_size(width: u32, height: u32) {
    // SAFETY: as in `tell_the_plugin`.
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

/// Asks from inside `onSize`, which is inside the host's answer to a request of its own. A width
/// of zero stops it asking.
pub(crate) fn tell_the_plugin_to_ask_again_from_inside_the_answer(width: u32, height: u32) {
    // SAFETY: as in `tell_the_plugin`.
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

/// One `beginEdit`, `count` values on the way down, one `endEdit`. The last value is
/// `1 / count`.
pub(crate) fn tell_the_plugin_to_edit_its_level(count: u32) {
    // SAFETY: as in `tell_the_plugin`.
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
pub(crate) struct LoggedCall {
    pub call: String,
    pub plugin: u64,
    pub thread: String,
    pub processed: u64,
}

pub(crate) fn lifecycle(path: &Path) -> Vec<LoggedCall> {
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

/// A check about the host and not one format runs once per format.
pub(crate) const FORMATS: [PluginFormat; 2] = [PluginFormat::Clap, PluginFormat::Vst3];

pub(crate) fn plugin_folder(root: &Path) -> PathBuf {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    test_vst3_plugin::install_into(&folder);
    folder
}

pub(crate) fn plugin_folder_of(root: &Path, format: PluginFormat) -> PathBuf {
    let folder = root.join("plugins");
    match format {
        PluginFormat::Clap => test_clap_plugin::install_into(&folder),
        PluginFormat::Vst3 => test_vst3_plugin::install_into(&folder),
    };
    folder
}

/// The runtime scans with its own executable; a test with this crate's `plugin-scan`.
pub(crate) fn scanner() -> ScanCommand {
    ScanCommand::new(env!("CARGO_BIN_EXE_plugin-scan"), [])
}

pub(crate) fn no_cache() -> ScanCache {
    ScanCache::none()
}

pub(crate) fn plugin_id(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::Clap => test_clap_plugin::PLUGIN_ID,
        PluginFormat::Vst3 => test_vst3_plugin::PLUGIN_ID,
    }
}

pub(crate) fn record(format: PluginFormat, state_asset: &str) -> PluginRecord {
    PluginRecord::new(format, plugin_id(format), state_asset).expect("a plugin record")
}

/// A CLAP asset is the plugin's own bytes. A VST 3 asset is the host's container of its two
/// states: `SVT3`, then the component's state with its length, then the controller's.
pub(crate) fn saved_state(format: PluginFormat, bytes: &[u8]) -> test_plugin_support::SavedState {
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

pub(crate) fn saved_transpose(format: PluginFormat, bytes: &[u8]) -> i32 {
    saved_state(format, bytes).semitones
}

/// In hundredths, so 100 is the level of a plugin nobody edited.
pub(crate) fn saved_edit_level(bytes: &[u8]) -> i32 {
    saved_state(PluginFormat::Vst3, bytes).edit_level
}

/// `None` when the asset holds no controller state.
pub(crate) fn saved_controller_level(bytes: &[u8]) -> Option<i32> {
    assert_eq!(&bytes[..4], b"SVT3", "not a VST 3 state asset");
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let rest = &bytes[8 + length..];
    let controller = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    test_plugin_support::load_controller_state(&rest[4..4 + controller])
}

pub(crate) fn vst3_state(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut bytes = b"SVT3".to_vec();
    bytes.extend_from_slice(&(component.len() as u32).to_le_bytes());
    bytes.extend_from_slice(component);
    bytes.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    bytes.extend_from_slice(controller);
    bytes
}

pub(crate) fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

pub(crate) fn state_asset(name: &str) -> AssetName {
    AssetName::new("plugin-state", name, "bin").expect("an asset name")
}

/// An open project on a temporary folder with an offline engine and a plugin host that looks
/// in a folder of its own.
pub(crate) struct Harness {
    pub project: Project,
    pub engine: Engine,
    pub plugins: Plugins,
    pub folder: tempfile::TempDir,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::open(tempfile::tempdir().expect("a temporary folder"), true)
    }

    /// What `--render` opens.
    pub(crate) fn rendering_offline() -> Self {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let scan_folder = plugin_folder(folder.path());
        let plugins = Plugins::new(vec![scan_folder], scanner(), no_cache());
        Self::with_plugins_and_engine(
            folder,
            plugins,
            EngineConfig::new(SAMPLE_RATE, 2).rendering_offline(),
        )
    }

    pub(crate) fn reopen(self) -> Self {
        let Self {
            project, folder, ..
        } = self;
        drop(project);
        Self::open(folder, true)
    }

    pub(crate) fn open(folder: tempfile::TempDir, writes_state: bool) -> Self {
        let scan_folder = plugin_folder(folder.path());
        Self::open_with_paths(folder, vec![scan_folder], writes_state)
    }

    pub(crate) fn open_with_paths(
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

    pub(crate) fn with_plugins(folder: tempfile::TempDir, plugins: Plugins) -> Self {
        Self::with_plugins_and_engine(folder, plugins, EngineConfig::new(SAMPLE_RATE, 2))
    }

    pub(crate) fn with_plugins_and_engine(
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
    pub(crate) fn add_track(&mut self, record: PluginRecord, played: Vec<Played>) {
        let mut changes = Changes::new();
        changes.create(id("track"), Rack::default());
        changes.create(id("track/keys"), Keys { played });
        changes.create(id("track/instrument"), record);
        self.project
            .commit("Add track", changes)
            .expect("the track is added");
    }

    /// A rack `track` whose instrument is a steady level, with the plugin of `record` as its
    /// effect.
    pub(crate) fn add_level_track(&mut self, value: f32, record: PluginRecord) {
        let mut changes = Changes::new();
        changes.create(id("track"), Rack::default());
        changes.create(id("track/instrument"), Level { value });
        changes.create(id(&format!("track/{EFFECT}")), record);
        self.project
            .commit("Add track", changes)
            .expect("the track is added");
    }

    pub(crate) fn add_effect(&mut self, record: PluginRecord) {
        let mut changes = Changes::new();
        changes.create(id(&format!("track/{EFFECT}")), record);
        self.project
            .commit("Add effect", changes)
            .expect("the effect is added");
    }

    /// The offset, in hundredths, is what the effect half of the test plugin adds to every
    /// sample.
    pub(crate) fn write_offset(&self, format: PluginFormat, name: &str, offset: i32) {
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

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    pub(crate) fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the folder");
        }
        std::fs::write(&path, contents).expect("the file is written");
        self.project
            .apply_outside_changes(&[path])
            .expect("the change applies")
    }

    pub(crate) fn problems(&self) -> Vec<String> {
        let problems = self.project.problems().into_iter();
        problems
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect()
    }

    /// Renders with nothing but the engine, and counts what this thread allocated meanwhile.
    pub(crate) fn render_counting_allocations(&mut self, frames: usize) -> (Render, u64) {
        let mut output = vec![0.0_f32; frames * 2];
        let ((), allocations) = allocations_during(|| {
            for buffer in output.chunks_mut(512 * 2) {
                self.engine.process_block(buffer);
            }
        });
        (Render { output }, allocations)
    }

    /// So a test decides itself when the host does its main-thread work.
    pub(crate) fn render_without_polling(&mut self, frames: usize) -> Render {
        let mut output = vec![0.0_f32; frames * 2];
        for buffer in output.chunks_mut(512 * 2) {
            self.engine.process_block(buffer);
        }
        self.project.engine().poll().expect("the engine polls");
        Render { output }
    }

    /// Renders in device buffers of 512, interleaved, and polls the host after every buffer as
    /// the runtime does.
    pub(crate) fn render(&mut self, frames: usize) -> Render {
        let mut output = vec![0.0_f32; frames * 2];
        for buffer in output.chunks_mut(512 * 2) {
            self.engine.process_block(buffer);
            self.project.engine().poll().expect("the engine polls");
            self.plugins.poll(&self.project);
            self.plugins.send_restarts(&mut self.project);
            let errors = self.plugins.follow_pins(&mut self.project);
            assert!(errors.is_empty(), "{errors:?}");
        }
        Render { output }
    }

    pub(crate) fn play(&mut self, frames: usize) -> Render {
        self.project.engine().play();
        self.render(frames)
    }
}

pub(crate) struct Render {
    output: Vec<f32>,
}

impl Render {
    pub(crate) fn left(&self) -> Vec<f32> {
        self.output.iter().step_by(2).copied().collect()
    }

    pub(crate) fn right(&self) -> Vec<f32> {
        self.output.iter().skip(1).step_by(2).copied().collect()
    }

    pub(crate) fn samples(&self) -> &[f32] {
        &self.output
    }

    pub(crate) fn first_sound(&self) -> Option<usize> {
        self.left().iter().position(|sample| *sample != 0.0)
    }
}
