//! Files dropped from the system on what has `onDrop`, as a composer and an agent drop them.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::host::wav;
use super::{act, control, laid_out, opened, redraw, try_act, with_tools};
use crate::app::State;
use crate::controls;

/// A pad that loads the sound dropped on it, a lane that writes down what it hears in the units
/// of its plane, and a box that takes no drop.
const PADS: &str = r#"import { h, input, text, tool, type Dropped } from "./sdk";
const said = (d: Dropped) =>
  `${d.x} ${d.y} | ${d.audio.join(",")} | ${d.parts.map((p) => `${p.name}:${p.notes.map((n) => n.pitch).join("/")}`).join(",")}`;
export const pads = tool({
  name: "pads", title: "Pads", when: "Never", doc: "A pad and a lane.", sound: () => input,
  state: { sample: text({ default: "" }), heard: text({ default: "" }) },
  view: (self, { update }) => (
    <col w={400}>
      <box key="pad" label="Pad" w={80} h={80} fill="fill" hover="raised"
        onDrop={(d) => update("Load sample", (draft) => void (draft.state.sample = d.audio[0] ?? ""))} />
      <box key="lane" label="Lane" w={400} h={100} plane={{ x: 0.1, y: 1 }} hover="raised"
        onDrop={(d) => update("Drop", (draft) => void (draft.state.heard = said(d)))} />
      <box key="plain" label="Plain" w={80} h={40} />
    </col>
  ),
});
"#;

const PREFIX: &str = "t00/chain/pads:";

fn pads() -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[("pads", PADS)]);
    (scratch, laid_out(opened(&folder)))
}

/// What the pads wrote down as `field`.
fn wrote(state: &State, field: &str) -> String {
    let id = InstanceId::new("song/t00/chain/pads").unwrap();
    let record: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    record[field].as_str().unwrap_or_default().to_string()
}

fn centre(window: &rune::Runtime<State>, key: &str) -> (f32, f32) {
    let ([x, y, w, h], _) = window.presented(&format!("{PREFIX}{key}")).unwrap();
    (x + w / 2.0, y + h / 2.0)
}

fn drop_on(window: &mut rune::Runtime<State>, files: &[&Path], at: (f32, f32)) {
    let files = files.iter().map(|file| file.to_path_buf()).collect();
    window.drop_files(files, Some(at));
    window.frame(1.0 / 60.0);
    redraw(window);
}

/// A MIDI file of a track for each of `parts`, named and playing one note of that pitch.
pub(super) fn midi(path: &Path, parts: &[(&str, u8)]) {
    let track = |name: &[u8], pitch: u8| {
        let mut events = vec![0x00, 0xFF, 0x03, name.len() as u8];
        events.extend(name);
        events.extend([0x00, 0x90, pitch, 0x64, 0x83, 0x60, 0x80, pitch, 0x00]);
        events.extend([0x00, 0xFF, 0x2F, 0x00]);
        let mut bytes = b"MTrk".to_vec();
        bytes.extend((events.len() as u32).to_be_bytes());
        bytes.extend(events);
        bytes
    };
    let mut bytes = b"MThd".to_vec();
    bytes.extend(6_u32.to_be_bytes());
    bytes.extend([0, 1, 0, parts.len() as u8, 0x01, 0xE0]);
    for (name, pitch) in parts {
        bytes.extend(track(name.as_bytes(), *pitch));
    }
    std::fs::write(path, bytes).unwrap();
}

fn outside() -> (tempfile::TempDir, impl Fn(&str) -> PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    (dir, move |name: &str| root.join(name))
}

#[test]
fn a_sound_dropped_on_a_pad_is_copied_in_and_loaded_as_one_undo_step() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    wav(&file("Kick (1).WAV"));
    let at = centre(&window, "pad");
    drop_on(&mut window, &[&file("Kick (1).WAV")], at);
    assert_eq!(wrote(&window.state, "sample"), "kick-1.wav");
    let copied = window
        .state
        .project()
        .root()
        .join("assets/audio/kick-1.wav");
    assert!(copied.is_file());
    act(&mut window.state, json!({ "action": "undo" }));
    assert_eq!(wrote(&window.state, "sample"), "");
}

#[test]
fn a_drop_on_a_plane_hears_where_it_fell_in_units_and_every_file_in_order() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    wav(&file("b.wav"));
    wav(&file("a.wav"));
    let ([x, y, ..], _) = window.presented(&format!("{PREFIX}lane")).unwrap();
    drop_on(
        &mut window,
        &[&file("b.wav"), &file("a.wav")],
        (x + 50.0, y + 20.0),
    );
    assert_eq!(wrote(&window.state, "heard"), "500 20 | b.wav,a.wav | ");
}

#[test]
fn a_midi_file_comes_as_its_parts() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    midi(&file("tune.mid"), &[("Bass", 48), ("Lead", 60)]);
    let ([x, y, ..], _) = window.presented(&format!("{PREFIX}lane")).unwrap();
    drop_on(&mut window, &[&file("tune.mid")], (x, y));
    assert_eq!(wrote(&window.state, "heard"), "0 0 |  | Bass:48,Lead:60");
}

#[test]
fn a_file_that_does_not_play_is_a_notice_and_code_hears_nothing() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    std::fs::write(file("notes.txt"), "not a sound").unwrap();
    let at = centre(&window, "lane");
    drop_on(&mut window, &[&file("notes.txt")], at);
    let notice = window
        .state
        .session
        .notice()
        .unwrap_or_default()
        .to_string();
    assert!(notice.contains("notes.txt cannot be played"), "{notice}");
    assert_eq!(wrote(&window.state, "heard"), "");
    assert!(
        !window
            .state
            .project()
            .root()
            .join("assets/audio/notes.txt")
            .exists()
    );
}

#[test]
fn a_drop_on_what_takes_none_does_nothing() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    wav(&file("kick.wav"));
    let at = centre(&window, "plain");
    drop_on(&mut window, &[&file("kick.wav")], at);
    assert_eq!(wrote(&window.state, "sample"), "");
    assert!(window.state.session.notice().is_none());
    assert!(!window.state.project().root().join("assets/audio").exists());
}

#[test]
fn files_over_an_element_show_its_hover_look_until_they_leave() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    wav(&file("kick.wav"));
    let (pad, lane) = (format!("{PREFIX}pad"), format!("{PREFIX}lane"));
    let at = centre(&window, "pad");
    window.drag_files(vec![file("kick.wav")], Some(at));
    window.frame(1.0 / 60.0);
    assert_eq!(window.drop_hovered().as_deref(), Some(pad.as_str()));
    assert!(window.state.drops.over(&pad) && !window.state.drops.over(&lane));
    window.drag_files_to(Some(centre(&window, "lane")));
    window.frame(1.0 / 60.0);
    assert!(!window.state.drops.over(&pad) && window.state.drops.over(&lane));
    window.drag_files_leave();
    window.frame(1.0 / 60.0);
    assert!(!window.state.drops.over(&lane));
    assert_eq!(window.drop_hovered(), None);
}

#[test]
fn an_agent_drops_by_label_through_the_same_import() {
    let (_scratch, mut window) = pads();
    let (_dir, file) = outside();
    wav(&file("snare.wav"));
    let controls = controls::collect(&window.state).controls;
    assert!(
        control(&controls, "Pad")
            .actions
            .contains(&"drop".to_string())
    );
    let files = json!([file("snare.wav")]);
    act(
        &mut window.state,
        json!({ "label": "Lane", "action": "drop", "value": { "files": files, "x": 960 } }),
    );
    assert_eq!(wrote(&window.state, "heard"), "960 0 | snare.wav | ");
    let wrong = json!({ "label": "Pad", "action": "drop", "value": { "audio": ["x.wav"] } });
    assert!(try_act(&mut window.state, wrong).is_err());
}
