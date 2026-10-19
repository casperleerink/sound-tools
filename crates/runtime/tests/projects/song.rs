//! A song as tools of the project build one: a `song` whose children are tracks, and tracks
//! whose children are a synth and `clip`s, each clip a record of its own that plays its notes at
//! its place in the song.

use crate::owners::{closed, rendered_frames};
use crate::support::{BAR, Harness, first_sound, write};

const TOOLS: &str = r#"import { tool, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", "*": "clip" } as const;

export const track = tool({
  name: "track",
  title: "Track",
  when: "Never",
  doc: "Plays its clips on its synth.",
  state: {},
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => ({
    connections: [
      ...children
        .filter((child) => child.tool === "clip")
        .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } })),
      { from: { child: "synth" }, to: { port: "audio" } },
    ],
  }),
});

const tracks = { "*": track } as const;

tool({
  name: "song",
  title: "Song",
  when: "Never",
  doc: "Plays its tracks.",
  state: {},
  children: tracks,
  behaviour: ({ children }): Declaration<typeof tracks> => ({
    connections: children.map((child) => ({ from: { child: child.name }, to: { port: "audio" } })),
  }),
});
"#;

/// A clip at `start` with one note `at` ticks into it.
fn clip(start: u64, at: u64) -> String {
    let note = format!(r#"{{"start": {at}, "length": 240, "pitch": 60, "velocity": 100}}"#);
    format!(r#"{{"tool": "clip", "state": {{"start": {start}, "length": 3840, "notes": [{note}]}}}}"#)
}

/// A song with the track `piano`, whose clip `verse` starts on the second bar with its note on
/// the second beat. A synth that lets go at once.
fn song() -> Option<Harness> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "song", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/song.ts", TOOLS);
    write(root, "state/song/instance.json", r#"{"tool": "song", "state": {}}"#);
    write(root, "state/song/piano/instance.json", r#"{"tool": "track", "state": {}}"#);
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    write(root, "state/song/piano/synth.json", synth);
    write(root, "state/song/piano/verse.json", &clip(3840, 960));
    Some(Harness::open(folder))
}

#[test]
fn a_render_plays_the_note_of_a_clip_of_a_track_at_its_place_in_the_song() {
    let Some(harness) = song() else {
        return;
    };
    assert_eq!(harness.project.problems(), []);
    let played = rendered_frames(closed(harness).path(), 2 * BAR);
    // A bar, then a beat: 4800 ticks are 120 000 frames. Within a millisecond.
    let sound = first_sound(&played, 0).unwrap();
    assert!((120_000..120_048).contains(&sound), "{sound}");
    // The note is a sixteenth, 6 000 frames, and nothing plays after it.
    assert_eq!(first_sound(&played, sound + 12_000), None);
}

#[test]
fn a_clip_moves_alone_a_second_track_plays_and_a_note_outside_its_clip_is_a_problem() {
    let Some(mut harness) = song() else {
        return;
    };
    harness.write_and_apply("state/song/piano/verse.json", &clip(0, 960));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(500));

    // A second track, whose clip plays on the third beat; the first still plays on the second.
    harness.write_and_apply("state/song/bass/instance.json", r#"{"tool": "track", "state": {}}"#);
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    harness.write_and_apply("state/song/bass/synth.json", synth);
    harness.write_and_apply("state/song/bass/line.json", &clip(0, 1920));
    let played = harness.play_from_the_start(BAR);
    let first = first_sound(&played, 0).unwrap();
    assert_eq!(first / 48, 500);
    assert_eq!(first_sound(&played, first + 12_000).map(|frame| frame / 48), Some(1000));
    assert_eq!(harness.project.problems(), []);

    // 4000 ticks is after the end of a clip of 3840.
    harness.write_and_apply("state/song/piano/verse.json", &clip(0, 4000));
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "state/song/piano/verse.json");
    assert!(
        problem.message.contains("notes[0].start must be less than the clip length 3840"),
        "{problem:?}"
    );
}
