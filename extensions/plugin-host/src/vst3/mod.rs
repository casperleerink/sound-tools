//! The VST 3 backend: what this machine has, loading one plugin, and playing it.
//!
//! The `vst3` crate gives only the raw COM interfaces, so every `unsafe` call into a plugin is
//! in this folder. `kResultTrue` is the same number as `kResultOk`, so success is checked
//! against `kResultOk` alone.
//!
//! - `IComponent` is the plugin: it owns the buses and the state. Main thread.
//! - `IAudioProcessor` is the same object. `setProcessing` and `process` belong to the thread
//!   that processes; `setupProcessing` and `setActive` to the main thread while nothing
//!   processes. Same rule as CLAP.
//! - `IEditController` is the interface side, the same object or a second one joined by
//!   `IConnectionPoint`. It makes the window (`IPlugView`). Main thread.
//!
//! VST 3 has no MIDI controller event. The format's answer is `IMidiMapping`: the controller
//! says which parameter a controller number is mapped to, and the host sends that parameter's
//! normalized value in the block's parameter changes. That is how the pedal and the wheels
//! reach a VST 3 plugin.

mod context;
mod module;
mod parameters;
mod plugin;
mod process;
mod stream;
mod view;

use std::path::{Path, PathBuf};

use vst3::Steinberg::TUID;

pub(crate) use plugin::{load, read_parameters};

use crate::scan::ScannedPlugin;
use crate::{PluginFormat, PluginProblem};

/// Lists one bundle. This runs in a child process: loading a bundle runs the plugin's own code.
pub(crate) fn scan_bundle(bundle: &Path) -> Result<Vec<ScannedPlugin>, String> {
    let module = module::Module::load(bundle)?;
    Ok(module
        .classes()
        .into_iter()
        .map(|class| ScannedPlugin {
            format: PluginFormat::Vst3,
            id: class_id_text(&class.id),
            name: class.name,
            vendor: class.vendor,
            features: class.subcategories,
            path: PathBuf::new(),
        })
        .collect())
}

/// The class id as thirty-two uppercase hex digits, as Steinberg's `FUID::toString` writes it,
/// so a record holds the id the plugin's maker publishes, the same on every platform.
pub(crate) fn class_id_text(id: &TUID) -> String {
    in_text_order(*id)
        .iter()
        .map(|byte| format!("{:02X}", byte.to_ne_bytes()[0]))
        .collect()
}

/// The sixteen bytes of a class id, from what a record holds. `None` when the text is not
/// thirty-two hex digits.
pub(crate) fn class_id_of(text: &str) -> Option<TUID> {
    if text.len() != 32 {
        return None;
    }
    // `TUID` holds C chars, which are signed on macOS and Windows and unsigned on Linux on
    // arm64.
    let mut id: TUID = [0; 16];
    for (byte, digits) in id.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let digits = std::str::from_utf8(digits).ok()?;
        *byte = u8::from_str_radix(digits, 16).ok()? as std::ffi::c_char;
    }
    Some(in_text_order(id))
}

/// A class id with its bytes in text order, or back: the swaps undo themselves. On Windows a
/// class id is a COM `GUID`, whose first three fields are little-endian numbers.
fn in_text_order(mut id: TUID) -> TUID {
    if cfg!(target_os = "windows") {
        id[..4].reverse();
        id[4..6].reverse();
        id[6..8].reverse();
    }
    id
}

/// The biggest state this host reads or writes, and the most a plugin may put in one stream.
/// One number for both, so a state that could not be read back is never written, and a file
/// that is not ours cannot make this process allocate more. Far above any real state, and it
/// keeps a state asset's lengths inside their four bytes.
pub(crate) const MAX_STATE: usize = 512 * 1024 * 1024;

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
        // Made the way a plugin declares its id, which lays it out as a `GUID` on Windows.
        let id: TUID = vst3::uid(0x6E332252, 0x54224A00, 0xAA69301A, 0xF318797D);
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
