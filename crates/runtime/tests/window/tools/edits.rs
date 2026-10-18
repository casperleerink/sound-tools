//! A tool edits its part of the project: its record and those of its children, from any of its
//! code, on the records as they are when the edit lands.

use std::time::Duration;

use gpui::{Modifiers, TestAppContext, point};
use sound_core::RUN_QUIET;

use super::{EXPERIMENT, ONLY_THE_TOOL, Window, open, write};
use crate::support::id;

impl Window<'_> {
    /// The file at `path` in the project folder, if there is one.
    fn file(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self._folder.path().join(path)).ok()
    }

    /// Polls once the runs of edits went quiet, so they are written and undoable.
    fn after_the_runs(&mut self) {
        std::thread::sleep(RUN_QUIET + Duration::from_millis(50));
        self.poll();
    }

    fn undo(&mut self) -> Option<String> {
        let session = self.session.clone();
        self.cx
            .update(|_, cx| session.update(cx, |session, cx| session.undo(cx)));
        self.cx
            .read(|cx| session.read(cx).project().redo_label().map(String::from))
    }

    fn undo_label(&mut self) -> Option<String> {
        let session = self.session.clone();
        self.cx
            .read(|cx| session.read(cx).project().undo_label().map(String::from))
    }

    fn middle(&mut self) -> gpui::Point<gpui::Pixels> {
        let middle = self.cx.update(|window, _| window.viewport_size()) / 2.;
        point(middle.width, middle.height)
    }
}

/// The project of `EXPERIMENT` with the tool `tool` at its top in place of `keys`.
fn at_the_top(tool: &str, extensions: &str) -> String {
    let (_, project) = EXPERIMENT[0];
    let project = project.replace("\"keys\"", &format!("\"{tool}\""));
    project.replace(
        "\"extensions\": []",
        &format!("\"extensions\": [{extensions}]"),
    )
}

/// Counts its first 20 frames in its record.
const FOLLOW: &str = r#"import { number, tool } from "./sdk";

tool({
  name: "follow",
  title: "Follow",
  when: "You want to count frames",
  doc: "Counts its first 20 frames in its record.",
  kind: "source",
  state: { count: number({ min: 0, max: 1000 }) },
  memory: () => ({ frames: 0 }),
  tick: ({ memory, update }) => {
    if (memory.frames < 20) {
      memory.frames += 1;
      update("Count frames", ({ state }) => {
        state.count = memory.frames;
      });
    }
  },
});
"#;

#[gpui::test]
fn the_edits_of_a_tick_land_as_one_undo_step_written_once(cx: &mut TestAppContext) {
    let record = r#"{"tool": "follow", "state": {}}"#;
    let files = [
        ONLY_THE_TOOL[0],
        ("state/follow.json", record),
        ("extensions/follow.ts", FOLLOW),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    window.until("20 frames", |window| window.record("follow")["count"] == 20);
    // Live, but neither a step nor a write per frame.
    assert_eq!(window.file("state/follow.json").as_deref(), Some(record));
    assert_eq!(window.undo_label(), None);

    window.after_the_runs();
    let written = window.file("state/follow.json").unwrap();
    assert!(written.contains("\"count\": 20"), "{written}");
    assert_eq!(window.undo().as_deref(), Some("Count frames"));
    assert_eq!(window.record("follow"), serde_json::json!({}));
    assert_eq!(window.undo_label(), None);
    assert_eq!(window.problems(), []);
}

/// A key that marks its record a moment later, as after a slow call.
const LATE: &str = r#"import { h, knob, number, tool } from "./sdk";

tool({
  name: "late",
  title: "Late",
  when: "You want to mark the record late",
  doc: "A key marks the record 0.3 s later.",
  state: { level: knob({ min: 0, max: 1, default: 0 }), late: number({ min: 0, max: 1 }) },
  onKey: async ({ update }, { key, down }) => {
    if (key !== "a" || !down) return;
    await new Promise((done) => setTimeout(done, 300));
    update("Arrive late", ({ state }) => {
      state.late = 1;
    });
  },
  page: () => h("div", {}, "Late"),
});
"#;

#[gpui::test]
fn an_edit_after_an_await_keeps_an_edit_made_meanwhile(cx: &mut TestAppContext) {
    let project = at_the_top("late", "");
    let files = [
        ("project.json", project.as_str()),
        ("state/late.json", r#"{"tool": "late", "state": {}}"#),
        ("extensions/late.ts", LATE),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    window.cx.simulate_keystrokes("a");
    window.poll();
    // The composer turns a knob while the code waits.
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Turn level");
                project.update_json(&mut edit, &id("late"), |state| {
                    state["level"] = 0.5.into();
                })?;
                project.finish(edit)
            })
        })
    });
    window.until("the late edit", |window| window.record("late")["late"] == 1);
    assert_eq!(window.record("late")["level"], 0.5);
    assert_eq!(window.problems(), []);
}

/// A parent whose page writes the patterns of its sequencer `bass`, which plays `synth`.
const WRITER: &str = r#"import { h, tool, type Declaration, type SequencerState } from "./sdk";

tool({
  name: "writer",
  title: "Writer",
  when: "You want to write a bass line",
  doc: "A click writes a bar of one low note.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "bass", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { port: "audio" } },
    ],
  }),
  transport: true,
  page: ({ size, update }) =>
    h("div", {
      style: { width: size.width, height: size.height },
      onClick: () => update("Write a bass line", ({ children }) => {
        const bass = children.find((child) => child.name === "bass")!.state as SequencerState;
        bass.patterns = [{ length: 3840, notes: [{ start: 0, length: 3840, pitch: 45, velocity: 100 }] }];
        bass.plays = 0;
      }),
    }, "Write"),
});
"#;

#[gpui::test]
fn a_page_writes_the_patterns_of_its_sequencer_and_they_play(cx: &mut TestAppContext) {
    let project = at_the_top("writer", "\"instrument\"");
    let files = [
        ("project.json", project.as_str()),
        (
            "state/writer/instance.json",
            r#"{"tool": "writer", "state": {}}"#,
        ),
        (
            "state/writer/synth.json",
            r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
        ),
        (
            "state/writer/bass.json",
            r#"{"tool": "sequencer", "state": {"patterns": []}}"#,
        ),
        ("extensions/writer.ts", WRITER),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let play = window.find("play").expect("the play button");
    window.cx.simulate_click(play, Modifiers::default());
    window.poll();
    assert_eq!(window.loudest(9_600), 0.0);

    let middle = window.middle();
    window.cx.simulate_click(middle, Modifiers::default());
    window.until("the bass line", |window| window.loudest(4_800) > 0.05);
    let bass = window.record("writer/bass");
    assert_eq!(bass["patterns"][0]["notes"][0]["pitch"], 45, "{bass}");

    assert_eq!(window.undo().as_deref(), Some("Write a bass line"));
    assert_eq!(
        window.record("writer/bass")["patterns"],
        serde_json::json!([])
    );
    assert_eq!(window.problems(), []);
}

/// A parent that plays every child it has. A key adds the drone `hi`, another takes it out, and
/// two more try to add what is no record.
const RACK: &str = r#"import { h, number, tool, type Declaration } from "./sdk";

tool({
  name: "rack",
  title: "Rack",
  when: "You want to add drones by key",
  doc: "Plays every child; a adds the drone hi, d takes it out.",
  state: { count: number({ min: 0, max: 10 }) },
  behaviour: ({ children }): Declaration => ({
    connections: children.map((child) => ({ from: { child: child.name }, to: { port: "audio" as const } })),
  }),
  onKey: ({ update }, { key, down }) => {
    if (!down) return;
    if (key === "a") update("Add a drone", ({ children }) => {
      children.push({ name: "hi", tool: "drone", state: { hz: 880 } });
    });
    if (key === "d") update("Take the drone out", (part) => {
      part.children = part.children.filter((child) => child.name !== "hi");
    });
    if (key === "b") update("Add a bad drone", ({ state, children }) => {
      state.count = 1;
      children.push({ name: "bad", tool: "drone", state: { hz: "loud" } });
    });
    if (key === "n") update("Add a drone with no name", ({ children }) => {
      children.push({ name: "Not A Name", tool: "drone", state: {} });
    });
    if (key === "s") update("Add a grandchild", ({ children }) => {
      children.push({ name: "x/y", tool: "drone", state: {} });
    });
  },
  page: () => h("div", {}, "Rack"),
});
"#;

fn open_rack(cx: &mut TestAppContext) -> Option<Window<'_>> {
    let project = at_the_top("rack", "");
    let files = [
        ("project.json", project.as_str()),
        (
            "state/rack/instance.json",
            r#"{"tool": "rack", "state": {}}"#,
        ),
        ("extensions/rack.ts", RACK),
        ("extensions/drone.ts", super::DRONE),
    ];
    open(cx, &files)
}

#[gpui::test]
fn a_key_adds_a_child_and_takes_it_out_its_parent_plays_it_and_undo_brings_it_back(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open_rack(cx) else {
        return;
    };
    assert_eq!(window.loudest(4_800), 0.0);
    window.cx.simulate_keystrokes("a");
    window.until("the new drone", |window| window.loudest(4_800) > 0.1);
    window.after_the_runs();
    let written = window
        .file("state/rack/hi.json")
        .expect("the record of the child");
    assert!(written.contains("880"), "{written}");

    window.cx.simulate_keystrokes("d");
    window.until("the drone gone", |window| window.loudest(4_800) == 0.0);
    window.after_the_runs();
    assert_eq!(window.file("state/rack/hi.json"), None);

    assert_eq!(window.undo().as_deref(), Some("Take the drone out"));
    window.until("the drone back", |window| window.loudest(4_800) > 0.1);
    assert!(window.file("state/rack/hi.json").is_some());
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_child_its_tool_refuses_is_a_problem_and_nothing_of_the_edit_applies(cx: &mut TestAppContext) {
    let Some(mut window) = open_rack(cx) else {
        return;
    };
    window.cx.simulate_keystrokes("b");
    window.until("the refusal", |window| !window.problems().is_empty());
    let problems = window.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/rack.ts");
    assert!(
        problem.message.contains("\"Add a bad drone\" was refused")
            && problem.message.contains("hz"),
        "{problem:?}"
    );
    assert_eq!(window.record("rack"), serde_json::json!({}));
    window.cx.simulate_keystrokes("n");
    // A file lists one problem, the newest.
    window.until("the name refused", |window| {
        let problems = window.problems();
        problems
            .iter()
            .any(|problem| problem.message.contains("names use lowercase"))
    });
    window.cx.simulate_keystrokes("s");
    window.until("the path refused", |window| {
        let problems = window.problems();
        problems
            .iter()
            .any(|problem| problem.message.contains("named with a `/`"))
    });
    window.after_the_runs();
    let session = window.session.clone();
    let instances = window
        .cx
        .read(|cx| session.read(cx).project().instances().count());
    assert_eq!(instances, 1);
    assert_eq!(window.loudest(4_800), 0.0);
    assert_eq!(window.undo_label(), None);
}

/// A parent of many takes whose control loop counts its first 40 frames in its record, so its
/// behaviour runs again each frame too.
const LIBRARY: &str = r#"import { notes, number, tool, type Declaration } from "./sdk";

tool({
  name: "take",
  title: "Take",
  when: "You want to keep a take",
  doc: "Notes played.",
  kind: "source",
  state: { notes: notes() },
});

tool({
  name: "shelf",
  title: "Library",
  when: "You want to keep takes",
  doc: "Holds takes.",
  state: { frames: number({ min: 0, max: 1000 }), takes: number({ min: 0, max: 1000 }) },
  memory: () => ({ frames: 0 }),
  behaviour: (): Declaration => ({ connections: [] }),
  tick: ({ memory, children, update }) => {
    if (memory.frames < 40) {
      memory.frames += 1;
      update("Count frames", ({ state }) => {
        state.frames = memory.frames;
        state.takes = children.length;
      });
    }
  },
});
"#;

#[gpui::test]
fn a_frame_of_a_parent_of_300_takes_tells_bun_none_of_them_again(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    write(folder.path(), "project.json", ONLY_THE_TOOL[0].1);
    write(folder.path(), "extensions/shelf.ts", LIBRARY);
    write(
        folder.path(),
        "state/shelf/instance.json",
        r#"{"tool": "shelf", "state": {}}"#,
    );
    let note = r#"{"start": 0, "length": 480, "pitch": 60, "velocity": 100}"#;
    let take =
        format!(r#"{{"tool": "take", "state": {{"notes": [{note}, {note}, {note}, {note}]}}}}"#);
    for index in 0..300 {
        write(
            folder.path(),
            &format!("state/shelf/take-{index}.json"),
            &take,
        );
    }
    let Some(mut window) = super::open_folder(cx, folder) else {
        return;
    };
    let frames = |window: &mut Window| window.record("shelf")["frames"].as_u64().unwrap_or(0);
    window.until("the first frames", |window| frames(window) >= 5);
    let (first, sent) = (frames(&mut window), sound_typescript::sent_bytes());
    window.until("more frames", |window| frames(window) >= first + 20);
    let per_frame = (sound_typescript::sent_bytes() - sent) / (frames(&mut window) - first);
    assert_eq!(window.record("shelf")["takes"], 300);
    // Telling the takes once is 300 times one.
    assert!(
        per_frame < take.len() as u64 * 4,
        "{per_frame} bytes a frame"
    );
}
