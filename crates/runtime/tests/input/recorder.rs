//! A `recorder` of a tool of the project, with a simulated input and keyboard: armed from the
//! tool's view, it opens the input before the song plays; while the song plays it records the
//! input and the notes of the keyboard, and when it is turned off what it heard becomes a
//! `region` and a `timeline` next to it, where it was heard, in one undo step.

use std::sync::atomic::Ordering;

use midi::Played;
use serde_json::Value;
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::{Amount, Bend};

use crate::support::{Opened, SimulatedInput, id, open, write};

const STUDIO: &str = r#"import { h, tool, type Declaration } from "./sdk";

export const studio = tool({
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
  view: (_, { send }) => (
    <row>
      <box key="arm" label="Arm" onPress={() => send({ child: "rec", number: "record" }, 1)} />
      <box key="done" label="Done" onPress={() => send({ child: "rec", number: "record" }, 0)} />
      <box key="arm-2" label="Arm 2" onPress={() => send({ child: "rec-2", number: "record" }, 1)} />
      <box key="done-2" label="Done 2" onPress={() => send({ child: "rec-2", number: "record" }, 0)} />
    </row>
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

/// The studio with `rec`, of the left channel, and `rec-2` of the right when `two`.
fn open_studio(two: bool) -> (Opened, SimulatedInput) {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "studio", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/theme.json", "{}");
    write(root, "extensions/studio.tsx", STUDIO);
    write(root, "state/studio/instance.json", r#"{"tool": "studio", "state": {}}"#);
    let recorder = |channel: usize| {
        format!(r#"{{"tool": "recorder", "state": {{"channels": [{channel}]}}}}"#)
    };
    write(root, "state/studio/rec.json", &recorder(0));
    if two {
        write(root, "state/studio/rec-2.json", &recorder(1));
    }
    let input = SimulatedInput::default();
    (open(folder, input.devices()), input)
}

impl Opened {
    /// The records of the children of `studio` of `tool`, by name.
    fn made(&self, tool: &str) -> Vec<(String, Value)> {
        let studio = id("studio");
        (self.project.instances())
            .filter(|(id, each)| id.parent().as_ref() == Some(&studio) && *each == tool)
            .filter_map(|(id, _)| {
                let state = self.project.state_json(id)?;
                Some((id.name().to_string(), serde_json::from_str(&state).ok()?))
            })
            .collect()
    }

    /// The engine plays for about `frames`, the input brings what it captured meanwhile, and
    /// the window polls.
    fn record_for(&mut self, input: &SimulatedInput, frames: u64) {
        for _ in 0..frames.div_ceil(2_048) {
            self.render(2_048);
            input.write_until(self.engine.frames(), tone);
            self.poll();
        }
        self.settle();
    }

    fn delete(&mut self, instance: &str) {
        let mut changes = Changes::new();
        changes.delete(&id(instance));
        self.project.commit("Delete", changes).unwrap();
    }
}

#[test]
fn a_recorder_records_the_input_and_the_keyboard_where_they_were_heard() {
    let (mut studio, input) = open_studio(false);
    // Armed from the view: the input opens before the song plays.
    studio.press("studio", "arm");
    studio.until("the input opens", |_| input.openings.load(Ordering::Relaxed) == 1);
    // The thread that opened it woke the owner.
    assert!(studio.wakes.load(Ordering::Relaxed) > 0);
    studio.start_playing();
    studio.record_for(&input, 24_000);
    assert!(studio.input.is_recording());
    studio.play_midi(60);
    studio.record_for(&input, 24_000);
    studio.press("studio", "done");
    studio.until("a region and a timeline", |studio| {
        studio.record_for(&input, 2_048);
        !studio.made("region").is_empty() && !studio.made("timeline").is_empty()
    });
    assert!(!studio.input.is_recording());

    let regions = studio.made("region");
    let [(name, region)] = regions.as_slice() else {
        panic!("{regions:?}");
    };
    assert_eq!(name, "rec-sound-1");
    assert_eq!(region["asset"], "rec-sound-1.wav");
    // It plays from where the song was when the take began, the first poll after play.
    let start = region["start"].as_u64().unwrap();
    assert!(start < 960, "{start}");

    let timelines = studio.made("timeline");
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
    let later = std::time::Instant::now() + std::time::Duration::from_secs(3_600);
    studio.project.finish_quiet_runs_at(later).unwrap();
    assert_eq!(studio.project.undo_label().as_deref(), Some("Record"));
    studio.project.undo().unwrap();
    studio.settle();
    assert!(studio.made("region").is_empty() && studio.made("timeline").is_empty());
    studio.project.redo().unwrap();
    studio.settle();
    // A project of views wants a generic card, which this one does not need.
    let problems = studio.project.problems();
    assert!(problems.iter().all(|problem| problem.message.contains("generic-card")), "{problems:?}");
    // The region plays what was recorded, the left side of the tone, once its file is read in
    // the background.
    studio.until("the region plays its take", |studio| {
        studio.project.engine().stop();
        studio.start_playing();
        studio.loudest(24_000) > 0.4
    });
    assert_eq!(studio.notices, [] as [String; 0]);
}

/// While a recorder is armed the input is open, and its level is there for a meter of its
/// channels at every poll.
#[test]
fn the_level_of_the_input_a_recorder_opened_is_there_for_a_meter() {
    let (mut studio, input) = open_studio(false);
    assert_eq!(studio.input.input_levels(), [] as [f32; 0]);
    studio.press("studio", "arm");
    studio.until("the level of the right side of the tone", |studio| {
        studio.render(2_048);
        input.write_until(studio.engine.frames(), tone);
        studio.poll();
        studio.input.input_levels().get(1) == Some(&0.25)
    });
    assert!(studio.input.armed().contains(&id("studio/rec")));
}

/// An input that cannot open ends the take that waited for it: its notes are kept, and nothing
/// waits for ever. The composer reads why.
#[test]
fn a_take_whose_input_cannot_open_keeps_its_notes() {
    let (mut studio, input) = open_studio(false);
    input.fails.store(true, Ordering::Relaxed);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 24_000);
    studio.play_midi(60);
    studio.record_for(&input, 24_000);
    studio.press("studio", "done");
    studio.until("the notes kept", |studio| {
        studio.record_for(&input, 2_048);
        !studio.made("timeline").is_empty()
    });
    assert!(studio.made("region").is_empty());
    assert!(studio.input.armed().is_empty());
    let why = sound_core::DeviceError::NoInputDevice.to_string();
    assert!(studio.notices.contains(&why), "{:?}", studio.notices);
}

/// A recorder deleted while it takes ends its take there: its notes are kept, and the input it
/// held is let go, off the thread that polls.
#[test]
fn a_recorder_deleted_while_it_takes_lets_go_of_the_input() {
    let (mut studio, input) = open_studio(false);
    studio.press("studio", "arm");
    studio.until("the input opens", |_| input.openings.load(Ordering::Relaxed) == 1);
    studio.start_playing();
    studio.record_for(&input, 24_000);
    studio.play_midi(60);
    studio.record_for(&input, 24_000);
    studio.delete("studio/rec");
    studio.until("the notes kept and the input let go", |studio| {
        studio.record_for(&input, 2_048);
        !studio.made("timeline").is_empty() && input.closed_on().len() == 1
    });
    assert_ne!(input.closed_on(), [std::thread::current().id()]);
}

/// A second recorder that starts while the first records gets a take of its own, of the sound
/// from where it started.
#[test]
fn a_recorder_that_starts_while_another_records_keeps_its_own_take() {
    let (mut studio, input) = open_studio(true);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 24_000);
    studio.press("studio", "arm-2");
    studio.record_for(&input, 24_000);
    studio.press("studio", "done");
    studio.press("studio", "done-2");
    studio.until("two regions", |studio| {
        studio.record_for(&input, 2_048);
        studio.made("region").len() == 2
    });
    let regions = studio.made("region");
    let start = |name: &str| {
        let (_, region) = regions.iter().find(|(each, _)| each.starts_with(name)).unwrap();
        region["start"].as_u64().unwrap()
    };
    // Half a second later at 120 bpm is a beat later.
    assert!(start("rec-2-sound") >= start("rec-sound") + 900, "{regions:?}");
    studio.until("the input let go", |studio| {
        studio.record_for(&input, 2_048);
        input.closed_on().len() == 1
    });
}

/// At a loop, a take ends and the next starts: one region for each round, and the input is let
/// go once recording stops, even when it stops before the round before was written.
#[test]
fn a_take_across_a_loop_is_a_region_for_each_round() {
    let (mut studio, input) = open_studio(false);
    studio.project.engine().set_loop(Some(Ticks(0)..Ticks(1_920)));
    studio.press("studio", "arm");
    studio.start_playing();
    // One round is a second; stop just after the loop.
    studio.record_for(&input, 50_000);
    studio.press("studio", "done");
    studio.until("a region for each round", |studio| {
        studio.record_for(&input, 2_048);
        studio.made("region").len() == 2
    });
    studio.until("the input let go", |studio| {
        studio.record_for(&input, 2_048);
        input.closed_on().len() == 1
    });
}

/// What a recorder heard just before it was deleted, and no poll read yet, is in its take.
#[test]
fn a_recorder_deleted_keeps_what_it_heard_since_the_last_poll() {
    let (mut studio, input) = open_studio(false);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 24_000);
    studio.play_midi(64);
    studio.render(2_048);
    studio.delete("studio/rec");
    studio.until("the notes kept", |studio| {
        studio.record_for(&input, 2_048);
        !studio.made("timeline").is_empty()
    });
    let (_, timeline) = &studio.made("timeline")[0];
    assert_eq!(timeline["notes"][0]["pitch"], 64, "{timeline}");
}

/// A take keeps how the keys were played, not only which: the bend and the mod wheel.
#[test]
fn a_take_keeps_the_bend_and_the_mod_wheel() {
    let (mut studio, input) = open_studio(false);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 24_000);
    studio.play_midi(60);
    studio.send_midi(Played::Bend(Bend::new(4_000).unwrap()));
    studio.send_midi(Played::ModWheel(Amount::new(64).unwrap()));
    studio.record_for(&input, 24_000);
    studio.press("studio", "done");
    studio.until("the notes kept", |studio| {
        studio.record_for(&input, 2_048);
        !studio.made("timeline").is_empty()
    });
    let (_, timeline) = &studio.made("timeline")[0];
    assert!(!timeline["bend"].as_array().unwrap().is_empty(), "{timeline}");
    assert!(!timeline["mod_wheel"].as_array().unwrap().is_empty(), "{timeline}");
}

/// While it records, a take shows the notes it heard so far, a key still held with no end, and
/// its sound as it grows; undo waits for it.
#[test]
fn a_take_shows_its_notes_and_its_sound_while_it_records() {
    let (mut studio, input) = open_studio(false);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 4_096);
    studio.play_midi(60);
    let recorder = id("studio/rec");
    studio.until("the held key in the take", |studio| {
        studio.record_for(&input, 2_048);
        let notes = studio.input.take_notes(&recorder).map(|take| take.notes.clone());
        matches!(notes.as_deref(), Some([note]) if note.pitch == 60 && note.end.is_none())
    });
    studio.until("the sound of the take", |studio| {
        studio.record_for(&input, 2_048);
        let sound = studio.input.take_sound(&recorder);
        sound.is_some_and(|sound| sound.overview.read(|overview| overview.peak(0, 48_000)) == 0.5)
    });
    assert!(studio.input.is_recording());
}

/// A stop ends the take where the song stopped.
#[test]
fn a_stop_ends_the_take() {
    let (mut studio, input) = open_studio(false);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.record_for(&input, 4_096);
    studio.play_midi(60);
    studio.record_for(&input, 4_096);
    studio.project.engine().stop();
    studio.until("the take", |studio| {
        studio.record_for(&input, 2_048);
        studio.made("timeline").len() == 1
    });
}

/// A take a recorder makes with no parent that keeps it keeps only its files, and says so.
#[test]
fn a_take_nothing_keeps_says_so() {
    let (mut studio, input) = open_studio(false);
    let code = format!("{}}});\n", &STUDIO[..STUDIO.find("  onTake:").unwrap()]);
    std::fs::write(studio.folder.path().join("extensions/studio.tsx"), code).unwrap();
    studio.tools.code_changed();
    studio.until("the new code", |studio| studio.tools.loads() > 0);
    studio.press("studio", "arm");
    studio.start_playing();
    studio.play_midi(60);
    studio.record_for(&input, 4_096);
    studio.press("studio", "done");
    let said = "The take of studio/rec was not kept: nothing keeps what it records yet. Ask the agent for it.";
    studio.until("the notice", |studio| {
        studio.record_for(&input, 2_048);
        studio.notices.iter().any(|notice| notice == said)
    });
    let recorder = InstanceId::new("studio/rec").unwrap();
    assert!(studio.project.tool_of(&recorder).is_some());
}
