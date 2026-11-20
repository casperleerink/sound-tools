//! What code changes of the records, as the window runs it: what the records refuse, what
//! happens meanwhile, and what Bun is told again.

use runtime::input::Devices;
use serde_json::json;
use sound_core::{InstanceId, Problem};

use crate::support::{Opened, id, open, write};

const PROJECT: &str = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []}"#;

/// A project of `files` with the instance `name` of `tool` at the top.
fn open_files(files: &[(&str, &str)]) -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", PROJECT);
    write(root, "extensions/theme.json", "{}");
    for (path, contents) in files {
        write(root, path, contents);
    }
    open(folder, Devices::default())
}

/// The problems but the one of a project with no card for devices, which these have not.
fn problems(opened: &Opened) -> Vec<Problem> {
    let problems = opened.project.problems();
    (problems.into_iter())
        .filter(|problem| !problem.message.contains("generic-card.tsx is missing"))
        .collect()
}

/// The labels of the view of `instance`, as Bun drew it last.
fn labels(opened: &Opened, instance: &str) -> String {
    match opened.tools.drawn(&id(instance)) {
        Some(sound_typescript::next::Drawn::View { root, .. }) => root.to_string(),
        other => panic!("no view of {instance}: {}", other.is_some()),
    }
}

/// A press that marks its record a moment later, as after a slow call.
const LATE: &str = r#"import { h, number, tool } from "./sdk";
export const late = tool({
  name: "late", title: "Late", when: "Never", doc: "A press marks the record 0.3 s later.",
  state: { level: number({ min: 0, max: 1, default: 0 }), late: number({ min: 0, max: 1, default: 0 }) },
  view: (_, { update }) => (
    <box key="go" label="Go" w={20} h={20} onPress={async () => {
      await new Promise((done) => setTimeout(done, 300));
      update("Arrive late", ({ state }) => void (state.late = 1));
    }} />
  ),
});
"#;

#[test]
fn an_edit_after_an_await_keeps_an_edit_made_meanwhile() {
    let mut opened = open_files(&[
        ("extensions/late.tsx", LATE),
        ("state/late.json", r#"{"tool": "late", "state": {}}"#),
    ]);
    opened.press("late", "go");
    // The composer turns a knob while the code waits.
    let mut edit = opened.project.begin("Turn level");
    (opened.project)
        .update_json(&mut edit, &id("late"), |state| state["level"] = json!(0.5))
        .unwrap();
    opened.project.finish(edit).unwrap();
    opened.until("the late edit", |opened| opened.record("late")["late"] == 1);
    assert_eq!(opened.record("late")["level"], 0.5);
    assert_eq!(problems(&opened), []);
}

/// A parent that plays every child it has. Presses add the drone `hi`, or what is no record.
const RACK: &str = r#"import { h, number, tool, type Declaration } from "./sdk";
export const rack = tool({
  name: "rack", title: "Rack", when: "Never", doc: "Plays every child.",
  state: { count: number({ min: 0, max: 10, default: 0 }) },
  behaviour: ({ children }): Declaration => ({
    connections: children.map((child) => ({ from: { child: child.name }, to: { port: "audio" as const } })),
  }),
  view: (_, { update }) => (
    <col>
      <box key="bad" label="Bad" w={20} h={20} onPress={() => update("Add a bad drone", ({ state, children }) => {
        state.count = 1;
        children.push({ name: "bad", tool: "drone", state: { hz: "loud" } } as never);
      })} />
      <box key="unnamed" label="Unnamed" w={20} h={20} onPress={() => update("Add a drone with no name", ({ children }) => {
        children.push({ name: "Not A Name", tool: "drone", state: {} } as never);
      })} />
      <box key="deep" label="Deep" w={20} h={20} onPress={() => update("Add a grandchild", ({ children }) => {
        children.push({ name: "x/y", tool: "drone", state: {} } as never);
      })} />
    </col>
  ),
});
"#;

const DRONE: &str = r#"import { number, sine, tool } from "./sdk";
export const drone = tool({
  name: "drone", title: "Drone", when: "Never", doc: "A sine.",
  state: { hz: number({ min: 20, max: 2000, default: 220 }) },
  sound: ({ hz }) => sine(hz).times(0.25),
});
"#;

/// A press whose update the records refuse did nothing: whoever pressed hears why, the composer
/// in a notice and an agent from `--act`.
#[test]
fn a_child_its_tool_refuses_does_nothing_and_says_why() {
    let mut opened = open_files(&[
        ("extensions/rack.tsx", RACK),
        ("extensions/drone.ts", DRONE),
        (
            "state/rack/instance.json",
            r#"{"tool": "rack", "state": {}}"#,
        ),
    ]);
    let label = opened.project.undo_label();
    for (key, why) in [
        ("bad", "\"Add a bad drone\" was refused"),
        ("unnamed", "names use lowercase"),
        ("deep", "named with a `/`"),
    ] {
        let said = opened.press_refused("rack", key);
        assert!(said.contains(why), "{key}: {said}");
    }
    assert_eq!(opened.record("rack"), json!({}));
    assert_eq!(opened.project.instances().count(), 1);
    assert_eq!(opened.project.undo_label(), label);
}

/// A box that a press makes wide by an edit the runtime refuses: past the range of `level`.
const REFUSED: &str = r#"import { h, number, tool } from "./sdk";
export const refused = tool({
  name: "refused", title: "Refused", when: "Never", doc: "A press sets its level past its range.",
  state: { level: number({ min: 0, max: 1, default: 0 }) },
  view: (self, { update }) => (
    <box key="box" label={self.state.level > 1 ? "Wide" : "Narrow"} w={20} h={20}
      onPress={() => update("Too far", ({ state }) => void (state.level = 5))} />
  ),
});
"#;

/// Bun's view after a refused edit is the view of the record as it is.
#[test]
fn an_edit_the_runtime_refuses_shows_no_more_once_it_is_heard() {
    let mut opened = open_files(&[
        ("extensions/refused.tsx", REFUSED),
        ("state/refused.json", r#"{"tool": "refused", "state": {}}"#),
    ]);
    let said = opened.press_refused("refused", "box");
    assert!(said.contains("outside [0, 1]"), "{said}");
    let drawn = labels(&opened, "refused");
    assert!(
        drawn.contains("Narrow") && !drawn.contains("Wide"),
        "{drawn}"
    );
    assert_eq!(opened.record("refused"), json!({}));
}

/// Two tools whose views say their names.
const WIDE_AND_NARROW: &str = r#"import { h, tool } from "./sdk";
export const wide = tool({ name: "wide", title: "Wide", when: "Never", doc: "Wide.", state: {}, view: () => <box key="box" label="Wide box" w={20} h={20} /> });
export const narrow = tool({ name: "narrow", title: "Narrow", when: "Never", doc: "Narrow.", state: {}, view: () => <box key="box" label="Narrow box" w={20} h={20} /> });
"#;

/// A record whose tool changes, as an agent may write it, shows as its new tool.
#[test]
fn a_record_of_another_tool_shows_as_that_tool() {
    let mut opened = open_files(&[
        ("extensions/shapes.tsx", WIDE_AND_NARROW),
        ("state/shape.json", r#"{"tool": "wide", "state": {}}"#),
    ]);
    opened.press_nothing("shape");
    assert!(labels(&opened, "shape").contains("Wide box"));
    let root = opened.project.root().to_path_buf();
    let record = write(
        &root,
        "state/shape.json",
        r#"{"tool": "narrow", "state": {}}"#,
    );
    assert_eq!(opened.project.apply_outside_changes(&[record]).unwrap(), 1);
    opened.until("the narrow box", |opened| {
        labels(opened, "shape").contains("Narrow box")
    });
}

/// A parent of many takes whose control loop counts its first 40 frames in its record, so its
/// behaviour runs again each frame too.
const SHELF: &str = r#"import { number, tool, type Declaration } from "./sdk";
export const take = tool({ name: "take", title: "Take", when: "Never", doc: "A take.", state: { n: number({ min: 0, max: 100, default: 0 }) } });
export const shelf = tool({
  name: "shelf", title: "Shelf", when: "Never", doc: "Holds takes.",
  state: { frames: number({ min: 0, max: 1000, default: 0 }), takes: number({ min: 0, max: 1000, default: 0 }) },
  memory: () => ({ frames: 0 }),
  behaviour: (): Declaration => ({ connections: [] }),
  tick: ({ memory, children, update }) => {
    if (memory.frames >= 40) return;
    memory.frames += 1;
    update("Count frames", ({ state }) => {
      state.frames = memory.frames;
      state.takes = children.length;
    });
  },
});
"#;

#[test]
fn a_frame_of_a_parent_of_300_takes_tells_bun_none_of_them_again() {
    let mut files = vec![
        ("extensions/shelf.ts".to_string(), SHELF.to_string()),
        (
            "state/shelf/instance.json".to_string(),
            r#"{"tool": "shelf", "state": {}}"#.to_string(),
        ),
    ];
    for index in 0..300 {
        files.push((
            format!("state/shelf/take-{index}.json"),
            r#"{"tool": "take", "state": {"n": 1}}"#.to_string(),
        ));
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut opened = open_files(&files);
    let frames = |opened: &Opened| opened.record("shelf")["frames"].as_u64().unwrap_or(0);
    opened.until("the first frames", |opened| frames(opened) >= 5);
    let (first, told) = (frames(&opened), opened.tools.records_told());
    opened.until("more frames", |opened| frames(opened) >= first + 20);
    assert_eq!(opened.record("shelf")["takes"], 300);
    let per_frame = (opened.tools.records_told() - told) / (frames(&opened) - first);
    // The shelf, which changed: never the takes, which did not.
    assert!((1..=2).contains(&per_frame), "{per_frame} records a frame");
}

impl Opened {
    /// Presses `key` of the view of `instance`, whose update the records refuse: why.
    fn press_refused(&mut self, instance: &str, key: &str) -> String {
        self.press_nothing(instance);
        self.tools.act(&id(instance), key, "press", None).unwrap();
        let mut said = Vec::new();
        self.until("the refusal", |opened| {
            said.extend(opened.tools.take_failed_actions());
            !said.is_empty()
        });
        said.join("\n")
    }

    /// Shows the view of `instance` and waits until Bun drew it.
    fn press_nothing(&mut self, instance: &str) {
        let instance: InstanceId = id(instance);
        self.tools.show(sound_typescript::next::Shown {
            instances: [instance.clone()].into(),
            ..Default::default()
        });
        self.until("the view", |opened| {
            matches!(
                opened.tools.drawn(&instance),
                Some(sound_typescript::next::Drawn::View { .. })
            )
        });
    }
}
