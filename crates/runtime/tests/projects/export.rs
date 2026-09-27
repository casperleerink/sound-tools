//! Exporting audio: a render of the whole project, or of a range, runs on past its end until
//! the reverbs and releases have died away, and is the same every time.

use sound_core::Ticks;

use crate::support::{BAR, Harness, clip};

/// Ticks per bar in 4/4.
const BAR_TICKS: u64 = 3840;

/// A track with a chord on the last beat of a clip of two bars, through a reverb.
fn chord_through_reverb() -> Harness {
    let mut harness = Harness::new();
    let last_beat = 2 * BAR_TICKS - 960;
    let chord = [60, 64, 67].map(|pitch| (last_beat, 480, pitch));
    harness.write_track(
        "piano",
        1,
        0.3,
        &[("chord", clip(0, 2 * BAR_TICKS, &chord))],
    );
    let folder = "state/arrangement/piano";
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["room"]}}"#;
    let reverb = r#"{"tool": "reverb", "state": {"decay_seconds": 2.0, "mix": 0.5}}"#;
    let paths = [
        harness.write(&format!("{folder}/instance.json"), track),
        harness.write(&format!("{folder}/room.json"), reverb),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn export(harness: &mut Harness, from: Ticks, to: Ticks) -> Vec<f32> {
    let mut output = Vec::new();
    let problems = runtime::render_range(
        &mut harness.project,
        &mut harness.engine,
        &harness.plugins,
        from,
        to,
        |samples| {
            output.extend_from_slice(samples);
            Ok(())
        },
    )
    .unwrap();
    assert!(problems.is_empty());
    output
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn the_whole_project_renders_to_the_end_of_the_last_clip_and_its_tail() {
    let mut harness = chord_through_reverb();
    let end = runtime::project_end(&harness.project).unwrap();
    assert_eq!(end, Ticks(2 * BAR_TICKS));

    let rendered = export(&mut harness, Ticks(0), end);
    let frames = rendered.len() / 2;
    // The reverb still sounds at the end of the clip, so the render goes on past it, and it
    // stops once half a second is silent, well before the ten seconds a tail may take.
    assert!(peak(&rendered[2 * (2 * BAR - 4_800)..2 * 2 * BAR]) > 1e-3);
    assert!(frames > 2 * BAR + 24_000, "{frames}");
    assert!(frames < 2 * BAR + 10 * 48_000, "{frames}");
    assert!(peak(&rendered[rendered.len() - 2 * 24_000..]) < 3.2e-5);

    // The same bytes every time.
    let mut again = harness.reopen();
    assert_eq!(export(&mut again, Ticks(0), end), rendered);
}

#[test]
fn a_range_renders_from_its_start_and_stops_the_notes_at_its_end() {
    let mut harness = Harness::new();
    // A note held for two bars from bar 2: a range of one bar from there cuts it off, and
    // the synth's release is the tail.
    let long = [(BAR_TICKS, 2 * BAR_TICKS, 60)];
    harness.write_track("lead", 1, 0.3, &[("long", clip(0, 4 * BAR_TICKS, &long))]);

    let rendered = export(&mut harness, Ticks(BAR_TICKS), Ticks(2 * BAR_TICKS));
    let frames = rendered.len() / 2;
    // The note sounds from the first frame of the range.
    assert!(peak(&rendered[..2 * 4_800]) > 1e-3);
    // One bar, the release and half a second of silence: far less than the second bar the
    // note would still have played.
    assert!(frames > BAR + 24_000, "{frames}");
    assert!(frames < BAR + 48_000, "{frames}");
}

#[test]
fn a_project_without_clips_has_no_end() {
    let harness = Harness::new();
    assert_eq!(runtime::project_end(&harness.project), None);
}
