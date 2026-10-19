//! A `recorder` of a tool of the project, with a simulated input and keyboard: armed from the
//! tool's view, it opens the input before the song plays; while the song plays it records the
//! input and the notes of the keyboard, and when it is turned off what it heard becomes a
//! `region` and a `timeline` next to it, where it was heard, in one undo step.

use gpui::{Modifiers, TestAppContext};
use plugin_host::{Plugins, ScanCache};
use runtime::window::DeviceAccess;
use serde_json::Value;
use sound_core::InstanceId;
use std::sync::atomic::Ordering;

use super::{Window, open_with_device, write};
use crate::plugin_hosts::scanner;
use crate::song::{HEADER, song_files};
use crate::support::SimulatedInput;

const STUDIO: &str = r#"import { h, tool, type Declaration } from "./sdk";

tool({
  name: "studio",
  title: "Studio",
  when: "Never",
  doc: "Records the input and the keyboard; arm and stop on its view.",
  state: {},
  behaviour: ({ children }): Declaration => ({
    connections: [
      { from: { notes: "notes" }, to: { child: "rec", notes: "notes" } },
      ...children
        .filter((child) => child.tool === "region")
        .map((child) => ({ from: { child: child.name }, to: { port: "audio" as const } })),
    ],
  }),
  view: ({ send }) =>
    h("div", { style: { direction: "row", gap: 8 } },
      h("div", { key: "arm", onPress: () => send({ child: "rec", number: "record" }, 1) }, "Arm"),
      h("div", { key: "done", onPress: () => send({ child: "rec", number: "record" }, 0) }, "Done"),
    ),
});
"#;

/// A steady tone on the input: the left at 0.5, the right at 0.25.
fn tone(_: u64) -> [f32; 2] {
    [0.5, 0.25]
}

fn open_studio(cx: &mut TestAppContext) -> Option<(Window<'_>, SimulatedInput)> {
    let folder = tempfile::tempdir().unwrap();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "studio", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(folder.path(), "project.json", project);
    write(folder.path(), "extensions/studio.ts", STUDIO);
    write(
        folder.path(),
        "state/studio/instance.json",
        r#"{"tool": "studio", "state": {}}"#,
    );
    write(
        folder.path(),
        "state/studio/rec.json",
        r#"{"tool": "recorder", "state": {"channels": [0]}}"#,
    );
    let input = SimulatedInput::default();
    let device = DeviceAccess {
        open_input: Some(input.opener(cx.executor())),
        ..DeviceAccess::default()
    };
    let plugins = Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let window = open_with_device(cx, folder, plugins, device)?;
    Some((window, input))
}

impl Window<'_> {
    /// The records of the children of `studio` of `tool`, by name.
    fn made(&mut self, tool: &str) -> Vec<(String, Value)> {
        let session = self.session.clone();
        self.cx.read(|cx| {
            let project = session.read(cx).project();
            let studio = InstanceId::new("studio").unwrap();
            (project.instances())
                .filter(|(id, each)| id.parent().as_ref() == Some(&studio) && *each == tool)
                .filter_map(|(id, _)| {
                    let state = project.state_json(id)?;
                    Some((id.name().to_string(), serde_json::from_str(&state).ok()?))
                })
                .collect()
        })
    }

    /// The engine plays for about `frames`, the input brings what it captured meanwhile, and
    /// the window polls.
    fn record_for(&mut self, input: &SimulatedInput, frames: u64) {
        for _ in 0..frames.div_ceil(2_048) {
            self.loudest(2_048);
            input.write_until(self.engine.frames(), tone);
            self.poll();
        }
    }
}

#[gpui::test]
fn a_recorder_records_the_input_and_the_keyboard_where_they_were_heard(cx: &mut TestAppContext) {
    let Some((mut window, input)) = open_studio(cx) else {
        return;
    };
    // Armed from the view: the input opens before the song plays.
    let arm = window.until_found("div-arm");
    window.cx.simulate_click(arm, Modifiers::default());
    window.until("the input opens", |_| {
        input.openings.load(Ordering::Relaxed) == 1
    });
    window.start_playing();
    window.record_for(&input, 24_000);
    window.play_midi(60);
    window.record_for(&input, 24_000);
    let done = window.until_found("div-done");
    window.cx.simulate_click(done, Modifiers::default());
    window.until("a region and a timeline", |window| {
        window.record_for(&input, 2_048);
        !window.made("region").is_empty() && !window.made("timeline").is_empty()
    });

    let regions = window.made("region");
    let [(name, region)] = regions.as_slice() else {
        panic!("{regions:?}");
    };
    assert_eq!(name, "rec-sound-1");
    assert_eq!(region["asset"], "rec-sound-1.wav");
    // It plays from where the song was when the take began, the first poll after play.
    let start = region["start"].as_u64().unwrap();
    assert!(start < 960, "{start}");

    let timelines = window.made("timeline");
    let [(name, timeline)] = timelines.as_slice() else {
        panic!("{timelines:?}");
    };
    assert!(name.starts_with("rec-take-"), "{name}");
    let notes = timeline["notes"].as_array().unwrap();
    let [note] = notes.as_slice() else {
        panic!("{timeline}");
    };
    assert_eq!(note["pitch"], 60);
    // Held until the take ended: it ends inside the take, after it started.
    let note_start = timeline["start"].as_u64().unwrap() + note["start"].as_u64().unwrap();
    assert!((900..1_400).contains(&note_start), "{note_start}");

    // One undo step for both, which takes both away.
    assert_eq!(window.undo_label().as_deref(), Some("Record"));
    assert_eq!(window.undo().as_deref(), Some("Record"));
    assert!(window.made("region").is_empty() && window.made("timeline").is_empty());
    let session = window.session.clone();
    (window.cx).update(|_, cx| session.update(cx, |session, cx| session.redo(cx)));
    assert_eq!(window.problems(), []);
    // The region plays what was recorded, the left side of the tone, from the start.
    let session = window.session.clone();
    (window.cx).update(|_, cx| session.update(cx, |session, _| session.engine().stop()));
    window.start_playing();
    assert!(window.loudest(24_000) > 0.4);
}

/// The song of the examples records its voice: R arms the audio track, the record button
/// records from the playhead, and a press again ends the take: a region of what the input
/// captured lands on the voice's row where it was heard.
#[gpui::test]
fn the_song_records_its_voice_into_a_region_on_its_row(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(4, 8) {
        write(folder.path(), &path, &text);
    }
    let input = SimulatedInput::default();
    let device = DeviceAccess {
        open_input: Some(input.opener(cx.executor())),
        ..DeviceAccess::default()
    };
    let plugins = Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let Some(mut window) = open_with_device(cx, folder, plugins, device) else {
        return;
    };
    window.wait_for_bun();
    // The R of the voice, the fifth row: after its name, M and S in the header.
    let row = window.on_lane(0, 4);
    let arm = gpui::point(row.x - gpui::px(HEADER) + gpui::px(132.), row.y);
    window.press(arm);
    window.release(arm);
    window.wait_for_bun();
    let record = window.until_found("div-record");
    window.cx.simulate_click(record, Modifiers::default());
    window.until("the input opens", |_| {
        input.openings.load(Ordering::Relaxed) == 1
    });
    window.record_for(&input, 48_000);
    window.cx.simulate_click(record, Modifiers::default());
    window.until("a region of the voice", |window| {
        window.record_for(&input, 2_048);
        window.song_regions() > 0
    });
    let state = window.record("song/voice/rec-sound-1");
    assert_eq!(state["asset"], "rec-sound-1.wav");
    assert_eq!(window.problems(), []);
}

impl Window<'_> {
    fn song_regions(&mut self) -> usize {
        let session = self.session.clone();
        self.cx.read(|cx| {
            let project = session.read(cx).project();
            let voice = InstanceId::new("song/voice").unwrap();
            (project.instances())
                .filter(|(id, tool)| id.parent().as_ref() == Some(&voice) && *tool == "region")
                .count()
        })
    }
}
