//! What `--inspect` prints for a small known project.

use crate::support::{Harness, clip};

#[test]
fn the_summary_tells_what_plays_where() {
    let mut harness = Harness::piece();
    harness.write_and_apply(
        "state/arrangement/pad/bars-5-8.json",
        &clip(15360, 15360, &[(960, 480, 70), (1440, 480, 74)]),
    );
    harness.write_and_apply(
        "state/drone.json",
        r#"{"tool": "tone", "state": {"frequency_hz": 110.0, "gain": 0.1}}"#,
    );
    harness.write_and_apply("state/arrangement/pad/broken.json", "{");
    let project_file = std::fs::read_to_string(harness.path("project.json")).unwrap();
    let project_file = project_file.replace(
        r#"[{"tick": 0, "bpm": 120.0}]"#,
        r#"[{"tick": 0, "bpm": 120.0}, {"tick": 15360, "bpm": 90.0}]"#,
    );
    harness.write_and_apply("project.json", &project_file);

    // A second, read-only open next to the live one sees the same: this is `--inspect`.
    let (inspected, _engine, _plugins) = runtime::open_read_only(harness.project.root()).unwrap();
    let expected = r#"extensions: arrangement, instrument, plugin-host, tone
time signature: 4/4, 3840 ticks per bar, 960 ticks per beat
tempo: 120 bpm from 1:1:000 (tick 0)
tempo: 90 bpm from 5:1:000 (tick 15360)
arrangement `arrangement`: 3 tracks. Positions are bar:beat:tick, a clip runs up to its end position
  track `arrangement/track-1` "Track 1": colour blue, order 0, instrument instrument.synth
    no clips
  track `arrangement/piano` "piano": colour blue, order 1, instrument instrument.synth
    clip `arrangement/piano/chords`: 1:1:000 to 5:1:000, ticks 0 to 15360, 5 notes, pitch 48 to 64
  track `arrangement/pad` "pad": colour blue, order 2, instrument instrument.synth
    clip `arrangement/pad/long`: 1:1:000 to 5:1:000, ticks 0 to 15360, 1 note, pitch 72 to 72
    clip `arrangement/pad/bars-5-8`: 5:1:000 to 9:1:000, ticks 15360 to 30720, 2 notes, pitch 70 to 74
instance `drone` [tone] {"frequency_hz":110.0,"gain":0.1}
connections: 0
problems: 1
  state/arrangement/pad/broken.json: ?: EOF while parsing an object at line 1 column 1"#;
    assert_eq!(runtime::summary(&inspected), expected);
    assert_eq!(runtime::summary(&harness.project), expected);
}
