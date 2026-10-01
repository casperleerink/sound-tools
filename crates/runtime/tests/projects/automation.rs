//! Automation lanes in the record of a real track: a sweep of the cutoff of the built-in filter
//! and a fade of the volume of the track, heard where the points say, the same to the byte in
//! every render, and a lane that cannot play reported while the rest plays.

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const TRACK_FILE: &str = "state/arrangement/piano/instance.json";
const FILTER_FILE: &str = "state/arrangement/piano/tone.json";

/// Ticks in a bar of 4/4.
const BAR_TICKS: u64 = 3840;

/// A track that holds a chord for four bars and plays it through a filter named `tone`, with
/// these fields in its record after its name.
fn piano(fields: &str, filter: &str) -> Harness {
    let mut harness = Harness::new();
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "effects": ["tone"]{fields}}}}}"#
    );
    let filter = format!(r#"{{"tool": "filter", "state": {filter}}}"#);
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), &track),
        harness.write(FILTER_FILE, &filter),
    ];
    assert_eq!(harness.apply(&paths), 2);
    harness
}

/// A lane as an agent writes it: `(tick, value)` points, the value as JSON.
fn lane(device: Option<&str>, parameter: &str, points: &[(u64, &str)]) -> String {
    let points: Vec<String> = points
        .iter()
        .map(|(tick, value)| format!(r#"{{"tick": {tick}, "value": {value}}}"#))
        .collect();
    let device = device.map_or(String::new(), |device| format!(r#""device": "{device}", "#));
    format!(
        r#"{{{device}"parameter": "{parameter}", "points": [{}]}}"#,
        points.join(", ")
    )
}

fn automation(lanes: &[String]) -> String {
    format!(r#", "automation": [{}]"#, lanes.join(", "))
}

/// The frames from `from` to `to` of an interleaved stereo render.
fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

fn largest_difference(a: &[f32], b: &[f32]) -> f32 {
    let differences = a.iter().zip(b).map(|(a, b)| (a - b).abs());
    differences.fold(0.0, f32::max)
}

fn rms(samples: &[f32]) -> f32 {
    let power: f32 = samples.iter().map(|sample| sample * sample).sum();
    (power / samples.len() as f32).sqrt()
}

fn bytes(samples: &[f32]) -> Vec<u8> {
    let bytes = samples.iter().flat_map(|sample| sample.to_le_bytes());
    bytes.collect()
}

/// The cutoff holds 300 Hz up to bar 2, sweeps to 8 kHz at bar 3 and holds it. Where it holds,
/// the track sounds as a filter that was set to that value by hand, so the lane is at its
/// points there. The sweep itself sounds like neither.
#[test]
fn a_cutoff_sweep_plays_the_values_of_its_points_and_renders_the_same_every_time() {
    let sweep = lane(
        Some("tone"),
        "cutoff_hz",
        &[(BAR_TICKS, "300.0"), (2 * BAR_TICKS, "8000.0")],
    );
    let mut harness = piano(&automation(&[sweep]), "{}");
    assert_eq!(harness.project.problems(), []);
    let swept = harness.play_from_the_start(4 * BAR);

    let reference = |cutoff: &str| {
        let mut harness = piano("", &format!(r#"{{"cutoff_hz": {cutoff}}}"#));
        harness.play_from_the_start(4 * BAR)
    };
    let (dark, bright) = (reference("300.0"), reference("8000.0"));

    // 100 ms for the filter to settle, after the glide from its record value at the start and
    // after the end of the sweep.
    let settle = 4_800;
    let held_dark = largest_difference(frames(&swept, settle, BAR), frames(&dark, settle, BAR));
    assert!(held_dark < 1e-5, "{held_dark}");
    let held_bright = largest_difference(
        frames(&swept, 2 * BAR + settle, 4 * BAR),
        frames(&bright, 2 * BAR + settle, 4 * BAR),
    );
    assert!(held_bright < 1e-5, "{held_bright}");
    let sweeping = frames(&swept, BAR + settle, 2 * BAR - settle);
    assert!(difference(sweeping, frames(&dark, BAR + settle, 2 * BAR - settle)).is_some());
    assert!(difference(sweeping, frames(&bright, BAR + settle, 2 * BAR - settle)).is_some());

    // Reopened twice: the renders are the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play_from_the_start(4 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let second = harness.play_from_the_start(4 * BAR);
    assert_eq!(bytes(&first), bytes(&second));
    assert_eq!(bytes(&first), bytes(&swept));
}

/// The volume holds 0 dB up to bar 2 and fades to silence at bar 3, on the travel of the
/// fader. Before the fade every sample is the one of the track without the lane, halfway the
/// fader is halfway down, and after it the track is silent.
#[test]
fn a_volume_fade_plays_the_values_of_its_points_and_renders_the_same_every_time() {
    let fade = lane(
        None,
        "gain_db",
        &[(BAR_TICKS, "0.0"), (2 * BAR_TICKS, r#""-inf""#)],
    );
    let mut harness = piano(&automation(&[fade]), "{}");
    assert_eq!(harness.project.problems(), []);
    let faded = harness.play_from_the_start(4 * BAR);
    let full = piano("", "{}").play_from_the_start(4 * BAR);

    assert_eq!(bytes(frames(&faded, 0, BAR)), bytes(frames(&full, 0, BAR)));
    // Halfway down the fader from 0 dB, at 0.4 of its travel, is 61.94 × log10(0.5) dB:
    // about -18.6 dB.
    let expected = 10_f32.powf(61.94 * 0.5_f32.log10() / 20.0);
    let middle = BAR + BAR / 2;
    let window = |samples| rms(frames(samples, middle - 240, middle + 240));
    let heard = window(&faded) / window(&full);
    assert!(
        (heard - expected).abs() < expected * 0.01,
        "{heard} {expected}"
    );
    let after = frames(&faded, 2 * BAR + 64, 4 * BAR);
    assert!(after.iter().all(|sample| *sample == 0.0));
    assert!(rms(frames(&full, 2 * BAR, 4 * BAR)) > 0.01);

    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let again = harness.play_from_the_start(4 * BAR);
    assert_eq!(bytes(&again), bytes(&faded));
    // The record is written back as it was read: `-inf` stays the string.
    let record = std::fs::read_to_string(harness.path(TRACK_FILE)).unwrap();
    assert!(record.contains(r#""value": "-inf""#), "{record}");
}

/// A lane whose device has no such number, or that takes no automation, or whose values are
/// outside the range is reported by the field, and the lanes that can play play. A record
/// whose points are out of order does not load, and the track keeps what it had.
#[test]
fn a_lane_that_cannot_play_is_reported_and_the_rest_plays() {
    let lanes = [
        lane(Some("tone"), "cutoff", &[(0, "300.0")]),
        lane(Some("instrument"), "gain", &[(0, "0.5")]),
        lane(Some("tone"), "resonance", &[(0, "3.0")]),
        lane(Some("tone"), "cutoff_hz", &[(0, "300.0")]),
    ];
    let mut harness = piano(&automation(&lanes), "{}");
    let problems = harness.project.problems();
    let messages: Vec<&str> = problems
        .iter()
        .map(|problem| problem.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            r#"automation[0].parameter is "cutoff", and tone has no number of that name. It has cutoff_hz, resonance, drive_db, mix, lfo_rate_hz, lfo_depth_octaves, so the lane moves nothing"#,
            r#"automation[1].device is "instrument", and instrument.json takes no automation, so the lane moves nothing"#,
            "automation[2].points[0].value must be from 0 to 1, not 3, so the lane moves nothing",
        ]
    );
    let played = harness.play_from_the_start(BAR);
    let mut dark = piano("", r#"{"cutoff_hz": 300.0}"#);
    let dark = dark.play_from_the_start(BAR);
    let held = largest_difference(frames(&played, 4_800, BAR), frames(&dark, 4_800, BAR));
    assert!(held < 1e-5, "{held}");

    let unordered = lane(None, "pan", &[(960, "0.5"), (480, "-0.5")]);
    let track = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "effects": ["tone"]{}}}}}"#,
        automation(&[unordered])
    );
    assert_eq!(harness.write_and_apply(TRACK_FILE, &track), 0);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: automation[0].points[1].tick must be after the tick of the point before it, 960, not 480. The points of a lane are in tick order, one per tick"
    );
}
