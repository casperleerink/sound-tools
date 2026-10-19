//! The ports of a parent itself are ends of its connections: its `notes` and its audio input
//! pass into a child, a child's number passes out to the parent's parent, and every device of a
//! track hears the track's notes. Bun is not part of the build, so where it is not installed,
//! these tests say so and pass.

use midi::{Keyboard, Played};
use plugin_host::PluginFormat;
use sound_core::live_input;
use sound_notes::{Pedal, Pitch, Velocity};

use crate::support::{Harness, first_sound, test_plugin_of, write};

/// A `project.json` with these extensions and connections.
fn project_json(extensions: &str, connections: &str) -> String {
    format!(
        r#"{{"format": 1, "extensions": [{extensions}],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{connections}]}}"#
    )
}

/// The connection that plays the instance `top` on the device.
fn to_device(top: &str) -> String {
    format!(r#"{{"from": {{"instance": "{top}", "port": "audio"}}, "to": {{"device_output": 0}}}}"#)
}

/// Plays its plugin child `keys` from its own notes, and plays what the child plays.
const PIANIST: &str = r#"import { tool, type Declaration } from "./sdk";

tool({
  name: "pianist",
  title: "Pianist",
  when: "Never",
  doc: "Plays its plugin from the keyboard.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { notes: "notes" }, to: { child: "keys", notes: "notes" } },
      { from: { child: "keys" }, to: { port: "audio" } },
    ],
  }),
});
"#;

/// What the keyboard at the top of a project plays: a key, then the pedal, each sent while the
/// engine stands on a frame of its own, as a hand plays them.
fn keyboard_into_the_top(mut harness: Harness) -> Vec<f32> {
    // Bun's answer, which wires the parent.
    let mut heard = harness.render(64);
    let mut keyboard = Keyboard::attach(harness.project.engine()).unwrap();
    let notes = runtime::window::live_input::notes_inputs(&harness.project, None);
    keyboard
        .play_into(harness.project.engine(), &notes)
        .unwrap();
    keyboard.poll(harness.project.engine(), None).unwrap();
    let key = Played::On {
        pitch: Pitch::new(60).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    };
    let pedal = Played::Pedal(Pedal::new(127).unwrap());
    for played in [key, pedal] {
        heard.extend(harness.render(1_000));
        assert!(keyboard.input().send(played));
    }
    heard.extend(harness.render(2_000));
    heard
}

#[test]
fn the_keyboard_plays_a_plugin_child_through_the_parents_notes_and_every_instance_at_the_top() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        let plugin = test_plugin_of(format, "keys");
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        write(
            root,
            "project.json",
            &project_json(r#""plugin-host""#, &to_device("pianist")),
        );
        write(root, "extensions/pianist.ts", PIANIST);
        write(
            root,
            "state/pianist/instance.json",
            r#"{"tool": "pianist", "state": {}}"#,
        );
        write(root, "state/pianist/keys.json", &plugin);
        let (harness, _plugins) = Harness::with_test_plugin(folder);
        assert_eq!(harness.project.problems(), [], "{format:?}");
        let through_the_parent = keyboard_into_the_top(harness);

        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        write(
            root,
            "project.json",
            &project_json(r#""plugin-host""#, &to_device("keys")),
        );
        write(root, "state/keys.json", &plugin);
        let (harness, _plugins) = Harness::with_test_plugin(folder);
        let at_the_top = keyboard_into_the_top(harness);

        // The parent and the plugin, both at the top: the keyboard plays each.
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let both = [to_device("pianist"), to_device("solo")].join(", ");
        write(
            root,
            "project.json",
            &project_json(r#""plugin-host""#, &both),
        );
        write(root, "extensions/pianist.ts", PIANIST);
        write(
            root,
            "state/pianist/instance.json",
            r#"{"tool": "pianist", "state": {}}"#,
        );
        write(root, "state/pianist/keys.json", &plugin);
        write(root, "state/solo.json", &test_plugin_of(format, "solo"));
        let (harness, _plugins) = Harness::with_test_plugin(folder);
        let twice: Vec<f32> = at_the_top.iter().map(|sample| sample * 2.0).collect();
        assert!(keyboard_into_the_top(harness) == twice, "{format:?}");

        // The key and the pedal, on the left and on the right, on the frame each was played.
        let right = |render: &[f32]| {
            render
                .iter()
                .skip(1)
                .step_by(2)
                .position(|pedal| *pedal == 1.0)
        };
        assert_eq!(first_sound(&at_the_top, 0), Some(1_064), "{format:?}");
        assert_eq!(right(&at_the_top), Some(2_064), "{format:?}");
        assert!(through_the_parent == at_the_top, "{format:?}");
    }
}

/// Plays what reaches its audio input through its child `half`, out of its own.
const ROUTER: &str = r#"import { tool, type Declaration } from "./sdk";

tool({
  name: "router",
  title: "Router",
  when: "Never",
  doc: "Plays its input through its child.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { port: "audio" }, to: { child: "half" } },
      { from: { child: "half" }, to: { port: "audio" } },
    ],
  }),
});
"#;

const HALF: &str = r#"import { input, tool } from "./sdk";

tool({ name: "half", title: "Half", when: "Never", doc: "Half as loud.", state: {}, sound: () => input.times(0.5) });
"#;

#[test]
fn the_microphone_into_a_parent_reaches_the_child_it_connects_it_to() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let microphone = r#"{"from": {"device_input": 0}, "to": {"input": {"instance": "router", "port": "audio"}}}"#;
    let project = project_json("", &format!("{microphone}, {}", to_device("router")));
    write(root, "project.json", &project);
    write(root, "extensions/router.ts", ROUTER);
    write(root, "extensions/half.ts", HALF);
    write(
        root,
        "state/router/instance.json",
        r#"{"tool": "router", "state": {}}"#,
    );
    write(
        root,
        "state/router/half.json",
        r#"{"tool": "half", "state": {}}"#,
    );
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness.render(64);
    let (mut microphone, input) = live_input(48_000, 1);
    harness.project.set_live_input(Some(input));
    let mut block = [0.0; 960];
    harness.engine.process_block(&mut block);
    microphone.write(&[0.8; 960]);
    harness.engine.process_block(&mut block);
    assert_eq!(block[959], 0.4);
}

/// A constant that sends its value out as the number `level`.
const LFO: &str = r#"import { output, tool } from "./sdk";

tool({ name: "lfo", title: "LFO", when: "Never", doc: "Sends 0.5.", state: {}, sound: () => { output("level", 0.5); return 0; } });
"#;

/// A parent that passes the number of its child `lfo` on as its own.
const INNER: &str = r#"import { tool, type Declaration } from "./sdk";

tool({
  name: "inner",
  title: "Inner",
  when: "Never",
  doc: "Passes a number on.",
  state: {},
  behaviour: (): Declaration => ({ connections: [{ from: { child: "lfo", number: "level" }, to: { number: "level" } }] }),
});
"#;

/// A constant 0.5, through `amp`, whose gain the number of `inner` turns.
const RACK: &str = r#"import { number, input, tool, type Declaration } from "./sdk";

tool({ name: "amp", title: "Amp", when: "Never", doc: "A gain.", state: { gain: number({ min: 0, max: 1, default: 0 }) }, sound: ({ gain }) => input.times(gain) });

tool({ name: "dc", title: "DC", when: "Never", doc: "A constant.", state: {}, sound: () => 0.5 });

tool({
  name: "rack",
  title: "Rack",
  when: "Never",
  doc: "A gain a grandchild turns.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "dc" }, to: { child: "amp" } },
      { from: { child: "inner", number: "level" }, to: { child: "amp", number: "gain" } },
      { from: { child: "amp" }, to: { port: "audio" } },
    ],
  }),
});
"#;

#[test]
fn a_number_of_a_child_passes_out_of_a_parent_to_the_parent_of_that() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", &project_json("", &to_device("rack")));
    write(root, "extensions/lfo.ts", LFO);
    write(root, "extensions/inner.ts", INNER);
    write(root, "extensions/rack.ts", RACK);
    write(
        root,
        "state/rack/instance.json",
        r#"{"tool": "rack", "state": {}}"#,
    );
    write(root, "state/rack/dc.json", r#"{"tool": "dc", "state": {}}"#);
    write(
        root,
        "state/rack/amp.json",
        r#"{"tool": "amp", "state": {}}"#,
    );
    write(
        root,
        "state/rack/inner/instance.json",
        r#"{"tool": "inner", "state": {}}"#,
    );
    write(
        root,
        "state/rack/inner/lfo.json",
        r#"{"tool": "lfo", "state": {}}"#,
    );
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    // The gain glides from its record, 0, to the number: 0.5 of 0.5.
    let played = harness.render(9_600);
    let last = played[played.len() - 1];
    assert!((last - 0.25).abs() < 1e-4, "{last}");
}

/// A parent whose behaviour names a child it finds by name, which is not there.
const LOST: &str = r#"import { tool, type Declaration } from "./sdk";

tool({
  name: "lost",
  title: "Lost",
  when: "Never",
  doc: "Names a child that is not there.",
  state: {},
  behaviour: ({ children }): Declaration => ({
    connections: [{ from: { child: "lfo", number: "level" }, to: { child: children.find((child) => child.name === "amp")?.name, number: "gain" } }],
  }),
});
"#;

/// JSON leaves an undefined `child` out, which would make the end the parent's own output.
#[test]
fn a_child_that_is_undefined_is_a_problem_and_no_port_of_the_parent() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", &project_json("", ""));
    write(root, "extensions/lfo.ts", LFO);
    write(root, "extensions/lost.ts", LOST);
    write(
        root,
        "state/lost/instance.json",
        r#"{"tool": "lost", "state": {}}"#,
    );
    write(
        root,
        "state/lost/lfo.json",
        r#"{"tool": "lfo", "state": {}}"#,
    );
    let harness = Harness::open(folder);
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert!(
        problem.message.contains(
            "connections[0].to.child is undefined: name a child, or leave child out for a port of the parent itself"
        ),
        "{problem:?}"
    );
}

/// `level` of `lfo` into the number input `gain` of `amp`, a grandchild inside `holder`, which
/// plays what it hears.
const DEEP: &str = r#"import { number, tool, type Declaration } from "./sdk";

tool({ name: "level", title: "Level", when: "Never", doc: "Plays its number.", state: {}, inputs: { gain: number({ min: 0, max: 1, default: 0 }) }, sound: ({ gain }) => gain });

tool({
  name: "holder",
  title: "Holder",
  when: "Never",
  doc: "Plays its child.",
  state: {},
  behaviour: (): Declaration => ({ connections: [{ from: { child: "amp" }, to: { port: "audio" } }] }),
});

tool({
  name: "deep",
  title: "Deep",
  when: "Never",
  doc: "A number into a grandchild.",
  state: {},
  behaviour: (): Declaration => ({
    connections: [
      { from: { child: "lfo", number: "level" }, to: { child: "holder/amp", number: "gain" } },
      { from: { child: "holder" }, to: { port: "audio" } },
    ],
  }),
});
"#;

#[test]
fn a_number_reaches_the_number_input_of_a_grandchild() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", &project_json("", &to_device("deep")));
    write(root, "extensions/lfo.ts", LFO);
    write(root, "extensions/deep.ts", DEEP);
    write(
        root,
        "state/deep/instance.json",
        r#"{"tool": "deep", "state": {}}"#,
    );
    write(
        root,
        "state/deep/lfo.json",
        r#"{"tool": "lfo", "state": {}}"#,
    );
    write(
        root,
        "state/deep/holder/instance.json",
        r#"{"tool": "holder", "state": {}}"#,
    );
    write(
        root,
        "state/deep/holder/amp.json",
        r#"{"tool": "level", "state": {}}"#,
    );
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    let played = harness.render(9_600);
    let last = played[played.len() - 1];
    assert!((last - 0.5).abs() < 1e-4, "{last}");
}
