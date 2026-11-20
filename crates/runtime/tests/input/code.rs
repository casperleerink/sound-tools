//! The code of a project as the window runs it: what it hears of the folder, and what happens
//! when it hangs.

use std::time::Duration;

use runtime::input::Devices;
use serde_json::Value;
use sound_core::{Changes, Problem};

use crate::support::{Opened, open, open_watched, write};

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

/// The one test of the system's watcher on `extensions/`: the others tell the host of their
/// saves. An agent makes a folder for new code after the project opened, and saves in it again.
#[test]
fn a_save_in_a_folder_made_after_the_project_opened_is_heard() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().to_path_buf();
    write(&root, "project.json", &project(Some("hum"), false));
    write(&root, "extensions/theme.json", "{}");
    write(&root, "extensions/lib/level.ts", &level(0.5));
    write(&root, "extensions/hum.ts", &hum("lib/level"));
    write(&root, "state/hum.json", r#"{"tool": "hum", "state": {}}"#);
    let mut opened = open_watched(folder);
    // The watcher needs a moment before it sees changes.
    std::thread::sleep(Duration::from_millis(200));
    let at = |level: f32| {
        move |opened: &mut crate::support::Opened| (opened.loudest(4_800) - level).abs() < 0.02
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

/// An effect `name` that keeps the last note of the MIDI keyboard in its record and plays at
/// half the level with `half`. When it `hangs`, its tick hangs once the engine runs: its check
/// at load runs at time 0, so it loads.
fn noted(name: &str, hangs: bool) -> String {
    let tick = match hangs {
        true => "tick: ({ time }) => { while (time > 0) {} },",
        false => "",
    };
    format!(
        r#"import {{ choice, input, number, tool }} from "./sdk";
export const {name} = tool({{
  name: "{name}", title: "{name}", when: "Never", doc: "Keeps the last note.",
  state: {{ last: number({{ min: 0, max: 127, default: 0 }}), mode: choice({{ options: ["full", "half"], default: "full" }}) }},
  sound: ({{ mode }}) => (mode === "half" ? input.times(0.5) : input),
  {tick}
  onNotes: ({{ update }}, event) => {{
    if (event.type !== "noteOn") return;
    update("Keep the note", ({{ state }}) => {{ state.last = event.pitch; }});
  }},
}});
"#
    )
}

/// A project whose instance `heard`, if any, plays to the output, with the built-in
/// `instrument` when `instrument`.
fn project(heard: Option<&str>, instrument: bool) -> String {
    let connections = heard.map_or(String::new(), |heard| {
        format!(r#"{{"from": {{"instance": "{heard}", "port": "audio"}}, "to": {{"device_output": 0}}}}"#)
    });
    let extensions = if instrument { r#""instrument""# } else { "" };
    format!(
        r#"{{"format": 1, "extensions": [{extensions}],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{connections}]}}"#
    )
}

/// A project of `tools`, each a file `extensions/<name>.ts` with an instance `<name>` at the
/// top, where the keyboard plays with nothing selected; `heard` plays to the output.
fn open_tools(heard: Option<&str>, tools: &[(&str, &str)]) -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", &project(heard, false));
    write(root, "extensions/theme.json", "{}");
    for (name, code) in tools {
        write(root, &format!("extensions/{name}.ts"), code);
        let record = format!(r#"{{"tool": "{name}", "state": {{}}}}"#);
        write(root, &format!("state/{name}.json"), &record);
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

#[test]
fn a_tool_whose_tick_hangs_is_a_problem_the_others_play_on_and_a_fix_brings_it_back() {
    let (heard, stuck) = (noted("heard", false), noted("stuck", true));
    let mut opened = open_tools(None, &[("heard", &heard), ("stuck", &stuck)]);
    opened.project.engine().play();
    opened.render(64);
    // The window does not wait for a Bun that hangs: a poll is a frame.
    for _ in 0..10 {
        let started = std::time::Instant::now();
        opened.poll();
        assert!(started.elapsed() < Duration::from_millis(200));
        std::thread::sleep(Duration::from_millis(50));
    }
    opened.until("the hang among the problems", |opened| {
        problems(opened)
            .iter()
            .any(|problem| problem.path == "extensions/stuck.ts")
    });
    // A new Bun runs the other tools.
    opened.play_midi(64);
    opened.until("the note in the other tool", |opened| {
        opened.record("heard")["last"] == 64
    });

    write(
        opened.folder.path(),
        "extensions/stuck.ts",
        &noted("stuck", false),
    );
    opened.tools.code_changed();
    opened.until("the fix", |opened| problems(opened).is_empty());
    opened.play_midi(65);
    opened.until("the note in the fixed tool", |opened| {
        opened.record("stuck")["last"] == 65
    });
}

/// A loop that never ends but keeps saying things, here moving a number input, is a hang of
/// its tool too.
#[test]
fn a_tick_that_never_ends_while_it_plays_is_a_problem() {
    let flood = r#"import { input, number, tool } from "./sdk";
export const flood = tool({
  name: "flood", title: "Flood", when: "Never", doc: "Its loop never ends and moves an input all the time.",
  state: {}, inputs: { x: number({ min: 0, max: 1, default: 0 }) },
  sound: ({ x }) => input.times(x),
  tick: ({ time, send }) => { while (time > 0) send("x", 0); },
});
"#;
    let mut opened = open_tools(None, &[("flood", flood)]);
    opened.start_playing();
    opened.until("the flood among the problems", |opened| {
        problems(opened)
            .iter()
            .any(|problem| problem.path == "extensions/flood.ts")
    });
}

/// Code a timer of a tool runs between two requests hangs Bun: the request that waits then is
/// not to blame, and no tool is. No tool runs until the code is saved again.
#[test]
fn a_hang_in_code_a_timer_runs_blames_no_tool() {
    let timer = r#"import { input, tool } from "./sdk";
export const timer = tool({
  name: "timer", title: "Timer", when: "Never", doc: "Its loop starts a timer that never ends.",
  state: {}, memory: () => ({ armed: false }), sound: () => input,
  tick: ({ memory, time }) => {
    if (time > 0 && !memory.armed) {
      memory.armed = true;
      setTimeout(() => { while (true) {} }, 0);
    }
  },
});
"#;
    let heard = noted("heard", false);
    let mut opened = open_tools(None, &[("heard", &heard), ("timer", timer)]);
    opened.start_playing();
    opened.until("the hang among the problems", |opened| {
        !problems(opened).is_empty()
    });
    let problems = problems(&opened);
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/");
    assert!(problem.message.contains("no tool to blame"), "{problem:?}");
    opened.play_midi(64);
    for _ in 0..20 {
        opened.settle();
    }
    assert_eq!(opened.record("heard")["last"], Value::Null);
}

/// An agent renames a tool in its code: the records of the old name have no code now.
#[test]
fn a_tool_gone_from_the_code_is_a_problem_and_its_records_play_on() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().to_path_buf();
    write(&root, "project.json", &project(Some("hum"), false));
    write(&root, "extensions/theme.json", "{}");
    write(&root, "extensions/lib/level.ts", &level(0.5));
    write(&root, "extensions/hum.ts", &hum("lib/level"));
    write(&root, "state/hum.json", r#"{"tool": "hum", "state": {}}"#);
    let mut opened = open(folder, Devices::default());
    assert!((opened.loudest(4_800) - 0.5).abs() < 0.02);

    write(
        &root,
        "extensions/hum.ts",
        &hum("lib/level").replace("\"hum\"", "\"drone\""),
    );
    opened.tools.code_changed();
    opened.until("the problem of the gone tool", |opened| {
        problems(opened).iter().any(|problem| {
            problem.path == "extensions/hum.ts" && problem.message.contains("gone from the code")
        })
    });
    for _ in 0..10 {
        opened.settle();
    }
    assert_eq!(problems(&opened).len(), 1, "{:?}", problems(&opened));
    assert!((opened.loudest(4_800) - 0.5).abs() < 0.02);
}

const SYNTH: &str = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;

/// A tool `pads` whose control loop runs `once` the first time it steps, with a child `synth`
/// it plays into. Saved again with no loop, its code holds and sends nothing.
fn pads(once: Option<&str>) -> String {
    let tick = once.map_or(String::new(), |body| {
        format!(
            "memory: () => ({{ done: false }}),\n  tick: ({{ memory, send, time }}) => {{ if (memory.done) return; memory.done = true; {body} }},"
        )
    });
    format!(
        r#"import {{ tool }} from "./sdk";
const ON = {{ type: "noteOn", pitch: 60, velocity: 0.8 }} as const;
const TO = {{ child: "synth", notes: "notes" }} as const;
export const pads = tool({{
  name: "pads", title: "Pads", when: "Never", doc: "Holds notes on its synth.", state: {{}},
  behaviour: () => ({{ connections: [{{ from: {{ child: "synth" }}, to: {{ port: "audio" }} }}] }}),
  {tick}
}});
"#
    )
}

fn open_pads(once: &str) -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    write(root, "project.json", &project(Some("pads"), true));
    write(root, "extensions/theme.json", "{}");
    write(root, "extensions/pads.ts", &pads(Some(once)));
    write(
        root,
        "state/pads/instance.json",
        r#"{"tool": "pads", "state": {}}"#,
    );
    write(root, "state/pads/synth.json", SYNTH);
    open(folder, Devices::default())
}

impl Opened {
    /// Saves the code of `pads` without its loop, and waits for the new Bun.
    fn save_pads(&mut self) {
        let loads = self.tools.loads();
        write(self.folder.path(), "extensions/pads.ts", &pads(None));
        self.tools.code_changed();
        self.until("the new Bun", |opened| opened.tools.loads() > loads);
    }
}

/// The new Bun knows nothing of what the old one held, and still each note it held ends.
#[test]
fn a_pitch_code_holds_twice_on_a_child_ends_with_a_new_bun() {
    let mut opened = open_pads("send(TO, ON); send(TO, ON);");
    opened.until("the note", |opened| opened.loudest(480) > 0.05);
    opened.save_pads();
    opened.until("the release of the note", |opened| {
        opened.loudest(4_800) == 0.0
    });
}

/// What old code sent for later is not played by the new one: a bang 5 s ahead that would
/// start noise that never ends.
#[test]
fn a_bang_old_code_sent_for_later_does_not_fire_with_a_new_bun() {
    let hits = |saved: bool| {
        let tick = match saved {
            true => "",
            false => {
                r#"memory: () => ({ done: false }),
  tick: ({ memory, send, time, update }) => {
    if (memory.done) return;
    memory.done = true;
    send("hit", { at: time + 5 });
    update("Send", ({ state }) => void (state.sent = true));
  },"#
            }
        };
        format!(
            r#"import {{ bang, boolean, feedback, max, noise, tool }} from "./sdk";
export const hits = tool({{
  name: "hits", title: "Hits", when: "Never", doc: "A hit later starts noise.", state: {{ sent: boolean({{ default: false }}) }},
  inputs: {{ hit: bang() }},
  sound: ({{ hit }}) => {{ const level = feedback(); level.set(max(hit, level)); return noise().times(level).times(0.2); }},
  {tick}
}});
"#
        )
    };
    let mut opened = open_tools(Some("hits"), &[("hits", &hits(false))]);
    opened.start_playing();
    opened.until("the hit sent", |opened| {
        opened.record("hits")["sent"] == true
    });
    let loads = opened.tools.loads();
    write(opened.folder.path(), "extensions/hits.ts", &hits(true));
    opened.tools.code_changed();
    opened.until("the new Bun", |opened| opened.tools.loads() > loads);
    // Past the frame the old code sent its bang for.
    opened.render(6 * 48_000);
    assert_eq!(opened.loudest(4_800), 0.0);
}

/// A child deleted and made again is another instrument: the note the old code held on the
/// first does not end what the second plays.
#[test]
fn a_child_made_again_keeps_its_note_when_the_code_that_held_one_on_the_old_child_ends() {
    let mut opened = open_pads("send(TO, ON);");
    opened.until("the note", |opened| opened.loudest(480) > 0.05);
    let synth = crate::support::id("pads/synth");
    let mut changes = Changes::new();
    changes.delete(&synth);
    opened.project.commit("Delete the synth", changes).unwrap();
    opened.until("the child gone", |opened| opened.loudest(4_800) == 0.0);
    opened.project.undo().unwrap();
    opened.until("the child back", |opened| {
        opened.project.tool_of(&synth).is_some()
    });
    opened.selected = Some(synth);
    opened.play_midi(60);
    opened.until("the note of the keyboard", |opened| {
        opened.loudest(480) > 0.05
    });
    opened.save_pads();
    opened.render(4_800);
    assert!(opened.loudest(4_800) > 0.05);
}

/// A player of `sine.wav` at `level`, over and over.
fn player(level: f32) -> String {
    format!(
        r#"import {{ file, phasor, tool }} from "./sdk";
export const player = tool({{
  name: "player", title: "Player", when: "Never", doc: "Plays a sound file over and over.",
  state: {{ sound: file() }},
  sound: ({{ sound }}) => sound.at(phasor(1).times(sound.length)).times({level}),
}});
"#
    )
}

/// Writes a second of a 441 Hz sine at `level` to `assets/audio/sine.wav`.
fn write_sine(folder: &std::path::Path, level: f32) {
    let path = folder.join("assets/audio/sine.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..44_100 {
        let sample = (frame as f32 * 441.0 / 44_100.0 * std::f32::consts::TAU).sin() * level;
        writer.write_sample((sample * f32::from(i16::MAX)) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// A save of the code keeps the sample a tool plays and does not read its file again.
#[test]
fn a_save_of_a_tool_keeps_its_sample_playing_and_does_not_read_it_again() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().to_path_buf();
    write(&root, "project.json", &project(Some("player"), false));
    write(&root, "extensions/theme.json", "{}");
    write(&root, "state/player.json", r#"{"tool": "player", "state": {"sound": "sine.wav"}}"#);
    write(&root, "extensions/player.ts", &player(1.0));
    write_sine(&root, 0.5);
    let mut opened = open(folder, Devices::default());
    let at = |level: f32| move |opened: &mut Opened| (opened.loudest(4_800) - level).abs() < 0.02;
    opened.until("the sine", at(0.5));

    // The file turns silent with the same size and time: only a new read would hear that.
    let sine = root.join("assets/audio/sine.wav");
    let modified = std::fs::metadata(&sine).unwrap().modified().unwrap();
    write_sine(&root, 0.0);
    let file = std::fs::File::options().write(true).open(&sine).unwrap();
    file.set_modified(modified).unwrap();

    // The save plays the new sound, still with the sine it read at the start.
    write(&root, "extensions/player.ts", &player(0.5));
    opened.tools.code_changed();
    opened.until("the saved tool at half the level", at(0.25));
}
