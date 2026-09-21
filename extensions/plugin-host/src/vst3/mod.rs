//! The VST 3 backend: what this machine has, loading one plugin, and playing it.
//!
//! VST 3 is a COM API. The `vst3` crate gives the raw interfaces generated from Steinberg's
//! headers and nothing else, so the safe layer is here. Every call into a plugin is `unsafe`
//! and every one of them is in this folder.
//!
//! What a plugin is made of, and which thread each part belongs to:
//!
//! - `IComponent` is the plugin. It is created from the factory, initialized with a host
//!   context, and it owns the buses and the state. Main thread.
//! - `IAudioProcessor` is the same object asked for another interface. `setProcessing` and
//!   `process` belong to the thread that processes; `setupProcessing` and `setActive` belong to
//!   the main thread while nothing is processing. Same rule as CLAP, so the host that step 4a
//!   built holds for both.
//! - `IEditController` is the interface side. It may be the same object or a second one, and
//!   the two are joined by `IConnectionPoint`. It is what knows the MIDI mapping, which is how
//!   the sustain pedal reaches a VST 3 plugin, and in step 5b it is what makes the window.
//!
//! VST 3 has no MIDI controller event. The format's own answer is `IMidiMapping`: the
//! controller asks which parameter a MIDI controller number is mapped to, and the host sends
//! that parameter as a normalized value in the block's parameter changes. See `plugin.rs`.

mod context;
mod module;
mod plugin;
mod process;
mod stream;

use std::path::{Path, PathBuf};

use vst3::Steinberg::TUID;

pub use plugin::load;

use crate::scan::ScannedPlugin;
use crate::{PluginFormat, PluginProblem};

/// The folders macOS keeps VST 3 plugins in, plus `VST3_PATH` from the environment. The list
/// and the variable are Steinberg's, from the VST 3 specification.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"));
    }
    paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
    paths.push(PathBuf::from("/Network/Library/Audio/Plug-Ins/VST3"));
    if let Some(extra) = std::env::var_os("VST3_PATH") {
        paths.extend(std::env::split_paths(&extra));
    }
    paths
}

/// Lists one bundle. This runs in a child process: loading a bundle runs the plugin's own code.
pub fn scan_bundle(bundle: &Path) -> Result<Vec<ScannedPlugin>, String> {
    let module = module::Module::load(bundle)?;
    Ok(module
        .classes()
        .into_iter()
        .map(|class| ScannedPlugin {
            format: PluginFormat::Vst3,
            id: class_id_text(&class.id),
            name: class.name,
            vendor: class.vendor,
            version: class.version,
            features: class.subcategories,
            path: PathBuf::new(),
        })
        .collect())
}

/// The plugin's class id as a record holds it: the sixteen bytes as thirty-two uppercase hex
/// digits.
///
/// This is what Steinberg's own `FUID::toString` gives on macOS and Linux, and what a
/// `.vstpreset` file holds, so the id in a record is the one a plugin's maker publishes. A
/// class id never changes, which is what makes it the name of a plugin for good.
pub fn class_id_text(id: &TUID) -> String {
    let mut text = String::with_capacity(32);
    for byte in id {
        use std::fmt::Write as _;
        // The digits cannot fail to be written into a string.
        let _ = write!(text, "{:02X}", *byte as u8);
    }
    text
}

/// The sixteen bytes of a class id, from what a record holds. `None` when the text is not
/// thirty-two hex digits.
pub fn class_id_of(text: &str) -> Option<TUID> {
    if text.len() != 32 {
        return None;
    }
    let mut id = [0_i8; 16];
    for (byte, digits) in id.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let digits = std::str::from_utf8(digits).ok()?;
        *byte = u8::from_str_radix(digits, 16).ok()? as i8;
    }
    Some(id)
}

/// The biggest state this host reads or writes, and the most a plugin may put in one stream.
///
/// One number for both sides: a state that would not be read back is not written over the one
/// that is there, and a file that is not ours cannot make this process allocate more than this.
/// Real plugin states are kilobytes to a few megabytes; a sampler that keeps its samples in its
/// state is the only thing that comes near, and half a gigabyte is past any of them. It is also
/// what keeps the lengths in a state asset inside the four bytes they are written in.
pub const MAX_STATE: usize = 512 * 1024 * 1024;

/// A call into a plugin that answered with a failure code.
fn refused(plugin_id: &str, call: &str, result: i32) -> PluginProblem {
    PluginProblem::DidNotLoad {
        plugin_id: plugin_id.to_string(),
        message: format!("the plugin answered {result} to {call}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_id_survives_the_way_to_a_record_and_back() {
        let id: TUID = [
            0x6E, 0x33, 0x22, 0x52, 0x54, 0x22, 0x4A, 0x00, -0x56, 0x69, 0x30, 0x1A, -0x0D, 0x18,
            0x79, 0x7D,
        ];
        let text = class_id_text(&id);
        assert_eq!(text, "6E33225254224A00AA69301AF318797D");
        assert_eq!(class_id_of(&text), Some(id));
    }

    #[test]
    fn a_plugin_id_that_is_not_a_class_id_is_refused_instead_of_guessed() {
        assert_eq!(class_id_of("com.example.piano"), None);
        assert_eq!(class_id_of(""), None);
        assert_eq!(class_id_of(&"Z".repeat(32)), None);
    }
}
