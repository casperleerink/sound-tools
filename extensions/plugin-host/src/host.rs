//! The plugins this project has loaded, and the host callbacks they call.
//!
//! [`Plugins`] lives on the thread the project lives on. CLAP puts a plugin's own handle on the
//! application's main thread, and only its audio processor may travel to the audio thread. So
//! this table keeps the handles and hands the audio processors to the engine.
//!
//! The rule for saving: a plugin's state is written to its asset when the plugin says it
//! changed, at the next [`Plugins::poll`]. See README.md for what a crash can lose.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::note_ports::{NoteDialect, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::prelude::*;
use sound_core::{AssetName, Assets, InstanceId, MAX_BLOCK, Project, State as _};

use crate::PluginRecord;
use crate::processor::{Dialect, Loaded};
use crate::scan::{Scan, ScanCommand, scan};

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
    #[error(
        "the state asset {asset} is already used by the instance {other}. Both plugins load it and the last one to change writes it. Give each its own `state_asset`"
    )]
    AssetTwice { asset: String, other: InstanceId },
    #[error("the state of the plugin {plugin_id:?} could not be read: {message}")]
    StateNotRead { plugin_id: String, message: String },
    #[error(
        "the plugin {plugin_id:?} takes no MIDI, so the sustain pedal does not reach it. Its notes play"
    )]
    NoPedal { plugin_id: String },
    #[error(
        "the plugin {plugin_id:?} has {channels} audio input channels, and nothing feeds them here: as the instrument of a track it only gets notes. An effect that says it is an instrument sounds like silence. Effect plugins are not built yet"
    )]
    HasAudioInput { plugin_id: String, channels: usize },
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

/// What [`Plugins::open`] gives back.
pub struct Opened {
    /// A plugin for the engine, when this call started one. `None` means the plugin that is
    /// already playing is the right one and keeps its voices.
    pub started: Option<Loaded>,
    /// What to report about this record every time the behaviour runs, such as a plugin that
    /// takes no sustain pedal. These are not failures: the plugin plays.
    pub notes: Vec<PluginProblem>,
}

/// One plugin this project has loaded, with the record it came from.
struct Hosted {
    plugin_id: String,
    asset: AssetName,
    instance: PluginInstance<SoundToolsHost>,
    /// What stays true about this plugin while it plays, such as taking no pedal.
    notes: Vec<PluginProblem>,
}

/// A plugin whose record is gone or changed. Its handle can only go once the engine has given
/// the audio processor back, else clack leaks the instance on purpose.
struct Retired {
    instance: PluginInstance<SoundToolsHost>,
}

#[derive(Default)]
struct Table {
    scanned: Option<Scan>,
    loaded: BTreeMap<InstanceId, Hosted>,
    retired: Vec<Retired>,
    /// Bundles that failed to scan, as one line each. The runtime shows them once.
    notices: Vec<String>,
}

struct Inner {
    search_paths: Vec<std::path::PathBuf>,
    scanner: ScanCommand,
    /// A read-only project (`--inspect`, `--render`) never writes plugin state.
    writes_state: bool,
    table: RefCell<Table>,
}

/// The plugins of one project. Cheap to clone: every copy is the same table.
///
/// The tool's behaviour keeps one, and so does whoever polls the project. It lives on one
/// thread, like the project.
#[derive(Clone)]
pub struct Plugins(Rc<Inner>);

impl Plugins {
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

    /// Makes the instance `id` hold the plugin its record names, and gives the audio processor
    /// when there is a new one for the engine. `Ok(None)` means the plugin it already has is
    /// the right one and keeps playing.
    pub fn open(
        &self,
        id: &InstanceId,
        record: &PluginRecord,
        assets: &Assets,
        sample_rate: u32,
    ) -> Result<Opened, PluginProblem> {
        let asset = record.asset();
        {
            let mut table = self.0.table.borrow_mut();
            // Two records that name one asset would write over each other's state.
            let other = table
                .loaded
                .iter()
                .find(|(other, hosted)| *other != id && hosted.asset == asset);
            if let Some((other, _)) = other {
                return Err(PluginProblem::AssetTwice {
                    asset: asset.to_string(),
                    other: other.clone(),
                });
            }
            match table.loaded.get(id) {
                // The same plugin and the same state file: it plays on, with its voices.
                Some(hosted) if hosted.plugin_id == record.plugin_id && hosted.asset == asset => {
                    return Ok(Opened {
                        started: None,
                        notes: hosted.notes.clone(),
                    });
                }
                // Another plugin, or another state file: the one that is there goes.
                Some(_) => {
                    if let Some(hosted) = table.loaded.remove(id) {
                        table.retired.push(Retired {
                            instance: hosted.instance,
                        });
                    }
                }
                None => {}
            }
        }
        let (started, notes) = self.load(id, record, &asset, assets, sample_rate)?;
        Ok(Opened {
            started: Some(started),
            notes,
        })
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
                notes: notes.clone(),
            },
        );
        Ok((loaded, notes))
    }

    /// Main-thread work for every loaded plugin: the callbacks they asked for, the state they
    /// said changed, and the handles of plugins whose record is gone.
    ///
    /// Call it as often as the project is polled. It writes at most one file per plugin that
    /// marked its state dirty.
    pub fn poll(&self, project: &Project) -> Vec<PluginProblem> {
        let mut problems = Vec::new();
        let mut table = self.0.table.borrow_mut();
        let Table {
            loaded, retired, ..
        } = &mut *table;

        // A record that is gone, or that is no longer a plugin, takes its plugin with it.
        let gone: Vec<InstanceId> = loaded
            .keys()
            .filter(|id| project.tool_of(id) != Some(PluginRecord::TOOL))
            .cloned()
            .collect();
        for id in gone {
            if let Some(hosted) = loaded.remove(&id) {
                retired.push(Retired {
                    instance: hosted.instance,
                });
            }
        }

        for hosted in loaded.values_mut() {
            let requested = hosted.instance.access_shared_handler(|shared| {
                shared.callback_requested.swap(false, Ordering::AcqRel)
            });
            if requested {
                hosted.instance.call_on_main_thread_callback();
            }
            let dirty = hosted
                .instance
                .access_handler(|main| main.state_is_dirty.replace(false));
            if dirty && self.0.writes_state {
                if let Err(problem) = save(hosted, project.assets()) {
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

/// Writes what the plugin says its state is, into the asset its record names.
fn save(hosted: &mut Hosted, assets: &Assets) -> Result<(), PluginProblem> {
    let state = hosted
        .instance
        .access_shared_handler(|shared| shared.state.get().copied());
    let Some(Some(state)) = state else {
        return Ok(());
    };
    let mut bytes = Vec::new();
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: hosted.plugin_id.clone(),
        message,
    };
    state
        .save(&mut hosted.instance.plugin_handle(), &mut bytes)
        .map_err(|error| fail(error.to_string()))?;
    assets
        .write(&hosted.asset, &bytes)
        .map_err(|error| fail(error.to_string()))
}

/// What stays true about a plugin while it plays. None of these stops it from sounding, so
/// they are reported and not errors.
fn standing_notes(plugin_id: &str, layout: &PortLayout) -> Vec<PluginProblem> {
    let mut notes = Vec::new();
    if layout.input_channels > 0 {
        notes.push(PluginProblem::HasAudioInput {
            plugin_id: plugin_id.to_string(),
            channels: layout.input_channels,
        });
    }
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

    #[test]
    fn an_instrument_with_note_and_audio_ports_as_expected_has_nothing_to_report() {
        assert_eq!(standing_notes("a.b", &layout(true, 0)), []);
    }

    /// A plugin that says it is an instrument but takes audio in is usually an effect with
    /// the wrong features. It plays notes into silence, and the composer should know why.
    #[test]
    fn a_plugin_with_audio_inputs_and_one_without_midi_are_both_reported() {
        let notes = standing_notes("a.b", &layout(false, 2));
        let messages: Vec<String> = notes.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages[0].contains("2 audio input channels"),
            "{messages:?}"
        );
        assert!(messages[1].contains("takes no MIDI"), "{messages:?}");
    }
}
