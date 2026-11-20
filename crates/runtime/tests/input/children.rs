//! A parent's code plays its children, and hears what they hear, on the offline engine with
//! Bun.

use runtime::OFFLINE;
use runtime::input::Devices;
use sound_core::Problem;

use crate::support::{Opened, open, write};

/// A project whose `heard` plays to the output, with the built-in instrument, of `files`.
fn open_files(heard: &str, files: &[(&str, &str)]) -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = format!(
        r#"{{"format": 1, "extensions": ["instrument", "utility"],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{{"from": {{"instance": "{heard}", "port": "audio"}}, "to": {{"device_output": 0}}}}]}}"#
    );
    write(root, "project.json", &project);
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

/// A parent whose page plays its children, a press each: a note on the built-in `synth`, a note
/// on the tool `keys`, the number input and the bang of `drop`; then a bang `drop` does not
/// have, and a child it does not own.
const CONDUCTOR: &str = r#"import { h, tool } from "./sdk";
export const conductor = tool({
  name: "conductor", title: "Conductor", when: "Never", doc: "Plays its children.", state: {},
  behaviour: () => ({ connections: ["synth", "keys", "drop"].map((child) => ({ from: { child }, to: { port: "audio" as const } })) }),
  view: (_, { play, send }) => (
    <col>
      <box key="synth" label="Synth" w={20} h={20} onPress={() => play(45, { child: "synth", seconds: 0.1 })} />
      <box key="keys" label="Keys" w={20} h={20} onPress={() => play(69, { child: "keys", seconds: 0.1 })} />
      <box key="drop" label="Drop" w={20} h={20} onPress={() => { send({ child: "drop", number: "gain" } as never, 1); send({ child: "drop", bang: "hit" } as never); }} />
      <box key="hat" label="Hat" w={20} h={20} onPress={() => send({ child: "drop", bang: "hat" } as never)} />
      <box key="drum" label="Drum" w={20} h={20} onPress={() => play(60, { child: "drum" as never })} />
    </col>
  ),
});
"#;

const KEYS: &str = r#"import { note, sine, tool } from "./sdk";
export const keys = tool({ name: "keys", title: "Keys", when: "Never", doc: "Plays notes.", voices: 4, state: {}, sound: () => sine(note.freq).times(note.gate).times(0.2) });
"#;

/// Noise that jumps up when `hit` fires and falls away, silent until `gain` is up.
const DROP: &str = r#"import { bang, feedback, max, noise, number, tool } from "./sdk";
export const drop = tool({
  name: "drop", title: "Drop", when: "Never", doc: "A burst of noise.", state: {},
  inputs: { hit: bang(), gain: number({ min: 0, max: 1, default: 0 }) },
  sound: ({ hit, gain }) => {
    const level = feedback();
    level.set(max(hit, level.times(0.9995)));
    return noise().times(level).times(gain);
  },
});
"#;

#[test]
fn a_page_of_a_parent_plays_notes_on_two_children_and_the_controls_of_a_third() {
    let mut opened = open_files(
        "conductor",
        &[
            ("extensions/conductor.tsx", CONDUCTOR),
            ("extensions/keys.ts", KEYS),
            ("extensions/drop.ts", DROP),
            (
                "state/conductor/instance.json",
                r#"{"tool": "conductor", "state": {}}"#,
            ),
            (
                "state/conductor/synth.json",
                r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
            ),
            (
                "state/conductor/keys.json",
                r#"{"tool": "keys", "state": {}}"#,
            ),
            (
                "state/conductor/drop.json",
                r#"{"tool": "drop", "state": {}}"#,
            ),
        ],
    );
    for child in ["synth", "keys", "drop"] {
        assert_eq!(opened.loudest(4_800), 0.0, "before the press for {child}");
        opened.press("conductor", child);
        opened.until(child, |opened| opened.loudest(480) > 0.05);
        // The notes end; the noise of `drop` only fades.
        if child != "drop" {
            opened.until("silence", |opened| opened.loudest(4_800) == 0.0);
        }
    }
    assert_eq!(problems(&opened), []);

    let problem = |message: &str| Problem {
        path: "extensions/conductor.tsx".to_string(),
        message: format!("tool conductor: a performer plays the child {message}"),
    };
    opened.press("conductor", "hat");
    opened.until("the problem of the bang", |opened| {
        !problems(opened).is_empty()
    });
    let trigger = problem("\"drop\", which has no bang input hat: its bang inputs are hit");
    assert_eq!(problems(&opened), std::slice::from_ref(&trigger));
    opened.press("conductor", "drum");
    // A file shows its newest problem.
    opened.until("the problem of the child", |opened| {
        problems(opened) != std::slice::from_ref(&trigger)
    });
    let child =
        problem("\"drum\", which conductor does not own: its children are drop, keys, synth");
    assert_eq!(problems(&opened), [child]);
}

/// An instrument that keeps every note event it hears in its record, with its time.
const HEARER: &str = r#"import { list, note, number, sine, text, tool } from "./sdk";
export const hearer = tool({
  name: "hearer", title: "Hearer", when: "Never", doc: "Keeps every note event it hears, with its time.",
  voices: 8,
  state: { heard: list({ type: text(), at: number({ min: 0, max: 100000, default: 0 }) }) },
  sound: () => sine(note.freq).times(note.gate).times(0.1),
  onNotes: ({ update }, event) => {
    update("Hear", ({ state }) => void state.heard.push({ type: event.type, at: event.at }));
  },
});
"#;

/// A parent whose timeline `take` plays its `instrument`.
const RACK: &str = r#"import { tool } from "./sdk";
export const rack = tool({
  name: "rack", title: "Rack", when: "Never", doc: "Its take plays its instrument.", state: {},
  behaviour: () => ({
    connections: [
      { from: { child: "take", notes: "notes" }, to: { child: "instrument", notes: "notes" } },
      { from: { child: "instrument" }, to: { port: "audio" } },
    ],
  }),
});
"#;

#[test]
fn an_instrument_hears_the_timeline_of_its_parent_at_the_samples_it_sounds() {
    let clip = r#"{"tool": "timeline", "state": {"start": 0, "length": 3840,
  "notes": [{"start": 1000, "length": 500, "pitch": 60, "velocity": 100}, {"start": 2000, "length": 100, "pitch": 62, "velocity": 100}],
  "pedal": [{"start": 1500, "value": 127}]}}"#;
    let mut opened = open_files(
        "rack",
        &[
            ("extensions/rack.ts", RACK),
            ("extensions/hearer.ts", HEARER),
            (
                "state/rack/instance.json",
                r#"{"tool": "rack", "state": {}}"#,
            ),
            (
                "state/rack/instrument.json",
                r#"{"tool": "hearer", "state": {}}"#,
            ),
            ("state/rack/take.json", clip),
        ],
    );
    let start = opened.project.engine().poll().unwrap().frames;
    opened.project.engine().play();
    // Past the last note: 2100 ticks, 25 frames each at 120 bpm.
    opened.loudest(54_000);
    let heard = |opened: &Opened| opened.record("rack/instrument")["heard"].clone();
    opened.until("every event of the clip", |opened| {
        heard(opened)
            .as_array()
            .is_some_and(|heard| heard.len() >= 6)
    });
    let frame_of = |seconds: f64| (seconds * f64::from(OFFLINE.sample_rate)).round() as u64;
    let heard: Vec<(String, u64)> = (heard(&opened).as_array().unwrap().iter())
        // A timeline lets go of everything as it starts, as every sender of notes may.
        .filter(|event| event["type"] != "allOff")
        .map(|event| {
            let at = frame_of(event["at"].as_f64().unwrap());
            (event["type"].as_str().unwrap().to_string(), at - start)
        })
        .collect();
    let expected = [
        ("noteOn", 25_000),
        ("cc", 37_500),
        ("noteOff", 37_500),
        ("noteOn", 50_000),
        ("noteOff", 52_500),
    ];
    let expected: Vec<(String, u64)> = (expected.iter())
        .map(|(kind, frame)| (kind.to_string(), *frame))
        .collect();
    assert_eq!(heard, expected);
}

/// Tools a parent asks about: one whose sound reads its notes, one that plays its own steps in
/// time, and one whose code hears notes; and a built-in instrument.
const HEARING: &str = r#"import { beat, h, hearsNotes, note, sine, tool } from "./sdk";
export const voice = tool({ name: "voice", title: "Voice", when: "Never", doc: "Plays notes.", state: {}, sound: () => sine(note.freq).times(note.gate) });
export const steps = tool({ name: "steps", title: "Steps", when: "Never", doc: "Plays its own steps.", state: {}, sound: () => sine(220).times(beat.mod(1).lt(0.5)) });
export const listens = tool({ name: "listens", title: "Listens", when: "Never", doc: "Its code hears notes.", state: {}, sound: () => sine(220), onNotes: () => {} });
export const asks = tool({
  name: "asks", title: "Asks", when: "Never", doc: "Says which tools notes play.", state: {},
  view: () => <col>{["voice", "steps", "listens", "instrument.synth"].map((name) => <box key={name} label={`${name} ${hearsNotes(name)}`} w={20} h={20} />)}</col>,
});
"#;

/// So a track can show the clips it sends a tool that makes its own notes as not played.
#[test]
fn a_parent_learns_whether_notes_play_a_tool() {
    let mut opened = open_files(
        "asks",
        &[
            ("extensions/hearing.tsx", HEARING),
            ("state/asks.json", r#"{"tool": "asks", "state": {}}"#),
        ],
    );
    opened.show("asks");
    let drawn = match opened.tools.drawn(&crate::support::id("asks")) {
        Some(sound_typescript::next::Drawn::View { root, .. }) => root.to_string(),
        _ => panic!("no view"),
    };
    for answer in [
        "voice true",
        "steps false",
        "listens true",
        "instrument.synth true",
    ] {
        assert!(drawn.contains(answer), "{answer}: {drawn}");
    }
    assert_eq!(problems(&opened), []);
}

/// So an agent can print-debug a tool it wrote while the window has the project open.
#[test]
fn what_the_code_of_a_tool_prints_is_in_the_log_until_its_code_is_saved_again() {
    let hum = |level: &str| {
        format!(
            r#"import {{ sine, tool }} from "./sdk";
console.log("level", {level});
export const hum = tool({{ name: "hum", title: "Hum", when: "Never", doc: "A sine.", state: {{}}, sound: () => sine(441).times({level}) }});
"#
        )
    };
    let mut opened = open_files(
        "hum",
        &[
            ("extensions/hum.ts", &hum("0.5")),
            ("state/hum.json", r#"{"tool": "hum", "state": {}}"#),
        ],
    );
    let root = opened.project.root().to_path_buf();
    let log = move || std::fs::read_to_string(root.join("extensions/log.txt")).unwrap_or_default();
    opened.until("the line of the code", |_| log().contains("level 0.5\n"));
    write(opened.project.root(), "extensions/hum.ts", &hum("0.25"));
    opened.tools.code_changed();
    opened.until("only the line of the new code", |_| {
        let log = log();
        log.contains("level 0.25\n") && !log.contains("level 0.5")
    });
}

impl Opened {
    /// Shows the view of `instance` and waits until Bun drew it.
    fn show(&mut self, instance: &str) {
        let instance = crate::support::id(instance);
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

/// A number that swings from -0.8 to 0.8 four times a second, for a parent to cable into the
/// pan of a utility.
const SWAY: &str = include_str!("../../../../tooling/agent-eval/examples/sway.ts");

const SWAYER: &str = r#"import { tool } from "./sdk";
export const swayer = tool({
  name: "swayer", title: "Swayer", when: "Never", doc: "Its sway moves the pan of its strip.", state: {},
  behaviour: () => ({
    connections: [
      { from: { child: "sway", number: "pan" }, to: { child: "strip", number: "pan" } },
      { from: { child: "strip" }, to: { port: "audio" } },
    ],
  }),
});
"#;

/// A knob shows the pan a cable plays into it, not the record's, which stays.
#[test]
fn a_number_shows_what_a_cable_plays_into_it_and_not_its_record() {
    let mut opened = open_files(
        "swayer",
        &[
            ("extensions/swayer.ts", SWAYER),
            ("extensions/sway.ts", SWAY),
            ("state/swayer/instance.json", r#"{"tool": "swayer", "state": {}}"#),
            ("state/swayer/sway.json", r#"{"tool": "sway", "state": {"rate": 4}}"#),
            ("state/swayer/strip.json", r#"{"tool": "utility", "state": {}}"#),
        ],
    );
    assert_eq!(problems(&opened), []);
    opened.project.engine().play();
    let swayer = crate::support::id("swayer");
    let pan = |opened: &mut Opened| {
        let tick = opened.project.engine().poll().unwrap().playhead_tick;
        (opened.tools).field_now(&opened.project, &swayer, Some("strip"), "pan", tick)
    };
    opened.until("the pan to swing", |opened| {
        opened.render(1_200);
        pan(opened).abs() > 0.3
    });
    assert_eq!(opened.record("swayer/strip")["pan"], 0.0);
}
