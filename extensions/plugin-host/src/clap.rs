//! The CLAP backend: the host callbacks a CLAP plugin makes, loading one, and playing it.
//!
//! CLAP splits a plugin in two. The plugin's own handle belongs to the application's main
//! thread and only its audio processor may go to the audio thread, which is the rule `host.rs`
//! and `processor.rs` are built on. `start_processing` and `stop_processing` belong to the
//! audio thread and `deactivate` to the main thread while nothing is processing.
//!
//! Nothing the plugin sends out is read: it is given a void event list, which takes every event
//! and keeps none. A buffer of ours would have to grow while the plugin pushed into it, on the
//! audio thread, where the sanitizer cannot see it because the plugin's own call is exempt.
//! What a plugin changed of its parameters is asked for on the main thread instead.
//!
//! A value the host sets goes in as a parameter event at the start of a block, through a ring
//! of a fixed size: CLAP has no main-thread call that sets a value while the plugin is active.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiError, GuiSize, HostGui, HostGuiImpl, PluginGui as ClapGui,
};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::note_ports::{NoteDialect, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamInfoBuffer,
    ParamInfoFlags, ParamRescanFlags, PluginParams,
};
use clack_extensions::render::{PluginRender, RenderMode};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::prelude::*;
use sound_core::{MAX_AUTOMATED, MAX_BLOCK, PrepareConfig};

use crate::backend::{Hand, LoadedPlugin, Opening, ParameterChange, PluginGui, Requests};
use crate::host::{HOST_NAME, HOST_URL, HOST_VENDOR, HOST_VERSION};
use crate::parameters::{Parameter, Steps, by_id, playable};
use crate::processor::{
    Control, EVENT_CAPACITY, PluginEvent, Started, copy_in, copy_out, not_ours,
};
use crate::scan::ScannedPlugin;
use crate::window::WindowSize;
use crate::{Pin, PluginFormat, PluginProblem};

/// The handlers a CLAP plugin calls. One set per plugin instance.
pub(crate) struct SoundToolsHost;

impl HostHandlers for SoundToolsHost {
    type Shared<'a> = SharedCallbacks;
    type MainThread<'a> = MainThreadCallbacks<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &SharedCallbacks) {
        builder
            .register::<HostState>()
            .register::<HostGui>()
            .register::<HostLatency>()
            .register::<HostParams>();
    }
}

/// How many values one block can carry into the plugin, apart from the notes: the pins of a
/// record, which are at most this many. A value that finds the ring full waits on the main
/// thread for the next poll. The lanes of the pins have as much room again.
const VALUE_CAPACITY: usize = MAX_AUTOMATED;

/// Callbacks a plugin may make from any thread. They only note what was asked for; the work
/// happens in the poll on the main thread.
#[derive(Default)]
pub(crate) struct SharedCallbacks {
    callback_requested: AtomicBool,
    /// The plugin asked for a flush, see [`HostParamsImplShared::request_flush`].
    flush_requested: AtomicBool,
    restart_requested: AtomicBool,
    /// The plugin closed its own window, or lost it. The next poll frees what is left.
    window_closed: AtomicBool,
    /// A size the plugin asked its window to be, packed into one number. Zero means none.
    window_size_wanted: AtomicU64,
    /// The plugin's own state extension, if it has one. Filled in while it initializes.
    state: OnceLock<Option<PluginState>>,
}

impl<'a> SharedHandler<'a> for SharedCallbacks {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        // CLAP calls this once per instance, so there is never a value here already.
        self.state.get_or_init(|| instance.get_extension());
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

/// The window callbacks. A plugin may make them from any thread, so they only note what
/// happened; the poll does the work on the main thread.
impl HostGuiImpl for SharedCallbacks {
    /// Only about resizing an embedded window by dragging its edge, which this host does not
    /// offer. The plugin says how big it is through `request_resize`.
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        // Acknowledged here and done at the next poll, which CLAP allows for a call that may
        // come from another thread.
        self.window_size_wanted
            .store(new_size.pack_to_u64(), Ordering::Release);
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Err(HostError::Message(
            "a plugin's window is opened from its card in the track panel",
        ))
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Err(HostError::Message(
            "a plugin's window is closed from its card in the track panel",
        ))
    }

    fn closed(&self, _was_destroyed: bool) {
        self.window_closed.store(true, Ordering::Release);
    }
}

impl HostParamsImplShared for SharedCallbacks {
    /// While the plugin is active the host calls `process` for every block, which is a flush.
    /// One asked for while it loads, which a plugin does as it reads its state, is done before
    /// it is activated: Six Sines puts a state it reads in a queue that a flush empties, and
    /// emptied by the first block instead it ends the notes of that block.
    fn request_flush(&self) {
        self.flush_requested.store(true, Ordering::Release);
    }
}

pub(crate) struct MainThreadCallbacks<'a> {
    /// Only here for its lifetime, which ties this handler to the shared one of the same
    /// instance.
    _shared: &'a SharedCallbacks,
    /// Set by `mark_dirty`, which the plugin may call while it initializes, before the table
    /// knows it.
    state_is_dirty: Cell<bool>,
    /// The plugin changed its parameters or their names, and the host reads them again.
    parameters_changed: Cell<bool>,
}

impl<'a> MainThreadHandler<'a> for MainThreadCallbacks<'a> {}

impl HostStateImpl for MainThreadCallbacks<'_> {
    fn mark_dirty(&self) {
        self.state_is_dirty.set(true);
    }
}

impl HostParamsImplMainThread for MainThreadCallbacks<'_> {
    /// Only a change of the list or its names is noted. New values need nothing: the host asks
    /// for the values of the pins at every poll anyway.
    fn rescan(&self, flags: ParamRescanFlags) {
        let list = ParamRescanFlags::INFO | ParamRescanFlags::TEXT | ParamRescanFlags::ALL;
        if flags.intersects(list) {
            self.parameters_changed.set(true);
        }
    }

    /// The host keeps nothing about a parameter but the value in a record, which is the
    /// composer's and stays.
    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

/// A plugin may only change its latency while it is being activated, and says so here. The
/// host reads the latency after every activation anyway, so there is nothing to note.
impl HostLatencyImpl for MainThreadCallbacks<'_> {
    fn changed(&self) {}
}

/// The CLAP entry of `bundle`, loaded.
///
/// clack loads the library with a plain `LoadLibraryExW`, which on Windows looks for the DLLs a
/// plugin imports beside this program and not beside the plugin. So the library is loaded
/// first with its own folder in the search, see `library.rs`; clack's load then finds it
/// loaded already, and this first load lets go once clack holds it.
///
/// # Safety
///
/// This runs the plugin's own code.
unsafe fn load_entry(bundle: &std::path::Path) -> Result<clack_host::entry::PluginEntry, String> {
    #[cfg(target_os = "windows")]
    // SAFETY: the caller agreed to run the plugin's code.
    let _first_load = unsafe { crate::library::Library::load(bundle) }
        .map_err(|error| format!("the library did not load: {error}"))?;
    // SAFETY: as above.
    unsafe { clack_host::entry::PluginEntry::load(bundle) }.map_err(|error| error.to_string())
}

/// The child side of a scan: loads one bundle and says what is in it. Everything that can go
/// wrong here is the plugin's, which is why the caller is a process of its own.
pub(crate) fn scan_bundle(bundle: &std::path::Path) -> Result<Vec<ScannedPlugin>, String> {
    // SAFETY: loading a plugin runs its code, which no host can check in advance. This is why
    // the scan runs in a child process. See `scan.rs`.
    let entry =
        unsafe { load_entry(bundle) }.map_err(|error| format!("{}: {error}", bundle.display()))?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| format!("{}: the bundle has no plugin factory", bundle.display()))?;
    let mut plugins = Vec::new();
    for descriptor in factory.plugin_descriptors() {
        let text = |value: Option<&std::ffi::CStr>| {
            value.map_or(String::new(), |value| value.to_string_lossy().into_owned())
        };
        let Some(id) = descriptor.id() else {
            continue;
        };
        plugins.push(ScannedPlugin {
            format: PluginFormat::Clap,
            id: text(Some(id)),
            name: text(descriptor.name()),
            vendor: text(descriptor.vendor()),
            features: descriptor
                .features()
                .map(|feature| feature.to_string_lossy().into_owned())
                .collect(),
            path: std::path::PathBuf::new(),
        });
    }
    Ok(plugins)
}

/// Makes an instance of the plugin `found` names, which runs its code. It is not activated.
fn instantiate(found: &ScannedPlugin) -> Result<PluginInstance<SoundToolsHost>, PluginProblem> {
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: found.id.clone(),
        message,
    };
    // SAFETY: loading a plugin runs its code, which no host can check in advance. The scan ran
    // this same bundle in a child process first, so a bundle that crashes on load is already
    // known and never reaches here.
    let entry = unsafe { load_entry(&found.path) }.map_err(fail)?;
    let host_info = HostInfo::new(HOST_NAME, HOST_VENDOR, HOST_URL, HOST_VERSION)
        .map_err(|error| fail(error.to_string()))?;
    let identifier =
        std::ffi::CString::new(found.id.as_str()).map_err(|error| fail(error.to_string()))?;
    PluginInstance::<SoundToolsHost>::new(
        |_| SharedCallbacks::default(),
        |shared| MainThreadCallbacks {
            _shared: shared,
            state_is_dirty: Cell::new(false),
            parameters_changed: Cell::new(false),
        },
        &entry,
        &identifier,
        &host_info,
    )
    .map_err(|error| fail(error.to_string()))
}

/// Every parameter a host may set of the plugin `found` names, read without activating it:
/// CLAP gives a plugin's parameters from the moment it is made, so nothing is prepared for audio
/// that is never played.
pub(crate) fn read_parameters(found: &ScannedPlugin) -> Result<Vec<Parameter>, PluginProblem> {
    let mut instance = instantiate(found)?;
    Ok(parameters_of(&mut instance))
}

/// Every parameter a host may set of a plugin that is made, active or not: CLAP puts these
/// calls on the main thread either way.
fn parameters_of(instance: &mut PluginInstance<SoundToolsHost>) -> Vec<Parameter> {
    let Some(params) = params_extension(instance) else {
        return Vec::new();
    };
    let plugin = instance.plugin_handle();
    let mut buffer = ParamInfoBuffer::new();
    let mut parameters = Vec::new();
    for index in 0..params.count(&plugin) {
        let Some(info) = params.get_info(&plugin, index, &mut buffer) else {
            continue;
        };
        // A read-only parameter is the plugin's to set, and a hidden one is not for a person.
        if info
            .flags
            .intersects(ParamInfoFlags::IS_READONLY | ParamInfoFlags::IS_HIDDEN)
        {
            continue;
        }
        let id = info.id;
        // CLAP says an enum is stepped as well.
        let stepped = info
            .flags
            .intersects(ParamInfoFlags::IS_STEPPED | ParamInfoFlags::IS_ENUM);
        let is_list = info.flags.contains(ParamInfoFlags::IS_ENUM);
        let (minimum, maximum) = (info.min_value, info.max_value);
        let mut parameter = Parameter {
            id: id.get(),
            name: String::from_utf8_lossy(info.name).into_owned(),
            minimum,
            maximum,
            default: info.default_value,
            steps: None,
            automatable: info.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
        };
        if stepped {
            let (first, count) = whole_steps(minimum, maximum);
            let last = first + f64::from(count.saturating_sub(1));
            parameter.steps = Some(Steps::new(count, first, last, is_list, |value| {
                text_of(&params, &plugin, id, value)
            }));
        }
        parameters.push(parameter);
    }
    parameters
}

/// The first value of a stepped parameter and how many it takes. The values of a stepped CLAP
/// parameter are whole numbers, which the specification makes of a plain value by cutting off
/// what follows the point. A range wider than a `u32` counts saturates: it is a knob anyway.
fn whole_steps(minimum: f64, maximum: f64) -> (f64, u32) {
    let (first, last) = (minimum.trunc(), maximum.trunc());
    (first, (last - first + 1.0).max(1.0) as u32)
}

/// The parameters extension. A plugin without one has no parameters a host can see.
fn params_extension(instance: &PluginInstance<SoundToolsHost>) -> Option<PluginParams> {
    instance.plugin_shared_handle().get_extension()
}

/// Loads the plugin `found` names, with `saved` as its own state and then `pins`, and starts it.
pub(crate) fn load(
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    config: PrepareConfig,
    pins: &BTreeMap<u32, Pin>,
) -> Result<Opening, PluginProblem> {
    let plugin_id = &found.id;
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: plugin_id.clone(),
        message,
    };
    let mut instance = instantiate(found)?;

    // What kind of run this is, before the plugin is activated and on the main thread, which
    // is where CLAP puts this call. A plugin that streams from disk may wait for its samples
    // in an offline render instead of playing silence. A plugin that has no opinion has no
    // extension, and one that refuses the mode keeps the one it had.
    if let Some(render) = instance
        .plugin_shared_handle()
        .get_extension::<PluginRender>()
    {
        let mode = match config.offline {
            true => RenderMode::Offline,
            false => RenderMode::Realtime,
        };
        let _refused = render.set(&instance.plugin_handle(), mode);
    }

    // The saved state before the plugin is activated, as CLAP asks.
    if let Some(bytes) = saved {
        let state = instance.access_shared_handler(|shared| shared.state.get().copied());
        if let Some(Some(state)) = state {
            let mut reader = std::io::Cursor::new(bytes);
            state
                .load(&mut instance.plugin_handle(), &mut reader)
                .map_err(|error| PluginProblem::StateNotRead {
                    plugin_id: plugin_id.clone(),
                    message: error.to_string(),
                })?;
        }
    }

    // The pins of the record before the plugin is activated, on the main thread, which is
    // where CLAP puts a flush while a plugin is inactive. A plugin that smooths its parameters
    // sets up its smoothing as it is activated, from the values it has then, so its first
    // block plays the pins and does not glide to them from its state.
    let parameters = (!pins.is_empty()).then(|| by_id(parameters_of(&mut instance)));
    let asked = instance
        .access_shared_handler(|shared| shared.flush_requested.swap(false, Ordering::AcqRel));
    if parameters.is_some() || asked {
        let pins = parameters
            .iter()
            .flat_map(|parameters| playable(parameters, pins));
        flush(&mut instance, pins);
    }

    let (started, values, ports) = activate(&mut instance, config).map_err(fail)?;
    // The pedal is only missing from a plugin that has somewhere to take notes. A plugin with
    // no note port at all, which is what an ordinary effect is, has no pedal to miss, and this
    // host cannot ask what a record is for. Audio inputs say nothing either way: Six Sines is
    // an instrument with a stereo input for audio-rate modulation.
    let notes = match ports.takes_notes && !ports.takes_midi {
        false => Vec::new(),
        true => vec![PluginProblem::NoPedal {
            plugin_id: plugin_id.clone(),
        }],
    };
    Ok(Opening {
        started: Box::new(started),
        plugin: Box::new(ClapPlugin {
            plugin_id: plugin_id.clone(),
            instance,
            created_window: false,
            values,
            waiting: BTreeMap::new(),
        }),
        notes,
    })
}

/// Gives an inactive plugin `changes` at once, with `params.flush`, the call CLAP has for a
/// value outside a block.
fn flush(
    instance: &mut PluginInstance<SoundToolsHost>,
    changes: impl Iterator<Item = ParameterChange>,
) {
    let Some(params) = params_extension(instance) else {
        return;
    };
    let mut events = EventBuffer::new();
    for change in changes {
        // An id CLAP calls invalid names no parameter, and a record cannot hold it.
        if let Some(id) = ClapId::from_raw(change.id) {
            events.push(&ParamValueEvent::new(
                0,
                id,
                Pckn::match_all(),
                change.value,
            ));
        }
    }
    let Some(mut plugin) = instance.inactive_plugin_handle() else {
        return;
    };
    // What a plugin says back is not read, as in a block.
    params.flush(&mut plugin, &events.as_input(), &mut OutputEvents::void());
}

/// Activates a plugin that is not active, and gives its audio side with what its ports say,
/// and the main thread's end of the values going into it. Its latency is read here, after the
/// activation, which is the one moment CLAP lets it change.
fn activate(
    instance: &mut PluginInstance<SoundToolsHost>,
    config: PrepareConfig,
) -> Result<(ClapStarted, ValuesIn, PortLayout), String> {
    let ports = read_ports(instance);
    let configuration = PluginAudioConfiguration {
        sample_rate: f64::from(config.sample_rate),
        min_frames_count: 1,
        max_frames_count: MAX_BLOCK as u32,
    };
    let audio = instance
        .activate(|_, _| (), configuration)
        .map_err(|error| error.to_string())?;
    let latency = match instance
        .plugin_shared_handle()
        .get_extension::<PluginLatency>()
    {
        Some(latency) => latency.get(&instance.plugin_handle()),
        // A plugin with no latency extension has none.
        None => 0,
    };
    let (ring, values) = rtrb::RingBuffer::new(VALUE_CAPACITY);
    let played = Arc::new(AtomicU64::new(0));
    let started = ClapStarted::new(
        audio.into(),
        ports.dialect,
        ports.takes_midi,
        ports.input_channels,
        ports.output_channels,
        latency,
        values,
        played.clone(),
    );
    let values = ValuesIn {
        ring,
        pushed: 0,
        played,
    };
    Ok((started, values, ports))
}

/// The main thread's end of the values going into a plugin's processor, one per audio side.
struct ValuesIn {
    ring: rtrb::Producer<ParameterChange>,
    /// How many values went into the ring, and how many the processor has played: a block takes
    /// them at its start and counts them once its `process` is over. Equal once every value
    /// sent is in what the plugin says.
    pushed: u64,
    played: Arc<AtomicU64>,
}

/// One loaded CLAP plugin, from the control thread.
pub(crate) struct ClapPlugin {
    plugin_id: String,
    instance: PluginInstance<SoundToolsHost>,
    /// Whether the plugin holds what it made for a window, so that `create` is never called
    /// twice, which CLAP forbids.
    created_window: bool,
    values: ValuesIn,
    /// The newest value of each parameter that found the ring full. It goes at the next poll.
    waiting: BTreeMap<u32, f64>,
}

impl ClapPlugin {
    /// Puts what is waiting into the ring, as far as there is room, and keeps the rest.
    fn send_waiting(&mut self) {
        let ValuesIn { ring, pushed, .. } = &mut self.values;
        self.waiting.retain(
            |&id, &mut value| match ring.push(ParameterChange { id, value }) {
                Ok(()) => {
                    *pushed += 1;
                    false
                }
                Err(_) => true,
            },
        );
    }
}

impl LoadedPlugin for ClapPlugin {
    fn poll(&mut self) -> Requests {
        let requested = self.instance.access_shared_handler(|shared| {
            shared.callback_requested.swap(false, Ordering::AcqRel)
        });
        if requested {
            self.instance.call_on_main_thread_callback();
        }
        // After the callback above, because a plugin may ask for any of these from there.
        let shared = |shared: &SharedCallbacks| {
            (
                shared.restart_requested.swap(false, Ordering::AcqRel),
                shared.window_closed.swap(false, Ordering::AcqRel),
                shared.window_size_wanted.swap(0, Ordering::AcqRel),
            )
        };
        let (restart, window_closed, size) = self.instance.access_shared_handler(shared);
        self.send_waiting();
        Requests {
            restart,
            // CLAP has no call that asks to be unloaded: a restart is all it asks for.
            reload: false,
            // The flag is cleared here and the host keeps what it was told until the bytes are
            // written, so a change that the once-a-second rule made wait is not forgotten.
            state_is_dirty: self
                .instance
                .access_handler(|main| main.state_is_dirty.replace(false)),
            // CLAP sends the sustain pedal as a MIDI message, so no mapping stands between the
            // pedal and the plugin and there is nothing that can move.
            pedal_unmapped: false,
            window_closed,
            window_size: (size != 0).then(|| {
                let size = GuiSize::unpack_from_u64(size);
                WindowSize {
                    width: size.width,
                    height: size.height,
                }
            }),
            parameters_changed: self
                .instance
                .access_handler(|main| main.parameters_changed.replace(false)),
        }
    }

    fn save_state(&mut self) -> Result<Vec<u8>, String> {
        let state = self
            .instance
            .access_shared_handler(|shared| shared.state.get().copied());
        let Some(Some(state)) = state else {
            return Ok(Vec::new());
        };
        let mut bytes = Vec::new();
        state
            .save(&mut self.instance.plugin_handle(), &mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }

    fn gui(&mut self) -> Option<&mut dyn PluginGui> {
        Some(self)
    }

    fn value(&mut self, id: u32) -> Option<f64> {
        let params = params_extension(&self.instance)?;
        params.get_value(&self.instance.plugin_handle(), ClapId::from_raw(id)?)
    }

    fn text(&mut self, id: u32, value: f64) -> Option<String> {
        let params = params_extension(&self.instance)?;
        text_of(
            &params,
            &self.instance.plugin_handle(),
            ClapId::from_raw(id)?,
            value,
        )
    }

    fn parameters(&mut self) -> Vec<Parameter> {
        parameters_of(&mut self.instance)
    }

    fn send(&mut self, change: ParameterChange) {
        // A newer value of a parameter that is still waiting takes its place: only the last
        // is the one the plugin is to end on.
        self.waiting.insert(change.id, change.value);
        self.send_waiting();
    }

    fn sent_values_played(&mut self) -> bool {
        self.send_waiting();
        let played = self.values.played.load(Ordering::Acquire);
        self.waiting.is_empty() && played >= self.values.pushed
    }

    /// CLAP says where a hand is in the gesture events of a block's output, which this host
    /// does not read.
    fn hand(&mut self) -> Hand {
        Hand::Unknown
    }

    fn released(&mut self) -> bool {
        // A plugin whose restart failed is not active, and has nothing left to give back.
        !self.instance.is_active() || self.instance.try_deactivate().is_ok()
    }

    fn restart(
        &mut self,
        config: PrepareConfig,
        pins: &[ParameterChange],
    ) -> Option<Result<Box<dyn Started>, PluginProblem>> {
        // Deactivating needs the audio side back: clack refuses while it is still held.
        if self.instance.is_active() && self.instance.try_deactivate().is_err() {
            return None;
        }
        // A pin that changed while the plugin waited went to the ring of the audio side that
        // was going, so the plugin has not heard it.
        flush(&mut self.instance, pins.iter().copied());
        let activated =
            activate(&mut self.instance, config).map_err(|message| PluginProblem::DidNotRestart {
                plugin_id: self.plugin_id.clone(),
                message,
            });
        Some(activated.map(|(started, values, _)| {
            // What the old ring still held went with the old audio side. The host sends every
            // pin again, as its record has it now, so nothing older is kept here.
            self.values = values;
            self.waiting.clear();
            Box::new(started) as Box<dyn Started>
        }))
    }
}

/// The plugin's own text for a value of a parameter. CLAP leaves the length of the text to the
/// host; what does not fit is cut off by the plugin.
fn text_of(
    params: &PluginParams,
    plugin: &PluginMainThreadHandle,
    id: ClapId,
    value: f64,
) -> Option<String> {
    let mut buffer = [0_u8; 128];
    let text = params.value_to_text(plugin, id, value, &mut buffer).ok()?;
    Some(String::from_utf8_lossy(text).into_owned())
}

/// How to show a plugin on this machine: the platform's windowing API, in a window of ours.
fn configuration() -> Option<GuiConfiguration<'static>> {
    Some(GuiConfiguration {
        api_type: GuiApiType::default_for_current_platform()?,
        is_floating: false,
    })
}

impl ClapPlugin {
    fn gui_extension(&mut self) -> Option<ClapGui> {
        self.instance.plugin_shared_handle().get_extension()
    }

    fn failed(&self, error: GuiError) -> PluginProblem {
        PluginProblem::WindowDidNotOpen {
            plugin_id: self.plugin_id.clone(),
            message: error.to_string(),
        }
    }

    fn no_window(&self) -> PluginProblem {
        PluginProblem::NoWindow {
            plugin_id: self.plugin_id.clone(),
        }
    }
}

/// CLAP has two ways to show a plugin: a floating window the plugin owns, or one the host owns
/// with the plugin's view in it. The specification calls the floating one a fallback every
/// plugin should support; in practice almost none do, and neither real CLAP instrument on the
/// machine this was written on does. So the host makes the window.
impl PluginGui for ClapPlugin {
    fn is_offered(&mut self) -> bool {
        let Some(configuration) = configuration() else {
            return false;
        };
        let Some(gui) = self.gui_extension() else {
            return false;
        };
        gui.is_api_supported(&self.instance.plugin_handle(), configuration)
    }

    fn create(&mut self) -> Result<(), PluginProblem> {
        if self.created_window {
            return Ok(());
        }
        let configuration = configuration().ok_or_else(|| self.no_window())?;
        let gui = self.gui_extension().ok_or_else(|| self.no_window())?;
        if !gui.is_api_supported(&self.instance.plugin_handle(), configuration) {
            return Err(self.no_window());
        }
        gui.create(&self.instance.plugin_handle(), configuration)
            .map_err(|error| self.failed(error))?;
        // From here the plugin holds resources for a window, and `destroy` frees them.
        self.created_window = true;
        Ok(())
    }

    fn size(&mut self) -> Option<WindowSize> {
        let gui = self.gui_extension()?;
        let size = gui.get_size(&self.instance.plugin_handle())?;
        Some(WindowSize {
            width: size.width,
            height: size.height,
        })
    }

    fn set_scale(&mut self, scale: f64) {
        let Some(gui) = self.gui_extension() else {
            return;
        };
        match gui.set_scale(&self.instance.plugin_handle(), scale) {
            Ok(()) => {}
            // A plugin that reads the scale from the system itself refuses, as CLAP allows.
            // Its sizes are physical pixels either way.
            Err(_ignored) => {}
        }
    }

    unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), PluginProblem> {
        let gui = self.gui_extension().ok_or_else(|| self.no_window())?;
        let configuration = configuration().ok_or_else(|| self.no_window())?;
        // The view of this platform's windowing API: an `NSView` for Cocoa, an `HWND` for
        // Win32. CLAP holds either as the same pointer.
        let parent =
            clack_extensions::gui::Window::from_generic_ptr(configuration.api_type, view.as_ptr());
        // SAFETY: the caller keeps the view alive until `destroy` has run.
        unsafe { gui.set_parent(&self.instance.plugin_handle(), parent) }
            .map_err(|error| self.failed(error))
    }

    fn show(&mut self) -> Result<(), PluginProblem> {
        let gui = self.gui_extension().ok_or_else(|| self.no_window())?;
        gui.show(&self.instance.plugin_handle())
            .map_err(|error| self.failed(error))
    }

    fn can_resize(&mut self) -> bool {
        let Some(gui) = self.gui_extension() else {
            return false;
        };
        gui.can_resize(&self.instance.plugin_handle())
    }

    /// The plugin's own answer to the size first (`adjust_size`), and then that size. A plugin
    /// that does not adjust takes the size as it was offered, as a VST 3 view that does not
    /// constrain does. One that refuses the size keeps the one it has, which the window goes
    /// back to; that is said on the terminal, because a drag has nobody else to tell.
    fn resize(&mut self, wanted: WindowSize) -> Option<WindowSize> {
        let gui = self.gui_extension()?;
        let plugin = self.instance.plugin_handle();
        let offered = GuiSize {
            width: wanted.width,
            height: wanted.height,
        };
        let adjusted = gui.adjust_size(&plugin, offered).unwrap_or(offered);
        if let Err(error) = gui.set_size(&plugin, adjusted) {
            eprintln!(
                "the plugin {:?} refused the window size {}x{}: {error}",
                self.plugin_id, adjusted.width, adjusted.height
            );
        }
        let size = gui.get_size(&plugin)?;
        Some(WindowSize {
            width: size.width,
            height: size.height,
        })
    }

    fn destroy(&mut self) {
        if std::mem::take(&mut self.created_window)
            && let Some(gui) = self.gui_extension()
        {
            gui.destroy(&self.instance.plugin_handle());
        }
    }
}

/// A CLAP plugin that is started, with every buffer its process call needs.
struct ClapStarted {
    audio: PluginAudioProcessor<SoundToolsHost>,
    dialect: Dialect,
    /// Whether the note port takes MIDI, which is the only way to send the pedal, the wheels
    /// and the key pressure.
    takes_midi: bool,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    /// The first audio input port of the plugin, one buffer per channel. Empty when the plugin
    /// takes no audio in, which is the usual case for an instrument.
    input_channels: Vec<Vec<f32>>,
    /// The first audio output port of the plugin, one buffer per channel.
    output_channels: Vec<Vec<f32>>,
    input_events: EventBuffer,
    /// How many more note and control events the buffer takes in this block, so a block never
    /// grows it. The values of parameters have room of their own on top.
    room: usize,
    /// How many more values of automation lanes the buffer takes in this block.
    lane_room: usize,
    /// What the plugin said its latency was when it was activated.
    latency: u32,
    /// The values the host sets, from the main thread.
    values: rtrb::Consumer<ParameterChange>,
    /// How many of them this block took, counted into `played` once the plugin has run.
    taken: u64,
    played: Arc<AtomicU64>,
}

/// Which events the plugin's note port takes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Dialect {
    /// CLAP note events. The pedal, the wheels and the key pressure still need MIDI, see
    /// [`midi`].
    Clap,
    Midi,
}

impl ClapStarted {
    fn new(
        audio: PluginAudioProcessor<SoundToolsHost>,
        dialect: Dialect,
        takes_midi: bool,
        input_channel_count: usize,
        output_channel_count: usize,
        latency: u32,
        values: rtrb::Consumer<ParameterChange>,
        played: Arc<AtomicU64>,
    ) -> Self {
        let buffers = |count: usize| (0..count).map(|_| vec![0.0; MAX_BLOCK]).collect();
        Self {
            audio,
            dialect,
            takes_midi,
            input_ports: AudioPorts::with_capacity(input_channel_count.max(1), 1),
            output_ports: AudioPorts::with_capacity(output_channel_count.max(1), 1),
            input_channels: buffers(input_channel_count),
            output_channels: buffers(output_channel_count),
            input_events: EventBuffer::with_capacity(EVENT_CAPACITY + 2 * VALUE_CAPACITY),
            room: EVENT_CAPACITY,
            lane_room: VALUE_CAPACITY,
            latency,
            values,
            taken: 0,
            played,
        }
    }
}

impl Drop for ClapStarted {
    fn drop(&mut self) {
        // Reached only when the engine itself is gone, which ends the audio thread before this
        // runs. Every other way out of the engine stops the plugin there first.
        self.stop();
    }
}

impl Started for ClapStarted {
    fn takes(&self, _control: Control) -> bool {
        self.takes_midi
    }

    fn latency(&self) -> u32 {
        self.latency
    }

    fn begin_block(&mut self) {
        self.input_events.clear();
        self.room = EVENT_CAPACITY;
        self.lane_room = VALUE_CAPACITY;
        // The values the host set since the last block, at its first frame, because they were
        // true before it began. Ahead of the notes, so the events stay in time order. Never more
        // than the room kept for them: the ring holds no more, and what the host could not put
        // in waits on its side.
        while (self.taken as usize) < VALUE_CAPACITY {
            let Ok(change) = self.values.pop() else {
                break;
            };
            self.taken += 1;
            // An id CLAP calls invalid names no parameter, and a record cannot hold it.
            if let Some(id) = ClapId::from_raw(change.id) {
                let event = ParamValueEvent::new(0, id, Pckn::match_all(), change.value);
                self.input_events.push(&event);
            }
        }
    }

    fn automate(&mut self, id: u32, value: f64) -> bool {
        if self.lane_room == 0 {
            return false;
        }
        self.lane_room -= 1;
        // An id CLAP calls invalid names no parameter, and a record cannot hold it.
        if let Some(id) = ClapId::from_raw(id) {
            let event = ParamValueEvent::new(0, id, Pckn::match_all(), value);
            self.input_events.push(&event);
        }
        true
    }

    fn push(&mut self, offset: u32, event: PluginEvent) -> bool {
        if self.room == 0 {
            return false;
        }
        self.room -= 1;
        match (event, self.dialect) {
            (PluginEvent::On { key, velocity }, Dialect::Clap) => {
                let pckn = Pckn::new(0_u16, 0_u16, u16::from(key), Match::All);
                let event = NoteOnEvent::new(offset, pckn, f64::from(velocity) / 127.0);
                self.input_events.push(&event);
            }
            (PluginEvent::On { key, velocity }, Dialect::Midi) => {
                self.input_events
                    .push(&MidiEvent::new(offset, 0, [0x90, key, velocity]));
            }
            (PluginEvent::Off { key }, Dialect::Clap) => {
                let pckn = Pckn::new(0_u16, 0_u16, u16::from(key), Match::All);
                // CLAP's note off carries a release velocity. The note contract has none, so
                // the usual half value goes out.
                self.input_events
                    .push(&NoteOffEvent::new(offset, pckn, 0.5));
            }
            (PluginEvent::Off { key }, Dialect::Midi) => {
                self.input_events
                    .push(&MidiEvent::new(offset, 0, [0x80, key, 64]));
            }
            (PluginEvent::Control(control), _) => {
                self.input_events
                    .push(&MidiEvent::new(offset, 0, midi(control)));
            }
        }
        true
    }

    fn run(
        &mut self,
        frames: usize,
        input: [&[f32]; sound_core::CHANNELS],
        left: &mut [f32],
        right: &mut [f32],
    ) -> bool {
        let Self {
            audio,
            input_ports,
            output_ports,
            input_channels,
            output_channels,
            input_events,
            ..
        } = self;

        copy_in(input_channels, frames, input);
        let inputs = if input_channels.is_empty() {
            InputAudioBuffers::empty()
        } else {
            input_ports.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    // Not `constant`: a constant buffer tells the plugin every sample of it is
                    // the same, which is true of the silence an instrument gets and not of the
                    // sound an effect is given.
                    input_channels
                        .iter_mut()
                        .map(|channel| InputChannel::variable(&mut channel[..frames])),
                ),
            }])
        };
        let mut outputs = if output_channels.is_empty() {
            OutputAudioBuffers::empty()
        } else {
            output_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    output_channels
                        .iter_mut()
                        .map(|channel| &mut channel[..frames]),
                ),
            }])
        };
        let input = input_events.as_input();
        // Nothing reads what a plugin sends out: MIDI from a plugin is not built. A void list
        // takes every event and keeps none, so a plugin that sends thousands grows nothing.
        let mut output = OutputEvents::void();

        let processed = match not_ours(|| audio.ensure_processing_started()) {
            Ok(started) => {
                not_ours(|| started.process(&inputs, &mut outputs, &input, &mut output, None, None))
                    .is_ok()
            }
            Err(_) => false,
        };
        // Only now does what the plugin says on the main thread hold the values of this block.
        // A plugin that failed has played them too, as far as the host can ever know.
        let taken = std::mem::take(&mut self.taken);
        if taken > 0 {
            self.played.fetch_add(taken, Ordering::Release);
        }
        if !processed {
            return false;
        }
        copy_out(&self.output_channels, frames, left, right);
        true
    }

    fn stop(&mut self) {
        // The plugin's own `stop_processing` runs in here.
        not_ours(|| self.audio.ensure_processing_stopped());
    }
}

/// A control as a MIDI message on channel 1, with its value as it was played. It always goes as
/// raw MIDI: CLAP note events have no sustain and no channel-wide bend, mod wheel or pressure,
/// so a plugin whose note port takes no MIDI does not get them at all.
fn midi(control: Control) -> [u8; 3] {
    // Controller 64 is the sustain pedal and controller 1 the mod wheel.
    match control {
        Control::Pedal(pedal) => [0xB0, 64, pedal.value()],
        Control::Bend(_) => {
            let value = control.midi_value();
            [0xE0, (value & 0x7F) as u8, (value >> 7) as u8]
        }
        Control::ModWheel(amount) => [0xB0, 1, amount.value()],
        // Channel pressure has one data byte.
        Control::Pressure(amount) => [0xD0, amount.value(), 0],
    }
}

/// What the plugin's ports say: whether and how to send it notes, and how many channels to
/// give it.
struct PortLayout {
    dialect: Dialect,
    /// Whether the plugin has a note input port at all. An ordinary effect has none.
    takes_notes: bool,
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
        takes_notes: true,
        takes_midi: false,
        input_channels: 0,
        output_channels: 2,
    };
    if let Some(notes) = notes {
        let mut buffer = NotePortInfoBuffer::new();
        layout.takes_notes = notes.count(&plugin, true) > 0;
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

    /// CLAP makes a whole number of a plain value by cutting off what follows the point, also
    /// below zero, so a range of -1.5 to 2.7 takes -1, 0, 1 and 2.
    #[test]
    fn the_steps_of_a_stepped_parameter_cut_its_bounds_toward_zero() {
        assert_eq!(whole_steps(-1.5, 2.7), (-1.0, 4));
        assert_eq!(whole_steps(0.0, 2.0), (0.0, 3));
    }
}
