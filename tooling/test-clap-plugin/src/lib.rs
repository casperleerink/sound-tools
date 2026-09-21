//! A CLAP instrument for the tests of the plugin host. It is not part of the product.
//!
//! What it sounds like, what it saves and the environment variables that make it misbehave are
//! all in `tooling/test-plugin-support`, which the VST 3 test plugin shares, so a test reads
//! either render the same way. What is here is the format.
//!
//! Its window is a window in name only. It draws nothing, because CI has no display: it
//! answers the calls of the GUI extension and writes them down, so a test can say which call
//! arrived, in what order and on which thread. Like the real plugins this was written against,
//! it offers an embedded window and not a floating one.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use test_plugin_support as support;

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::{
    GuiConfiguration, GuiSize, HostGui, PluginGui, PluginGuiImpl, Window as GuiWindow,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
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
            .register::<PluginGui>()
            .register::<PluginRender>()
            .register::<PluginState>();
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
            .with_description("A test instrument. One cosine per key, the pedal on the right.")
            .with_features([c"instrument", c"synthesizer", c"stereo"])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<TestToneShared, PluginError> {
        Ok(TestToneShared {
            semitones: AtomicI32::new(0),
            state_is_dirty: AtomicBool::new(false),
            close_the_window: AtomicBool::new(false),
            answered: AtomicBool::new(!support::told_to(support::NEEDS_HOST_VARIABLE)),
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a TestToneShared,
    ) -> Result<TestToneMainThread<'a>, PluginError> {
        Ok(TestToneMainThread { host, shared })
    }
}

/// What both threads read: the transpose, whether the host still has to save it, and whether
/// this plugin is about to close its own window.
pub struct TestToneShared {
    semitones: AtomicI32,
    state_is_dirty: AtomicBool,
    close_the_window: AtomicBool,
    /// Whether the host has answered the callback this plugin asked for. Until it has, a plugin
    /// that was told to wait for one is silent, as a sampler waiting for its samples is.
    answered: AtomicBool,
}

impl PluginShared<'_> for TestToneShared {}

pub struct TestToneMainThread<'a> {
    host: HostMainThreadHandle<'a>,
    shared: &'a TestToneShared,
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
        Some(GuiSize {
            width: support::WINDOW_WIDTH,
            height: support::WINDOW_HEIGHT,
        })
    }

    fn set_size(&self, _size: GuiSize) -> Result<(), PluginError> {
        Err(PluginError::Message("this window is not resizable"))
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
        let semitones = self.shared.semitones.load(Ordering::Acquire);
        // The CLAP plugin has no parameter a host can edit, so its level is always the full
        // one. The two formats keep one state format all the same, so a test reads either.
        output.write_all(&support::save_state(semitones, support::FULL_EDIT_LEVEL))?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read as _;
        let mut bytes = [0_u8; 12];
        input.read_exact(&mut bytes)?;
        let Some((semitones, _level)) = support::load_state(&bytes) else {
            return Err(PluginError::Message("not a Test Tone state"));
        };
        self.shared.semitones.store(semitones, Ordering::Release);
        Ok(())
    }
}

impl PluginAudioPortsImpl for TestToneMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(!is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if is_input || index != 0 {
            return;
        }
        writer.set(&AudioPortInfo {
            id: ClapId::new(0),
            name: b"main",
            channel_count: 2,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(AudioPortType::STEREO),
            in_place_pair: None,
        });
    }
}

impl PluginNotePortsImpl for TestToneMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if !is_input || index != 0 {
            return;
        }
        writer.set(&NotePortInfo {
            id: ClapId::new(0),
            name: b"notes",
            supported_dialects: NoteDialects::CLAP | NoteDialects::MIDI,
            preferred_dialect: Some(NoteDialect::Clap),
        });
    }
}

pub struct TestToneAudio<'a> {
    shared: &'a TestToneShared,
    host: HostAudioProcessorHandle<'a>,
    tone: support::Tone,
    /// Which plugin of this library this is, for the log.
    plugin: u64,
    /// Process calls so far, so the log says what happened before and after playing.
    processed: u64,
    /// How many events to send out of every process call.
    events_out: u32,
}

impl<'a> PluginAudioProcessor<'a, TestToneShared, TestToneMainThread<'a>> for TestToneAudio<'a> {
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        _main_thread: &TestToneMainThread<'a>,
        shared: &'a TestToneShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let plugin = support::next_plugin();
        log("activate", plugin, 0);
        let mut tone = support::Tone::new(audio_config.sample_rate as f32);
        tone.set_semitones(shared.semitones.load(Ordering::Acquire));
        Ok(Self {
            shared,
            host,
            tone,
            plugin,
            processed: 0,
            events_out: support::events_out(),
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
                        self.tone.note_on(key as u8, note.velocity() as f32);
                    }
                }
                Some(CoreEventSpace::NoteOff(note)) => {
                    self.tone
                        .note_off(note.key().into_specific().map(|key| key as u8));
                }
                Some(CoreEventSpace::Midi(midi)) => self.midi(midi.data()),
                _ => {}
            }
        }
        self.tone
            .render(&mut left[played..frames], &mut right[played..frames]);
        self.processed += 1;
        // A plugin that sends more than a host kept room for. Whatever the host does with them,
        // it must not grow a buffer on this thread.
        for index in 0..self.events_out {
            let pckn = Pckn::new(0_u16, 0_u16, 60_u16, Match::All);
            let event = NoteEndEvent::new(index.min(frames as u32 - 1), pckn);
            let _ = events.output.try_push(event.as_unknown());
        }
        Ok(ProcessStatus::Continue)
    }

    fn reset(&mut self) {
        self.tone.reset();
    }
}

impl TestToneAudio<'_> {
    /// Raw MIDI. Only controller 64, the sustain pedal, means anything here.
    fn midi(&mut self, data: [u8; 3]) {
        let is_controller = data[0] & 0xF0 == 0xB0;
        if !is_controller || data[1] != 64 {
            return;
        }
        if !self.tone.pedal(data[2]) {
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
