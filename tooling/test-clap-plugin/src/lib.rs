//! A CLAP instrument for the tests of the plugin host. It is not part of the product.
//!
//! CI has no third-party plugin, so the repository builds one. It is small on purpose and
//! everything it does is directly readable from a render:
//!
//! - The left channel is the sum of `velocity * cos(2 pi f t)` over the keys that are down, so
//!   a note is audible from exactly the frame its note on arrived on, and a test can say which
//!   frame that was.
//! - The right channel is the sustain pedal as a number, `value / 127`. So a test reads the
//!   pedal value the plugin received, not whether it was up or down.
//! - The saved state is one number, `semitones`, which transposes every note. A pedal value of
//!   64 or more sets it to `value - 64` and marks the state dirty, which is the only way a
//!   plugin can change its own state without a window of its own. A pedal that comes up leaves
//!   it alone, so releasing the pedal, or the all-notes-off of a stop, does not transpose.
//!
//! So a test presses the pedal to 100, the plugin transposes by 36 from then on, and the host
//! saves `36` into the project. After a close and a reopen the notes are still transposed,
//! with no pedal in the clip.
//!
//! Environment variables change what it does, for tests that need a plugin that misbehaves.
//! While its bundle is listed, which is what a scan does in a child process:
//! `SOUND_TOOLS_TEST_PLUGIN_CRASH` aborts, `SOUND_TOOLS_TEST_PLUGIN_CHATTER` prints a line to
//! standard output as real plugins do, and `SOUND_TOOLS_TEST_PLUGIN_HANG` never returns.
//! While it plays, which is in the process of whoever loads it:
//! `SOUND_TOOLS_TEST_PLUGIN_EVENTS` sends that many events out of every process call,
//! `SOUND_TOOLS_TEST_PLUGIN_LOG` names a file this plugin writes one line to for every
//! lifecycle call it gets, with the thread it arrived on, and
//! `SOUND_TOOLS_TEST_PLUGIN_CLOSE_GUI` makes it close its own window as soon as it was shown,
//! which is what a composer does with the title bar of a real plugin's window.
//!
//! Its window is a window in name only. It makes no real one, because CI has no display: it
//! answers the calls of the GUI extension and writes them down, so a test can say which call
//! arrived, in what order and on which thread.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

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
use clack_extensions::state::{HostState, PluginState, PluginStateImpl};
use clack_plugin::events::Match;
use clack_plugin::events::event_types::NoteEndEvent;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

/// The id a project record names. Also the name of the file the tests copy into a scan folder.
pub const PLUGIN_ID: &str = "sound-tools.test-tone";

/// How many keys can sound at once. Fixed, so `process` never allocates.
const VOICES: usize = 16;

/// The first four bytes of the saved state, so a wrong file is refused instead of read.
const STATE_MAGIC: [u8; 4] = *b"STT1";

/// Aborts the process while the bundle is listed, for a test of a scan that crashes.
const CRASH_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CRASH";

/// Prints to standard output while the bundle is listed, as real plugins do. A scan must read
/// its own lines and leave the plugin's alone.
const CHATTER_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CHATTER";

/// Never returns while the bundle is listed, as a licensed plugin that cannot reach its server
/// does. A scan must give up on it and live.
const HANG_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_HANG";

/// How many events to send out of every process call. A host must have somewhere to put them
/// that neither grows nor allocates on the audio thread.
const EVENTS_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_EVENTS";

/// A file this plugin appends one line to for every lifecycle call, with the thread it came in
/// on and how many process calls had happened by then. A test reads it to check that the host
/// starts and stops processing on the thread that processes.
const LOG_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_LOG";

/// Makes the plugin close its own window as soon as the host has shown it, which is what a
/// composer does with the title bar of a real plugin's window.
const CLOSE_GUI_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CLOSE_GUI";

/// Appends `call` to the log file, when there is one. The thread is the number the operating
/// system gives it, so a test can say "the same thread as `process`" without naming it.
fn log(call: &str, plugin: u64, processed: u64) {
    let Some(path) = std::env::var_os(LOG_VARIABLE) else {
        return;
    };
    use std::io::Write as _;
    let thread = format!("{:?}", std::thread::current().id());
    let line = format!("{call} plugin={plugin} thread={thread} processed={processed}\n");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Counts the audio processors this library has made, so a log says which plugin a call is
/// about when a test swaps one for another.
static NEXT_PLUGIN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for TestTone {
    fn get_descriptor() -> PluginDescriptor {
        // Listing what a bundle holds is the first thing a scan does. A crash here is a crash
        // of whoever scans, which is what a test of a failing scan wants.
        if std::env::var_os(CRASH_VARIABLE).is_some() {
            std::process::abort();
        }
        if std::env::var_os(CHATTER_VARIABLE).is_some() {
            println!("test-clap-plugin: initializing, version 0.1.0");
        }
        if std::env::var_os(HANG_VARIABLE).is_some() {
            // Long past any deadline a host could give it. Whoever waits must stop waiting.
            std::thread::sleep(std::time::Duration::from_secs(600));
        }
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
/// Only a floating window is offered, which is the one CLAP says every plugin must support.
impl PluginGuiImpl for TestToneMainThread<'_> {
    fn is_api_supported(&self, configuration: GuiConfiguration) -> bool {
        log("gui_is_api_supported", 0, 0);
        configuration.is_floating
    }

    fn get_preferred_api(&self) -> Option<GuiConfiguration<'_>> {
        None
    }

    fn create(&self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        log("gui_create", 0, 0);
        match configuration.is_floating {
            true => Ok(()),
            false => Err(PluginError::Message("this plugin only floats")),
        }
    }

    fn destroy(&self) {
        log("gui_destroy", 0, 0);
    }

    fn set_scale(&self, _scale: f64) -> Result<(), PluginError> {
        Err(PluginError::Message("a floating window scales itself"))
    }

    fn get_size(&self) -> Option<GuiSize> {
        Some(GuiSize {
            width: 320,
            height: 240,
        })
    }

    fn set_size(&self, _size: GuiSize) -> Result<(), PluginError> {
        Err(PluginError::Message("a floating window sizes itself"))
    }

    fn set_parent(&self, _window: GuiWindow) -> Result<(), PluginError> {
        Err(PluginError::Message("this plugin embeds nowhere"))
    }

    fn set_transient(&self, _window: GuiWindow) -> Result<(), PluginError> {
        log("gui_set_transient", 0, 0);
        Ok(())
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
        // A window the composer closes by its title bar. The host may not be told from inside
        // one of its own calls, so this asks for a call on the main thread and tells it there.
        if std::env::var_os(CLOSE_GUI_VARIABLE).is_some() {
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

impl PluginStateImpl for TestToneMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write as _;
        let semitones = self.shared.semitones.load(Ordering::Acquire);
        output.write_all(&STATE_MAGIC)?;
        output.write_all(&semitones.to_le_bytes())?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read as _;
        let mut bytes = [0_u8; 8];
        input.read_exact(&mut bytes)?;
        if bytes[..4] != STATE_MAGIC {
            return Err(PluginError::Message("not a Test Tone state"));
        }
        let semitones = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
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

/// One key that is down.
#[derive(Copy, Clone)]
struct Voice {
    key: u16,
    amplitude: f32,
    /// Radians per frame.
    step: f32,
    phase: f32,
}

pub struct TestToneAudio<'a> {
    shared: &'a TestToneShared,
    host: HostAudioProcessorHandle<'a>,
    voices: [Option<Voice>; VOICES],
    sample_rate: f32,
    pedal: u8,
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
        let plugin = NEXT_PLUGIN.fetch_add(1, Ordering::AcqRel);
        log("activate", plugin, 0);
        let events_out = std::env::var(EVENTS_VARIABLE)
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(0);
        Ok(Self {
            shared,
            host,
            voices: [None; VOICES],
            sample_rate: audio_config.sample_rate as f32,
            pedal: 0,
            plugin,
            processed: 0,
            events_out,
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
        left[..frames].fill(0.0);

        let mut played = 0;
        for event in events.input {
            let at = (event.header().time() as usize).min(frames);
            // Everything up to this event, with the voices and the pedal as they were.
            self.render(&mut left[played..at], &mut right[played..at]);
            played = at;
            match event.as_core_event() {
                Some(CoreEventSpace::NoteOn(note)) => self.note_on(note),
                Some(CoreEventSpace::NoteOff(note)) => self.note_off(note),
                Some(CoreEventSpace::Midi(midi)) => self.midi(midi.data()),
                _ => {}
            }
        }
        self.render(&mut left[played..frames], &mut right[played..frames]);
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
        self.voices = [None; VOICES];
        self.pedal = 0;
    }
}

impl TestToneAudio<'_> {
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let pedal = f32::from(self.pedal) / 127.0;
        right.fill(pedal);
        for voice in self.voices.iter_mut().flatten() {
            for sample in left.iter_mut() {
                *sample += voice.amplitude * voice.phase.cos();
                voice.phase += voice.step;
                // Wrapping by subtraction keeps the phase exact enough and never allocates.
                if voice.phase > std::f32::consts::TAU {
                    voice.phase -= std::f32::consts::TAU;
                }
            }
        }
    }

    fn note_on(&mut self, note: &clack_plugin::events::event_types::NoteOnEvent) {
        let Some(key) = note.key().into_specific() else {
            return;
        };
        let semitones = self.shared.semitones.load(Ordering::Acquire);
        let sounding = i32::from(key).saturating_add(semitones).clamp(0, 127);
        let frequency = 440.0 * ((sounding as f32 - 69.0) / 12.0).exp2();
        let voice = Voice {
            key,
            amplitude: note.velocity() as f32,
            step: std::f32::consts::TAU * frequency / self.sample_rate,
            phase: 0.0,
        };
        // A key that is already down keeps sounding: a new voice takes the first free place,
        // and a full list drops the note. Neither allocates.
        if let Some(free) = self.voices.iter_mut().find(|voice| voice.is_none()) {
            *free = Some(voice);
        }
    }

    fn note_off(&mut self, note: &clack_plugin::events::event_types::NoteOffEvent) {
        let key = note.key().into_specific();
        for slot in &mut self.voices {
            let matches = slot.is_some_and(|voice| key.is_none_or(|key| voice.key == key));
            if matches {
                *slot = None;
            }
        }
    }

    /// Raw MIDI. Only controller 64, the sustain pedal, means anything here.
    fn midi(&mut self, data: [u8; 3]) {
        let is_controller = data[0] & 0xF0 == 0xB0;
        if !is_controller || data[1] != 64 {
            return;
        }
        self.pedal = data[2].min(127);
        // Down: the transpose follows the value, so a test changes the plugin's own state
        // through the note contract. Up: leave it, else every stop would transpose.
        if self.pedal >= 64 {
            let semitones = i32::from(self.pedal) - 64;
            if self.shared.semitones.swap(semitones, Ordering::AcqRel) != semitones {
                self.shared.state_is_dirty.store(true, Ordering::Release);
                // Only the main thread may tell the host. Ask for a call there.
                self.host.request_callback();
            }
        }
    }
}

clack_export_entry!(SinglePluginEntry<TestTone>);

/// Where `cargo` put this crate's dynamic library, for a test that wants to load it.
///
/// `cargo test` builds the crate for testing but not its dynamic library, so this builds it
/// first. That is a few tenths of a second when it is already up to date, and it means a test
/// always loads the plugin as the source says it is.
pub fn built_library() -> std::path::PathBuf {
    let name = format!(
        "{}test_clap_plugin{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    BUILT.get_or_init(|| {
        let output = std::process::Command::new(env!("CARGO"))
            .args(["build", "--package", "test-clap-plugin"])
            .output()
            .expect("cargo builds the test plugin");
        assert!(
            output.status.success(),
            "cargo build -p test-clap-plugin failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
    let mut folder = std::env::current_exe().expect("the path of the test executable");
    while folder.pop() {
        let candidate = folder.join(&name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("{name} is not built. Run `cargo build -p test-clap-plugin` first");
}

/// Copies the built library into `folder` as a bundle a CLAP scan finds, and gives the folder
/// to search. On macOS a plain file with a `.clap` name is a valid bundle.
pub fn install_into(folder: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(folder).expect("the plugin folder");
    let bundle = folder.join("test-tone.clap");
    std::fs::copy(built_library(), &bundle).expect("a copy of the test plugin");
    bundle
}
