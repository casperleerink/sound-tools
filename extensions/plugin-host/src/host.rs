//! The plugins this project has loaded, whatever their format.
//!
//! [`Plugins`] lives on the thread the project lives on. Both formats put a plugin's own handle
//! on the application's main thread and allow only its audio side on the audio thread, so this
//! table keeps the handles and hands the audio sides to the engine. Nothing here knows CLAP or
//! VST 3: a format is a [`crate::backend::LoadedPlugin`].
//!
//! The rule for saving: a plugin's state is written to its asset when the plugin says it
//! changed, at the next [`Plugins::poll`] and then at most once a second while it keeps saying
//! so, and always when the plugin goes or the project closes. A crash can lose up to a second
//! of a plugin's own changes, and anything a plugin changed without saying so.
//!
//! The table never decides what the engine gets. [`Plugins::open`] loads a plugin and hands it
//! over every time its record names another plugin or state file, and [`Plugins::poll`] lets
//! go of every entry whose record no longer says what the entry holds. So an edit that the
//! project rejects, which never reaches the engine, leaves nothing behind here either. A record
//! whose pins are all that changed keeps its plugin: [`Plugins::follow_pins`] sends the pins as
//! the record has them at each poll, so a rejected edit is never sent.
//!
//! This file holds the table itself: what a record loads, what a poll serves and saves, and a
//! restart. The rest is in `host/`, as more `impl Plugins`:
//! - `scanning`: the scan of this machine's plugins, looking again, and records that wait.
//! - `pins`: the pins of a record, both ways, and one undo step per turn of a knob.
//! - `windows`: the plugins' own windows and where they are kept.

mod pins;
mod scanning;
mod windows;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::WindowHandle;
use sound_core::{AssetName, Assets, InstanceId, PrepareConfig, Project};

use self::pins::{PinGesture, PinState, Shown, listed_lanes, pins_of};
use self::scanning::Scanning;
use self::windows::write_placements;
use crate::backend::LoadedPlugin;
use crate::parameters::{Parameter, ParameterValue, by_id, pin_problem};
use crate::placements::{PlacementStore, Placements};
use crate::processor::{
    AutomatedPin, AutomatedPins, HostedPlugin, HostedUpdate, LanedPins, Playing,
};
use crate::scan::{ScanCache, ScanCommand};
use crate::window::{PluginFrame, PluginWindow};
use crate::{PluginFormat, PluginRecord};

/// How often a plugin that keeps saying its state changed is written. A plugin marks itself
/// dirty on every step of a knob drag, and serializing a sampler's state is not cheap, so the
/// first change is written at once and then at most one write a second. Going or closing
/// writes whatever is left, so nothing is lost by waiting.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);

/// What the host tells a plugin about itself.
pub(crate) const HOST_NAME: &str = "Sound Tools";
pub(crate) const HOST_VENDOR: &str = "Sound Tools";
pub(crate) const HOST_URL: &str = "https://github.com/casperleerink/sound-tools";
pub(crate) const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a plugin record is not playing. Each becomes one line in `problems.txt`.
///
/// A message says what happens to the slot and not what happens to the track: this host knows
/// no slots. What a missing plugin costs is the owner's rule, which for a track is that an
/// instrument goes silent and an effect lets the sound through. An outside agent read the
/// older wording, which spoke of the track, and called it a disagreement with the docs.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PluginProblem {
    #[error(
        "this machine has no {format} plugin with the id {plugin_id:?}. The record is left as it is and nothing plays through it: a missing instrument is silent, a missing effect lets the sound through unchanged. Install the plugin, or correct `plugin_id`"
    )]
    NotInstalled { format: String, plugin_id: String },
    #[error(
        "the plugins of this machine are still being looked at, so {plugin_id:?} is not there yet. Nothing plays through it until the scan reaches it, which needs nothing of you"
    )]
    StillScanning { plugin_id: String },
    #[error("the plugin {plugin_id:?} did not load: {message}")]
    DidNotLoad { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} asked to be started again, because its latency or its buses changed, and did not start: {message}. Nothing plays through it until its record changes"
    )]
    DidNotRestart { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be read: {message}")]
    StateNotRead { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be saved: {message}")]
    StateNotWritten { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} offers the host no way to send the sustain pedal, so the pedal does not reach it. Its notes play"
    )]
    NoPedal { plugin_id: String },
    #[error("the plugin {plugin_id:?} has no window of its own")]
    NoWindow { plugin_id: String },
    #[error("the window of the plugin {plugin_id:?} did not open: {message}")]
    WindowDidNotOpen { plugin_id: String, message: String },
    #[error("where the plugin windows are is not kept on this computer: {message}")]
    WindowPlaces { message: String },
    #[error(
        "the plugin {plugin_id:?} has no parameter with the id {id} that a host may set, so `parameters.{id}` moves nothing. The rest of the record plays. `sound-tools --plugin-params` lists the ones it has"
    )]
    NoSuchParameter { plugin_id: String, id: u32 },
    #[error("what a plugin changed of the parameters its record holds was not written: {message}")]
    PinsNotWritten { message: String },
    #[error(
        "`parameters.{id}.value` is {value}, outside the range of {name:?} of the plugin {plugin_id:?}, {minimum} to {maximum}, so it moves nothing. The rest of the record plays"
    )]
    OutOfRange {
        plugin_id: String,
        id: u32,
        name: String,
        value: f64,
        minimum: f64,
        maximum: f64,
    },
}

/// What [`Plugins::open`] gives back.
pub(crate) struct Opened {
    pub engine: ForEngine,
    /// What to report about this record every time the behaviour runs, such as a plugin the
    /// sustain pedal cannot reach, or a pin it has no parameter for. These are not failures:
    /// the plugin plays.
    pub notes: Vec<PluginProblem>,
    /// The pins of the record, those an automation lane may move marked, see
    /// [`Hosted::automated`].
    pub lanes: Vec<AutomatedPin>,
}

/// What the behaviour hands the engine.
pub(crate) enum ForEngine {
    /// Nothing: the engine plays on with the plugin it has, which is the one this table holds
    /// for the record. Only the pins of the record changed, and the poll sends those.
    Same,
    /// The audio side of a plugin that was just loaded, or `None` from a host that loads none
    /// ([`Plugins::listing`]), which is a slot that plays nothing and reports nothing.
    Play(Option<Playing>),
}

/// One plugin this project holds, with the record it came from.
///
/// It is `loaded` while a record names it and `retired` once it does not. A retired one is kept
/// until the engine gives its audio side back, because letting it go before that would leave
/// the two ends of one plugin in different hands. It is polled and saved until then, so a
/// plugin that is still playing while it waits does not lose what it changes.
struct Hosted {
    format: PluginFormat,
    plugin_id: String,
    asset: AssetName,
    plugin: Box<dyn LoadedPlugin>,
    /// When its state was last written, for the once-a-second rule.
    last_saved: Option<Instant>,
    /// The plugin said its state changed and it is not written yet. It is kept here and not in
    /// the backend, so that a change the once-a-second rule made wait is written by a later
    /// poll and is never forgotten.
    pending_save: bool,
    /// Whether the plugin has a window at all. A card of a rack reads it on every frame it
    /// draws, and a frame must call into no plugin. CLAP answers it while the plugin loads;
    /// VST 3 cannot be asked without building the plugin's whole interface, so it says yes and
    /// [`Plugins::open_window`] writes the answer here the first time one is asked for.
    has_window: bool,
    /// The plugin's own window, while it is open.
    window: PluginWindow,
    /// What the engine's processors were prepared with, for starting the plugin again.
    config: PrepareConfig,
    /// Where a restart the plugin asked for stands, see [`Plugins::restarts`].
    restart: Restart,
    /// The next run of the behaviour loads the plugin again instead of keeping it: the plugin
    /// asked to be loaded again, or it did not start again after a restart.
    needs_load: bool,
    /// What the load reported that stays true while the plugin plays, which a run of the
    /// behaviour that keeps the plugin reports again.
    notes: Vec<PluginProblem>,
    /// Every parameter a host may set, by id, read from the plugin the first time a pin or the
    /// card needs them and again when the plugin says they changed. Shared, so a card can keep
    /// the list of a plugin with thousands of parameters without a copy per frame.
    parameters: Option<Rc<BTreeMap<u32, Parameter>>>,
    /// Where each pin of the record that moves something stands between the record and the
    /// plugin.
    pins: BTreeMap<u32, PinState>,
    /// The pins the audio side says a lane holds, see [`Hosted::follow_lanes`].
    laned: Arc<LanedPins>,
    /// The pins of the record as the behaviour last ran, which the lane players were given
    /// with them. A plugin started again gets these, so the index of a lane means the same
    /// pin to both until the next run, whatever the plugin says of its parameters meanwhile.
    lanes: AutomatedPins,
    /// What the plugin's own window was last shown of each pin a lane holds.
    shown: BTreeMap<u32, Shown>,
}

/// A plugin that asked to be started again, which is how both formats let a latency change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Restart {
    Idle,
    /// The plugin asked. The engine is to give its audio side back.
    Asked,
    /// The engine was told to give the audio side back. The first poll that finds it back
    /// starts the plugin again and hands it over.
    Waiting,
}

impl Hosted {
    /// Whether this entry holds what `record` names: the plugin and its state file.
    fn holds(&self, record: &PluginRecord) -> bool {
        record.format == self.format
            && record.plugin_id == self.plugin_id
            && record.asset() == self.asset
    }

    /// Whether a run of the behaviour with `record` may keep this plugin as it plays: the
    /// same plugin, state file and set-up, and nothing asked for a load since. A plugin that is
    /// being started again is kept too: the engine gets it when it has started, and the pins
    /// are sent to it then.
    fn keeps(&self, record: &PluginRecord, config: PrepareConfig) -> bool {
        self.holds(record) && self.config == config && !self.needs_load
    }

    /// Every parameter a host may set, read from the plugin the first time it is asked.
    fn parameters(&mut self) -> &Rc<BTreeMap<u32, Parameter>> {
        let Self {
            parameters, plugin, ..
        } = self;
        parameters.get_or_insert_with(|| Rc::new(listed(plugin.as_mut())))
    }

    /// What a run of the behaviour reports: the notes of the load, and every pin of `record`
    /// that moves nothing.
    fn notes(&mut self, record: &PluginRecord) -> Vec<PluginProblem> {
        let mut notes = self.notes.clone();
        if !record.parameters.is_empty() {
            let plugin_id = self.plugin_id.clone();
            let parameters = self.parameters();
            let pins = record.parameters.iter();
            notes
                .extend(pins.filter_map(|(id, pin)| pin_problem(&plugin_id, parameters, *id, pin)));
        }
        notes
    }

    /// Whether the record of `id` in the project still says what this entry holds.
    fn matches(&self, project: &Project, id: &InstanceId) -> bool {
        record_of(project, id).is_some_and(|record| self.holds(record))
    }
}

/// The parameters a plugin lists, by id.
fn listed(plugin: &mut dyn LoadedPlugin) -> BTreeMap<u32, Parameter> {
    by_id(plugin.parameters())
}

/// The plugin record of `id`, when there is one.
fn record_of<'a>(project: &'a Project, id: &InstanceId) -> Option<&'a PluginRecord> {
    project.state(&project.resolve::<PluginRecord>(id)?)
}

#[derive(Default)]
struct Table {
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Hosted>,
    /// Something a card of a plugin shows changed since whoever draws the rack last asked: a
    /// window opened or closed, or a plugin's parameters or their text.
    card_changed: bool,
    /// Windows whose plugin has gone. Their views are already freed; taking a window down
    /// needs the application, which the moments that find them do not have.
    finished_windows: Vec<WindowHandle<PluginFrame>>,
    /// Where each plugin's window was and whether it was open, read from this machine's
    /// store when the project is first polled, and written back there. See `placements.rs`
    /// and [`Plugins::settle_windows`].
    placements: Placements,
    /// A placement changed since it was last written, or since a write of it failed: a write
    /// that fails is not tried again until a window changes again, so it is said once.
    placements_changed: bool,
    /// When they were last written, for the once-a-second rule a plugin's state has too: a
    /// window that is dragged moves many times a second.
    placements_written: Option<Instant>,
    /// What went wrong while the windows were settled, which has no project to report to. The
    /// next poll gives it.
    window_problems: Vec<PluginProblem>,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    cache: ScanCache,
    scanned: Arc<Mutex<Scanning>>,
    /// Whether a scan has run or is running. A host that scans in the background sets it as it
    /// starts, so nothing blocks on the first plugin a project names.
    started: Cell<bool>,
    /// Whether this host scans in the background, which is the window's. Only such a host looks
    /// again while it runs: the others run once and end.
    in_background: Cell<bool>,
    /// When the host last started to look again, see [`Plugins::look_again_while_waiting`].
    last_look: Cell<Option<Instant>>,
    /// Ends the scan thread between bundles when the host goes.
    stop: Arc<AtomicBool>,
    /// The generation the last poll acted on.
    seen: Cell<u64>,
    /// Records whose plugin the scan has not found yet, and the ones that are worth running
    /// again now that it has.
    waiting: RefCell<BTreeSet<InstanceId>>,
    retries: RefCell<Vec<InstanceId>>,
    /// A read-only project (`--inspect`, `--render`) never writes plugin state.
    writes_state: bool,
    /// Whether a record's plugin is loaded at all. `--inspect` does not, see [`Plugins::listing`].
    loads: bool,
    /// The `assets/` folder of the project, from the first plugin that loaded. Kept so that
    /// dropping the host can still save, see [`Drop`].
    assets: RefCell<Option<Assets>>,
    /// The folder of the project, from the first poll. It names the windows, and this machine
    /// keeps their places by it.
    root: RefCell<Option<std::path::PathBuf>>,
    /// Where this machine keeps the plugin windows of every project, next to the scan cache.
    placement_store: PlacementStore,
    table: RefCell<Table>,
    /// What each plugin is changing of its pins, by record, see [`Plugins::follow_pins`]. Kept
    /// out of the table, which a behaviour that a write of a record runs borrows.
    gestures: RefCell<BTreeMap<InstanceId, PinGesture>>,
}

/// The last chance to free what a plugin holds for its window and to save its state. On macOS
/// the application ends without unwinding: GPUI drops the main window and its views, and with
/// them the session and the project, and then the process is gone. Dropping the project drops
/// the registry, the behaviour and this host, so that is the moment. A plugin's window is
/// still standing then, empty, and goes with the application; the runtime ends the application
/// with the main window for exactly that reason. Whoever polls the host must therefore hold
/// it weakly ([`Plugins::downgrade`]), else nothing is saved. [`Plugins::close`] does the same
/// with the project still in hand, and leaves nothing for this.
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let assets = self.writes_state.then(|| self.assets.get_mut().clone());
        let table = self.table.get_mut();
        for hosted in table.loaded.values_mut().chain(&mut table.retired) {
            // Every plugin's view goes, whether this project writes or not. The windows that
            // held them cannot be taken down from here, and nothing will: this is the project
            // closing, which on macOS is the application quitting.
            // The handle is left where it is: the window goes with the application.
            let _window = hosted.window.give_up(hosted.plugin.gui());
            if let Some(Some(assets)) = &assets
                && let Err(problem) = save(hosted, assets)
            {
                // Nobody is left to tell. The composer at least sees it in the terminal.
                eprintln!("error: {problem}");
            }
        }
        if self.writes_state
            && let Some(root) = self.root.get_mut()
            && let Err(problem) = write_placements(table, &self.placement_store, root, None)
        {
            eprintln!("error: {problem}");
        }
    }
}

/// The plugins of one project. Cheap to clone: every copy is the same table.
///
/// The tool's behaviour keeps one, and so does whoever polls the project. It lives on one
/// thread, like the project.
#[derive(Clone)]
pub struct Plugins(Rc<Inner>);

/// A handle that does not keep the plugins alive. Whoever polls the host holds one of these,
/// so that dropping the project is what ends the host and saves every plugin.
#[derive(Clone)]
pub struct WeakPlugins(Weak<Inner>);

impl WeakPlugins {
    pub fn upgrade(&self) -> Option<Plugins> {
        self.0.upgrade().map(Plugins)
    }
}

impl Plugins {
    pub fn downgrade(&self) -> WeakPlugins {
        WeakPlugins(Rc::downgrade(&self.0))
    }

    /// A host that saves plugin state into the project.
    pub fn new(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, true, true)
    }

    /// A host for a project that is open read-only. It loads plugins and never writes.
    pub fn read_only(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, true)
    }

    /// A host that looks a plugin up and never loads one. `runtime --inspect` uses it.
    ///
    /// Loading a plugin runs somebody else's code in this process, and a plugin that only ever
    /// loads and goes can take the process down with it: Crow Hill Origins ends `--inspect`
    /// in a segmentation fault in its own teardown, having never processed a block. Inspecting
    /// prints a project and makes no sound, so it needs no plugin at all.
    ///
    /// It still does everything this side can do without the plugin, so that `--inspect`
    /// reports what it always reported: the scan says whether this machine has the plugin, and
    /// the state asset is read, so a state file that cannot be read is still a problem an
    /// agent sees before playback finds it. What is lost is only what the plugin itself can
    /// say, such as a note port that takes no sustain pedal.
    pub fn listing(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, false)
    }

    fn with(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
        writes_state: bool,
        loads: bool,
    ) -> Self {
        let placement_store = cache.placement_store();
        Self(Rc::new(Inner {
            search_paths,
            scanner,
            cache,
            scanned: Arc::new(Mutex::new(Scanning::default())),
            started: Cell::new(false),
            in_background: Cell::new(false),
            last_look: Cell::new(None),
            stop: Arc::new(AtomicBool::new(false)),
            seen: Cell::new(0),
            waiting: RefCell::new(BTreeSet::new()),
            retries: RefCell::new(Vec::new()),
            writes_state,
            loads,
            assets: RefCell::new(None),
            root: RefCell::new(None),
            placement_store,
            table: RefCell::new(Table::default()),
            gestures: RefCell::new(BTreeMap::new()),
        }))
    }

    /// What the parameter `parameter` of this record's plugin is now, with the plugin's own
    /// text for it. `None` when no plugin is loaded for the record, or the plugin cannot say.
    ///
    /// It calls into the plugin, so it belongs to the main thread and not to drawing a frame.
    /// It gives up rather than wait for a table that a plugin's own call has borrowed.
    pub fn parameter_value(&self, id: &InstanceId, parameter: u32) -> Option<ParameterValue> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        let plugin = &mut table.loaded.get_mut(id)?.plugin;
        let value = plugin.value(parameter)?;
        Some(ParameterValue {
            value,
            text: plugin.text(parameter, value),
        })
    }

    /// Every parameter a host may set of this record's plugin, by id. `None` when no plugin is
    /// loaded for the record. The same list until the plugin says its parameters changed, so a
    /// card can tell a new list by its pointer.
    ///
    /// The first call asks the plugin, so it belongs to the main thread and not to drawing a
    /// frame. It gives up rather than wait for a table that a plugin's own call has borrowed.
    pub fn parameters(&self, id: &InstanceId) -> Option<Rc<BTreeMap<u32, Parameter>>> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        Some(table.loaded.get_mut(id)?.parameters().clone())
    }

    /// The plugin's own text for `value` of the parameter `parameter`, such as `1.2 kHz`, which
    /// a card shows under the control of a pin. `None` when no plugin is loaded for the record or
    /// the plugin gives no text. It calls into the plugin, as [`Self::parameter_value`].
    pub fn parameter_text(&self, id: &InstanceId, parameter: u32, value: f64) -> Option<String> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        table.loaded.get_mut(id)?.plugin.text(parameter, value)
    }

    /// Loads the plugin the record names and gives it to the caller for the engine.
    ///
    /// It loads every time the record names another plugin, another state file, or the plugin
    /// asked to be loaded again, and on every run whose processor is new, which is a record
    /// that came back. Then nothing here has to guess what the engine holds: the caller hands
    /// over a plugin, and an edit the project rejects simply never reaches the engine. A run
    /// whose record changed only its pins keeps the plugin as it plays, when `processor_kept`
    /// says the engine still has it: reloading a sampler for a turn of a knob would stop its
    /// sound for seconds.
    ///
    /// Whatever this instance held goes first, saved and waiting to be let go of, so a failure
    /// below leaves no entry behind and the record and the engine agree: silence.
    pub(crate) fn open(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        assets: &Assets,
        config: PrepareConfig,
        processor_kept: bool,
    ) -> Result<Opened, PluginProblem> {
        // Kept for the drop of this host, which is the last moment a plugin can be saved.
        *self.0.assets.borrow_mut() = Some(assets.clone());
        self.0.waiting.borrow_mut().remove(id);
        {
            let mut table = self.0.table.borrow_mut();
            if let Some(hosted) = table.loaded.get_mut(id)
                && processor_kept
                && hosted.keeps(record, config)
            {
                return Ok(Opened {
                    engine: ForEngine::Same,
                    notes: hosted.notes(record),
                    lanes: hosted.automated(record),
                });
            }
            if let Some(hosted) = table.loaded.remove(id) {
                // The same plugin loading again, which is a reload or another state file, gets
                // its window back where it was. Another plugin in the record does not get the
                // window of the one it replaces.
                if hosted.format != record.format || hosted.plugin_id != record.plugin_id {
                    table.keep_open(id, false);
                }
                retire(hosted, &mut table, self.0.writes_state.then_some(assets));
            }
        }
        let asset = record.asset();
        match self.load(id, record, &asset, assets, config) {
            Ok(opened) => Ok(opened),
            Err(problem) => {
                // A plugin the scan has not reached yet is worth trying again when it has, and
                // one this machine does not have when a look again finds it: the composer may
                // install it while the app runs. The poll looks again while one waits.
                if matches!(
                    problem,
                    PluginProblem::StillScanning { .. } | PluginProblem::NotInstalled { .. }
                ) {
                    self.0.waiting.borrow_mut().insert(id.clone());
                }
                Err(problem)
            }
        }
    }

    fn load(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        asset: &AssetName,
        assets: &Assets,
        config: PrepareConfig,
    ) -> Result<Opened, PluginProblem> {
        self.ensure_scan();
        let scanned = self.known();
        let found = match scanned.find(record.format, &record.plugin_id) {
            Some(found) => found.clone(),
            None if !scanned.finished => {
                return Err(PluginProblem::StillScanning {
                    plugin_id: record.plugin_id.clone(),
                });
            }
            None => {
                return Err(PluginProblem::NotInstalled {
                    format: record.format.name().to_string(),
                    plugin_id: record.plugin_id.clone(),
                });
            }
        };
        // Nothing checks here what the plugin says it is. One record serves an instrument slot
        // and an effect slot, and this host knows no slots: a track decides what it wires a
        // record to. What a plugin declares is what a picker offers it for, and that is not the
        // same question: Spectral Freeze on the machine this was written on declares itself an
        // instrument and is an effect.
        // Empty bytes are a state file that was made to reserve its name, which is how a plugin
        // the window puts on a track gets one, and that the plugin has not written into yet.
        let saved = assets
            .read(asset)
            .map_err(|error| PluginProblem::StateNotRead {
                plugin_id: record.plugin_id.clone(),
                message: error.to_string(),
            })?
            .filter(|bytes| !bytes.is_empty());
        // A host that only lists stops here, after everything this side can check without the
        // plugin: the scan has the plugin and its state file can be read. Nothing of the
        // plugin's own code runs in this process, so nothing of it can fail here, in its
        // `process`, or in the teardown it never expected. What is left out is only what the
        // plugin itself would have said.
        if !self.0.loads {
            return Ok(Opened {
                engine: ForEngine::Play(None),
                notes: Vec::new(),
                lanes: listed_lanes(record),
            });
        }
        let pins = &record.parameters;
        let opening = match record.format {
            PluginFormat::Clap => crate::clap::load(&found, saved.as_deref(), config, pins),
            PluginFormat::Vst3 => crate::vst3::load(&found, saved.as_deref(), config, pins),
        }?;
        let crate::backend::Opening {
            mut plugin,
            started,
            notes,
        } = opening;
        let has_window = plugin.gui().is_some_and(|gui| gui.is_offered());
        let mut hosted = Hosted {
            format: record.format,
            plugin_id: record.plugin_id.clone(),
            asset: asset.clone(),
            plugin,
            last_saved: None,
            pending_save: false,
            has_window,
            window: PluginWindow::default(),
            config,
            restart: Restart::Idle,
            needs_load: false,
            notes,
            parameters: None,
            pins: BTreeMap::new(),
            laned: Arc::new(LanedPins::new()),
            lanes: AutomatedPins::NONE,
            shown: BTreeMap::new(),
        };
        // The record wins over the state just loaded. The backend gave the plugin every pin
        // before it was activated, where the format lets it. They go again in the first block,
        // for a plugin that took nothing then, which changes nothing for one that did.
        hosted.send_pins(record);
        let notes = hosted.notes(record);
        let lanes = hosted.automated(record);
        let laned = hosted.laned.clone();
        self.0.table.borrow_mut().loaded.insert(id.clone(), hosted);
        Ok(Opened {
            engine: ForEngine::Play(Some(Playing { started, laned })),
            notes,
            lanes,
        })
    }

    /// Asks whoever polls to run the behaviour of `id` again, once.
    fn retry(&self, id: &InstanceId) {
        let mut retries = self.0.retries.borrow_mut();
        if !retries.contains(id) {
            retries.push(id.clone());
        }
    }

    /// Records whose behaviour is worth running again: their plugin was not there when it ran
    /// and may be now, because the scan has learned something since, or their plugin asked to
    /// be unloaded and loaded again. Whoever polls runs their behaviour again, which is what
    /// makes a plugin play and takes its problem away, and what loads a plugin afresh.
    pub fn take_retries(&self) -> Vec<InstanceId> {
        std::mem::take(&mut self.0.retries.borrow_mut())
    }

    /// Whether any plugin's window opened or closed, or a plugin said its parameters or their
    /// text changed, since the last call. Whoever polls asks, so the cards are drawn again: one
    /// says "Open window" or "Close window", and reads its parameters and their text again.
    pub fn take_card_change(&self) -> bool {
        std::mem::take(&mut self.0.table.borrow_mut().card_changed)
    }

    /// Saves the state of every plugin, whether it said so or not, and lets them all go.
    ///
    /// Call it when the project closes. A plugin that changes its state without telling the
    /// host is saved here all the same. Nothing is written when the bytes are the ones already
    /// in the project, so a session that changed nothing leaves no diff. A turn of a knob that
    /// is still open ends here, so its record is written too.
    pub fn close(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        for error in self.end_turns(project) {
            problems.push(PluginProblem::PinsNotWritten {
                message: error.to_string(),
            });
        }
        let project = &*project;
        let mut table = self.0.table.borrow_mut();
        if self.0.writes_state
            && let Some(root) = self.0.root.borrow().as_deref()
            && let Err(problem) =
                write_placements(&mut table, &self.0.placement_store, root, Some(project))
        {
            problems.push(problem);
        }
        let Table {
            loaded,
            retired,
            card_changed,
            finished_windows,
            ..
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());
        for hosted in loaded.values_mut().chain(&mut *retired) {
            // Every window closes with the project, whether it writes or not, and the project
            // remembers the ones that were open.
            if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                finished_windows.push(handle);
                *card_changed = true;
            }
            if let Some(assets) = assets
                && let Err(problem) = save(hosted, assets)
            {
                problems.push(problem);
            }
        }
        retired.extend(std::mem::take(loaded).into_values());
        problems
    }

    /// Main-thread work for every plugin this host holds: the callbacks they asked for, the
    /// state they said changed, and letting go of the ones no record names any more. A plugin
    /// that asked to be started again is noted here and started by [`Self::send_restarts`].
    ///
    /// Call it as often as the project is polled.
    pub fn poll(&self, project: &Project) -> Vec<PluginProblem> {
        self.poll_at(project, Instant::now())
    }

    /// Moves every restart a plugin asked for one step on, see [`Self::restarts`]. Call it
    /// after [`Self::poll`], which is what notes that one asked. It needs the project
    /// mutably only to hand the engine the plugin's audio side, which is not an edit: nothing
    /// is written and there is no undo step.
    pub fn send_restarts(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        // Outside the borrow of the table: the engine is the project's.
        let updates = self.restarts(project, &mut problems);
        for (id, plugin_id, update) in updates {
            if let Err(error) = project.send::<HostedPlugin>(&id, crate::PROCESSOR, update) {
                problems.push(PluginProblem::DidNotRestart {
                    plugin_id,
                    message: error.to_string(),
                });
            }
        }
        problems
    }

    /// A plugin that asked to be started again goes in two steps, one poll or more apart.
    /// First the engine is told to give its audio side back, which stops it on the audio
    /// thread. Once the engine has, the plugin is deactivated, activated again and handed back
    /// with the latency it says it has now, and the engine compensates that from the block it
    /// arrives in. In between the slot plays what an empty one does: silence for an
    /// instrument, the sound going through unchanged for an effect.
    ///
    /// Gives what to send the engine, for which instance.
    fn restarts(
        &self,
        project: &Project,
        problems: &mut Vec<PluginProblem>,
    ) -> Vec<(InstanceId, String, HostedUpdate)> {
        let mut table = self.0.table.borrow_mut();
        let mut updates = Vec::new();
        for (id, hosted) in &mut table.loaded {
            match hosted.restart {
                Restart::Idle => {}
                Restart::Asked => {
                    hosted.restart = Restart::Waiting;
                    let none = HostedUpdate::Plugin(None, AutomatedPins::NONE);
                    updates.push((id.clone(), hosted.plugin_id.clone(), none));
                }
                Restart::Waiting => match hosted
                    .plugin
                    .restart(hosted.config, &pins_of(hosted, project, id))
                {
                    // The engine has not given it back yet.
                    None => {}
                    Some(Ok(started)) => {
                        hosted.restart = Restart::Idle;
                        // What was on its way to the old audio side went with it, and the
                        // record may have changed meanwhile: every pin goes again, as the
                        // record has it, ahead of the first block of the new audio side.
                        hosted.pins.clear();
                        if let Some(record) = record_of(project, id) {
                            hosted.send_pins(record);
                        }
                        let laned = hosted.laned.clone();
                        let playing = Playing { started, laned };
                        let update = HostedUpdate::Plugin(Some(playing), hosted.lanes);
                        updates.push((id.clone(), hosted.plugin_id.clone(), update));
                    }
                    Some(Err(problem)) => {
                        // Silent until its record changes, which loads it.
                        hosted.restart = Restart::Idle;
                        hosted.needs_load = true;
                        problems.push(problem);
                    }
                },
            }
        }
        updates
    }

    /// [`Self::poll`] with the time given, so a test can move it.
    pub fn poll_at(&self, project: &Project, now: Instant) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        self.remember_the_windows(project, &mut problems);
        self.note_what_the_scan_found(project);
        self.look_again_while_waiting(now);
        let mut table = self.0.table.borrow_mut();
        problems.append(&mut table.window_problems);
        let Table {
            loaded,
            retired,
            card_changed,
            finished_windows,
            ..
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());
        // Windows that close for good, see `Table::keep_open`. Noted after the loop, which
        // has the table in pieces.
        let mut closed_for_good = Vec::new();

        // Everything the records no longer say. A record that is gone, that is no longer a
        // plugin, or that names another plugin or another state file than the entry holds:
        // the last of those is an edit the project rolled back after this host had loaded it.
        let stale: Vec<InstanceId> = loaded
            .iter()
            .filter(|(id, hosted)| !hosted.matches(project, id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(mut hosted) = loaded.remove(&id) {
                // The window of a plugin that is going goes with it: a record that was deleted
                // from a file or by an undo leaves no window behind, and an undo that brings
                // the record back does not bring the window.
                if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                    finished_windows.push(handle);
                    *card_changed = true;
                }
                closed_for_good.push(id.clone());
                // Saved on the way out, so undo of a delete brings the plugin back as it
                // sounded and not as it was last written.
                if let Some(assets) = assets
                    && let Err(problem) = save(&mut hosted, assets)
                {
                    problems.push(problem);
                }
                retired.push(hosted);
            }
        }

        // Retired plugins are served too: one that is still playing, because the engine has
        // not given its audio side back yet, must not miss a callback or lose a change.
        let serving = loaded.iter_mut().map(|(id, hosted)| (Some(id), hosted));
        for (id, hosted) in serving.chain(retired.iter_mut().map(|hosted| (None, hosted))) {
            let requests = hosted.plugin.poll();
            if let Some(wanted) = requests.window_size {
                hosted.window.wants_size(wanted);
            }
            // The plugin closed its own window, by its title bar or by losing it. This host
            // keeps no window that is not shown.
            if requests.window_closed
                && let Some(handle) = hosted.window.give_up(hosted.plugin.gui())
            {
                finished_windows.push(handle);
                *card_changed = true;
                if let Some(id) = id {
                    closed_for_good.push(id.clone());
                }
            }
            // A plugin that asks to be deactivated and activated again, which is how its
            // latency changes. A retired plugin is marked as well, but only loaded ones are
            // started again, see `Self::restarts`.
            if requests.restart && hosted.restart == Restart::Idle {
                hosted.restart = Restart::Asked;
            }
            // A plugin that asks to be unloaded and loaded again gets exactly what a record
            // that changed gets: whoever polls runs its behaviour again, which saves this one
            // on its way out and loads its record from that state. A retired plugin is going
            // anyway.
            if requests.reload
                && let Some(id) = id
            {
                hosted.needs_load = true;
                self.retry(id);
            }
            // The plugin has other parameters, or other names or text for them. The pins of the
            // record are checked again, by a run of its behaviour that keeps the plugin: that is
            // where what a pin cannot move is reported. A list nobody read yet is read when a pin
            // or a card needs it. The list is a new one even when it is equal, because the
            // plugin's text, which it does not hold, may have changed: a card tells by that.
            if requests.parameters_changed
                && let Some(id) = id
                && let Some(before) = hosted.parameters.take()
            {
                let parameters = Rc::new(listed(hosted.plugin.as_mut()));
                if parameters != before {
                    self.retry(id);
                }
                hosted.parameters = Some(parameters);
                *card_changed = true;
            }
            // The plugin now maps its sustain pedal to nothing, so the pedal stops reaching
            // it. The same line a plugin gets that never mapped one.
            if requests.pedal_unmapped {
                problems.push(PluginProblem::NoPedal {
                    plugin_id: hosted.plugin_id.clone(),
                });
            }
            hosted.pending_save |= requests.state_is_dirty;
            let due = hosted
                .last_saved
                .is_none_or(|last| now.duration_since(last) >= SAVE_INTERVAL);
            if let Some(assets) = assets
                && hosted.pending_save
                && due
            {
                hosted.last_saved = Some(now);
                if let Err(problem) = save(hosted, assets) {
                    problems.push(problem);
                }
            }
        }

        // A plugin may only go once the engine has given its audio side back. Until then
        // letting it go would leave the two ends of one plugin in different hands.
        retired.retain_mut(|hosted| !hosted.plugin.released());
        for id in closed_for_good {
            table.keep_open(&id, false);
        }

        // Where the windows are, at most once a second while one is dragged. The project
        // closing writes whatever is left.
        let due = table
            .placements_written
            .is_none_or(|last| now.duration_since(last) >= SAVE_INTERVAL);
        if self.0.writes_state
            && table.placements_changed
            && due
            && let Some(root) = self.0.root.borrow().as_deref()
        {
            table.placements_written = Some(now);
            let store = &self.0.placement_store;
            if let Err(problem) = write_placements(&mut table, store, root, Some(project)) {
                problems.push(problem);
            }
        }
        problems
    }
}

/// Saves a plugin that is going, when the project is one that writes, and puts its handle
/// where it waits for the engine to give the audio side back.
fn retire(mut hosted: Hosted, table: &mut Table, assets: Option<&Assets>) {
    // A record that now names another plugin takes the window of the old one with it.
    if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
        table.finished_windows.push(handle);
        table.card_changed = true;
    }
    if let Some(assets) = assets
        && let Err(problem) = save(&mut hosted, assets)
    {
        // The caller is inside a behaviour and has no way to report. The composer at least
        // sees it in the terminal.
        eprintln!("error: {problem}");
    }
    table.retired.push(hosted);
}

/// Writes what the plugin says its state is, into the asset its record names. Bytes that are
/// already there are not written again, so a session that changed nothing leaves no diff.
fn save(hosted: &mut Hosted, assets: &Assets) -> Result<(), PluginProblem> {
    let fail = |message: String| PluginProblem::StateNotWritten {
        plugin_id: hosted.plugin_id.clone(),
        message,
    };
    let bytes = hosted.plugin.save_state().map_err(fail)?;
    // Cleared only once the bytes are where they belong, so a write that failed is tried again
    // at a later poll instead of being forgotten.
    let written = || {
        if bytes.is_empty() {
            return Ok(());
        }
        let there = assets
            .read(&hosted.asset)
            .map_err(|error| fail(error.to_string()))?;
        if there.as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }
        assets
            .write(&hosted.asset, &bytes)
            .map_err(|error| fail(error.to_string()))
    };
    let result = written();
    if result.is_ok() {
        hosted.pending_save = false;
    }
    result
}
