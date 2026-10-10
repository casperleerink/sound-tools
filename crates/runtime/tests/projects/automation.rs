//! Automation lanes in the record of a real track: a sweep of the cutoff of the built-in filter
//! and a fade of the volume of the track, heard where the points say, the same to the byte in
//! every render, a lane of every other built-in device heard, and a lane that cannot play
//! reported while the rest plays.

use std::f64::consts::TAU;

use sound_core::{InstanceId, Ticks};

use crate::support::{BAR, Harness, clip, difference, write_samples};

const FOLDER: &str = "state/arrangement/piano";
const TRACK_FILE: &str = "state/arrangement/piano/instance.json";
const FILTER_FILE: &str = "state/arrangement/piano/tone.json";
const INSTRUMENT_FILE: &str = "state/arrangement/piano/instrument.json";

/// Ticks in a bar of 4/4.
const BAR_TICKS: u64 = 3840;

/// A track that holds a chord for four bars and plays it through a filter named `tone`, with
/// these fields in its record after its name.
fn piano(fields: &str, filter: &str) -> Harness {
    track(Harness::new(), fields, None, &record("filter", filter))
}

/// A track of `harness` that holds a chord for four bars on a synth, or on this instrument
/// record, and plays it through this effect record named `tone`, with these fields in its
/// record after its name.
fn track(mut harness: Harness, fields: &str, instrument: Option<&str>, effect: &str) -> Harness {
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "order": 1, "effects": ["tone"]{fields}}}}}"#
    );
    let mut paths = vec![
        harness.write(&format!("{FOLDER}/instance.json"), &track),
        harness.write(FILTER_FILE, effect),
    ];
    paths.extend(instrument.map(|instrument| harness.write(INSTRUMENT_FILE, instrument)));
    // An instrument record may hold what the synth of the track already holds.
    assert!(harness.apply(&paths) >= 2);
    harness
}

fn record(tool: &str, state: &str) -> String {
    format!(r#"{{"tool": "{tool}", "state": {state}}}"#)
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

    // From the first frame: the lane takes its value at once when the render starts, with no
    // glide from the record. After the sweep the filter takes 100 ms to settle.
    let settle = 4_800;
    let held_dark = largest_difference(frames(&swept, 0, BAR), frames(&dark, 0, BAR));
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

/// The views are shown what the lanes play, at any tick: the cutoff halfway up its sweep on
/// the knob, and the volume of the track itself. A track whose lanes are gone shows none.
#[test]
fn the_views_are_shown_the_value_each_lane_plays() {
    let sweep = lane(
        Some("tone"),
        "cutoff_hz",
        &[(BAR_TICKS, "300.0"), (2 * BAR_TICKS, "8000.0")],
    );
    let fade = lane(None, "gain_db", &[(0, "-6.0")]);
    let mut harness = piano(&automation(&[sweep, fade]), "{}");
    assert_eq!(harness.project.problems(), []);
    let values = |harness: &Harness, instance: &str, tick: u64| {
        let instance = InstanceId::new(instance).unwrap();
        let lanes = harness.project.lanes(&instance);
        lanes.map(|lanes| {
            let mut values = Vec::new();
            lanes.values_at(Ticks(tick), &mut values);
            let values = values.into_iter();
            let values = values.map(|(field, value)| (field.to_string(), value));
            values.collect::<Vec<_>>()
        })
    };
    let middle = BAR_TICKS + BAR_TICKS / 2;
    let tone = values(&harness, "arrangement/piano/tone", middle).unwrap();
    let [(ref field, cutoff)] = tone[..] else {
        panic!("{tone:?}");
    };
    assert_eq!(field, "cutoff_hz");
    assert!((cutoff - (300_f32 * 8000.).sqrt()).abs() < 0.1, "{cutoff}");
    // On the travel of the fader and back, as the mixer hears it.
    let track = values(&harness, "arrangement/piano", 0).unwrap();
    let [(ref field, gain)] = track[..] else {
        panic!("{track:?}");
    };
    assert_eq!(field, "gain_db");
    assert!((gain + 6.).abs() < 1e-4, "{gain}");

    let plain = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["tone"]}}"#;
    assert_eq!(harness.write_and_apply(TRACK_FILE, plain), 1);
    assert_eq!(values(&harness, "arrangement/piano/tone", 0), None);
    assert_eq!(values(&harness, "arrangement/piano", 0), None);
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

/// A fade in from silence starts silent: the lane is at `-inf` from the first frame of the
/// render, and does not glide down from the record's 0 dB.
#[test]
fn a_fade_in_from_silence_starts_silent() {
    let fade = lane(None, "gain_db", &[(0, r#""-inf""#), (BAR_TICKS, "0.0")]);
    let mut harness = piano(&automation(&[fade]), "{}");
    assert_eq!(harness.project.problems(), []);
    let faded = harness.play_from_the_start(BAR);
    let full = piano("", "{}").play_from_the_start(BAR);
    let start = |samples| rms(frames(samples, 0, 256));
    assert!(start(&full) > 0.001, "{}", start(&full));
    assert!(start(&faded) < start(&full) * 1e-3, "{}", start(&faded));
}

/// Every built-in instrument and effect takes a lane: a lane that holds a value from the first
/// frame sounds as the record set to that value, and not as the record. A number inside an
/// object or a list is named by its path.
#[test]
fn a_lane_of_every_built_in_device_sounds_as_its_record_set_to_that_value() {
    // The tool, its number and the value of the lane, the record, and the record set to it.
    let devices = [
        (
            "instrument.synth",
            "cutoff_hz",
            "300.0",
            "{}",
            r#"{"cutoff_hz": 300.0}"#,
        ),
        (
            "wavetable",
            "filter_1.cutoff_hz",
            "300.0",
            "{}",
            r#"{"filter_1": {"cutoff_hz": 300.0}}"#,
        ),
        (
            "drum-pad",
            "pads.48.volume_db",
            "-24.0",
            "{}",
            r#"{"pads": {"48": {"volume_db": -24.0}}}"#,
        ),
        (
            "sampler",
            "gain_db",
            "-12.0",
            r#"{"sample": "tone.wav"}"#,
            r#"{"sample": "tone.wav", "gain_db": -12.0}"#,
        ),
        (
            "compressor",
            "threshold_db",
            "-40.0",
            "{}",
            r#"{"threshold_db": -40.0}"#,
        ),
        ("delay", "mix", "1.0", "{}", r#"{"mix": 1.0}"#),
        (
            "eq",
            "bands[1].gain_db",
            "12.0",
            "{}",
            r#"{"bands": [{}, {"gain_db": 12.0}]}"#,
        ),
        (
            "gate",
            "threshold_db",
            "0.0",
            "{}",
            r#"{"threshold_db": 0.0}"#,
        ),
        ("limiter", "gain_db", "12.0", "{}", r#"{"gain_db": 12.0}"#),
        ("modulation", "mix", "1.0", "{}", r#"{"mix": 1.0}"#),
        ("reverb", "mix", "1.0", "{}", r#"{"mix": 1.0}"#),
        (
            "saturator",
            "drive_db",
            "24.0",
            "{}",
            r#"{"drive_db": 24.0}"#,
        ),
        ("utility", "pan", "-1.0", "{}", r#"{"pan": -1.0}"#),
    ];
    let instruments = ["instrument.synth", "wavetable", "drum-pad", "sampler"];
    for (tool, parameter, value, state, set) in devices {
        let is_instrument = instruments.contains(&tool);
        let render = |lanes: &str, state: &str| {
            let harness = Harness::new();
            // What the sampler plays.
            let tone = |time: f64| (0.25 * (TAU * 220.0 * time).sin()) as f32;
            write_samples(&harness, "tone.wav", 48_000, 3.0, tone);
            let device = record(tool, state);
            let mut harness = match is_instrument {
                true => track(harness, lanes, Some(&device), &record("filter", "{}")),
                false => track(harness, lanes, None, &device),
            };
            assert_eq!(harness.project.problems(), [], "{tool}");
            harness.play_from_the_start(BAR)
        };
        let device = if is_instrument { "instrument" } else { "tone" };
        let held = lane(Some(device), parameter, &[(0, value)]);
        let automated = render(&automation(&[held]), state);
        let heard = largest_difference(&automated, &render("", set));
        assert!(heard < 1e-4, "{tool} {parameter}: {heard}");
        let moved = largest_difference(&automated, &render("", state));
        assert!(moved > 1e-2, "{tool} {parameter}: {moved}");
    }
}

/// Unmuting a utility while a lane moves its gain glides over the 20 ms of an edit, and does
/// not jump in the next block: the moves of the lane right after the edit end with its glide.
#[test]
fn unmuting_while_a_gain_lane_moves_glides_over_twenty_milliseconds() {
    let sweep = lane(
        Some("tone"),
        "gain_db",
        &[(0, "-12.0"), (4 * BAR_TICKS, "0.0")],
    );
    let lanes = automation(&[sweep]);
    let utility = |mute: bool| record("utility", &format!(r#"{{"mute": {mute}}}"#));
    let mut muted = track(Harness::new(), &lanes, None, &utility(true));
    muted.play_from_the_start(BAR);
    assert_eq!(muted.write_and_apply(FILTER_FILE, &utility(false)), 1);
    let unmuted = muted.render(2_000);
    let mut open = track(Harness::new(), &lanes, None, &utility(false));
    let reference = open.play_from_the_start(BAR + 2_000);
    let reference = frames(&reference, BAR, BAR + 2_000);
    let part = |from, to| rms(frames(&unmuted, from, to)) / rms(frames(reference, from, to));
    let start = part(0, 64);
    assert!(start < 0.1, "{start}");
    let halfway = part(440, 520);
    assert!((halfway - 0.5).abs() < 0.1, "{halfway}");
    let after = part(1_000, 2_000);
    assert!((after - 1.0).abs() < 1e-3, "{after}");
}

/// A gate: from `closed` to `open` in two ticks, 50 frames at 120 BPM, from tick 3841. It starts
/// at frame 25 and ends at frame 75 of the second bar, both inside a block of 64 frames.
pub(crate) fn gate(device: Option<&str>, parameter: &str, closed: &str, open: &str) -> String {
    let (start, end) = (BAR_TICKS + 1, BAR_TICKS + 3);
    lane(device, parameter, &[(start, closed), (end, open)])
}

/// How far a render through a [`gate`] is from its frames: the frames that sound before it
/// opens, and the frames after it is open that differ from the sound with it open. A lane that
/// bends only on the edge of a block sounds early; one that glides arrives late.
pub(crate) fn off_the_gate(gated: &[f32], open: &[f32]) -> (usize, usize) {
    let (start, end) = (BAR + 25, BAR + 75);
    let early = frames(gated, 0, start).chunks(2);
    let early = early.filter(|frame| frame.iter().any(|sample| *sample != 0.0));
    let after = frames(gated, end, end + 4_800).chunks(2);
    let after = after.zip(frames(open, end, end + 4_800).chunks(2));
    let late = after.filter(|(gated, open)| largest_difference(gated, open) > 1e-6);
    (early.count(), late.count())
}

/// The volume of a track opens from silence to full in 50 frames, both ends inside a block: it
/// is silent up to the frame the lane opens, and full from the frame it is open.
#[test]
fn a_fast_volume_ramp_starts_and_ends_on_its_frames() {
    let fade = gate(None, "gain_db", r#""-inf""#, "0.0");
    let mut harness = piano(&automation(&[fade]), "{}");
    assert_eq!(harness.project.problems(), []);
    let gated = harness.play_from_the_start(2 * BAR);
    let open = piano("", "{}").play_from_the_start(2 * BAR);
    assert_eq!(off_the_gate(&gated, &open), (0, 0));
}

/// A lane whose device or track has no such number, or that takes no automation, or whose
/// values are outside the range is reported by the field, and the lanes that can play play. A record
/// whose points are out of order does not load, and the track keeps what it had. A sine of the
/// `tone` tool in the track folder takes no automation: it is not part of the arrangement.
#[test]
fn a_lane_that_cannot_play_is_reported_and_the_rest_plays() {
    let lanes = [
        lane(Some("tone"), "cutoff", &[(0, "300.0")]),
        lane(Some("drone"), "gain", &[(0, "0.5")]),
        lane(Some("tone"), "resonance", &[(0, "3.0")]),
        lane(Some("tone"), "cutoff_hz", &[(0, "300.0")]),
        lane(None, "volume", &[(0, "0.0")]),
        lane(None, "pan", &[(0, "3.0")]),
    ];
    let mut harness = piano(&automation(&lanes), "{}");
    let drone = r#"{"tool": "tone", "state": {"frequency_hz": 220.0, "gain": 0.0}}"#;
    assert_eq!(
        harness.write_and_apply(&format!("{FOLDER}/drone.json"), drone),
        1
    );
    let problems = harness.project.problems();
    let messages: Vec<&str> = problems
        .iter()
        .map(|problem| problem.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            r#"automation[0].parameter is "cutoff", and tone takes no automation of a number of that name. It takes cutoff_hz, resonance, drive_db, mix, lfo_rate_hz, lfo_depth_octaves, so the lane moves nothing"#,
            r#"automation[1].device is "drone", and drone.json takes no automation, so the lane moves nothing"#,
            "automation[2].points[0].value must be from 0 to 1, not 3, so the lane moves nothing",
            r#"automation[4].parameter is "volume", and the track takes no automation of a number of that name. It takes gain_db, pan, so the lane moves nothing"#,
            "automation[5].points[0].value must be from -1 to 1, not 3, so the lane moves nothing",
        ]
    );
    let played = harness.play_from_the_start(BAR);
    let mut dark = piano("", r#"{"cutoff_hz": 300.0}"#);
    let dark = dark.play_from_the_start(BAR);
    let held = largest_difference(frames(&played, 0, BAR), frames(&dark, 0, BAR));
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
