//! The plugins this project has loaded, and the host callbacks they call.
//!
//! [`Plugins`] lives on the thread the project lives on. CLAP puts a plugin's own handle on the
//! application's main thread, and only its audio processor may travel to the audio thread. So
//! this table keeps the handles and hands the audio processors to the engine.
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
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::note_ports::{NoteDialect, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::prelude::*;
use sound_core::{AssetName, Assets, InstanceId, MAX_BLOCK, Project};

use crate::PluginRecord;
use crate::processor::{Dialect, Loaded};
use crate::scan::{Scan, ScanCommand, scan};

/// How often a plugin that keeps saying its state changed is written. A plugin marks itself
/// dirty on every step of a knob drag, and serializing a sampler's state is not cheap, so the
/// first change is written at once and then at most one write a second. Going or closing
/// writes whatever is left, so nothing is lost by waiting.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);

/// What the host tells a plugin about itself.
const HOST_NAME: &str = "Sound Tools";
const HOST_VENDOR: &str = "Sound Tools";
const HOST_URL: &str = "https://github.com/casperleerink/sound-tools";
const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a plugin record is not playing. Each becomes one line in `problems.txt`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PluginProblem {
    #[error(
        "this machine has no {format} plugin with the id {plugin_id:?}. The record is left as it is and the track is silent. Install the plugin, or correct `plugin_id`"
    )]
    NotInstalled { format: String, plugin_id: String },
    #[error("the plugin {plugin_id:?} did not load: {message}")]
    DidNotLoad { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} is not an instrument, so it has no notes to play. Its features are: {features}"
    )]
    NotAnInstrument { plugin_id: String, features: String },
    #[error("the state of the plugin {plugin_id:?} could not be read: {message}")]
    StateNotRead { plugin_id: String, message: String },
    #[error("the state of the plugin {plugin_id:?} could not be saved: {message}")]
    StateNotWritten { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} asked to be started again, which this build does not do. Take it off the track and put it back if it stopped sounding"
    )]
    AskedForRestart { plugin_id: String },
    #[error(
        "the plugin {plugin_id:?} takes no MIDI, so the sustain pedal does not reach it. Its notes play"
    )]
    NoPedal { plugin_id: String },
}

/// The handlers a CLAP plugin calls. One set per plugin instance.
pub struct SoundToolsHost;

impl HostHandlers for SoundToolsHost {
    type Shared<'a> = SharedCallbacks;
    type MainThread<'a> = MainThreadCallbacks<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &SharedCallbacks) {
        builder.register::<HostState>();
    }
}

/// Callbacks a plugin may make from any thread. They only note what was asked for; the work
/// happens in [`Plugins::poll`] on the main thread.
#[derive(Default)]
pub struct SharedCallbacks {
    callback_requested: AtomicBool,
    restart_requested: AtomicBool,
    /// The plugin's own state extension, if it has one. Filled in while it initializes.
    state: OnceLock<Option<PluginState>>,
}

impl<'a> SharedHandler<'a> for SharedCallbacks {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.state.set(instance.get_extension());
    }

    fn request_restart(&self) {
        self.restart_requested.store(true, Ordering::Release);
    }

    fn request_process(&self) {
        // We call the plugin every block while its track exists, so there is nothing to start.
    }

    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

pub struct MainThreadCallbacks<'a> {
    /// The plugin may call `mark_dirty` while it initializes, before the table knows it.
    /// The lifetime ties this handler to the shared one of the same instance.
    _shared: &'a SharedCallbacks,
    state_is_dirty: Cell<bool>,
}

impl<'a> MainThreadHandler<'a> for MainThreadCallbacks<'a> {}

impl HostStateImpl for MainThreadCallbacks<'_> {
    fn mark_dirty(&self) {
        self.state_is_dirty.set(true);
    }
}

/// What [`Plugins::open`] gives back. There is always a plugin: the behaviour hands the engine
/// a plugin every time it runs, so nothing the engine has depends on what this table remembers.
pub struct Opened {
    pub started: Loaded,
    /// What to report about this record every time the behaviour runs, such as a plugin that
    /// takes no sustain pedal. These are not failures: the plugin plays.
    pub notes: Vec<PluginProblem>,
}

/// One plugin this project holds, with the record it came from.
///
/// It is `loaded` while a record names it and `retired` once it does not. A retired one is kept
/// until the engine gives its audio processor back, because dropping it before that would leak
/// the plugin, which is what clack does on purpose. It is polled and saved until then, so a
/// plugin that is still playing while it waits does not lose what it changes.
struct Hosted {
    plugin_id: String,
    asset: AssetName,
    instance: PluginInstance<SoundToolsHost>,
    /// When its state was last written, for the once-a-second rule.
    last_saved: Option<Instant>,
}

#[derive(Default)]
struct Table {
    scanned: Option<Scan>,
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Hosted>,
    /// Bundles that failed to scan, as one line each. The runtime shows them once.
    notices: Vec<String>,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    /// A read-only project (`--inspect`, `--render`) never writes plugin state.
    writes_state: bool,
    /// The `assets/` folder of the project, from the first plugin that loaded. Kept so that
    /// dropping the host can still save, see [`Drop`].
    assets: RefCell<Option<Assets>>,
    table: RefCell<Table>,
}

/// The last chance to save. On macOS the application ends without unwinding: GPUI drops the
/// window and its views, and with them the session and the project, and then the process is
/// gone. Dropping the project drops the registry, the behaviour and this host, so that is the
/// moment. Whoever polls the host must therefore hold it weakly ([`Plugins::downgrade`]), else
/// nothing is saved. [`Plugins::close`] does the same with the project still in hand, and
/// leaves nothing for this.
impl Drop for Inner {
    fn drop(&mut self) {
        if !self.writes_state {
            return;
        }
        let Some(assets) = self.assets.get_mut().clone() else {
            return;
        };
        let table = self.table.get_mut();
        for hosted in table.loaded.values_mut().chain(&mut table.retired) {
            if let Err(problem) = save(hosted, &assets) {
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
    pub fn new(search_paths: Vec<std::path::PathBuf>, scanner: ScanCommand) -> Self {
        Self::with_writing(search_paths, scanner, true)
    }

    /// A host for a project that is open read-only. It loads plugins and never writes.
    pub fn read_only(search_paths: Vec<std::path::PathBuf>, scanner: ScanCommand) -> Self {
        Self::with_writing(search_paths, scanner, false)
    }

    fn with_writing(
        search_paths: Vec<std::path::PathBuf>,
        scanner: ScanCommand,
        writes_state: bool,
    ) -> Self {
        Self(Rc::new(Inner {
            search_paths,
            scanner,
            writes_state,
            assets: RefCell::new(None),
            table: RefCell::new(Table::default()),
        }))
    }

    /// Every plugin this machine has, scanned once per session. The first call pays for it.
    pub fn scan(&self) -> Scan {
        let mut table = self.0.table.borrow_mut();
        if let Some(scanned) = &table.scanned {
            return scanned.clone();
        }
        let scanned = scan(&self.0.search_paths, &self.0.scanner);
        for failure in &scanned.failures {
            table.notices.push(format!(
                "{} could not be scanned: {}",
                failure.path.display(),
                failure.message
            ));
        }
        table.scanned = Some(scanned.clone());
        scanned
    }

    /// Lines about the scan that a person should see once, such as a bundle that crashed.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut self.0.table.borrow_mut().notices)
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
        sample_rate: u32,
    ) -> Result<Opened, PluginProblem> {
        // Kept for the drop of this host, which is the last moment a plugin can be saved.
        *self.0.assets.borrow_mut() = Some(assets.clone());
        {
            let mut table = self.0.table.borrow_mut();
            if let Some(hosted) = table.loaded.remove(id) {
                retire(hosted, &mut table, self.0.writes_state.then_some(assets));
            }
        }
        let asset = record.asset();
        let (started, notes) = self.load(id, record, &asset, assets, sample_rate)?;
        Ok(Opened { started, notes })
    }

    fn load(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        asset: &AssetName,
        assets: &Assets,
        sample_rate: u32,
    ) -> Result<(Loaded, Vec<PluginProblem>), PluginProblem> {
        let scanned = self.scan();
        let found = scanned
            .find(&record.plugin_id)
            .ok_or_else(|| PluginProblem::NotInstalled {
                format: record.format.name().to_string(),
                plugin_id: record.plugin_id.clone(),
            })?;
        if !found.is_instrument() {
            return Err(PluginProblem::NotAnInstrument {
                plugin_id: record.plugin_id.clone(),
                features: found.features.join(", "),
            });
        }
        let fail = |message: String| PluginProblem::DidNotLoad {
            plugin_id: record.plugin_id.clone(),
            message,
        };

        // SAFETY: loading a plugin runs its code, which no host can check in advance. The scan
        // ran this same bundle in a child process first, so a bundle that crashes on load is
        // already known and never reaches here.
        let entry = unsafe { clack_host::entry::PluginEntry::load(&found.path) }
            .map_err(|error| fail(error.to_string()))?;
        let host_info = HostInfo::new(HOST_NAME, HOST_VENDOR, HOST_URL, HOST_VERSION)
            .map_err(|error| fail(error.to_string()))?;
        let plugin_id = std::ffi::CString::new(record.plugin_id.as_str())
            .map_err(|error| fail(error.to_string()))?;
        let mut instance = PluginInstance::<SoundToolsHost>::new(
            |_| SharedCallbacks::default(),
            |shared| MainThreadCallbacks {
                _shared: shared,
                state_is_dirty: Cell::new(false),
            },
            &entry,
            &plugin_id,
            &host_info,
        )
        .map_err(|error| fail(error.to_string()))?;

        // The saved state before the plugin is activated, as CLAP asks.
        let saved = assets
            .read(asset)
            .map_err(|error| PluginProblem::StateNotRead {
                plugin_id: record.plugin_id.clone(),
                message: error.to_string(),
            })?;
        if let Some(bytes) = saved {
            let state = instance.access_shared_handler(|shared| shared.state.get().copied());
            if let Some(Some(state)) = state {
                let mut reader = std::io::Cursor::new(bytes);
                state
                    .load(&mut instance.plugin_handle(), &mut reader)
                    .map_err(|error| PluginProblem::StateNotRead {
                        plugin_id: record.plugin_id.clone(),
                        message: error.to_string(),
                    })?;
            }
        }

        let ports = read_ports(&mut instance);
        let configuration = PluginAudioConfiguration {
            sample_rate: f64::from(sample_rate),
            min_frames_count: 1,
            max_frames_count: MAX_BLOCK as u32,
        };
        let audio = instance
            .activate(|_, _| (), configuration)
            .map_err(|error| fail(error.to_string()))?;
        let loaded = Loaded::new(
            audio.into(),
            ports.dialect,
            ports.takes_midi,
            ports.input_channels,
            ports.output_channels,
        );
        let notes = standing_notes(&record.plugin_id, &ports);
        self.0.table.borrow_mut().loaded.insert(
            id.clone(),
            Hosted {
                plugin_id: record.plugin_id.clone(),
                asset: asset.clone(),
                instance,
                last_saved: None,
            },
        );
        Ok((loaded, notes))
    }

    /// Saves the state of every plugin, whether it said so or not, and lets them all go.
    ///
    /// Call it when the project closes. A plugin that changes its state without telling the
    /// host, which CLAP asks it not to do, is saved here all the same. Nothing is written when
    /// the bytes are the ones already in the project, so a session that changed nothing leaves
    /// no diff.
    pub fn close(&self, project: &Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded, retired, ..
        } = &mut *table;
        if self.0.writes_state {
            let assets = project.assets();
            for hosted in loaded.values_mut().chain(&mut *retired) {
                if let Err(problem) = save(hosted, assets) {
                    problems.push(problem);
                }
            }
        }
        retired.extend(std::mem::take(loaded).into_values());
        problems
    }

    /// Main-thread work for every plugin this host holds: the callbacks they asked for, the
    /// state they said changed, and letting go of the ones no record names any more.
    ///
    /// Call it as often as the project is polled.
    pub fn poll(&self, project: &Project) -> Vec<PluginProblem> {
        self.poll_at(project, Instant::now())
    }

    /// [`Self::poll`] with the time given, so a test can move it.
    pub fn poll_at(&self, project: &Project, now: Instant) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded, retired, ..
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
        // not given its processor back yet, must not miss a callback or lose a change.
        for hosted in loaded.values_mut().chain(&mut *retired) {
            let requested = hosted.instance.access_shared_handler(|shared| {
                shared.callback_requested.swap(false, Ordering::AcqRel)
            });
            if requested {
                hosted.instance.call_on_main_thread_callback();
            }
            // A plugin that asks to be deactivated and activated again. This build does not,
            // so the composer is told instead of being left with a plugin that stopped.
            let restart = hosted.instance.access_shared_handler(|shared| {
                shared.restart_requested.swap(false, Ordering::AcqRel)
            });
            if restart {
                problems.push(PluginProblem::AskedForRestart {
                    plugin_id: hosted.plugin_id.clone(),
                });
            }
            let dirty = hosted
                .instance
                .access_handler(|main| main.state_is_dirty.get());
            let due = hosted
                .last_saved
                .is_none_or(|last| now.duration_since(last) >= SAVE_INTERVAL);
            if let Some(assets) = assets
                && dirty
                && due
            {
                // The flag stays set until it is written, so a change that waits for the
                // second to pass is written by a later poll and not forgotten.
                hosted.last_saved = Some(now);
                if let Err(problem) = save(hosted, assets) {
                    problems.push(problem);
                }
            }
        }

        // A handle may only go once the engine has given its audio processor back. Until then
        // dropping it would leak the plugin, which is what clack does on purpose.
        retired.retain_mut(|plugin| plugin.instance.try_deactivate().is_err());
        problems
    }
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
        record.plugin_id == self.plugin_id && record.asset() == self.asset
    }
}

/// Saves a plugin that is going, when the project is one that writes, and puts its handle
/// where it waits for the engine to give the audio processor back.
fn retire(mut hosted: Hosted, table: &mut Table, assets: Option<&Assets>) {
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
    let state = hosted
        .instance
        .access_shared_handler(|shared| shared.state.get().copied());
    let Some(Some(state)) = state else {
        return Ok(());
    };
    let mut bytes = Vec::new();
    let fail = |message: String| PluginProblem::StateNotWritten {
        plugin_id: hosted.plugin_id.clone(),
        message,
    };
    state
        .save(&mut hosted.instance.plugin_handle(), &mut bytes)
        .map_err(|error| fail(error.to_string()))?;
    let there = assets
        .read(&hosted.asset)
        .map_err(|error| fail(error.to_string()))?;
    if there.as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    assets
        .write(&hosted.asset, &bytes)
        .map_err(|error| fail(error.to_string()))
}

/// What stays true about a plugin while it plays. None of these stops it from sounding, so
/// they are reported and not errors.
fn standing_notes(plugin_id: &str, layout: &PortLayout) -> Vec<PluginProblem> {
    // Audio inputs say nothing: Six Sines is an instrument with a stereo input for audio-rate
    // modulation. They are fed with silence and the plugin plays its notes.
    let mut notes = Vec::new();
    if !layout.takes_midi {
        notes.push(PluginProblem::NoPedal {
            plugin_id: plugin_id.to_string(),
        });
    }
    notes
}

/// What the plugin's ports say: how to send it notes, and how many channels to give it.
struct PortLayout {
    dialect: Dialect,
    takes_midi: bool,
    input_channels: usize,
    output_channels: usize,
}

fn read_ports(instance: &mut PluginInstance<SoundToolsHost>) -> PortLayout {
    let handle = instance.plugin_shared_handle();
    let notes = handle.get_extension::<PluginNotePorts>();
    let audio = handle.get_extension::<PluginAudioPorts>();
    let plugin = instance.plugin_handle();
    // A plugin that says nothing about its ports gets what an instrument usually has: notes
    // in as CLAP events, and one stereo port out.
    let mut layout = PortLayout {
        dialect: Dialect::Clap,
        takes_midi: false,
        input_channels: 0,
        output_channels: 2,
    };
    if let Some(notes) = notes {
        let mut buffer = NotePortInfoBuffer::new();
        if let Some(port) = notes.get(&plugin, 0, true, &mut buffer) {
            layout.takes_midi = port.supported_dialects.supports(NoteDialect::Midi);
            let clap = port.supported_dialects.supports(NoteDialect::Clap);
            layout.dialect = if clap { Dialect::Clap } else { Dialect::Midi };
        }
    }
    if let Some(audio) = audio {
        let mut buffer = AudioPortInfoBuffer::new();
        let mut channels = |is_input: bool| {
            if audio.count(&plugin, is_input) == 0 {
                return 0;
            }
            audio
                .get(&plugin, 0, is_input, &mut buffer)
                .map_or(0, |port| port.channel_count as usize)
        };
        layout.input_channels = channels(true);
        layout.output_channels = channels(false);
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(takes_midi: bool, input_channels: usize) -> PortLayout {
        PortLayout {
            dialect: Dialect::Clap,
            takes_midi,
            input_channels,
            output_channels: 2,
        }
    }

    /// An instrument with audio inputs is ordinary: they are fed with silence. Six Sines is
    /// one, with a stereo input for audio-rate modulation.
    #[test]
    fn an_instrument_that_takes_midi_has_nothing_to_report_whatever_its_audio_inputs() {
        assert_eq!(standing_notes("a.b", &layout(true, 0)), []);
        assert_eq!(standing_notes("a.b", &layout(true, 2)), []);
    }

    #[test]
    fn a_plugin_whose_note_port_takes_no_midi_is_reported_for_the_pedal() {
        let notes = standing_notes("a.b", &layout(false, 0));
        let messages: Vec<String> = notes.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("takes no MIDI"), "{messages:?}");
    }
}
