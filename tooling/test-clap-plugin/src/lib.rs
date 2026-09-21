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
//! Two environment variables change what it does while its bundle is listed, for tests of the
//! scan: `SOUND_TOOLS_TEST_PLUGIN_CRASH` aborts the process, and
//! `SOUND_TOOLS_TEST_PLUGIN_CHATTER` prints a line to standard output, as real plugins do.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::state::{HostState, PluginState, PluginStateImpl};
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

pub struct TestTone;

impl Plugin for TestTone {
    type AudioProcessor<'a> = TestToneAudio<'a>;
    type Shared<'a> = TestToneShared;
    type MainThread<'a> = TestToneMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&TestToneShared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
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
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a TestToneShared,
    ) -> Result<TestToneMainThread<'a>, PluginError> {
        Ok(TestToneMainThread { host, shared })
    }
}

/// What both threads read: the transpose and whether the host still has to save it.
pub struct TestToneShared {
    semitones: AtomicI32,
    state_is_dirty: AtomicBool,
}

impl PluginShared<'_> for TestToneShared {}

pub struct TestToneMainThread<'a> {
    host: HostMainThreadHandle<'a>,
    shared: &'a TestToneShared,
}

impl<'a> PluginMainThread<'a, TestToneShared> for TestToneMainThread<'a> {
    /// The audio thread asked for this call after it changed the state. Only the main thread
    /// may tell the host that the state is dirty.
    fn on_main_thread(&self) {
        if !self.shared.state_is_dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(state) = self.host.shared().get_extension::<HostState>() {
            state.mark_dirty(&self.host);
        }
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
}

impl<'a> PluginAudioProcessor<'a, TestToneShared, TestToneMainThread<'a>> for TestToneAudio<'a> {
    fn activate(
        host: HostAudioProcessorHandle<'a>,
        _main_thread: &TestToneMainThread<'a>,
        shared: &'a TestToneShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            host,
            voices: [None; VOICES],
            sample_rate: audio_config.sample_rate as f32,
            pedal: 0,
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
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
