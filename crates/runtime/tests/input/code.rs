//! The code of a project as the window runs it: what it hears of the folder, and what happens
//! when it hangs.

use std::time::Duration;

use crate::support::{open_watched, write};

/// A sine at the level `extensions/<module>.ts` exports.
fn hum(module: &str) -> String {
    format!(
        r#"import {{ sine, tool }} from "./sdk";
import {{ LEVEL }} from "./{module}";
export const hum = tool({{ name: "hum", title: "Hum", when: "Never", doc: "A sine.", state: {{}}, sound: () => sine(441).times(LEVEL) }});
"#
    )
}

fn level(value: f32) -> String {
    format!("export const LEVEL = {value};\n")
}

const ONLY_HUM: &str = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "hum", "port": "audio"}, "to": {"device_output": 0}}]}"#;

/// The one test of the system's watcher on `extensions/`: the others tell the host of their
/// saves. An agent makes a folder for new code after the project opened, and saves in it again.
#[test]
fn a_save_in_a_folder_made_after_the_project_opened_is_heard() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().to_path_buf();
    write(&root, "project.json", ONLY_HUM);
    write(&root, "extensions/theme.json", "{}");
    write(&root, "extensions/lib/level.ts", &level(0.5));
    write(&root, "extensions/hum.ts", &hum("lib/level"));
    write(&root, "state/hum.json", r#"{"tool": "hum", "state": {}}"#);
    let mut opened = open_watched(folder);
    // The watcher needs a moment before it sees changes.
    std::thread::sleep(Duration::from_millis(200));
    let at = |level: f32| move |opened: &mut crate::support::Opened| {
        (opened.loudest(4_800) - level).abs() < 0.02
    };
    opened.until("the hum at its level", at(0.5));

    write(&root, "extensions/lib/level.ts", &level(0.25));
    opened.until("a save in a folder there at the start", at(0.25));

    write(&root, "extensions/drums/level.ts", &level(0.125));
    write(&root, "extensions/hum.ts", &hum("drums/level"));
    opened.until("the code of the new folder", at(0.125));
    write(&root, "extensions/drums/level.ts", &level(0.0625));
    opened.until("a save in the new folder", at(0.0625));
}
