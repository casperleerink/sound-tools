//! The building blocks a parent of the project plays with: a `timeline` whose pedal and
//! expression lanes move with its notes, a `lane` that moves a number of another child over the
//! song, and a `region` that plays an audio file at its place in it.

use crate::support::{BAR, Harness, difference, write, write_samples};

const BAR_TICKS: u64 = 3840;

const TOOLS: &str = r#"import { note, sine, tool, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", gain: "utility", filter: "filter", level: "lane", chord: "timeline" } as const;

export const keys = tool({
  name: "keys",
  title: "Keys",
  when: "Never",
  doc: "Plays its chord on its synth, through its gain, which its lane moves, and a filter.",
  state: {},
  children: parts,
  behaviour: (): Declaration<typeof parts> => ({
    connections: [
      { from: { child: "chord", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "gain" } },
      { from: { child: "gain" }, to: { child: "filter" } },
      { from: { child: "level", number: "value" }, to: { child: "gain", number: "gain_db" } },
      { from: { child: "filter" }, to: { port: "audio" } },
    ],
  }),
});

const drums = { drums: "drum-pad", late: "filter", line: "timeline" } as const;

export const beat = tool({
  name: "beat",
  title: "Beat",
  when: "Never",
  doc: "Plays its timeline on its drums, through a filter.",
  state: {},
  children: drums,
  behaviour: (): Declaration<typeof drums> => ({
    connections: [
      { from: { child: "line", notes: "notes" }, to: { child: "drums", notes: "notes" } },
      { from: { child: "drums" }, to: { child: "late" } },
      { from: { child: "late" }, to: { port: "audio" } },
    ],
  }),
});

export const pure = tool({
  name: "pure",
  title: "Pure",
  when: "Never",
  doc: "A sine at the newest note, bent as the note is.",
  state: {},
  sound: () => sine(note.freq).times(note.gate).times(0.5),
});

const bent = { line: "timeline", voice: pure } as const;

export const bender = tool({
  name: "bender",
  title: "Bender",
  when: "Never",
  doc: "Plays its timeline on its sine.",
  state: {},
  children: bent,
  behaviour: (): Declaration<typeof bent> => ({
    connections: [
      { from: { child: "line", notes: "notes" }, to: { child: "voice", notes: "notes" } },
      { from: { child: "voice" }, to: { port: "audio" } },
    ],
  }),
});

const takes = { "*": "region" } as const;

export const tape = tool({
  name: "tape",
  title: "Tape",
  when: "Never",
  doc: "Plays its regions.",
  state: {},
  children: takes,
  behaviour: ({ children }): Declaration<typeof takes> => ({
    connections: children.map((child) => ({ from: { child: child.name }, to: { port: "audio" } })),
  }),
});
"#;

/// A project of one tool at the top, `name`, with these records under it.
fn project(name: &str, records: &[(&str, String)]) -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = format!(
        r#"{{"format": 1, "extensions": ["instrument", "utility", "filter", "drum-pad"],
  "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
  "connections": [{{"from": {{"instance": "{name}", "port": "audio"}}, "to": {{"device_output": 0}}}}]}}"#
    );
    write(root, "project.json", &project);
    write(root, "extensions/tools.ts", TOOLS);
    write(
        root,
        &format!("state/{name}/instance.json"),
        &format!(r#"{{"tool": "{name}", "state": {{}}}}"#),
    );
    for (path, record) in records {
        write(root, &format!("state/{name}/{path}"), record);
    }
    Harness::open(folder)
}

/// The keys, whose lane holds `points`.
fn keys(points: &str) -> Harness {
    let chord = r#"{"tool": "timeline", "state": {"length": 7680, "notes": [{"start": 0, "length": 7680, "pitch": 48, "velocity": 100}, {"start": 0, "length": 7680, "pitch": 64, "velocity": 100}]}}"#;
    let records = [
        (
            "synth.json",
            r#"{"tool": "instrument.synth", "state": {}}"#.to_string(),
        ),
        (
            "gain.json",
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ),
        (
            "filter.json",
            r#"{"tool": "filter", "state": {"cutoff_hz": 2000}}"#.to_string(),
        ),
        ("chord.json", chord.to_string()),
        (
            "level.json",
            format!(r#"{{"tool": "lane", "state": {{"points": {points}}}}}"#),
        ),
    ];
    let harness = project("keys", &records);
    assert_eq!(harness.project.problems(), []);
    harness
}

/// A lane opens the gain from -36 dB to 0 dB between two ticks inside a block. The keys sound as
/// with the gain held at -36 dB up to the frame it starts to open, and as with it held at 0 dB
/// once the gain has glided there: the lane is led by the latency of the filter after the gain,
/// as the gain is, so it opens on its frame and not that latency late.
#[test]
fn a_lane_moves_a_number_of_another_child_on_its_frames() {
    let (start, end) = (BAR_TICKS + 1, BAR_TICKS + 3);
    let gate = format!(r#"[{{"tick": {start}, "value": -36}}, {{"tick": {end}, "value": 0}}]"#);
    let mut gated = keys(&gate);
    let gated = gated.play_from_the_start(2 * BAR);
    let held = |value: f32| {
        let mut harness = keys(&format!(r#"[{{"tick": 0, "value": {value}}}]"#));
        harness.play_from_the_start(2 * BAR)
    };
    let (closed, open) = (held(-36.0), held(0.0));
    assert!(closed.iter().any(|sample| sample.abs() > 1e-4));
    // 25 frames a tick at 120 bpm and 48 kHz.
    let (first, _) = difference(&gated, &closed).unwrap();
    assert!((BAR + 25..BAR + 30).contains(&first), "{first}");
    // The utility glides every change over 20 ms, 960 frames.
    let (_, last) = difference(&gated, &open).unwrap();
    assert!(last < BAR + 75 + 1_000, "{last}");
}

/// A bender whose timeline holds A4 for a bar, with `lanes` beside its notes.
fn bender(lanes: &str) -> Harness {
    let line = format!(
        r#"{{"tool": "timeline", "state": {{"length": 3840, "notes": [{{"start": 0, "length": 3840, "pitch": 69, "velocity": 100}}]{lanes}}}}}"#
    );
    let voice = r#"{"tool": "pure", "state": {}}"#.to_string();
    project("bender", &[("line.json", line), ("voice.json", voice)])
}

fn rises(samples: &[f32]) -> usize {
    let left: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let pairs = left.windows(2);
    pairs.filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0).count()
}

/// The bend lane of a timeline bends the notes it plays: a full bend is two semitones up. A
/// loop with lanes does not load, and says why.
#[test]
fn a_timeline_bends_its_notes_as_its_lane_says_and_a_loop_has_no_lanes() {
    let mut straight = bender("");
    let straight = straight.play_from_the_start(BAR / 2);
    // A second: 440 rises at A4, 494 two semitones up.
    assert!(
        (rises(&straight) as i64 - 440).abs() <= 2,
        "{}",
        rises(&straight)
    );
    let up = r#", "bend": [{"tick": 0, "value": 8191}]"#;
    let mut bent = bender(up);
    assert_eq!(bent.project.problems(), []);
    let bent = bent.play_from_the_start(BAR / 2);
    assert!((rises(&bent) as i64 - 494).abs() <= 2, "{}", rises(&bent));

    let looped = bender(&format!(r#", "loop": true{up}"#));
    let problems = looped.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("plays notes only")),
        "{problems:?}"
    );
}

/// A lane checks its record: points in tick order, and a log lane above 0.
#[test]
fn a_lane_whose_points_are_out_of_order_or_below_zero_on_a_log_scale_is_a_problem() {
    let mut harness = keys(r#"[{"tick": 0, "value": 0}]"#);
    let lane = |state: &str| format!(r#"{{"tool": "lane", "state": {state}}}"#);
    harness.write_and_apply(
        "state/keys/level.json",
        &lane(r#"{"points": [{"tick": 10, "value": 0}, {"tick": 5, "value": 1}]}"#),
    );
    let problems = harness.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("in tick order")),
        "{problems:?}"
    );
    harness.write_and_apply(
        "state/keys/level.json",
        &lane(r#"{"points": [{"tick": 0, "value": 0}], "scale": "log"}"#),
    );
    let problems = harness.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("above 0")),
        "{problems:?}"
    );
}

#[test]
fn a_timeline_that_ends_past_the_last_tick_is_a_problem() {
    let mut harness = keys(r#"[{"tick": 0, "value": 0}]"#);
    let far = format!(
        r#"{{"tool": "timeline", "state": {{"start": {}, "length": 1}}}}"#,
        u64::MAX
    );
    harness.write_and_apply("state/keys/chord.json", &far);
    let problems = harness.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("past the last tick")),
        "{problems:?}"
    );
}

/// A region plays its file from its start in the song, fading in over its fade, and is silent
/// before and after it. Its gain and its trim play as the record says.
#[test]
fn a_region_plays_its_file_at_its_place_with_its_fade_gain_and_trim() {
    let region = |extra: &str| {
        format!(
            r#"{{"tool": "region", "state": {{"asset": "steady.wav", "start": {BAR_TICKS}, "fade_in_ms": 10{extra}}}}}"#
        )
    };
    let mut harness = project("tape", &[("take.json", region(""))]);
    // Half a second of 0.5, then half a second of 0.25.
    write_samples(&harness, "steady.wav", 48_000, 1.0, |seconds| {
        match seconds < 0.5 {
            true => 0.5,
            false => 0.25,
        }
    });
    let file = harness.path("assets/audio/steady.wav");
    harness.apply(&[file]);
    assert_eq!(harness.project.problems(), []);
    let played = harness.play_from_the_start(2 * BAR);
    let left = |frame: usize| played[2 * frame];
    assert!((0..BAR).all(|frame| left(frame) == 0.0));
    // Halfway through the fade in, half the level; after it, the file as it is.
    assert!((left(BAR + 240) - 0.25).abs() < 0.01, "{}", left(BAR + 240));
    assert!(
        (left(BAR + 1_000) - 0.5).abs() < 1e-4,
        "{}",
        left(BAR + 1_000)
    );
    assert!(
        (left(BAR + 30_000) - 0.25).abs() < 1e-4,
        "{}",
        left(BAR + 30_000)
    );
    // Its end ramps down in 2 ms; after that, nothing.
    assert!((BAR + 48_000 + 200..2 * BAR).all(|frame| left(frame) == 0.0));

    // Trimmed to its second half and 6 dB quieter.
    harness.write_and_apply(
        "state/tape/take.json",
        &region(r#", "file_start_seconds": 0.5, "gain_db": -6.0206"#),
    );
    let played = harness.play_from_the_start(2 * BAR);
    let level = played[2 * (BAR + 1_000)];
    assert!((level - 0.125).abs() < 1e-4, "{level}");
    assert!((BAR + 24_000 + 200..2 * BAR).all(|frame| played[2 * frame] == 0.0));
}

/// A region whose file is not there is a problem that names the file; it plays once the file
/// arrives, without a change of its record.
#[test]
fn a_region_waits_for_its_file_and_plays_once_it_arrives() {
    let region =
        format!(r#"{{"tool": "region", "state": {{"asset": "later.wav", "start": {BAR_TICKS}}}}}"#);
    let mut harness = project("tape", &[("take.json", region)]);
    let problems = harness.project.problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("assets/audio/later.wav")),
        "{problems:?}"
    );
    write_samples(&harness, "later.wav", 48_000, 0.5, |_| 0.5);
    let file = harness.path("assets/audio/later.wav");
    harness.apply(&[file]);
    assert_eq!(harness.project.problems(), []);
    let played = harness.play_from_the_start(2 * BAR);
    assert!((played[2 * (BAR + 1_000)] - 0.5).abs() < 1e-4);
}

/// A beat on the drums, a kick on every beat for `beats` beats.
fn beat(beats: u64) -> Harness {
    let notes: Vec<String> = (0..beats)
        .map(|beat| {
            format!(
                r#"{{"start": {}, "length": 240, "pitch": 36, "velocity": 120}}"#,
                beat * 960
            )
        })
        .collect();
    let line = format!(
        r#"{{"tool": "timeline", "state": {{"length": {}, "notes": [{}]}}}}"#,
        beats * 960,
        notes.join(", ")
    );
    let drums =
        r#"{"tool": "drum-pad", "state": {"pads": {"36": {"sound": "kick", "decay_ms": 600}}}}"#;
    let records = [
        ("drums.json", drums.to_string()),
        (
            "late.json",
            r#"{"tool": "filter", "state": {}}"#.to_string(),
        ),
        ("line.json", line),
    ];
    let harness = project("beat", &records);
    assert_eq!(harness.project.problems(), []);
    harness
}

/// A bar of kicks on a loop sounds, up to where it goes back, as two bars of kicks with no
/// loop. The filter after the drums has latency, so the drums are ahead of the device: they go
/// back on their own frame. Before, they played on past the end of the bar, where the kick
/// that still rang was cut, one latency before the device went back: a click on every repeat.
#[test]
fn a_loop_sounds_up_to_its_end_as_what_it_repeats() {
    let mut looped = beat(4);
    let engine = looped.project.engine();
    engine.set_loop(Some(sound_core::Ticks(0)..sound_core::Ticks(BAR_TICKS)));
    let looped = looped.play_from_the_start(BAR + BAR / 4);
    let twice = beat(8).play_from_the_start(BAR + BAR / 4);
    let differ = crate::support::difference(&looped[..2 * BAR], &twice[..2 * BAR]);
    assert_eq!(differ, None);
}
