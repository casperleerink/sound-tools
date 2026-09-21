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

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiError, GuiSize, HostGui, HostGuiImpl, PluginGui as ClapGui,
};
use clack_extensions::note_ports::{NoteDialect, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent};
use clack_host::prelude::*;
use sound_core::MAX_BLOCK;

use crate::backend::{LoadedPlugin, Opening, PluginGui, Requests};
use crate::host::{HOST_NAME, HOST_URL, HOST_VENDOR, HOST_VERSION};
use crate::processor::{
    EVENT_CAPACITY, PluginEvent, SUSTAIN_CONTROLLER, Started, copy_out, not_ours,
};
use crate::scan::ScannedPlugin;
use crate::window::WindowSize;
use crate::{PluginFormat, PluginProblem};

/// The handlers a CLAP plugin calls. One set per plugin instance.
pub struct SoundToolsHost;

impl HostHandlers for SoundToolsHost {
    type Shared<'a> = SharedCallbacks;
    type MainThread<'a> = MainThreadCallbacks<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &SharedCallbacks) {
        builder.register::<HostState>().register::<HostGui>();
    }
}

/// Callbacks a plugin may make from any thread. They only note what was asked for; the work
/// happens in the poll on the main thread.
#[derive(Default)]
pub struct SharedCallbacks {
    callback_requested: AtomicBool,
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

/// The child side of a scan: loads one bundle and says what is in it. Everything that can go
/// wrong here is the plugin's, which is why the caller is a process of its own.
pub fn scan_bundle(bundle: &std::path::Path) -> Result<Vec<ScannedPlugin>, String> {
    // SAFETY: loading a plugin runs its code, which no host can check in advance. This is why
    // the scan runs in a child process. See `scan.rs`.
    let entry = unsafe { clack_host::entry::PluginEntry::load(bundle) }
        .map_err(|error| format!("{}: {error}", bundle.display()))?;
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
            version: text(descriptor.version()),
            features: descriptor
                .features()
                .map(|feature| feature.to_string_lossy().into_owned())
                .collect(),
            path: std::path::PathBuf::new(),
        });
    }
    Ok(plugins)
}

/// The folders macOS keeps CLAP plugins in, plus `CLAP_PATH` from the environment.
pub fn default_search_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
    }
    paths.push(std::path::PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    if let Some(extra) = std::env::var_os("CLAP_PATH") {
        paths.extend(std::env::split_paths(&extra));
    }
    paths
}

/// Loads the plugin `found` names, with `saved` as its own state, and starts it.
pub fn load(
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    sample_rate: u32,
) -> Result<Opening, PluginProblem> {
    let plugin_id = &found.id;
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: plugin_id.clone(),
        message,
    };
    // SAFETY: loading a plugin runs its code, which no host can check in advance. The scan ran
    // this same bundle in a child process first, so a bundle that crashes on load is already
    // known and never reaches here.
    let entry = unsafe { clack_host::entry::PluginEntry::load(&found.path) }
        .map_err(|error| fail(error.to_string()))?;
    let host_info = HostInfo::new(HOST_NAME, HOST_VENDOR, HOST_URL, HOST_VERSION)
        .map_err(|error| fail(error.to_string()))?;
    let identifier =
        std::ffi::CString::new(plugin_id.as_str()).map_err(|error| fail(error.to_string()))?;
    let mut instance = PluginInstance::<SoundToolsHost>::new(
        |_| SharedCallbacks::default(),
        |shared| MainThreadCallbacks {
            _shared: shared,
            state_is_dirty: Cell::new(false),
        },
        &entry,
        &identifier,
        &host_info,
    )
    .map_err(|error| fail(error.to_string()))?;

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

    let ports = read_ports(&mut instance);
    let configuration = PluginAudioConfiguration {
        sample_rate: f64::from(sample_rate),
        min_frames_count: 1,
        max_frames_count: MAX_BLOCK as u32,
    };
    let audio = instance
        .activate(|_, _| (), configuration)
        .map_err(|error| fail(error.to_string()))?;
    let notes = match ports.takes_midi {
        true => Vec::new(),
        // Audio inputs say nothing about a plugin: Six Sines is an instrument with a stereo
        // input for audio-rate modulation. They are fed with silence and it plays its notes.
        false => vec![PluginProblem::NoPedal {
            plugin_id: plugin_id.clone(),
        }],
    };
    let started = ClapStarted::new(
        audio.into(),
        ports.dialect,
        ports.takes_midi,
        ports.input_channels,
        ports.output_channels,
    );
    Ok(Opening {
        started: Box::new(started),
        plugin: Box::new(ClapPlugin {
            plugin_id: plugin_id.clone(),
            instance,
            created_window: false,
        }),
        notes,
    })
}

/// One loaded CLAP plugin, from the control thread.
pub struct ClapPlugin {
    plugin_id: String,
    instance: PluginInstance<SoundToolsHost>,
    /// Whether the plugin holds what it made for a window, so that `create` is never called
    /// twice, which CLAP forbids.
    created_window: bool,
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
        Requests {
            restart,
            // The flag is cleared here and the host keeps what it was told until the bytes are
            // written, so a change that the once-a-second rule made wait is not forgotten.
            state_is_dirty: self
                .instance
                .access_handler(|main| main.state_is_dirty.replace(false)),
            window_closed,
            window_size: (size != 0).then(|| {
                let size = GuiSize::unpack_from_u64(size);
                WindowSize {
                    width: size.width,
                    height: size.height,
                }
            }),
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

    fn released(&mut self) -> bool {
        self.instance.try_deactivate().is_ok()
    }
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

    unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), PluginProblem> {
        let gui = self.gui_extension().ok_or_else(|| self.no_window())?;
        // SAFETY: the caller keeps the view alive until `destroy` has run.
        let parent = unsafe { clack_extensions::gui::Window::from_cocoa_nsview(view.as_ptr()) };
        // SAFETY: as above.
        unsafe { gui.set_parent(&self.instance.plugin_handle(), parent) }
            .map_err(|error| self.failed(error))
    }

    fn show(&mut self) -> Result<(), PluginProblem> {
        let gui = self.gui_extension().ok_or_else(|| self.no_window())?;
        gui.show(&self.instance.plugin_handle())
            .map_err(|error| self.failed(error))
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
    /// Whether the note port takes MIDI, which is the only way to send the sustain pedal.
    takes_midi: bool,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    /// Silence for the first audio input port of the plugin, one buffer per channel. Empty
    /// when the plugin takes no audio in, which is the usual case for an instrument.
    input_channels: Vec<Vec<f32>>,
    /// The first audio output port of the plugin, one buffer per channel.
    output_channels: Vec<Vec<f32>>,
    input_events: EventBuffer,
    /// How many events are in the buffer, so a block never grows it.
    room: usize,
}

/// Which events the plugin's note port takes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Dialect {
    /// CLAP note events. The sustain pedal still needs MIDI, which has no note event for it.
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
            input_events: EventBuffer::with_capacity(EVENT_CAPACITY),
            room: EVENT_CAPACITY,
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
    fn takes_pedal(&self) -> bool {
        self.takes_midi
    }

    fn begin_block(&mut self) {
        self.input_events.clear();
        self.room = EVENT_CAPACITY;
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
            // The pedal always goes as raw MIDI, with its value: CLAP note events have no
            // sustain, so a plugin whose note port takes none does not get it at all.
            (PluginEvent::Pedal(pedal), _) => {
                let data = [0xB0, SUSTAIN_CONTROLLER, pedal.value()];
                self.input_events.push(&MidiEvent::new(offset, 0, data));
            }
        }
        true
    }

    fn run(&mut self, frames: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        let Self {
            audio,
            input_ports,
            output_ports,
            input_channels,
            output_channels,
            input_events,
            ..
        } = self;

        let inputs = if input_channels.is_empty() {
            InputAudioBuffers::empty()
        } else {
            input_ports.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    input_channels
                        .iter_mut()
                        .map(|channel| InputChannel::constant(&mut channel[..frames])),
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

        let Ok(started) = not_ours(|| audio.ensure_processing_started()) else {
            return false;
        };
        if not_ours(|| started.process(&inputs, &mut outputs, &input, &mut output, None, None))
            .is_err()
        {
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
