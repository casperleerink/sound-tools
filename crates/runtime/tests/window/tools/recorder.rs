//! A `recorder` of a tool of the project, with a simulated input and keyboard: armed from the
//! tool's view, it opens the input before the song plays; while the song plays it records the
//! input and the notes of the keyboard, and when it is turned off what it heard becomes a
//! `region` and a `timeline` next to it, where it was heard, in one undo step.

use gpui::{Modifiers, TestAppContext};
use midi::Played;
use plugin_host::{Plugins, ScanCache};
use runtime::window::DeviceAccess;
use serde_json::Value;
use sound_core::InstanceId;
use sound_notes::{Amount, Bend};
use std::sync::atomic::Ordering;

use super::{Opened, write};
use crate::plugin_hosts::scanner;
use crate::song::{HEADER, song_files};
use crate::support::SimulatedInput;
use crate::support::open_folder_with;

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
      h("div", { key: "arm-2", onPress: () => send({ child: "rec-2", number: "record" }, 1) }, "Arm 2"),
      h("div", { key: "done-2", onPress: () => send({ child: "rec-2", number: "record" }, 0) }, "Done 2"),
    ),
  onTake: ({ update }, take) =>
    update("Record", ({ children }) => {
      children.push({ name: take.name, tool: take.tool, state: take.state });
    }),
});
"#;

/// A steady tone on the input: the left at 0.5, the right at 0.25.
fn tone(_: u64) -> [f32; 2] {
    [0.5, 0.25]
}

fn open_studio(cx: &mut TestAppContext) -> (Opened<'_>, SimulatedInput) {
    open_studio_of(cx, &[])
}

/// The studio with more recorders: `rec-2` and on, of the right channel.
fn open_studio_of<'a>(cx: &'a mut TestAppContext, more: &[&str]) -> (Opened<'a>, SimulatedInput) {
    let folder = tempfile::tempdir().unwrap();
    for name in more {
        write(
            folder.path(),
            &format!("state/studio/{name}.json"),
            r#"{"tool": "recorder", "state": {"channels": [1]}}"#,
        );
    }
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
    let plugins = |_: &std::path::Path| Plugins::new(Vec::new(), scanner(), ScanCache::none());
    (open_folder_with(cx, folder, plugins, device), input)
}

impl Opened<'_> {
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
            self.settle();
        }
    }
}

#[gpui::test]
fn a_recorder_records_the_input_and_the_keyboard_where_they_were_heard(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
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
    window.after_the_runs();
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

/// The song of the examples records its voice: R arms the audio track, which opens its input,
/// the record button records from the playhead, and a press again ends the take: a region of what the input
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
    let plugins = |_: &std::path::Path| Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let mut window = open_folder_with(cx, folder, plugins, device);
    window.wait_for_bun();
    // The R of the voice, the fifth row: after its name, M and S in the header.
    let row = window.on_lane(0, 4);
    let arm = gpui::point(row.x - gpui::px(HEADER) + gpui::px(132.), row.y);
    window.press(arm);
    window.release(arm);
    // Armed, its input opens before it records.
    window.until("the input opens", |window| {
        window.settle();
        input.openings.load(Ordering::Relaxed) == 1
    });
    let record = window.until_found("div-record");
    window.cx.simulate_click(record, Modifiers::default());
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

/// A track's recorder of the audio input takes a region, which a track does not keep: that is a
/// problem that says what to do, not a take that goes nowhere.
#[gpui::test]
fn a_take_of_sound_on_a_track_is_a_problem(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(1, 1) {
        write(folder.path(), &path, &text);
    }
    write(
        folder.path(),
        "state/song/t00/rec.json",
        r#"{"tool": "recorder", "state": {"channels": [0]}}"#,
    );
    let input = SimulatedInput::default();
    let device = DeviceAccess {
        open_input: Some(input.opener(cx.executor())),
        ..DeviceAccess::default()
    };
    let plugins = |_: &std::path::Path| Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let mut window = open_folder_with(cx, folder, plugins, device);
    window.wait_for_bun();
    // The R of the track, on the first row.
    let row = window.on_lane(0, 0);
    let arm = gpui::point(row.x - gpui::px(HEADER) + gpui::px(132.), row.y);
    window.press(arm);
    window.release(arm);
    let record = window.until_found("div-record");
    window.cx.simulate_click(record, Modifiers::default());
    window.record_for(&input, 48_000);
    window.cx.simulate_click(record, Modifiers::default());
    window.until("the take refused", |window| {
        window.record_for(&input, 2_048);
        !window.problems().is_empty()
    });
    let problems = window.problems();
    assert!(problems[0].message.contains("record sound on an audio track"), "{problems:?}");
}

impl Opened<'_> {
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

/// An input that cannot open ends the take that waited for it: its notes are kept, and nothing
/// waits for ever.
#[gpui::test]
fn a_take_whose_input_cannot_open_keeps_its_notes(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
    input.fails.store(true, Ordering::Relaxed);
    let arm = window.until_found("div-arm");
    window.cx.simulate_click(arm, Modifiers::default());
    window.start_playing();
    window.record_for(&input, 24_000);
    window.play_midi(60);
    window.record_for(&input, 24_000);
    let done = window.until_found("div-done");
    window.cx.simulate_click(done, Modifiers::default());
    window.until("the notes kept", |window| {
        window.record_for(&input, 2_048);
        !window.made("timeline").is_empty()
    });
    assert!(window.made("region").is_empty());
}

/// A recorder deleted while it takes ends its take there: its notes are kept, and the input it
/// held is let go.
#[gpui::test]
fn a_recorder_deleted_while_it_takes_lets_go_of_the_input(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
    let arm = window.until_found("div-arm");
    window.cx.simulate_click(arm, Modifiers::default());
    window.until("the input opens", |_| {
        input.openings.load(Ordering::Relaxed) == 1
    });
    window.start_playing();
    window.record_for(&input, 24_000);
    window.play_midi(60);
    window.record_for(&input, 24_000);
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = sound_core::Changes::new();
                changes.delete(&InstanceId::new("studio/rec").unwrap());
                project.commit("Delete the recorder", changes)
            })
        })
    });
    window.until("the notes kept and the input let go", |window| {
        window.record_for(&input, 2_048);
        !window.made("timeline").is_empty() && input.closings() == 1
    });
}

impl Opened<'_> {
    fn press_found(&mut self, selector: &str) {
        let at = self.until_found(selector);
        self.cx.simulate_click(at, Modifiers::default());
    }
}

/// A second recorder that starts while the first records gets a take of its own, of the sound
/// from where it started.
#[gpui::test]
fn a_recorder_that_starts_while_another_records_keeps_its_own_take(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio_of(cx, &["rec-2"]);
    window.press_found("div-arm");
    window.start_playing();
    window.record_for(&input, 24_000);
    window.press_found("div-arm-2");
    window.record_for(&input, 24_000);
    window.press_found("div-done");
    window.press_found("div-done-2");
    window.until("two regions", |window| {
        window.record_for(&input, 2_048);
        window.made("region").len() == 2
    });
    let regions = window.made("region");
    let start = |name: &str| {
        let (_, region) = regions
            .iter()
            .find(|(each, _)| each.starts_with(name))
            .unwrap();
        region["start"].as_u64().unwrap()
    };
    // Half a second later at 120 bpm is a beat later.
    assert!(
        start("rec-2-sound") >= start("rec-sound") + 900,
        "{regions:?}"
    );
    window.until("the input let go", |window| {
        window.record_for(&input, 2_048);
        input.closings() == 1
    });
}

/// At a loop, a take ends and the next starts: one region for each round, and the input is let
/// go once recording stops, even when it stops before the round before was written.
#[gpui::test]
fn a_take_across_a_loop_is_a_region_for_each_round(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, _| {
            session
                .engine()
                .set_loop(Some(sound_core::Ticks(0)..sound_core::Ticks(1_920)))
        })
    });
    window.press_found("div-arm");
    window.start_playing();
    // One round is a second; stop just after the loop.
    window.record_for(&input, 50_000);
    window.press_found("div-done");
    window.until("a region for each round", |window| {
        window.record_for(&input, 2_048);
        window.made("region").len() == 2
    });
    window.until("the input let go", |window| {
        window.record_for(&input, 2_048);
        input.closings() == 1
    });
}

/// What a recorder heard just before it was deleted, and no poll read yet, is in its take.
#[gpui::test]
fn a_recorder_deleted_keeps_what_it_heard_since_the_last_poll(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
    window.press_found("div-arm");
    window.start_playing();
    window.record_for(&input, 24_000);
    window.play_midi(64);
    window.loudest(2_048);
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = sound_core::Changes::new();
                changes.delete(&InstanceId::new("studio/rec").unwrap());
                project.commit("Delete the recorder", changes)
            })
        })
    });
    window.until("the notes kept", |window| {
        window.record_for(&input, 2_048);
        !window.made("timeline").is_empty()
    });
    let (_, timeline) = &window.made("timeline")[0];
    assert_eq!(timeline["notes"][0]["pitch"], 64, "{timeline}");
}

/// A take keeps how the keys were played, not only which: the bend and the mod wheel.
#[gpui::test]
fn a_take_keeps_the_bend_and_the_mod_wheel(cx: &mut TestAppContext) {
    let (mut window, input) = open_studio(cx);
    window.press_found("div-arm");
    window.start_playing();
    window.record_for(&input, 24_000);
    window.play_midi(60);
    let shell = window.shell.clone();
    let live = window.cx.read(|cx| shell.read(cx).live_input().clone());
    let midi = window.cx.read(|cx| live.read(cx).midi_input()).unwrap();
    assert!(midi.send(Played::Bend(Bend::new(4_000).unwrap())));
    assert!(midi.send(Played::ModWheel(Amount::new(64).unwrap())));
    window.record_for(&input, 24_000);
    window.press_found("div-done");
    window.until("the notes kept", |window| {
        window.record_for(&input, 2_048);
        !window.made("timeline").is_empty()
    });
    let (_, timeline) = &window.made("timeline")[0];
    assert!(
        !timeline["bend"].as_array().unwrap().is_empty(),
        "{timeline}"
    );
    assert!(
        !timeline["mod_wheel"].as_array().unwrap().is_empty(),
        "{timeline}"
    );
}
