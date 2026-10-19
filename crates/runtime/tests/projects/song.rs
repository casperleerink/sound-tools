//! A song as tools of the project build one: a `song` whose children are tracks, and tracks
//! whose children are a synth and `timeline`s, the clips, each a record of its own that plays its
//! notes at their positions in the song.

use sound_core::Ticks;

use crate::owners::{closed, rendered_frames};
use crate::plugin_hosts::open_read_only;
use crate::support::{BAR, Harness, first_sound, write};

const TOOLS: &str = r#"import { tool, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", "*": "timeline" } as const;

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
        .filter((child) => child.tool === "timeline")
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

/// A clip, a `timeline` at `start`, with one note `at` ticks into it.
fn clip(start: u64, at: u64) -> String {
    let note = format!(r#"{{"start": {at}, "length": 240, "pitch": 60, "velocity": 100}}"#);
    format!(
        r#"{{"tool": "timeline", "state": {{"start": {start}, "length": 3840, "notes": [{note}]}}}}"#
    )
}

/// A song with the track `piano`, whose clip `verse` starts on the second bar with its note on
/// the second beat. A synth that lets go at once.
fn song() -> Harness {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "song", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/song.ts", TOOLS);
    write(
        root,
        "state/song/instance.json",
        r#"{"tool": "song", "state": {}}"#,
    );
    write(
        root,
        "state/song/piano/instance.json",
        r#"{"tool": "track", "state": {}}"#,
    );
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    write(root, "state/song/piano/synth.json", synth);
    write(root, "state/song/piano/verse.json", &clip(3840, 960));
    Harness::open(folder)
}

#[test]
fn a_render_plays_the_note_of_a_clip_of_a_track_at_its_place_in_the_song() {
    let harness = song();
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
    let mut harness = song();
    harness.write_and_apply("state/song/piano/verse.json", &clip(0, 960));
    let played = harness.play_from_the_start(BAR);
    assert_eq!(first_sound(&played, 0).map(|frame| frame / 48), Some(500));

    // A second track, whose clip plays on the third beat; the first still plays on the second.
    harness.write_and_apply(
        "state/song/bass/instance.json",
        r#"{"tool": "track", "state": {}}"#,
    );
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    harness.write_and_apply("state/song/bass/synth.json", synth);
    harness.write_and_apply("state/song/bass/line.json", &clip(0, 1920));
    let played = harness.play_from_the_start(BAR);
    let first = first_sound(&played, 0).unwrap();
    assert_eq!(first / 48, 500);
    assert_eq!(
        first_sound(&played, first + 12_000).map(|frame| frame / 48),
        Some(1000)
    );
    assert_eq!(harness.project.problems(), []);

    // A timeline with no start and no length plays from the start of the song.
    let free = r#"{"tool": "timeline", "state": {"notes": [{"start": 2880, "length": 240, "pitch": 60, "velocity": 100}]}}"#;
    harness.write_and_apply("state/song/bass/line.json", free);
    let played = harness.play_from_the_start(BAR);
    let first = first_sound(&played, 0).unwrap();
    assert_eq!(
        first_sound(&played, first + 12_000).map(|frame| frame / 48),
        Some(1500)
    );

    // 4000 ticks is after the end of a clip of 3840.
    harness.write_and_apply("state/song/piano/verse.json", &clip(0, 4000));
    let problems = harness.project.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "state/song/piano/verse.json");
    assert!(
        problem
            .message
            .contains("notes[0].start must be less than the length 3840"),
        "{problem:?}"
    );
}

/// Two clips of one track meet on a bar line with one pitch: the first ends the note on the
/// frame the second starts it. The end comes first, whichever clip connects first, so the new
/// note sounds on.
#[test]
fn a_note_that_ends_where_another_clip_starts_it_again_sounds_on() {
    for (first, second) in [("a", "b"), ("b", "a")] {
        let mut harness = song();
        let held = |start: u64, length: u64| {
            format!(
                r#"{{"tool": "timeline", "state": {{"start": {start}, "length": 3840, "notes": [{{"start": 0, "length": {length}, "pitch": 60, "velocity": 100}}]}}}}"#
            )
        };
        harness.write_and_apply("state/song/piano/verse.json", &held(7680, 240));
        harness.write_and_apply(&format!("state/song/piano/{first}.json"), &held(0, 3840));
        harness.write_and_apply(&format!("state/song/piano/{second}.json"), &held(3840, 960));
        assert_eq!(harness.project.problems(), []);
        let played = harness.play_from_the_start(2 * BAR);
        // A bar is 96 000 frames; a twentieth of a second into the second clip, it sounds.
        let later = 96_000 + 2_400;
        assert_eq!(
            first_sound(&played, later),
            Some(later),
            "{first} then {second}"
        );
    }
}

/// The song ends where its last clip stops, which is what a render and an export play to. A
/// loop and a cue that waits for a launch play for as long as someone lets them, so neither
/// makes the song longer.
#[test]
fn the_song_ends_where_its_last_clip_stops_and_a_loop_or_a_cue_adds_nothing() {
    let mut harness = song();
    assert_eq!(harness.project.end(), Some(Ticks(2 * 3840)));
    let looped = r#"{"tool": "timeline", "state": {"start": 38400, "length": 3840, "loop": true}}"#;
    harness.write_and_apply("state/song/piano/groove.json", looped);
    let cue = r#"{"tool": "timeline", "state": {"start": 38400, "length": 3840, "plays": false}}"#;
    harness.write_and_apply("state/song/piano/cue.json", cue);
    assert_eq!(harness.project.end(), Some(Ticks(2 * 3840)));
    // With no length, as long as its notes.
    let free = r#"{"tool": "timeline", "state": {"start": 7680, "notes": [{"start": 960, "length": 480, "pitch": 60, "velocity": 100}]}}"#;
    harness.write_and_apply("state/song/piano/tail.json", free);
    let end = Ticks(7680 + 1440);
    assert_eq!(harness.project.end(), Some(end));

    // What `--render` and an export play: from the start to that end, then the tail. Its last
    // note sounds in it.
    let folder = closed(harness);
    let (mut project, mut engine, plugins) = open_read_only(folder.path());
    assert_eq!(project.end(), Some(end));
    let mut output = Vec::new();
    let problems = runtime::render_range(
        &mut project,
        &mut engine,
        &plugins,
        Ticks(0),
        end,
        |samples| {
            output.extend_from_slice(samples);
            Ok(())
        },
    )
    .unwrap();
    assert!(problems.is_empty());
    // 7680 + 960 ticks is 4.5 s at 120 bpm: the last note starts there.
    let last = first_sound(&output, 216_000 - 480).unwrap();
    assert!((216_000..216_048).contains(&last), "{last}");
}

/// `--only song/bass` plays the bass alone: the piano, which plays in the second bar, is not
/// heard, and no file changes.
#[test]
fn only_one_track_of_the_song_is_heard_and_no_file_changes() {
    let mut harness = song();
    harness.write_and_apply(
        "state/song/bass/instance.json",
        r#"{"tool": "track", "state": {}}"#,
    );
    let synth = r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#;
    harness.write_and_apply("state/song/bass/synth.json", synth);
    harness.write_and_apply("state/song/bass/line.json", &clip(0, 1920));
    let folder = closed(harness);
    let project_file = std::fs::read_to_string(folder.path().join("project.json")).unwrap();

    let (mut project, mut engine, plugins) = open_read_only(folder.path());
    runtime::listen_only(&mut project, "song/bass").unwrap();
    project.engine().play();
    let played = runtime::render(&mut project, &mut engine, &plugins, 2 * BAR).unwrap();
    // The bass on the third beat of the first bar, and nothing in the second bar.
    let first = first_sound(&played, 0).unwrap();
    assert_eq!(first / 48, 1000);
    assert_eq!(first_sound(&played, first + 12_000), None);
    assert!(runtime::listen_only(&mut project, "song/drums").is_err());
    drop((project, engine));
    let after = std::fs::read_to_string(folder.path().join("project.json")).unwrap();
    assert_eq!(after, project_file);
}
