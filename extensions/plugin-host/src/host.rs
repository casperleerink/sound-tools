//! The plugins this project has loaded, whatever their format.
//!
//! Both formats keep a plugin's own handle on the main thread and allow only its audio side on
//! the audio thread, so [`Plugins`] keeps the handles and hands the audio sides to the engine.
//!
//! The table never decides what the engine gets. [`Plugins::open`] hands over a plugin every
//! time a record names another one, and [`Plugins::poll`] lets go of every entry whose record no
//! longer says what it holds. So an edit the project rejects leaves nothing behind here either.

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
    AutomatedPin, AutomatedPins, HostedPlugin, HostedUpdate, Laned, Player, PluginAudio, laned_here,
};
use crate::scan::{ScanCache, ScanCommand};
use crate::window::{PluginFrame, PluginWindow};
use crate::{PluginFormat, PluginRecord};

/// At most one state write a second per plugin: a plugin marks itself dirty on every step of a
/// knob drag, and serializing a sampler's state is not cheap. Going or closing writes the rest,
/// so a crash loses at most a second, and whatever a plugin changed without saying so.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);

/// How long closing may wait for plugins in processes of their own, all of them together: one
/// that does not answer by then is killed.
const CLOSING: Duration = Duration::from_secs(10);

/// What the host tells a plugin about itself.
pub(crate) const HOST_NAME: &str = "Sound Tools";
pub(crate) const HOST_VENDOR: &str = "Sound Tools";
pub(crate) const HOST_URL: &str = "https://github.com/casperleerink/sound-tools";
pub(crate) const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Each becomes one line in `problems.txt`. A message speaks of the plugin, never of a track:
/// this host does not know where the record sits.
#[derive(Clone, Debug, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
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
        "{name} stopped working and was closed. The rest plays on. Choose Start again on its card, or open the project again, to bring it back as last saved"
    )]
    Crashed { plugin_id: String, name: String },
    #[error(
        "{name} cannot load until Sound Tools is restarted: Sound Tools was updated while it ran"
    )]
    Updated { name: String },
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

/// How a host runs its plugins in processes of their own.
#[derive(Clone, Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub struct Isolation {
    /// Whether the plugin's process keeps quiet: what it writes to standard error goes nowhere.
    /// A headless host's own output is what its caller reads.
    pub(crate) quiet: bool,
    /// How long one block may take before the plugin counts as hung. A block of 64 frames is
    /// about a millisecond of sound.
    pub(crate) block_timeout: Duration,
    /// How long the plugin's main thread may stop before it counts as hung. The app waits that
    /// long, so it is short.
    pub(crate) hung_after: Duration,
    /// The same while the plugin loads, saves or opens its window, which may take long: as long
    /// as the scan gives a bundle.
    pub(crate) hung_loading_after: Duration,
    pub(crate) windows: bool,
}

impl Isolation {
    pub fn new(quiet: bool) -> Self {
        Self {
            quiet,
            block_timeout: Duration::from_millis(250),
            hung_after: Duration::from_secs(10),
            hung_loading_after: crate::scan::SCAN_TIMEOUT,
            windows: false,
        }
    }

    /// The plugin's process opens the plugin's window, in a window session on macOS.
    pub fn with_windows(mut self) -> Self {
        self.windows = cfg!(target_os = "macos");
        self
    }

    pub fn with_timeouts(
        mut self,
        block: Duration,
        hung_after: Duration,
        hung_loading_after: Duration,
    ) -> Self {
        self.block_timeout = block;
        self.hung_after = hung_after;
        self.hung_loading_after = hung_loading_after;
        self
    }
}

/// What a record held when its plugin stopped working, see `Inner::stopped`.
struct Stopped {
    format: PluginFormat,
    plugin_id: String,
    asset: AssetName,
    name: String,
}

impl Stopped {
    fn of(record: &PluginRecord, name: String) -> Self {
        Self {
            format: record.format,
            plugin_id: record.plugin_id.clone(),
            asset: record.asset(),
            name,
        }
    }

    fn holds(&self, record: &PluginRecord) -> bool {
        record.format == self.format
            && record.plugin_id == self.plugin_id
            && record.asset() == self.asset
    }

    fn problem(&self) -> PluginProblem {
        PluginProblem::Crashed {
            plugin_id: self.plugin_id.clone(),
            name: self.name.clone(),
        }
    }
}

/// What [`Plugins::open`] gives back.
pub(crate) struct Opened {
    pub engine: ForEngine,
    /// Reported on every run, though the plugin plays.
    pub notes: Vec<PluginProblem>,
    pub lanes: Vec<AutomatedPin>,
}

pub(crate) enum ForEngine {
    /// The engine plays on with the plugin it has. Only pins changed, and the poll sends those.
    Same,
    /// `None` from a host that loads nothing ([`Plugins::listing`]).
    Play(Option<Box<dyn PluginAudio>>),
}

/// One plugin this project holds. It is `loaded` while a record names it and `retired` once it
/// does not, until the engine gives its audio side back.
struct Hosted {
    format: PluginFormat,
    plugin_id: String,
    asset: AssetName,
    plugin: Box<dyn LoadedPlugin>,
    last_saved: Option<Instant>,
    /// Kept here and not in the backend, so a change the once-a-second rule made wait is
    /// written by a later poll.
    pending_save: bool,
    /// A card reads it on every frame, and a frame must call into no plugin. A VST 3 plugin
    /// says yes until [`Plugins::open_window`] finds out.
    has_window: bool,
    window: PluginWindow,
    /// For starting the plugin again.
    config: PrepareConfig,
    restart: Restart,
    /// The next run of the behaviour loads the plugin again instead of keeping it.
    needs_load: bool,
    /// What the load reported that stays true while the plugin plays.
    notes: Vec<PluginProblem>,
    /// Read the first time a pin or the card needs them. Shared, so a card keeps the list of a
    /// plugin with thousands of parameters without a copy per frame.
    parameters: Option<Rc<BTreeMap<u32, Parameter>>>,
    pins: BTreeMap<u32, PinState>,
    laned: Laned,
    /// The pins as the behaviour last ran. A plugin started again gets these, so the index of a
    /// lane means the same pin to both until the next run.
    lanes: AutomatedPins,
    shown: BTreeMap<u32, Shown>,
}

/// Both formats change a plugin's latency by starting it again.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Restart {
    Idle,
    Asked,
    /// The engine was told to give the audio side back.
    Waiting,
}

impl Hosted {
    /// The same plugin and state file.
    fn holds(&self, record: &PluginRecord) -> bool {
        record.format == self.format
            && record.plugin_id == self.plugin_id
            && record.asset() == self.asset
    }

    /// Whether a run of the behaviour may keep this plugin as it plays. One that is being
    /// started again is kept too: the engine gets it, with its pins, when it has started.
    fn keeps(&self, record: &PluginRecord, config: PrepareConfig) -> bool {
        self.holds(record) && self.config == config && !self.needs_load
    }

    fn parameters(&mut self) -> &Rc<BTreeMap<u32, Parameter>> {
        let Self {
            parameters, plugin, ..
        } = self;
        parameters.get_or_insert_with(|| Rc::new(listed(plugin.as_mut())))
    }

    /// The notes of the load, and every pin of `record` that moves nothing.
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

    /// Frees what the plugin holds for its window, wherever that is, and gives back a window of
    /// this process to take down.
    #[must_use]
    fn give_up_window(&mut self) -> Option<WindowHandle<PluginFrame>> {
        if let Some(remote) = self.plugin.remote_window() {
            remote.close();
        }
        self.window.give_up(self.plugin.gui())
    }

    fn matches(&self, project: &Project, id: &InstanceId) -> bool {
        record_of(project, id).is_some_and(|record| self.holds(record))
    }
}

fn listed(plugin: &mut dyn LoadedPlugin) -> BTreeMap<u32, Parameter> {
    by_id(plugin.parameters())
}

fn record_of<'a>(project: &'a Project, id: &InstanceId) -> Option<&'a PluginRecord> {
    project.state(&project.resolve::<PluginRecord>(id)?)
}

#[derive(Default)]
struct Table {
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Hosted>,
    /// A window opened or closed, or a plugin's parameters or their text changed.
    card_changed: bool,
    /// Their views are already freed. Taking a window down needs the application, which the
    /// moments that find them do not have.
    finished_windows: Vec<WindowHandle<PluginFrame>>,
    placements: Placements,
    /// A write that fails is not tried again until a window changes again, so it is said once.
    placements_changed: bool,
    /// A dragged window moves many times a second, so at most one write a second.
    placements_written: Option<Instant>,
    /// Settling windows has no project to report to. The next poll gives these.
    window_problems: Vec<PluginProblem>,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    cache: ScanCache,
    scanned: Arc<Mutex<Scanning>>,
    started: Cell<bool>,
    /// The window's host. Only it looks again while it runs: the others run once and end.
    in_background: Cell<bool>,
    last_look: Cell<Option<Instant>>,
    /// Ends the scan thread between bundles when the host goes.
    stop: Arc<AtomicBool>,
    /// The scan generation the last poll acted on.
    seen: Cell<u64>,
    /// Records whose plugin the scan has not found yet.
    waiting: RefCell<BTreeSet<InstanceId>>,
    retries: RefCell<Vec<InstanceId>>,
    writes_state: bool,
    /// `false` for [`Plugins::listing`].
    loads: bool,
    /// Whether each plugin plays in a process of its own, see `bridge`.
    isolation: Option<Isolation>,
    /// Records whose plugin stopped working, with what they held then. Each stays silent and
    /// reported until [`Plugins::start_again`], or until its record names another plugin or
    /// state file: never loaded again by itself, so a plugin that crashes as it loads does not
    /// crash on every run of its behaviour.
    stopped: RefCell<BTreeMap<InstanceId, Stopped>>,
    /// Kept so that dropping the host can still save.
    assets: RefCell<Option<Assets>>,
    /// The project folder, from the first poll. Window placements are kept by it.
    root: RefCell<Option<std::path::PathBuf>>,
    placement_store: PlacementStore,
    table: RefCell<Table>,
    /// Kept out of the table, which a behaviour run by a write of a record borrows.
    gestures: RefCell<BTreeMap<InstanceId, PinGesture>>,
}

/// The last chance to save. On macOS the application ends without unwinding: GPUI drops the
/// main window, and with it the project and this host, and then the process is gone. So whoever
/// polls the host must hold it weakly ([`Plugins::downgrade`]), else nothing is saved.
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let assets = self.writes_state.then(|| self.assets.get_mut().clone());
        let table = self.table.get_mut();
        let deadline = Instant::now() + CLOSING;
        for hosted in table.loaded.values_mut().chain(&mut table.retired) {
            hosted.plugin.closing(deadline);
        }
        for hosted in table.loaded.values_mut().chain(&mut table.retired) {
            // The window cannot be taken down from here; it goes with the application.
            let _window = hosted.give_up_window();
            if let Some(Some(assets)) = &assets
                && let Err(problem) = save(hosted, assets)
            {
                // Nobody is left to tell.
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

/// The plugins of one project. Every clone is the same table.
#[derive(Clone)]
pub struct Plugins(Rc<Inner>);

/// Whoever polls the host holds one of these, so dropping the project ends the host and saves
/// every plugin.
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

    pub fn new(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, true, true, None)
    }

    /// A host that saves plugin state into the project and plays every plugin in a process of
    /// its own, the scanner's program, so a plugin that crashes costs its own slot. Where the
    /// system cannot (Windows, for now), it is [`Self::new`].
    pub fn isolated(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
        isolation: Isolation,
    ) -> Self {
        let isolation = cfg!(unix).then_some(isolation);
        Self::with(search_paths, scanner, cache, true, true, isolation)
    }

    /// Loads plugins and never writes.
    pub fn read_only(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, true, None)
    }

    /// Looks a plugin up and reads its state file, and never loads it: a plugin that only loads
    /// and goes can still crash the process, as Crow Hill Origins does in its own teardown.
    /// For `--inspect`, which makes no sound.
    pub fn listing(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
    ) -> Self {
        Self::with(search_paths, scanner, cache, false, false, None)
    }

    fn with(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        cache: ScanCache,
        writes_state: bool,
        loads: bool,
        isolation: Option<Isolation>,
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
            isolation,
            stopped: RefCell::new(BTreeMap::new()),
            assets: RefCell::new(None),
            root: RefCell::new(None),
            placement_store,
            table: RefCell::new(Table::default()),
            gestures: RefCell::new(BTreeMap::new()),
        }))
    }

    /// Calls into the plugin, so not for drawing a frame. Gives up on a table that a plugin's
    /// own call has borrowed.
    pub fn parameter_value(&self, id: &InstanceId, parameter: u32) -> Option<ParameterValue> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        let plugin = &mut table.loaded.get_mut(id)?.plugin;
        let value = plugin.value(parameter)?;
        Some(ParameterValue {
            value,
            text: plugin.text(parameter, value),
        })
    }

    /// The same `Rc` until the plugin says its parameters changed, so a card can tell a new list
    /// by its pointer. The first call asks the plugin, as [`Self::parameter_value`].
    pub fn parameters(&self, id: &InstanceId) -> Option<Rc<BTreeMap<u32, Parameter>>> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        Some(table.loaded.get_mut(id)?.parameters().clone())
    }

    /// The plugin's own text, such as `1.2 kHz`. Calls into the plugin, as
    /// [`Self::parameter_value`].
    pub fn parameter_text(&self, id: &InstanceId, parameter: u32, value: f64) -> Option<String> {
        let mut table = self.0.table.try_borrow_mut().ok()?;
        table.loaded.get_mut(id)?.plugin.text(parameter, value)
    }

    /// Loads the plugin the record names, for the engine. A run whose record changed only its
    /// pins keeps the plugin, when `processor_kept` says the engine still has it: reloading a
    /// sampler for a turn of a knob would stop its sound for seconds.
    ///
    /// Whatever this instance held goes first, so a failure leaves no entry behind.
    pub(crate) fn open(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        assets: &Assets,
        config: PrepareConfig,
        processor_kept: bool,
    ) -> Result<Opened, PluginProblem> {
        *self.0.assets.borrow_mut() = Some(assets.clone());
        self.0.waiting.borrow_mut().remove(id);
        {
            let mut stopped = self.0.stopped.borrow_mut();
            match stopped.get(id) {
                Some(stopped) if stopped.holds(record) => return Err(stopped.problem()),
                Some(_) => {
                    stopped.remove(id);
                }
                None => {}
            }
        }
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
                // The same plugin loading again gets its window back. Another plugin does not.
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
                // The composer may install a missing plugin while the app runs.
                if matches!(
                    problem,
                    PluginProblem::StillScanning { .. } | PluginProblem::NotInstalled { .. }
                ) {
                    self.0.waiting.borrow_mut().insert(id.clone());
                }
                if let PluginProblem::Crashed { name, .. } = &problem {
                    let stopped = Stopped::of(record, name.clone());
                    self.0.stopped.borrow_mut().insert(id.clone(), stopped);
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
        // Empty bytes are a state file reserved by `new_state_asset` that the plugin has not
        // written into yet.
        let saved = assets
            .read(asset)
            .map_err(|error| PluginProblem::StateNotRead {
                plugin_id: record.plugin_id.clone(),
                message: error.to_string(),
            })?
            .filter(|bytes| !bytes.is_empty());
        if !self.0.loads {
            return Ok(Opened {
                engine: ForEngine::Play(None),
                notes: Vec::new(),
                lanes: listed_lanes(record),
            });
        }
        let pins = &record.parameters;
        let (mut plugin, audio, laned, notes): (_, Box<dyn PluginAudio>, _, _) = match &self
            .0
            .isolation
        {
            #[cfg(unix)]
            Some(isolation) => {
                let saved = saved.as_deref();
                let opened =
                    crate::bridge::open(&self.0.scanner, isolation, &found, saved, config, pins)?;
                (opened.plugin, opened.audio, opened.laned, opened.notes)
            }
            _ => {
                let opening = match record.format {
                    PluginFormat::Clap => crate::clap::load(&found, saved.as_deref(), config, pins),
                    PluginFormat::Vst3 => crate::vst3::load(&found, saved.as_deref(), config, pins),
                }?;
                let crate::backend::Opening {
                    plugin,
                    started,
                    notes,
                } = opening;
                let laned = laned_here();
                let audio = Box::new(Player::new(started, laned.clone()));
                (plugin, audio, laned, notes)
            }
        };
        let has_window =
            plugin.gui().is_some_and(|gui| gui.is_offered()) || plugin.remote_window().is_some();
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
            laned,
            lanes: AutomatedPins::NONE,
            shown: BTreeMap::new(),
        };
        // The record wins over the state just loaded. The backend gave the plugin every pin
        // before activating it; they go again in the first block for a plugin that took
        // nothing then.
        hosted.send_pins(record);
        let notes = hosted.notes(record);
        let lanes = hosted.automated(record);
        self.0.table.borrow_mut().loaded.insert(id.clone(), hosted);
        Ok(Opened {
            engine: ForEngine::Play(Some(audio)),
            notes,
            lanes,
        })
    }

    /// The name the composer knows a plugin by: the one the scan found, or its id.
    fn name_of(&self, hosted: &Hosted) -> String {
        self.installed_name(hosted.format, &hosted.plugin_id)
            .unwrap_or_else(|| hosted.plugin_id.clone())
    }

    /// Whether the plugin of this record stopped working and waits for [`Self::start_again`].
    /// Read while drawing a card, so it gives up on a borrowed table.
    pub fn has_stopped(&self, id: &InstanceId) -> bool {
        self.0
            .stopped
            .try_borrow()
            .is_ok_and(|stopped| stopped.contains_key(id))
    }

    /// Loads the plugin of a record that stopped working again, from its state as last saved,
    /// for the composer: whoever polls runs its behaviour again.
    pub fn start_again(&self, id: &InstanceId) {
        if self.0.stopped.borrow_mut().remove(id).is_some() {
            self.retry(id);
            self.0.table.borrow_mut().card_changed = true;
        }
    }

    fn retry(&self, id: &InstanceId) {
        let mut retries = self.0.retries.borrow_mut();
        if !retries.contains(id) {
            retries.push(id.clone());
        }
    }

    /// Records whose behaviour is worth running again: the scan found their plugin, the plugin
    /// asked to be loaded again, or its parameters changed.
    pub fn take_retries(&self) -> Vec<InstanceId> {
        std::mem::take(&mut self.0.retries.borrow_mut())
    }

    /// Whether a card needs drawing again since the last call.
    pub fn take_card_change(&self) -> bool {
        std::mem::take(&mut self.0.table.borrow_mut().card_changed)
    }

    /// For a project that closes: ends open turns of a knob and saves every plugin, whether it
    /// said its state changed or not.
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
        let deadline = Instant::now() + CLOSING;
        for hosted in loaded.values_mut().chain(&mut *retired) {
            hosted.plugin.closing(deadline);
        }
        for hosted in loaded.values_mut().chain(&mut *retired) {
            // The placements still say which were open.
            if let Some(handle) = hosted.give_up_window() {
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

    /// Main-thread work for every plugin: the callbacks they asked for, the state they said
    /// changed, and letting go of the ones no record names any more.
    pub fn poll(&self, project: &Project) -> Vec<PluginProblem> {
        self.poll_at(project, Instant::now())
    }

    /// Moves every restart a plugin asked for one step on. Call it after [`Self::poll`]. Handing
    /// the engine an audio side is not an edit and has no undo step.
    pub fn send_restarts(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
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

    /// Two steps, one poll or more apart: the engine gives the audio side back, then the plugin
    /// is deactivated, activated again and handed back with its new latency. In between it
    /// passes its input through.
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
                Restart::Waiting => match hosted.plugin.restart(
                    hosted.config,
                    &pins_of(hosted, project, id),
                    &hosted.laned,
                ) {
                    // The engine has not given it back yet.
                    None => {}
                    Some(Ok(audio)) => {
                        hosted.restart = Restart::Idle;
                        // What was on its way to the old audio side went with it, and the
                        // record may have changed meanwhile: every pin goes again.
                        hosted.pins.clear();
                        if let Some(record) = record_of(project, id) {
                            hosted.send_pins(record);
                        }
                        let update = HostedUpdate::Plugin(Some(audio), hosted.lanes);
                        updates.push((id.clone(), hosted.plugin_id.clone(), update));
                    }
                    Some(Err(problem)) => {
                        // Plays nothing until its record changes, which loads it.
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
        // Noted after the loop, which has the table in pieces.
        let mut closed_for_good = Vec::new();
        // Where windows in plugins' own processes moved, noted after the loop too.
        let mut moved = Vec::new();

        // Includes an edit the project rolled back after this host had loaded it.
        let stale: Vec<InstanceId> = loaded
            .iter()
            .filter(|(id, hosted)| !hosted.matches(project, id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(mut hosted) = loaded.remove(&id) {
                if let Some(handle) = hosted.give_up_window() {
                    finished_windows.push(handle);
                    *card_changed = true;
                }
                closed_for_good.push(id.clone());
                // So undo of a delete brings the plugin back as it sounded.
                if let Some(assets) = assets
                    && let Err(problem) = save(&mut hosted, assets)
                {
                    problems.push(problem);
                }
                retired.push(hosted);
            }
        }

        // Retired plugins may still be playing, so they are served too.
        let serving = loaded.iter_mut().map(|(id, hosted)| (Some(id), hosted));
        for (id, hosted) in serving.chain(retired.iter_mut().map(|hosted| (None, hosted))) {
            let requests = hosted.plugin.poll();
            if let Some(wanted) = requests.window_size {
                hosted.window.wants_size(wanted);
            }
            if requests.window_closed && hosted.window.is_open() {
                if let Some(handle) = hosted.give_up_window() {
                    finished_windows.push(handle);
                }
                *card_changed = true;
                if let Some(id) = id {
                    closed_for_good.push(id.clone());
                }
            }
            if let (Some(id), Some(placement)) = (id, requests.placement) {
                moved.push((id.clone(), placement));
            }
            // Only loaded plugins are started again.
            if requests.restart && hosted.restart == Restart::Idle {
                hosted.restart = Restart::Asked;
            }
            // As for a record that changed: its behaviour runs again, which saves this one and
            // loads it from that state.
            if requests.reload
                && let Some(id) = id
            {
                hosted.needs_load = true;
                self.retry(id);
            }
            // A run of the behaviour checks the pins again. The list is a new `Rc` even when
            // equal, because the plugin's text, which it does not hold, may have changed.
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

        // A plugin whose process stopped working goes at once: its slot is silent and reported
        // until the composer starts it again, by a run of its behaviour that finds it stopped.
        // Its window, which may be what crashed, does not come back by itself.
        let crashed: Vec<InstanceId> = (loaded.iter())
            .filter(|(_, hosted)| hosted.plugin.crashed())
            .map(|(id, _)| id.clone())
            .collect();
        for id in crashed {
            if let Some(mut hosted) = loaded.remove(&id) {
                if let Some(handle) = hosted.give_up_window() {
                    finished_windows.push(handle);
                }
                *card_changed = true;
                let stopped = Stopped {
                    format: hosted.format,
                    plugin_id: hosted.plugin_id.clone(),
                    asset: hosted.asset.clone(),
                    name: self.name_of(&hosted),
                };
                self.0.stopped.borrow_mut().insert(id.clone(), stopped);
                self.retry(&id);
                closed_for_good.push(id);
                retired.push(hosted);
            }
        }

        retired.retain_mut(|hosted| !hosted.plugin.released());
        for id in closed_for_good {
            table.keep_open(&id, false);
        }
        for (id, placement) in moved {
            table.place(&id, placement);
        }

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

fn retire(mut hosted: Hosted, table: &mut Table, assets: Option<&Assets>) {
    if let Some(handle) = hosted.give_up_window() {
        table.finished_windows.push(handle);
        table.card_changed = true;
    }
    if let Some(assets) = assets
        && let Err(problem) = save(&mut hosted, assets)
    {
        // A behaviour has no way to report this.
        eprintln!("error: {problem}");
    }
    table.retired.push(hosted);
}

/// Bytes that are already there are not written again, so a session that changed nothing
/// leaves no diff.
fn save(hosted: &mut Hosted, assets: &Assets) -> Result<(), PluginProblem> {
    // A plugin that stopped working has nothing to give: its asset keeps what it had.
    if hosted.plugin.crashed() {
        return Ok(());
    }
    let fail = |message: String| PluginProblem::StateNotWritten {
        plugin_id: hosted.plugin_id.clone(),
        message,
    };
    let bytes = hosted.plugin.save_state().map_err(fail)?;
    // `pending_save` is cleared only once written, so a failed write is tried again.
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
