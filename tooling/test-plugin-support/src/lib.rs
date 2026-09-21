//! What the repository's own test plugins have in common, so that the CLAP one and the VST 3
//! one behave the same and a test can read either render the same way. Not part of the product.
//!
//! CI has no third-party plugin, so the repository builds one per format. Everything they do is
//! directly readable from a render:
//!
//! - The left channel is the sum of `velocity * cos(2 pi f t)` over the keys that are down, so
//!   a note is audible from exactly the frame its note on arrived on, and a test can say which
//!   frame that was.
//! - The right channel is the sustain pedal as a number, `value / 127`. So a test reads the
//!   pedal value the plugin received, not whether it was up or down.
//! - The saved state is one number, `semitones`, which transposes every note. A pedal value of
//!   64 or more sets it to `value - 64` and says the state changed, which is how a test makes
//!   a plugin change its own state without a window of its own. A pedal that comes up leaves it
//!   alone, so releasing the pedal, or the all-notes-off of a stop, does not transpose.
//!
//! So a test presses the pedal to 100, the plugin transposes by 36 from then on, and the host
//! saves `36` into the project. After a close and a reopen the notes are still transposed, with
//! no pedal in the clip.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Aborts the process while the bundle is listed, for a test of a scan that crashes.
pub const CRASH_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CRASH";

/// Prints to standard output while the bundle is listed, as real plugins do. A scan must read
/// its own lines and leave the plugin's alone. It prints more than a pipe holds, so a scan
/// that only reads when the child has ended would leave the child blocked on its own write.
pub const CHATTER_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CHATTER";

/// How much a chatty plugin prints. A pipe on macOS holds 64 kB, and this is more.
const CHATTER_LINES: usize = 4000;

/// Never returns while the bundle is listed, as a licensed plugin that cannot reach its server
/// does. A scan must give up on it and live. `1` makes every test plugin hang; the name of one
/// format makes only that one hang, so a scan can have a bundle that answers next to one that
/// never does.
pub const HANG_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_HANG";

/// How many things to send out of every process call: CLAP events, VST 3 parameter changes. A
/// host must have somewhere to put them that neither grows nor allocates on the audio thread.
pub const EVENTS_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_EVENTS";

/// A file the plugin appends one line to for every lifecycle call, with the thread it came in
/// on and how many process calls had happened by then. A test reads it to check that the host
/// starts and stops processing on the thread that processes.
pub const LOG_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_LOG";

/// Makes the plugin close its own window as soon as the host has shown it, which is what a
/// composer does with the title bar of a real plugin's window. CLAP only: VST 3 has no call a
/// plugin can close its window with, because the host owns that window.
pub const CLOSE_GUI_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CLOSE_GUI";

/// Makes the plugin offer no window at all, so a host has to say so instead of offering one.
/// The CLAP plugin then says no windowing API suits it; the VST 3 one makes no view.
pub const NO_WINDOW_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_NO_WINDOW";

/// Makes the plugin ask its host for another window size as soon as it has a window, the way a
/// plugin that sizes itself as it opens does. The value is `<width>x<height>`.
pub const RESIZE_GUI_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_RESIZE_GUI";

/// Makes the VST 3 plugin's view ask for another size from inside `onSize`, which is inside the
/// host's answer to a request of its own. A host without a guard runs out of stack on this. The
/// value is `<width>x<height>`; the same size as the outer request is the worst case, because a
/// host that only compares sizes would answer for ever. VST 3 only.
pub const RESIZE_IN_ON_SIZE_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_RESIZE_IN_ON_SIZE";

/// Makes the VST 3 plugin's view ask its host for another size from inside `attached`, which
/// `iplugview.h` says a plugin may do. The value is `<width>x<height>`. VST 3 only.
pub const RESIZE_IN_ATTACHED_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_RESIZE_IN_ATTACHED";

/// Makes the VST 3 plugin's view refuse `attached`, so a host has to leave it alone afterwards
/// and must not call `removed` for an `attached` that never happened. VST 3 only.
pub const ATTACH_FAILS_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_ATTACH_FAILS";

/// Makes the plugin's controller edit a parameter through the host as soon as it has a
/// component handler, the way a plugin's own window does when the composer turns a knob: a
/// `beginEdit`, that many `performEdit`s ending on `1 / count`, and an `endEdit`. The plugin's
/// processor is what reads the parameter, so a host that does not carry the edit across plays
/// the plugin at its full level. The value is the count.
pub const EDITS_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_EDITS";

/// The size a plugin was told to ask its window to be, if it was told.
pub fn wanted_window_size() -> Option<(u32, u32)> {
    size_from(RESIZE_GUI_VARIABLE)
}

/// The size a plugin was told to ask for from inside `onSize`, if it was told.
pub fn wanted_size_in_on_size() -> Option<(u32, u32)> {
    size_from(RESIZE_IN_ON_SIZE_VARIABLE)
}

/// The size a plugin was told to ask for from inside `attached`, if it was told.
pub fn wanted_size_in_attached() -> Option<(u32, u32)> {
    size_from(RESIZE_IN_ATTACHED_VARIABLE)
}

fn size_from(variable: &str) -> Option<(u32, u32)> {
    let told = std::env::var(variable).ok()?;
    let (width, height) = told.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// How many edits the plugin was told to make. `None` says it was not told.
pub fn wanted_edits() -> Option<u32> {
    let count: u32 = std::env::var(EDITS_VARIABLE).ok()?.parse().ok()?;
    (count > 0).then_some(count)
}

/// How big a test plugin's window is until it asks for another size. Both formats answer this,
/// so a test reads either window the same way.
pub const WINDOW_WIDTH: u32 = 320;
pub const WINDOW_HEIGHT: u32 = 240;

/// Makes the plugin say its output is silent and write nothing into it, from its second block
/// on. VST 3 allows that (`silenceFlags`), and a host that does not clear its own output
/// buffers would then play the block before over and over. VST 3 only: CLAP has no such flag.
pub const SILENT_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_SILENT";

/// Makes the VST 3 plugin's edit controller keep a state of its own: how loud it plays. One
/// object that is both halves does not promise that its two states are the same bytes, and a
/// host that only asks a controller that is a second object loses this one. VST 3 only.
pub const CONTROLLER_STATE_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CONTROLLER_STATE";

/// Makes the VST 3 plugin's edit controller fail to give its state. A host must not write an
/// empty state over the good one when it cannot get the real one. VST 3 only.
pub const CONTROLLER_FAILS_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_CONTROLLER_FAILS";

/// Makes the VST 3 plugin write its state with the header last: it leaves room, writes the
/// payload, seeks back and fills the header in. Plugins really do this, and a host stream that
/// clamps a seek to what it has so far turns it into a state that is not the plugin's.
/// VST 3 only.
pub const HEADER_LAST_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_HEADER_LAST";

/// Makes the plugin silent until the host has answered it on the main thread, which is what a
/// plugin that streams from disk does while it waits for its samples. A render that never does
/// the main-thread work of its host renders that silence and calls it music.
///
/// CLAP asks with `request_callback`. VST 3 has no such call: its plugin reports a parameter it
/// changed by itself and waits for the host to give it back through `setParamNormalized`, which
/// is the main-thread work a VST 3 host does for a plugin.
pub const NEEDS_HOST_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_NEEDS_HOST";

/// Makes the plugin leave a helper process behind while its bundle is listed, one that holds
/// the output pipe open and outlives the child of the scan, as a licensing helper does.
///
/// The value is a path the test owns. The helper holds the pipe until `<path>.stop` is there,
/// and then makes `<path>.done`, so a test says itself when the helper may go and can wait for
/// it: nothing of the test is left running. It also gives up by itself after half a minute, so
/// nothing can wait for ever.
pub const DESCENDANT_VARIABLE: &str = "SOUND_TOOLS_TEST_PLUGIN_DESCENDANT";

/// How long the helper holds the pipe when nobody stops it, in twentieths of a second.
const DESCENDANT_LIMIT: u32 = 600;

/// The first four bytes of the saved state, so a wrong file is refused instead of read.
const STATE_MAGIC: [u8; 4] = *b"STT1";

/// Whether an environment variable is set, which is how a test tells the plugin to misbehave.
pub fn told_to(variable: &str) -> bool {
    std::env::var_os(variable).is_some()
}

/// What every test plugin does while its bundle is listed, which is the first thing a scan
/// makes it do. A crash here is a crash of whoever scans, which is what those tests want.
///
/// `format` is `clap` or `vst3`, so a test can make one format hang and leave the other alone.
pub fn while_listed(format: &str) {
    if told_to(CRASH_VARIABLE) {
        std::process::abort();
    }
    if told_to(CHATTER_VARIABLE) {
        for line in 0..CHATTER_LINES {
            println!("test-{format}-plugin: initializing, version 0.1.0, line {line} of noise");
        }
    }
    if let Some(done) = std::env::var_os(DESCENDANT_VARIABLE) {
        // A helper that inherits this process's standard output and outlives it. The pipe the
        // scan reads ends when its last writer lets go, so whoever waits for the end of that
        // pipe waits for this helper and not for the plugin. The test says when it may go.
        let script = format!(
            "n=0; while [ ! -f \"$0.stop\" ] && [ $n -lt {DESCENDANT_LIMIT} ]; do sleep 0.05; n=$((n+1)); done; : > \"$0.done\""
        );
        // Starting it blocks this thread for as long as a fork takes, which is what a plugin
        // that starts a licensing helper does to whoever loads it. This is the child of a
        // scan; nothing here is near an audio thread.
        #[allow(clippy::disallowed_methods)]
        let _started = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg(done)
            .spawn();
    }
    let hang = std::env::var(HANG_VARIABLE).unwrap_or_default();
    if hang == "1" || hang == format {
        // Long past any deadline a host could give it. Whoever waits must stop waiting.
        std::thread::sleep(std::time::Duration::from_secs(600));
    }
}

/// The second state of a VST 3 plugin: the one its edit controller keeps, which a host saves
/// next to the component's. One number, as the component's state is.
const CONTROLLER_MAGIC: [u8; 4] = *b"STC1";

/// How loud the plugin plays, from the controller's own state, as hundredths.
pub fn save_controller_state(level: i32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&CONTROLLER_MAGIC);
    bytes.extend_from_slice(&level.to_le_bytes());
    bytes
}

/// What a controller state holds. `None` says the bytes are not one of ours.
pub fn load_controller_state(bytes: &[u8]) -> Option<i32> {
    let rest = bytes.strip_prefix(&CONTROLLER_MAGIC)?;
    let four: [u8; 4] = rest.get(..4)?.try_into().ok()?;
    Some(i32::from_le_bytes(four))
}

/// How many things to send out of every process call.
pub fn events_out() -> u32 {
    std::env::var(EVENTS_VARIABLE)
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

/// Appends `call` to the log file, when there is one. The thread is the number the operating
/// system gives it, so a test can say "the same thread as `process`" without naming it.
pub fn log(call: &str, plugin: u64, processed: u64) {
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

/// Counts the audio processors the test plugins have made, so a log says which plugin a call is
/// about when a test swaps one for another.
static NEXT_PLUGIN: AtomicU64 = AtomicU64::new(1);

pub fn next_plugin() -> u64 {
    NEXT_PLUGIN.fetch_add(1, Ordering::AcqRel)
}

/// How many keys can sound at once. Fixed, so `process` never allocates.
const VOICES: usize = 16;

/// One key that is down.
#[derive(Copy, Clone)]
struct Voice {
    key: u8,
    amplitude: f32,
    /// Radians per frame.
    step: f32,
    phase: f32,
}

/// The sound of a test plugin: one cosine per key that is down, the pedal on the right, and a
/// transpose the pedal sets.
pub struct Tone {
    voices: [Option<Voice>; VOICES],
    sample_rate: f32,
    pedal: u8,
    semitones: i32,
}

impl Tone {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [None; VOICES],
            sample_rate,
            pedal: 0,
            semitones: 0,
        }
    }

    pub fn semitones(&self) -> i32 {
        self.semitones
    }

    pub fn set_semitones(&mut self, semitones: i32) {
        self.semitones = semitones;
    }

    /// Everything a stop or a reload resets.
    pub fn reset(&mut self) {
        self.voices = [None; VOICES];
        self.pedal = 0;
    }

    pub fn note_on(&mut self, key: u8, velocity: f32) {
        let sounding = i32::from(key).saturating_add(self.semitones).clamp(0, 127);
        let frequency = 440.0 * ((sounding as f32 - 69.0) / 12.0).exp2();
        let voice = Voice {
            key,
            amplitude: velocity,
            step: std::f32::consts::TAU * frequency / self.sample_rate,
            phase: 0.0,
        };
        // A key that is already down keeps sounding: a new voice takes the first free place,
        // and a full list drops the note. Neither allocates.
        if let Some(free) = self.voices.iter_mut().find(|voice| voice.is_none()) {
            *free = Some(voice);
        }
    }

    /// `None` for the key ends every voice, which is what a note off that matches every key is.
    pub fn note_off(&mut self, key: Option<u8>) {
        for slot in &mut self.voices {
            let matches = slot.is_some_and(|voice| key.is_none_or(|key| voice.key == key));
            if matches {
                *slot = None;
            }
        }
    }

    /// The sustain pedal, 0 to 127. `true` says the transpose changed, which is what makes the
    /// plugin's own state change without a window.
    pub fn pedal(&mut self, value: u8) -> bool {
        self.pedal = value.min(127);
        // Down: the transpose follows the value, so a test changes the plugin's own state
        // through the note contract. Up: leave it, else every stop would transpose.
        if self.pedal < 64 {
            return false;
        }
        let semitones = i32::from(self.pedal) - 64;
        let changed = self.semitones != semitones;
        self.semitones = semitones;
        changed
    }

    /// The left channel is the sum of the voices, the right one is the pedal as a number.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let pedal = f32::from(self.pedal) / 127.0;
        right.fill(pedal);
        left.fill(0.0);
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
}

/// How loud a plugin plays when nothing has edited it, as hundredths. It is the level the
/// `Level` parameter of the VST 3 plugin starts at, and what the CLAP one always plays at.
pub const FULL_EDIT_LEVEL: i32 = 100;

/// The saved state of a test plugin: a magic number, the transpose, and the level a parameter
/// edit left the plugin on. The level is in the state because a host that carried an edit to
/// the processor has to save what the processor now holds.
pub fn save_state(semitones: i32, edit_level: i32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&STATE_MAGIC);
    bytes.extend_from_slice(&semitones.to_le_bytes());
    bytes.extend_from_slice(&edit_level.to_le_bytes());
    bytes
}

/// The transpose and the level in a saved state. `None` says the bytes are not one of ours.
pub fn load_state(bytes: &[u8]) -> Option<(i32, i32)> {
    let rest = bytes.strip_prefix(&STATE_MAGIC)?;
    let semitones: [u8; 4] = rest.get(..4)?.try_into().ok()?;
    let level: [u8; 4] = match rest.get(4..8) {
        Some(four) => four.try_into().ok()?,
        // A state the CLAP plugin wrote, which keeps no level.
        None => FULL_EDIT_LEVEL.to_le_bytes(),
    };
    Some((i32::from_le_bytes(semitones), i32::from_le_bytes(level)))
}

/// Where `cargo` put a test plugin's dynamic library, building it first.
///
/// `cargo test` builds a crate for testing but not its dynamic library, so this builds it
/// first. That is a few tenths of a second when it is already up to date, and it means a test
/// always loads the plugin as the source says it is.
pub fn built_library(package: &str) -> PathBuf {
    let name = format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        package.replace('-', "_"),
        std::env::consts::DLL_SUFFIX
    );
    // Once per package per test process: a test folder is made several times in one process.
    static BUILT: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> =
        std::sync::Mutex::new(None);
    let mut built = BUILT.lock().expect("nothing panics while this is held");
    if built
        .get_or_insert_with(Default::default)
        .insert(package.to_string())
    {
        let output = std::process::Command::new(env!("CARGO"))
            .args(["build", "--package", package])
            .output()
            .expect("cargo builds the test plugin");
        assert!(
            output.status.success(),
            "cargo build -p {package} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    drop(built);
    let mut folder = std::env::current_exe().expect("the path of the test executable");
    while folder.pop() {
        let candidate = folder.join(&name);
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("{name} is not built. Run `cargo build -p {package}` first");
}

/// Copies a built library into `folder` as a macOS bundle of `extension`, and gives back the
/// bundle. CLAP takes a plain file; VST 3 wants a real bundle with its `Info.plist`, which is
/// what `CFBundle` needs to find the binary.
pub fn install_bundle(folder: &Path, library: &Path, name: &str, extension: &str) -> PathBuf {
    let bundle = folder.join(format!("{name}.{extension}"));
    if extension == "clap" {
        std::fs::create_dir_all(folder).expect("the plugin folder");
        std::fs::copy(library, &bundle).expect("a copy of the test plugin");
        return bundle;
    }
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).expect("the bundle folder");
    std::fs::copy(library, contents.join("MacOS").join(name)).expect("a copy of the test plugin");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>{name}</string>
  <key>CFBundleIdentifier</key><string>com.sound-tools.{name}</string>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundlePackageType</key><string>BNDL</string>
  <key>CFBundleSignature</key><string>????</string>
  <key>CFBundleVersion</key><string>1.0</string>
</dict>
</plist>
"#
    );
    std::fs::write(contents.join("Info.plist"), plist).expect("the bundle Info.plist");
    bundle
}
