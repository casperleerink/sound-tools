//! A CLAP instrument for the tests of the plugin host. It is not part of the product.
//!
//! What it sounds like, what it saves and the environment variables that make it misbehave are
//! all in `tooling/test-plugin-support`, which the VST 3 test plugin shares, so a test reads
//! either render the same way. What is here is the format.
//!
//! Two parameters a host can list, `Cutoff` and `Wave`, with the plugin's own text for their
//! values, and a read-only `Meter` and a `Hidden` one a host must leave out. `Wave` is a list of
//! names that may not be automated. Nothing moves them and nothing plays them.
//!
//! And `Level`, which is heard: how loud the instrument half plays, as the VST 3 test plugin's
//! `Level`. A host sets it with a parameter event in a block, and the plugin sets it itself on
//! `test_plugin_support::LEVEL_KEY`.
//!
//! Its window is a window in name only. It draws nothing, because CI has no display: it
//! answers the calls of the GUI extension and writes them down, so a test can say which call
//! arrived, in what order and on which thread. Like the real plugins this was written against,
//! it offers an embedded window and not a floating one.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use test_plugin_support as support;

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::{
    GuiConfiguration, GuiSize, HostGui, PluginGui, PluginGuiImpl, Window as GuiWindow,
};
use clack_extensions::latency::{HostLatency, PluginLatency, PluginLatencyImpl};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    HostParams, ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, ParamRescanFlags,
    PluginAudioProcessorParams, PluginMainThreadParams, PluginParams,
};
use clack_extensions::render::{PluginRender, PluginRenderImpl, RenderMode};
use clack_extensions::state::{HostState, PluginState, PluginStateImpl};
use clack_plugin::events::Match;
use clack_plugin::events::event_types::NoteEndEvent;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

/// The id a project record names. Also the name of the file the tests copy into a scan folder.
pub const PLUGIN_ID: &str = "sound-tools.test-tone";

use support::log;

pub struct TestTone;

impl Plugin for TestTone {
    type AudioProcessor<'a> = TestToneAudio<'a>;
    type Shared<'a> = TestToneShared;
    type MainThread<'a> = TestToneMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&TestToneShared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<PluginParams>()
            .register::<PluginGui>()
            .register::<PluginRender>()
            .register::<PluginState>()
            .register::<PluginLatency>();
    }
}

impl DefaultPluginFactory for TestTone {
    fn get_descriptor() -> PluginDescriptor {
        // Listing what a bundle holds is the first thing a scan does. A crash here is a crash
        // of whoever scans, which is what a test of a failing scan wants.
        support::while_listed("clap");
        PluginDescriptor::new(PLUGIN_ID, "Sound Tools Test Tone")
            .with_vendor("Sound Tools")
            .with_version("0.1.0")
            .with_description(
                "A test instrument and effect. One cosine per key, the pedal on the right, and what it is played times a half plus its offset.",
            )
            // Both, because this one plugin is both. A picker offers it for an instrument slot
            // and for an effect slot, and a record names it in either. Told to be audio only,
            // it says what an ordinary effect says.
            .with_features(
                match support::is_audio_only() {
                    true => [c"audio-effect", c"stereo"].as_slice(),
                    false => [c"instrument", c"audio-effect", c"synthesizer", c"stereo"].as_slice(),
                }
                .iter()
                .copied(),
            )
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<TestToneShared, PluginError> {
        Ok(TestToneShared {
            semitones: AtomicI32::new(0),
            offset: AtomicI32::new(0),
            state_is_dirty: AtomicBool::new(false),
            latency: AtomicU32::new(0),
            wanted_latency: AtomicU32::new(NO_LATENCY_WANTED),
            close_the_window: AtomicBool::new(false),
            answered: AtomicBool::new(!support::told_to(support::NEEDS_HOST_VARIABLE)),
            level: AtomicI32::new(support::FULL_EDIT_LEVEL),
            level_listed: AtomicBool::new(!support::told_to(support::LATE_LEVEL_VARIABLE)),
            rescan_wanted: AtomicBool::new(false),
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a TestToneShared,
    ) -> Result<TestToneMainThread<'a>, PluginError> {
        Ok(TestToneMainThread {
            host,
            shared,
            size: Cell::new((support::WINDOW_WIDTH, support::WINDOW_HEIGHT)),
        })
    }
}

/// What both threads read: the transpose, the offset of the effect half, whether the host
/// still has to save them, and whether this plugin is about to close its own window.
pub struct TestToneShared {
    semitones: AtomicI32,
    offset: AtomicI32,
    state_is_dirty: AtomicBool,
    /// The latency the plugin reports and plays with, in frames.
    latency: AtomicU32,
    /// A latency a note asked for, which the plugin takes the next time it is activated.
    /// [`NO_LATENCY_WANTED`] when none was.
    wanted_latency: AtomicU32,
    close_the_window: AtomicBool,
    /// Whether the host has answered the callback this plugin asked for. Until it has, a plugin
    /// that was told to wait for one is silent, as a sampler waiting for its samples is.
    answered: AtomicBool,
    /// How loud the instrument half plays, in hundredths: the `Level` parameter. The audio
    /// thread sets it and the host reads it on the main thread, so the two agree once a block
    /// has played the value.
    level: AtomicI32,
    /// Whether `Level` is in the list of parameters, see `test_plugin_support::LATE_LEVEL_VARIABLE`.
    level_listed: AtomicBool,
    /// The list changed and the host is to be told, which only the main thread may do.
    rescan_wanted: AtomicBool,
}

impl PluginShared<'_> for TestToneShared {}

/// What [`TestToneShared::wanted_latency`] holds when no note asked for one.
const NO_LATENCY_WANTED: u32 = u32::MAX;

impl TestToneShared {
    /// The latency the plugin will have once it is activated again: the one a note asked for,
    /// or else the one it has.
    fn next_latency(&self) -> u32 {
        match self.wanted_latency.load(Ordering::Acquire) {
            NO_LATENCY_WANTED => self.latency.load(Ordering::Acquire),
            wanted => wanted,
        }
    }
}

/// What the host reads after it activated the plugin, and after every restart.
impl PluginLatencyImpl for TestToneMainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.latency.load(Ordering::Acquire)
    }
}

pub struct TestToneMainThread<'a> {
    host: HostMainThreadHandle<'a>,
    shared: &'a TestToneShared,
    /// How big its window is. It changes only when the host sets another size, which only a
    /// resizable one takes.
    size: Cell<(u32, u32)>,
}

impl<'a> PluginMainThread<'a, TestToneShared> for TestToneMainThread<'a> {
    /// The audio thread asked for this call after it changed the state, or [`PluginGuiImpl::show`]
    /// did because the plugin is to close its own window. Both belong to the main thread.
    fn on_main_thread(&self) {
        // What a plugin that is waiting for its host waits for. A render that never does the
        // main-thread work of its host never gets here, and this plugin stays silent.
        self.shared.answered.store(true, Ordering::Release);
        if self.shared.close_the_window.swap(false, Ordering::AcqRel)
            && let Some(gui) = self.host.shared().get_extension::<HostGui>()
        {
            log("closed", 0, 0);
            gui.closed(&self.host.shared(), true);
        }
        if self.shared.rescan_wanted.swap(false, Ordering::AcqRel)
            && let Some(params) = self.host.shared().get_extension::<HostParams>()
        {
            log("rescan", 0, 0);
            params.rescan(&self.host, ParamRescanFlags::INFO);
        }
        if !self.shared.state_is_dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(state) = self.host.shared().get_extension::<HostState>() {
            state.mark_dirty(&self.host);
        }
    }
}

/// The window, in name only: no real one is made, because a test has no display. Every call is
/// written to the log, with the thread it came in on, so a test reads exactly what a host did.
///
/// Only an embedded window is offered, which is what the real CLAP plugins on the machine this
/// was written on offer, and what the host asks for.
impl PluginGuiImpl for TestToneMainThread<'_> {
    fn is_api_supported(&self, configuration: GuiConfiguration) -> bool {
        log("gui_is_api_supported", 0, 0);
        // A plugin with no window of its own at all, which a host has to say instead of
        // offering one. Every windowing API is refused, floating or not.
        !configuration.is_floating && !support::told_to(support::NO_WINDOW_VARIABLE)
    }

    fn get_preferred_api(&self) -> Option<GuiConfiguration<'_>> {
        None
    }

    fn create(&self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        log("gui_create", 0, 0);
        match configuration.is_floating {
            false => Ok(()),
            true => Err(PluginError::Message("this plugin does not float")),
        }
    }

    fn destroy(&self) {
        log("gui_destroy", 0, 0);
    }

    fn set_scale(&self, _scale: f64) -> Result<(), PluginError> {
        Err(PluginError::Message("Cocoa sizes are already logical"))
    }

    fn get_size(&self) -> Option<GuiSize> {
        let (width, height) = self.size.get();
        Some(GuiSize { width, height })
    }

    /// Whether the composer may resize the window by dragging its edge. Only when a test says
    /// so, because most real instruments keep one size.
    fn can_resize(&self) -> bool {
        log("gui_can_resize", 0, 0);
        support::told_to(support::RESIZABLE_VARIABLE)
    }

    fn adjust_size(&self, size: GuiSize) -> Option<GuiSize> {
        log("gui_adjust_size", 0, 0);
        if !support::told_to(support::RESIZABLE_VARIABLE)
            || support::told_to(support::NO_ADJUST_VARIABLE)
        {
            return None;
        }
        let (width, height) = support::constrained_size(size.width, size.height);
        Some(GuiSize { width, height })
    }

    fn set_size(&self, size: GuiSize) -> Result<(), PluginError> {
        log("gui_on_size", 0, 0);
        if !support::told_to(support::RESIZABLE_VARIABLE) {
            return Err(PluginError::Message("this window is not resizable"));
        }
        self.size.set((size.width, size.height));
        Ok(())
    }

    fn set_parent(&self, _window: GuiWindow) -> Result<(), PluginError> {
        log("gui_set_parent", 0, 0);
        Ok(())
    }

    fn set_transient(&self, _window: GuiWindow) -> Result<(), PluginError> {
        Err(PluginError::Message("this plugin does not float"))
    }

    /// A line of the log is read by splitting on spaces, so the title goes in with `_` for
    /// every space. A test still sees which title the host suggested.
    fn suggest_title(&self, title: &str) {
        log(
            &format!("gui_suggest_title[{}]", title.replace(' ', "_")),
            0,
            0,
        );
    }

    fn show(&self) -> Result<(), PluginError> {
        log("gui_show", 0, 0);
        // A plugin that sizes itself as it opens, which is what a real one does when its
        // interface is bigger than the size it first reported.
        if let Some((width, height)) = support::wanted_window_size()
            && let Some(gui) = self.host.shared().get_extension::<HostGui>()
        {
            log("gui_request_resize", 0, 0);
            let _asked = gui.request_resize(&self.host.shared(), width, height);
        }
        // A window the composer closes by its title bar. The host may not be told from inside
        // one of its own calls, so this asks for a call on the main thread and tells it there.
        if support::told_to(support::CLOSE_GUI_VARIABLE) {
            self.shared.close_the_window.store(true, Ordering::Release);
            self.host.shared().request_callback();
        }
        Ok(())
    }

    fn hide(&self) -> Result<(), PluginError> {
        log("gui_hide", 0, 0);
        Ok(())
    }
}

/// The ids of the parameters.
pub const CUTOFF: u32 = 0;
pub const WAVE: u32 = 1;
const METER: u32 = 2;
const HIDDEN: u32 = 3;
pub const LEVEL: u32 = 4;

/// The parameters, in the order `get_info` lists them: the id, the name, the range, the default
/// and the flags.
const PARAMETERS: [(u32, &str, f64, f64, f64, ParamInfoFlags); 5] = [
    (
        CUTOFF,
        "Cutoff",
        20.0,
        20_000.0,
        1000.0,
        ParamInfoFlags::IS_AUTOMATABLE,
    ),
    (
        WAVE,
        "Wave",
        0.0,
        2.0,
        0.0,
        ParamInfoFlags::IS_STEPPED.union(ParamInfoFlags::IS_ENUM),
    ),
    (METER, "Meter", 0.0, 1.0, 0.0, ParamInfoFlags::IS_READONLY),
    (
        HIDDEN,
        "Hidden",
        0.0,
        1.0,
        0.0,
        ParamInfoFlags::IS_AUTOMATABLE.union(ParamInfoFlags::IS_HIDDEN),
    ),
    (
        LEVEL,
        "Level",
        0.0,
        1.0,
        1.0,
        ParamInfoFlags::IS_AUTOMATABLE,
    ),
];

impl TestToneMainThread<'_> {
    /// The parameters the plugin lists now.
    fn listed(&self) -> impl Iterator<Item = &(u32, &str, f64, f64, f64, ParamInfoFlags)> {
        let level = self.shared.level_listed.load(Ordering::Acquire);
        PARAMETERS
            .iter()
            .filter(move |parameter| parameter.0 != LEVEL || level)
    }
}

/// Every parameter but `Level` stays at its default: nothing here moves one.
impl PluginMainThreadParams for TestToneMainThread<'_> {
    fn count(&self) -> u32 {
        self.listed().count() as u32
    }

    fn get_info(&self, param_index: u32, info: &mut ParamInfoWriter) {
        let Some(&(id, name, minimum, maximum, default, flags)) =
            self.listed().nth(param_index as usize)
        else {
            return;
        };
        info.set(&ParamInfo {
            id: ClapId::new(id),
            flags,
            cookie: Default::default(),
            name: name.as_bytes(),
            module: b"",
            min_value: minimum,
            max_value: maximum,
            default_value: default,
        });
    }

    fn get_value(&self, param_id: ClapId) -> Option<f64> {
        if param_id.get() == LEVEL {
            let level = self.shared.level.load(Ordering::Acquire);
            return Some(f64::from(level) / f64::from(support::FULL_EDIT_LEVEL));
        }
        PARAMETERS
            .iter()
            .find(|parameter| parameter.0 == param_id.get())
            .map(|parameter| parameter.4)
    }

    fn value_to_text(
        &self,
        param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        use std::fmt::Write as _;
        match param_id.get() {
            CUTOFF => write!(writer, "{value:.0} Hz"),
            LEVEL => write!(writer, "{} %", support::hundredths(value)),
            WAVE => {
                let wave = support::WAVES.get(value.round() as usize);
                writer.write_str(wave.ok_or(std::fmt::Error)?)
            }
            _ => Err(std::fmt::Error),
        }
    }

    fn text_to_value(&self, _param_id: ClapId, _text: &std::ffi::CStr) -> Option<f64> {
        None
    }

    fn flush(&self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

impl PluginAudioProcessorParams for TestToneAudio<'_> {
    fn flush(&mut self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

/// What kind of run this is. A plugin that streams from disk uses it to wait for its samples
/// instead of playing silence; this one writes it down, so a test can say what the host said.
impl PluginRenderImpl for TestToneMainThread<'_> {
    fn has_hard_realtime_requirement(&self) -> bool {
        false
    }

    fn set(&self, mode: RenderMode) -> Result<(), PluginError> {
        let name = match mode {
            RenderMode::Offline => "offline",
            RenderMode::Realtime => "realtime",
        };
        log(&format!("mode[{name}]"), 0, 0);
        Ok(())
    }
}

impl PluginStateImpl for TestToneMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write as _;
        // The two formats keep one state format, so a test reads either.
        let state = support::SavedState {
            semitones: self.shared.semitones.load(Ordering::Acquire),
            edit_level: self.shared.level.load(Ordering::Acquire),
            offset: self.shared.offset.load(Ordering::Acquire),
            latency: self.shared.next_latency() as i32,
        };
        output.write_all(&support::save_state(state))?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read as _;
        // To the end, not a fixed length: a state written before the effect half existed is
        // shorter, and `load_state` fills in what it does not carry.
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;
        let Some(state) = support::load_state(&bytes) else {
            return Err(PluginError::Message("not a Test Tone state"));
        };
        self.shared
            .semitones
            .store(state.semitones, Ordering::Release);
        self.shared.offset.store(state.offset, Ordering::Release);
        self.shared.level.store(state.edit_level, Ordering::Release);
        let latency = u32::try_from(state.latency).unwrap_or_default();
        self.shared
            .latency
            .store(latency.min(support::MAX_LATENCY), Ordering::Release);
        self.shared
            .wanted_latency
            .store(NO_LATENCY_WANTED, Ordering::Release);
        Ok(())
    }
}

/// One stereo port each way. The input is what the effect half is played; an instrument gets
/// silence there, which is what every audio input of a plugin got before effects existed.
impl PluginAudioPortsImpl for TestToneMainThread<'_> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index != 0 {
            return;
        }
        writer.set(&AudioPortInfo {
            id: ClapId::new(u32::from(is_input)),
            name: if is_input {
                b"in".as_slice()
            } else {
                b"main".as_slice()
            },
            channel_count: 2,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(AudioPortType::STEREO),
            in_place_pair: None,
        });
    }
}

/// One note input, unless this plugin is the audio-only effect: that one has nowhere to take
/// notes at all, which is what an ordinary effect looks like.
impl PluginNotePortsImpl for TestToneMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        match support::is_audio_only() {
            true => 0,
            false => u32::from(is_input),
        }
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if !is_input || index != 0 || support::is_audio_only() {
            return;
        }
        // Told to take no pedal, the port takes no MIDI, which is the only way a CLAP plugin
        // can be sent the sustain pedal: CLAP note events have none.
        let supported_dialects = match support::takes_no_pedal() {
            true => NoteDialects::CLAP,
            false => NoteDialects::CLAP | NoteDialects::MIDI,
        };
        writer.set(&NotePortInfo {
            id: ClapId::new(0),
            name: b"notes",
            supported_dialects,
            preferred_dialect: Some(NoteDialect::Clap),
        });
    }
}

pub struct TestToneAudio<'a> {
    shared: &'a TestToneShared,
    host: HostAudioProcessorHandle<'a>,
    tone: support::Tone,
    /// What the host played into this plugin, one buffer per channel. The input port may not be
    /// held while the output port is written, so a block is copied here first. It is as long as
    /// the host said a block can be, so `process` allocates nothing.
    input: [Vec<f32>; 2],
    /// Which plugin of this library this is, for the log.
    plugin: u64,
    /// Process calls so far, so the log says what happened before and after playing.
    processed: u64,
    /// How many events to send out of every process call.
    events_out: u32,
    /// The block from which this plugin gives up and writes nothing, when it was told to.
    fails_from: Option<u64>,
}

impl<'a> PluginAudioProcessor<'a, TestToneShared, TestToneMainThread<'a>> for TestToneAudio<'a> {
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        main_thread: &TestToneMainThread<'a>,
        shared: &'a TestToneShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let plugin = support::next_plugin();
        log("activate", plugin, 0);
        let mut tone = support::Tone::new(audio_config.sample_rate as f32);
        tone.set_semitones(shared.semitones.load(Ordering::Acquire));
        tone.set_offset(shared.offset.load(Ordering::Acquire));
        // The one moment CLAP lets a latency change: a note asked for it, and the host has
        // deactivated the plugin and is activating it again because the plugin asked.
        let wanted = shared
            .wanted_latency
            .swap(NO_LATENCY_WANTED, Ordering::AcqRel);
        if wanted != NO_LATENCY_WANTED {
            shared.latency.store(wanted, Ordering::Release);
            if let Some(latency) = main_thread.host.shared().get_extension::<HostLatency>() {
                latency.changed(&main_thread.host);
            }
        }
        tone.set_latency(shared.latency.load(Ordering::Acquire));
        let block = audio_config.max_frames_count as usize;
        Ok(Self {
            shared,
            host,
            tone,
            input: [vec![0.0; block], vec![0.0; block]],
            plugin,
            processed: 0,
            events_out: support::events_out(),
            fails_from: support::fails_from(),
        })
    }

    fn start_processing(&mut self) -> Result<(), PluginError> {
        log("start_processing", self.plugin, self.processed);
        Ok(())
    }

    fn stop_processing(&mut self) {
        log("stop_processing", self.plugin, self.processed);
    }

    fn deactivate(self, _main_thread: &TestToneMainThread<'a>) {
        log("deactivate", self.plugin, self.processed);
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        // A plugin that gives up: it writes nothing into its output and says so, which is
        // what a host must not turn into a gap in the chain.
        if self.fails_from.is_some_and(|block| self.processed >= block) {
            self.processed += 1;
            return Err(PluginError::Message("this plugin was told to fail"));
        }
        // The first one names the thread that processes, which is what the rest of a log is
        // read against.
        if self.processed == 0 {
            log("process", self.plugin, 0);
        }
        // A plugin that is waiting for its host: it asks for a callback on the main thread and
        // makes no sound until it gets one.
        if !self.shared.answered.load(Ordering::Acquire) {
            self.processed += 1;
            self.host.request_callback();
            return Ok(ProcessStatus::Continue);
        }
        let frames = audio.frames_count() as usize;
        // What the effect half is played, copied out before the output port is taken: the two
        // ports cannot be held at once. An unconnected input is silence.
        for channel in &mut self.input {
            channel[..frames].fill(0.0);
        }
        if let Some(port) = audio.input_port(0)
            && let Some(channels) = port.channels().ok().and_then(|it| it.into_f32())
        {
            for (index, played) in channels.iter().enumerate().take(self.input.len()) {
                self.input[index][..frames].copy_from_slice(&played[..frames]);
            }
        }
        let Some(mut port) = audio.output_port(0) else {
            return Ok(ProcessStatus::Sleep);
        };
        let Some(mut channels) = port.channels()?.into_f32() else {
            return Ok(ProcessStatus::Sleep);
        };
        let (mut first, mut second) = channels.split_at_mut(1);
        let (Some(left), Some(right)) = (first.channel_mut(0), second.channel_mut(0)) else {
            return Ok(ProcessStatus::Sleep);
        };

        let mut played = 0;
        for event in events.input {
            let at = (event.header().time() as usize).min(frames);
            // Everything up to this event, with the voices and the pedal as they were.
            self.tone
                .render(&mut left[played..at], &mut right[played..at]);
            played = at;
            match event.as_core_event() {
                Some(CoreEventSpace::NoteOn(note)) => {
                    if let Some(key) = note.key().into_specific() {
                        self.note_on(key as u8, note.velocity() as f32);
                    }
                }
                Some(CoreEventSpace::NoteOff(note)) => {
                    self.tone
                        .note_off(note.key().into_specific().map(|key| key as u8));
                }
                Some(CoreEventSpace::Midi(midi)) => self.midi(midi.data()),
                // A value the host sends. The last one of the block is how loud the whole
                // block plays, as in the VST 3 plugin.
                Some(CoreEventSpace::ParamValue(event))
                    if event.param_id().map(ClapId::get) == Some(LEVEL) =>
                {
                    let level = support::hundredths(event.value());
                    self.shared.level.store(level, Ordering::Release);
                }
                _ => {}
            }
        }
        self.tone
            .render(&mut left[played..frames], &mut right[played..frames]);
        let level = self.shared.level.load(Ordering::Acquire);
        if level != support::FULL_EDIT_LEVEL {
            let level = level as f32 / support::FULL_EDIT_LEVEL as f32;
            for sample in left.iter_mut() {
                *sample *= level;
            }
        }
        // The effect half, on top of whatever the instrument half played. It may learn an
        // offset from a loud input, which is a change of this plugin's own state.
        let [input_left, input_right] = &self.input;
        let played = (&input_left[..frames], &input_right[..frames]);
        if self.tone.effect(played, left, right) {
            self.shared
                .offset
                .store(self.tone.offset(), Ordering::Release);
            self.shared.state_is_dirty.store(true, Ordering::Release);
            self.host.request_callback();
        }
        // Last, so that everything it plays comes out as late as it says.
        self.tone.delay(left, right);
        self.processed += 1;
        // A plugin that sends more than a host kept room for. Whatever the host does with them,
        // it must not grow a buffer on this thread.
        for index in 0..self.events_out {
            let pckn = Pckn::new(0_u16, 0_u16, 60_u16, Match::All);
            let event = NoteEndEvent::new(index.min(frames as u32 - 1), pckn);
            // A host with no room left takes none of the rest either.
            if events.output.try_push(event.as_unknown()).is_err() {
                break;
            }
        }
        Ok(ProcessStatus::Continue)
    }

    fn reset(&mut self) {
        self.tone.reset();
    }
}

impl TestToneAudio<'_> {
    /// A note on, or a key that asks for something: a new latency, the plugin's own `Level`, or
    /// the list of its parameters with `Level` in it. The latency can only change while the
    /// host activates the plugin, so the plugin asks for that, and says its state changed,
    /// which holds the latency.
    fn note_on(&mut self, key: u8, velocity: f32) {
        if key == support::LEVEL_KEY {
            // A move of its own: the host reads the value on the main thread, and the state
            // that holds it is to be saved.
            let level = support::hundredths(f64::from(velocity));
            self.shared.level.store(level, Ordering::Release);
            self.shared.state_is_dirty.store(true, Ordering::Release);
            self.host.request_callback();
            return;
        }
        if key == support::LIST_LEVEL_KEY {
            // Only the main thread may tell the host that the list changed.
            log("level_listed", self.plugin, self.processed);
            self.shared.level_listed.store(true, Ordering::Release);
            self.shared.rescan_wanted.store(true, Ordering::Release);
            self.host.request_callback();
            return;
        }
        let Some(latency) = support::asked_latency(key, velocity) else {
            self.tone.note_on(key, velocity);
            return;
        };
        log("latency_asked", self.plugin, self.processed);
        self.shared
            .wanted_latency
            .store(latency.min(support::MAX_LATENCY), Ordering::Release);
        self.shared.state_is_dirty.store(true, Ordering::Release);
        self.host.request_restart();
        self.host.request_callback();
    }

    /// Raw MIDI: the sustain pedal, the bend and mod wheels and the channel pressure. The rest
    /// means nothing here.
    fn midi(&mut self, data: [u8; 3]) {
        let seven_bits = |byte: u8| f64::from(byte) / 127.0;
        match (data[0] & 0xF0, data[1]) {
            (0xE0, _) => {
                let value = u16::from(data[1]) | u16::from(data[2]) << 7;
                self.tone
                    .wheel(support::Wheel::Bend, f64::from(value) / 16383.0);
            }
            (0xB0, 1) => self
                .tone
                .wheel(support::Wheel::ModWheel, seven_bits(data[2])),
            (0xD0, _) => self
                .tone
                .wheel(support::Wheel::Pressure, seven_bits(data[1])),
            (0xB0, 64) => self.pedal(data[2]),
            _ => {}
        }
    }

    fn pedal(&mut self, value: u8) {
        if !self.tone.pedal(value) {
            return;
        }
        // The transpose changed, which is a change of the plugin's own state. Only the main
        // thread may tell the host, so this asks for a call there.
        self.shared
            .semitones
            .store(self.tone.semitones(), Ordering::Release);
        self.shared.state_is_dirty.store(true, Ordering::Release);
        self.host.request_callback();
    }
}

clack_export_entry!(SinglePluginEntry<TestTone>);

/// Where `cargo` put this crate's dynamic library, building it first.
pub fn built_library() -> std::path::PathBuf {
    support::built_library("test-clap-plugin")
}

/// Copies the built library into `folder` as a bundle a CLAP scan finds, and gives back the
/// bundle. On macOS a plain file with a `.clap` name is a valid bundle.
pub fn install_into(folder: &std::path::Path) -> std::path::PathBuf {
    support::install_bundle(folder, &built_library(), "test-tone", "clap")
}
