//! `workspace.json`: where the plugin windows of a project were and whether they were open.
//!
//! ARCHITECTURE.md keeps the layout of the workspace apart from the piece, in `workspace.json`
//! at the root of the project. It is no record: the watcher does not read it, a change of it is
//! never an edit and never an undo step, and an agent has no reason to touch it. This host owns
//! one key of it, `plugin_windows`, and leaves every other key as it finds it, so that another
//! part of the application can keep its own layout in the same file later.
//!
//! ```json
//! {
//!   "plugin_windows": {
//!     "arrangement/piano/instrument": {"open": true, "x": 120, "y": 80, "display": 1}
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};
use sound_core::InstanceId;

use crate::window::Placement;

pub(crate) const FILE: &str = "workspace.json";
const KEY: &str = "plugin_windows";

/// The window of every plugin record that had one, by the record's id.
pub(crate) type Placements = BTreeMap<InstanceId, Placement>;

/// What the project folder at `root` remembers. A project with no file remembers nothing.
pub(crate) fn read(root: &Path) -> Result<Placements, String> {
    let Some(mut whole) = read_object(root)? else {
        return Ok(Placements::new());
    };
    let Some(ours) = whole.remove(KEY) else {
        return Ok(Placements::new());
    };
    serde_json::from_value(ours).map_err(|error| format!("{FILE}: {KEY}: {error}"))
}

/// Keeps `placements` in the project folder at `root`, and every other key of the file as it
/// was. Nothing is written when the text would not change, so a session that moved no window
/// leaves no diff, and no file is made for a project that never had a plugin window.
pub(crate) fn write(root: &Path, placements: &Placements) -> Result<(), String> {
    let path = root.join(FILE);
    let before = read_object(root)?;
    let mut whole = before.clone().unwrap_or_default();
    if placements.is_empty() {
        whole.remove(KEY);
    } else {
        let ours = serde_json::to_value(placements).map_err(|error| error.to_string())?;
        whole.insert(KEY.to_string(), ours);
    }
    if Some(&whole) == before.as_ref() || (before.is_none() && whole.is_empty()) {
        return Ok(());
    }
    let mut text = serde_json::to_string_pretty(&whole).map_err(|error| error.to_string())?;
    text.push('\n');
    crate::scan::write_whole(&path, text.as_bytes())
}

/// The file as a JSON object, or `None` when there is no file. A file that is there and holds
/// anything else is an error, and it is left as it is: it is somebody's, and this host does not
/// write over what it cannot read.
fn read_object(root: &Path) -> Result<Option<Map<String, Value>>, String> {
    let path = root.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{FILE}: {error}")),
    };
    match serde_json::from_str(&text) {
        Ok(Value::Object(whole)) => Ok(Some(whole)),
        Ok(_) => Err(format!(
            "{FILE} is not a JSON object, so it is left as it is"
        )),
        Err(error) => Err(format!("{FILE}: {error}. It is left as it is")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(open: bool, x: i32) -> Placement {
        Placement {
            open,
            x,
            y: 40,
            display: None,
        }
    }

    fn id(text: &str) -> InstanceId {
        InstanceId::new(text).expect("an id")
    }

    #[test]
    fn what_was_written_reads_back_and_other_keys_stay() {
        let folder = tempfile::tempdir().expect("a folder");
        std::fs::write(folder.path().join(FILE), r#"{"click": true}"#).expect("a file");
        let mut placements = Placements::new();
        placements.insert(id("arrangement/piano/instrument"), placement(true, 120));
        placements.insert(id("arrangement/bass/space"), placement(false, 300));
        write(folder.path(), &placements).expect("it writes");
        assert_eq!(read(folder.path()).expect("it reads"), placements);
        let text = std::fs::read_to_string(folder.path().join(FILE)).expect("the file");
        assert!(text.contains(r#""click": true"#), "{text}");
    }

    #[test]
    fn nothing_is_written_when_nothing_changed_or_there_is_nothing_to_keep() {
        let folder = tempfile::tempdir().expect("a folder");
        write(folder.path(), &Placements::new()).expect("it writes nothing");
        assert!(!folder.path().join(FILE).exists());

        let mut placements = Placements::new();
        placements.insert(id("track/instrument"), placement(true, 1));
        write(folder.path(), &placements).expect("it writes");
        let path = folder.path().join(FILE);
        let written = std::fs::metadata(&path).and_then(|file| file.modified());
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(folder.path(), &placements).expect("it writes nothing");
        assert_eq!(
            std::fs::metadata(&path)
                .and_then(|file| file.modified())
                .ok(),
            written.ok()
        );
    }

    #[test]
    fn a_file_that_is_not_an_object_is_reported_and_left_alone() {
        let folder = tempfile::tempdir().expect("a folder");
        let path = folder.path().join(FILE);
        std::fs::write(&path, "[1, 2").expect("a file");
        assert!(read(folder.path()).is_err());
        let mut placements = Placements::new();
        placements.insert(id("track/instrument"), placement(true, 1));
        assert!(write(folder.path(), &placements).is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("the file"), "[1, 2");
    }
}
