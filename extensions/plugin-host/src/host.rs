//! The plugins this project has loaded, whatever their format.
//!
//! [`Plugins`] lives on the thread the project lives on. Both formats put a plugin's own handle
//! on the application's main thread and allow only its audio side on the audio thread, so this
//! table keeps the handles and hands the audio sides to the engine. Nothing here knows CLAP or
//! VST 3: a format is a [`crate::backend::LoadedPlugin`].
//!
//! The rule for saving: a plugin's state is written to its asset when the plugin says it
//! changed, at the next [`Plugins::poll`] and then at most once a second while it keeps saying
//! so, and always when the plugin goes or the project closes. See README.md for what a crash
//! can lose.
//!
//! The table never decides what the engine gets. [`Plugins::open`] loads a plugin and hands it
//! over every time it runs, and [`Plugins::poll`] lets go of every entry whose record no longer
//! says what the entry holds. So an edit that the project rejects, which never reaches the
//! engine, leaves nothing behind here either.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::WindowHandle;
use sound_core::{AssetName, Assets, InstanceId, PrepareConfig, Project};

use crate::processor::{HostedPlugin, HostedUpdate};

use crate::backend::LoadedPlugin;
use crate::scan::{Scan, ScanCache, ScanCommand, ScannedPlugin, scan_folders};
use crate::window::{PluginFrame, PluginWindow, Prepared, WindowOwner};
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
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
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
        "the plugin {plugin_id:?} asked to be started again, to change its latency, and did not start: {message}. Nothing plays through it until its record changes"
    )]
    DidNotRestart { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be read: {message}")]
    StateNotRead { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be saved: {message}")]
    StateNotWritten { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} asked to be started again, which this build does not do. Take it off the track and put it back if it stopped sounding"
    )]
    AskedForRestart { plugin_id: String },
    #[error(
        "the plugin {plugin_id:?} offers the host no way to send the sustain pedal, so the pedal does not reach it. Its notes play"
    )]
    NoPedal { plugin_id: String },
    #[error(
        "the plugin {plugin_id:?} moved the parameter its sustain pedal is mapped to. The pedal still reaches the parameter it was mapped to when the plugin loaded, which may now be another control. Open the project again to pick the new mapping up"
    )]
    PedalMappingMoved { plugin_id: String },
    #[error("the plugin {plugin_id:?} has no window of its own")]
    NoWindow { plugin_id: String },
    #[error("the window of the plugin {plugin_id:?} did not open: {message}")]
    WindowDidNotOpen { plugin_id: String, message: String },
}

/// What [`Plugins::open`] gives back. The behaviour hands the engine whatever is here every
/// time it runs, so nothing the engine has depends on what this table remembers.
pub struct Opened {
    /// The audio side of the plugin, or `None` from a host that loads none ([`Plugins::listing`]),
    /// which is a slot that plays nothing and reports nothing.
    pub started: Option<Box<dyn crate::processor::Started>>,
    /// What to report about this record every time the behaviour runs, such as a plugin the
    /// sustain pedal cannot reach. These are not failures: the plugin plays.
    pub notes: Vec<PluginProblem>,
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
    /// Whether the record of `id` in the project still says what this entry holds.
    fn matches(&self, project: &Project, id: &InstanceId) -> bool {
        let Some(instance) = project.resolve::<PluginRecord>(id) else {
            return false;
        };
        let Some(record) = project.state(&instance) else {
            return false;
        };
        record.format == self.format
            && record.plugin_id == self.plugin_id
            && record.asset() == self.asset
    }
}

#[derive(Default)]
struct Table {
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Hosted>,
    /// A plugin's window opened or closed since whoever draws the rack last asked.
    window_changed: bool,
    /// Windows whose plugin has gone. Their views are already freed; taking a window down
    /// needs the application, which the moments that find them do not have.
    finished_windows: Vec<WindowHandle<PluginFrame>>,
}

/// What this machine has, filled in by whoever scans. Shared with the scan thread, so this is
/// the one place in the host with a lock, and the audio thread never touches it.
#[derive(Default)]
struct Scanning {
    scan: Scan,
    /// Bundles that failed, as one line each. The runtime shows them once.
    notices: Vec<String>,
    /// Goes up whenever the scan learns something, so a poll can tell that a record that was
    /// waiting for a plugin is worth trying again.
    generation: u64,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    cache: ScanCache,
    scanned: Arc<Mutex<Scanning>>,
    /// Whether a scan has run or is running. A host that scans in the background sets it as it
    /// starts, so nothing blocks on the first plugin a project names.
    started: Cell<bool>,
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
    table: RefCell<Table>,
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
        Self(Rc::new(Inner {
            search_paths,
            scanner,
            cache,
            scanned: Arc::new(Mutex::new(Scanning::default())),
            started: Cell::new(false),
            stop: Arc::new(AtomicBool::new(false)),
            seen: Cell::new(0),
            waiting: RefCell::new(BTreeSet::new()),
            retries: RefCell::new(Vec::new()),
            writes_state,
            loads,
            assets: RefCell::new(None),
            table: RefCell::new(Table::default()),
        }))
    }

    /// Starts the scan on a thread of its own and comes back at once.
    ///
    /// The window calls this before it opens a project, so that no plugin of this machine is
    /// ever looked at on the thread that draws. A record whose plugin the scan has not reached
    /// yet is reported and played as soon as it turns up, see [`Self::take_retries`].
    /// `--render`, `--inspect` and `--headless` do not call it and wait for the scan the first
    /// time a record needs one.
    pub fn start_scanning(&self) {
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        let stop = self.0.stop.clone();
        let (paths, scanner, cache) = (
            self.0.search_paths.clone(),
            self.0.scanner.clone(),
            self.0.cache.clone(),
        );
        // Detached: nothing waits for it. A host that goes sets `stop`, and the thread ends
        // after the bundle it is on, which is bounded by the deadline of one child.
        std::thread::Builder::new()
            .name("plugin-scan".to_string())
            .spawn(move || {
                // Whatever ends this thread, the scan is over: it was stopped, or it panicked
                // inside a bundle. Nothing may wait for a scan that is not running.
                let _over = Over(scanned.clone());
                scan_folders(&paths, &scanner, &cache, &stop, |scan| {
                    publish(&scanned, scan);
                });
            })
            .map_or_else(
                |error| {
                    // A machine that cannot start a thread scans where it stands.
                    eprintln!("error: the plugin scan needs a thread: {error}");
                    self.0.started.set(false);
                },
                |_handle| (),
            );
    }

    /// Every plugin this machine has. The first call pays for the scan unless one is already
    /// running in the background, and then it is what is known so far. See README.md.
    pub fn scan(&self) -> Scan {
        self.ensure_scan();
        self.known()
    }

    fn known(&self) -> Scan {
        match self.0.scanned.lock() {
            Ok(scanned) => scanned.scan.clone(),
            // A scan thread that panicked leaves what it had. Nothing of ours can panic while
            // it holds this lock, so this is only so that a project still opens.
            Err(poisoned) => poisoned.into_inner().scan.clone(),
        }
    }

    /// A number that goes up whenever the scan learns something, and once more when it ends.
    ///
    /// Whoever draws a picker keeps it and fills the menu again when it changes, because a
    /// picker built while a scan ran holds a part of the list and a line that says so. It
    /// copies nothing, so a poll may ask on every frame.
    pub fn scan_generation(&self) -> u64 {
        match self.0.scanned.lock() {
            Ok(scanned) => scanned.generation,
            Err(poisoned) => poisoned.into_inner().generation,
        }
    }

    /// Whether a scan is still running. The picker says so quietly while it is, and whoever
    /// polls asks on every poll, so this copies nothing.
    pub fn scan_is_running(&self) -> bool {
        let finished = match self.0.scanned.lock() {
            Ok(scanned) => scanned.scan.finished,
            Err(poisoned) => poisoned.into_inner().scan.finished,
        };
        self.0.started.get() && !finished
    }

    /// Scans if this session has not, and waits for it. Does nothing once a scan has been
    /// started in the background.
    fn ensure_scan(&self) {
        if self.0.started.replace(true) {
            return;
        }
        let scanned = self.0.scanned.clone();
        scan_folders(
            &self.0.search_paths,
            &self.0.scanner,
            &self.0.cache,
            &self.0.stop,
            |scan| publish(&scanned, scan),
        );
    }

    /// Waits for the scan to finish, whoever started it. `--render`, `--inspect` and
    /// `--headless` may block, and this is where they do.
    pub fn wait_for_scan(&self) {
        self.ensure_scan();
        while self.scan_is_running() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Lines about the scan that a person should see once, such as a bundle that crashed.
    pub fn take_notices(&self) -> Vec<String> {
        match self.0.scanned.lock() {
            Ok(mut scanned) => std::mem::take(&mut scanned.notices),
            Err(poisoned) => std::mem::take(&mut poisoned.into_inner().notices),
        }
    }

    /// Every instrument this machine has, of every format, in one line each, for a picker. It
    /// scans on the first call of the session, as loading a plugin does.
    pub fn instruments(&self) -> Vec<ScannedPlugin> {
        let mut instruments = self.scan().plugins;
        instruments.retain(ScannedPlugin::is_instrument);
        instruments
    }

    /// Every effect this machine has, for the picker that adds one to a rack. A plugin decides
    /// which list it is in by what it declares; nothing checks that it is true, and a record
    /// written by hand may name any plugin in any slot.
    pub fn effects(&self) -> Vec<ScannedPlugin> {
        let mut effects = self.scan().plugins;
        effects.retain(ScannedPlugin::is_effect);
        effects
    }

    /// The name the maker gave the plugin with this id, when this machine has it. `None` says
    /// the plugin is missing, which is what the card of a record shows.
    ///
    /// A card asks on every frame it draws, so this scans nothing and copies one name.
    pub fn installed_name(&self, format: PluginFormat, plugin_id: &str) -> Option<String> {
        let scanned = self.0.scanned.lock().ok()?;
        Some(scanned.scan.find(format, plugin_id)?.name.clone())
    }

    /// What this machine knows of the plugin with this id, for the line on its card. `None`
    /// when it is missing.
    pub fn installed(&self, format: PluginFormat, plugin_id: &str) -> Option<ScannedPlugin> {
        let scanned = self.0.scanned.lock().ok()?;
        scanned.scan.find(format, plugin_id).cloned()
    }

    /// Loads the plugin the record names and gives it to the caller for the engine.
    ///
    /// It loads every time. A behaviour runs when its own record changed, on opening the
    /// project and on a retry, and every change a plugin record can have needs another plugin
    /// or another state file, so there is nothing to keep. In return nothing here has to guess
    /// what the engine holds: the caller hands over a plugin on every run, and an edit the
    /// project rejects simply never reaches the engine.
    ///
    /// Whatever this instance held goes first, saved and waiting to be let go of, so a failure
    /// below leaves no entry behind and the record and the engine agree: silence.
    pub fn open(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        assets: &Assets,
        config: PrepareConfig,
    ) -> Result<Opened, PluginProblem> {
        // Kept for the drop of this host, which is the last moment a plugin can be saved.
        *self.0.assets.borrow_mut() = Some(assets.clone());
        self.0.waiting.borrow_mut().remove(id);
        {
            let mut table = self.0.table.borrow_mut();
            if let Some(hosted) = table.loaded.remove(id) {
                retire(hosted, &mut table, self.0.writes_state.then_some(assets));
            }
        }
        let asset = record.asset();
        match self.load(id, record, &asset, assets, config) {
            Ok(opened) => Ok(opened),
            Err(problem) => {
                // A plugin the scan has not reached yet is worth trying again when it has.
                if matches!(problem, PluginProblem::StillScanning { .. }) {
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
                started: None,
                notes: Vec::new(),
            });
        }
        let opening = match record.format {
            PluginFormat::Clap => crate::clap::load(&found, saved.as_deref(), config),
            PluginFormat::Vst3 => crate::vst3::load(&found, saved.as_deref(), config),
        }?;
        let crate::backend::Opening {
            mut plugin,
            started,
            notes,
        } = opening;
        let has_window = plugin.gui().is_some_and(|gui| gui.is_offered());
        self.0.table.borrow_mut().loaded.insert(
            id.clone(),
            Hosted {
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
            },
        );
        Ok(Opened {
            started: Some(started),
            notes,
        })
    }

    /// Records whose plugin was not there when their behaviour ran and may be now, because the
    /// scan has learned something since. Whoever polls runs their behaviour again, which is
    /// what makes a plugin play and takes its problem away.
    pub fn take_retries(&self) -> Vec<InstanceId> {
        std::mem::take(&mut self.0.retries.borrow_mut())
    }

    /// Whether the plugin of this record has a window of its own to open. `None` says the
    /// record has no plugin loaded at all, which is a plugin that did not load and is already
    /// reported; its card says that instead of offering a window.
    ///
    /// A card asks on every frame it draws. The answer is the one the plugin gave while it
    /// loaded, so this calls into no plugin, and it gives up rather than wait for a table that
    /// a plugin's own call has borrowed.
    pub fn window_offered(&self, id: &InstanceId) -> Option<bool> {
        let table = self.0.table.try_borrow().ok()?;
        table.loaded.get(id).map(|hosted| hosted.has_window)
    }

    /// Whether the window of this record's plugin is open. Read while drawing a card, so it
    /// gives up on a table that a plugin's own call has borrowed, as [`Self::window_offered`].
    pub fn window_is_open(&self, id: &InstanceId) -> bool {
        let Ok(table) = self.0.table.try_borrow() else {
            return false;
        };
        table
            .loaded
            .get(id)
            .is_some_and(|hosted| hosted.window.is_open())
    }

    /// Opens the plugin's own window, or brings the one that is open forward. `title` is what
    /// the window is called.
    ///
    /// It is in three steps because the table may not be borrowed while GPUI runs: opening a
    /// window draws, and a card being drawn asks this host what its plugin has.
    pub fn open_window(
        &self,
        id: &InstanceId,
        title: &str,
        cx: &mut gpui::App,
    ) -> Result<(), PluginProblem> {
        // One: what the plugin says, with the table borrowed and no GPUI in sight.
        let prepared = {
            let mut table = self.0.table.borrow_mut();
            let Some(hosted) = table.loaded.get_mut(id) else {
                // The record names a plugin this machine does not have, or the load failed.
                // That is reported, and the card shows it instead of offering a window.
                return Ok(());
            };
            let plugin_id = hosted.plugin_id.clone();
            let no_window = || PluginProblem::NoWindow {
                plugin_id: plugin_id.clone(),
            };
            let Hosted {
                window,
                plugin,
                has_window,
                ..
            } = hosted;
            let prepared = match plugin.gui() {
                Some(gui) => window.prepare(gui),
                None => Err(no_window()),
            };
            // A plugin that turns out to have no window says so once. The card stops offering
            // one for the rest of this session, so the composer is not asked to find out again.
            // A VST 3 plugin is offered a window without being asked, because asking means
            // building its whole interface; this is where the answer arrives instead.
            if matches!(prepared, Err(PluginProblem::NoWindow { .. })) {
                *has_window = false;
            }
            table.window_changed = true;
            prepared.map(|prepared| (prepared, plugin_id))?
        };
        // Two: the window itself, with nothing borrowed.
        let (prepared, plugin_id) = prepared;
        let wanted = match prepared {
            Prepared::AlreadyOpen(handle) => {
                return handle
                    .update(cx, |_, window, _| window.activate_window())
                    .map_err(|error| PluginProblem::WindowDidNotOpen {
                        plugin_id,
                        message: error.to_string(),
                    });
            }
            Prepared::Wanted(size) => size,
        };
        let owner = WindowOwner {
            instance: id.clone(),
            plugins: self.downgrade(),
        };
        let opened = crate::window::open_window(&owner, title, wanted, cx);
        let (handle, view, closed) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                // The plugin already holds what it needs for a window. Give it back.
                let _window = self.give_up_window(id);
                return Err(PluginProblem::WindowDidNotOpen {
                    plugin_id,
                    message: error.to_string(),
                });
            }
        };
        // Three: the plugin fills it. A window whose plugin went while it opened, or that the
        // plugin refused, waits for the next poll to be taken down.
        let mut table = self.0.table.borrow_mut();
        let Some(hosted) = table.loaded.get_mut(id) else {
            table.finished_windows.push(handle);
            return Ok(());
        };
        let Hosted { window, plugin, .. } = hosted;
        let attached = match plugin.gui() {
            Some(gui) => window.attach(gui, handle, view, closed),
            None => Err((PluginProblem::NoWindow { plugin_id }, handle)),
        };
        match attached {
            Ok(()) => Ok(()),
            Err((problem, handle)) => {
                table.finished_windows.push(handle);
                Err(problem)
            }
        }
    }

    /// Closes the plugin's own window. Its sound and its state are untouched.
    pub fn close_window(&self, id: &InstanceId, cx: &mut gpui::App) {
        // Outside the borrow: taking a window down runs GPUI.
        if let Some(handle) = self.give_up_window(id) {
            crate::window::remove(handle, cx);
        }
    }

    /// Frees whatever the plugin of `id` holds for a window and gives back the window it was
    /// in, for the caller to take down.
    #[must_use]
    fn give_up_window(&self, id: &InstanceId) -> Option<WindowHandle<PluginFrame>> {
        let mut table = self.0.table.borrow_mut();
        let hosted = table.loaded.get_mut(id)?;
        let finished = hosted.window.give_up(hosted.plugin.gui());
        table.window_changed = true;
        finished
    }

    /// The window is going, whatever took it down. GPUI tells its observers while it still
    /// holds the window, so this is the moment the plugin lets go of the view it is in, before
    /// that view is released. See `window::open_window`.
    pub(crate) fn window_was_closed(&self, id: &InstanceId) {
        let mut table = self.0.table.borrow_mut();
        if let Some(hosted) = table.loaded.get_mut(id)
            && hosted.window.give_up(hosted.plugin.gui()).is_some()
        {
            table.window_changed = true;
        }
    }

    /// The window work that needs the application: taking down the windows of plugins that
    /// have gone, and giving a window the size its plugin asked for. Whoever polls the host
    /// calls it after [`Self::poll`]; the moments that find such a plugin, a record that was
    /// deleted or an undo, have no application at hand.
    pub fn settle_windows(&self, cx: &mut gpui::App) {
        // Everything is read out first: running GPUI while the table is borrowed would let a
        // card that is drawn ask this host about its plugin.
        let (finished, resize) = {
            let mut table = self.0.table.borrow_mut();
            let Table {
                loaded,
                retired,
                finished_windows,
                ..
            } = &mut *table;
            let resize: Vec<_> = loaded
                .values_mut()
                .chain(retired)
                .filter_map(|hosted| hosted.window.take_wanted_size())
                .collect();
            (std::mem::take(finished_windows), resize)
        };
        for (handle, wanted) in resize {
            crate::window::resize(handle, wanted, cx);
        }
        for handle in finished {
            crate::window::remove(handle, cx);
        }
    }

    /// Frees the view of every plugin window and takes the windows down. The application
    /// calls it as it quits, before anything of it is torn down, so that no plugin is left
    /// holding a view of a window that is going.
    pub fn close_all_windows(&self, cx: &mut gpui::App) {
        let open: Vec<InstanceId> = {
            let table = self.0.table.borrow();
            let open = table.loaded.iter();
            open.filter(|(_, hosted)| hosted.window.is_open())
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in open {
            self.close_window(&id, cx);
        }
    }

    /// Whether any plugin's window opened or closed since the last call. Whoever polls asks,
    /// so the card that says "Open window" or "Close window" is drawn again.
    pub fn take_window_change(&self) -> bool {
        std::mem::take(&mut self.0.table.borrow_mut().window_changed)
    }

    /// Saves the state of every plugin, whether it said so or not, and lets them all go.
    ///
    /// Call it when the project closes. A plugin that changes its state without telling the
    /// host is saved here all the same. Nothing is written when the bytes are the ones already
    /// in the project, so a session that changed nothing leaves no diff.
    pub fn close(&self, project: &Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded,
            retired,
            window_changed,
            finished_windows,
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());
        for hosted in loaded.values_mut().chain(&mut *retired) {
            // Every window closes with the project, whether it writes or not.
            if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                finished_windows.push(handle);
                *window_changed = true;
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

    /// [`Self::poll`] with the time given, so a test can move it.
    pub fn poll_at(&self, project: &Project, now: Instant) -> Vec<PluginProblem> {
        self.serve(project, now)
    }

    /// Whether a plugin is waiting for [`Self::send_restarts`]. Cheap, so a caller that has to
    /// ask for the project mutably only does so while this is true.
    pub fn restarts_pending(&self) -> bool {
        let table = self.0.table.borrow();
        let mut loaded = table.loaded.values();
        loaded.any(|hosted| hosted.restart != Restart::Idle)
    }

    /// Moves every restart a plugin asked for one step on, see [`Self::restarts`]. Call it
    /// after [`Self::poll`] while [`Self::restarts_pending`] says so. It needs the project
    /// mutably only to hand the engine the plugin's audio side, which is not an edit: nothing
    /// is written and there is no undo step.
    pub fn send_restarts(&self, project: &mut Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        // Outside the borrow of the table: the engine is the project's.
        for (id, plugin_id, update) in self.restarts(&mut problems) {
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
        problems: &mut Vec<PluginProblem>,
    ) -> Vec<(InstanceId, String, HostedUpdate)> {
        let mut table = self.0.table.borrow_mut();
        let mut updates = Vec::new();
        for (id, hosted) in &mut table.loaded {
            match hosted.restart {
                Restart::Idle => {}
                Restart::Asked => {
                    hosted.restart = Restart::Waiting;
                    updates.push((id.clone(), hosted.plugin_id.clone(), None));
                }
                Restart::Waiting => match hosted.plugin.restart(hosted.config) {
                    // The engine has not given it back yet.
                    None => {}
                    Some(started) => {
                        hosted.restart = Restart::Idle;
                        match started {
                            Ok(started) => {
                                updates.push((id.clone(), hosted.plugin_id.clone(), Some(started)))
                            }
                            Err(problem) => problems.push(problem),
                        }
                    }
                },
            }
        }
        updates
    }

    fn serve(&self, project: &Project, now: Instant) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        self.note_what_the_scan_found(project);
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded,
            retired,
            window_changed,
            finished_windows,
        } = &mut *table;
        let assets = self.0.writes_state.then(|| project.assets());

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
                // from a file or by an undo leaves no window behind.
                if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
                    finished_windows.push(handle);
                    *window_changed = true;
                }
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
        for hosted in loaded.values_mut().chain(&mut *retired) {
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
                *window_changed = true;
            }
            // A plugin that asks to be deactivated and activated again, which is how its
            // latency changes. A retired plugin is marked as well, but only loaded ones are
            // started again, see `Self::restarts`.
            if requests.restart && hosted.restart == Restart::Idle {
                hosted.restart = Restart::Asked;
            }
            // A VST 3 restart this build does not do, so the composer is told instead of being
            // left with a plugin that stopped.
            if requests.restart_not_done {
                problems.push(PluginProblem::AskedForRestart {
                    plugin_id: hosted.plugin_id.clone(),
                });
            }
            // The plugin moved the parameter the sustain pedal reaches. The host looked that
            // mapping up while the plugin loaded and keeps it, so the pedal goes on reaching
            // the parameter it reached before, which is now the wrong one.
            if requests.midi_mapping_changed {
                problems.push(PluginProblem::PedalMappingMoved {
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
        problems
    }

    /// Records that were waiting for a plugin the scan had not reached. When it has learned
    /// something since the last poll, their behaviours are worth running again.
    fn note_what_the_scan_found(&self, project: &Project) {
        if self.0.waiting.borrow().is_empty() {
            return;
        }
        let generation = match self.0.scanned.lock() {
            Ok(scanned) => scanned.generation,
            Err(poisoned) => poisoned.into_inner().generation,
        };
        if self.0.seen.replace(generation) == generation {
            return;
        }
        let known = self.known();
        let mut waiting = self.0.waiting.borrow_mut();
        let mut retries = self.0.retries.borrow_mut();
        waiting.retain(|id| {
            // A record that is gone, or no longer a plugin, waits for nothing.
            let Some(instance) = project.resolve::<PluginRecord>(id) else {
                return false;
            };
            let Some(record) = project.state(&instance) else {
                return false;
            };
            let found = known.find(record.format, &record.plugin_id).is_some();
            if found || known.finished {
                retries.push(id.clone());
                return false;
            }
            true
        });
    }
}

/// Says the scan is over, however its thread ended.
struct Over(Arc<Mutex<Scanning>>);

impl Drop for Over {
    fn drop(&mut self) {
        let held = match self.0.lock() {
            Ok(held) => Some(held),
            Err(poisoned) => Some(poisoned.into_inner()),
        };
        if let Some(mut held) = held {
            held.scan.finished = true;
            held.generation += 1;
        }
    }
}

/// Puts what the scan has found where the host can read it, and counts the change so that a
/// record that is waiting for a plugin is tried again.
fn publish(scanned: &Arc<Mutex<Scanning>>, scan: &Scan) {
    let Ok(mut held) = scanned.lock() else {
        return;
    };
    let said = held.scan.failures.len().min(scan.failures.len());
    for failure in &scan.failures[said..] {
        held.notices.push(format!(
            "{} could not be scanned: {}",
            failure.path.display(),
            failure.message
        ));
    }
    held.scan = scan.clone();
    held.generation += 1;
}

/// Saves a plugin that is going, when the project is one that writes, and puts its handle
/// where it waits for the engine to give the audio side back.
fn retire(mut hosted: Hosted, table: &mut Table, assets: Option<&Assets>) {
    // A record that now names another plugin takes the window of the old one with it.
    if let Some(handle) = hosted.window.give_up(hosted.plugin.gui()) {
        table.finished_windows.push(handle);
        table.window_changed = true;
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
