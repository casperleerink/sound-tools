//! Gain, pan and mute of a track, rendered offline with numbers. The probe holds a level for
//! as long as its note sounds, so the samples are what the mixer did to it and nothing else.

use arrangement::{RAMP_SECONDS, TrackState, channel_gains};
use sound_core::EngineConfig;

use crate::support::{Harness, SAMPLE_RATE, clip, id, note};

/// Ticks in a bar of 4/4.
const BAR: u64 = 3840;

/// The one note every test here plays. The probe puts out its pitch as a level.
const PITCH: u8 = 60;
const LEVEL: f32 = 60.0;

const TRACK_FILE: &str = "state/arrangement/piano/instance.json";

/// A stereo project with one track holding one long note, playing from its first frame.
fn playing() -> Harness {
    let mut harness = Harness::with_config(EngineConfig::new(SAMPLE_RATE, 2));
    harness.add_track("piano", 1.0);
    let mut changes = sound_core::Changes::new();
    changes.create(
        id("arrangement/piano/long"),
        clip(0, 8 * BAR, vec![note(0, 8 * BAR, PITCH)]),
    );
    harness.project.commit("Add clip", changes).unwrap();
    harness.project.engine().play();
    harness
}

/// The record an agent writes, with the fields it names.
fn track_record(fields: &str) -> String {
    format!(r#"{{"tool": "arrangement.track", "state": {{"name": "piano"{fields}}}}}"#)
}

/// Renders `frames` frames and gives the two channels apart.
fn render(harness: &mut Harness, frames: usize) -> (Vec<f32>, Vec<f32>) {
    split(&harness.render(frames * 2))
}

fn split(interleaved: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let channel = |first: usize| interleaved.iter().skip(first).step_by(2).copied().collect();
    (channel(0), channel(1))
}

/// The loudest sample.
fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
}

/// The largest step from one sample to the next.
fn largest_step(samples: &[f32]) -> f32 {
    samples.windows(2).fold(0.0_f32, |largest, pair| {
        largest.max((pair[1] - pair[0]).abs())
    })
}

#[test]
fn a_track_in_the_middle_leaves_every_sample_as_its_instrument_made_it() {
    let mut harness = playing();
    let (left, right) = render(&mut harness, 2_000);
    // Exactly, not nearly: a project from before the mixer existed sounds the same.
    assert_eq!(peak(&left), LEVEL);
    assert_eq!(left, right);
    assert_eq!(
        channel_gains(&TrackState::new("piano", Default::default(), 0)),
        [1.0, 1.0]
    );
}

#[test]
fn hard_left_leaves_the_right_channel_silent_and_keeps_the_loudness() {
    let mut harness = playing();
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "pan": -1.0"#));
    // Past the ramp of the change, which fades the right channel out and does not cut it.
    let (left, right) = render(&mut harness, 8_000);
    let settled = 2_000..8_000;
    assert_eq!(peak(&right[settled.clone()]), 0.0);
    let left = peak(&left[settled]);
    assert!(
        (left - LEVEL * std::f32::consts::SQRT_2).abs() < 1e-3,
        "{left}"
    );

    // Hard right is its mirror.
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "pan": 1.0"#));
    let (left, right) = render(&mut harness, 8_000);
    assert_eq!(peak(&left[2_000..]), 0.0);
    assert!((peak(&right[2_000..]) - LEVEL * std::f32::consts::SQRT_2).abs() < 1e-3);
}

#[test]
fn the_pan_law_keeps_the_power_from_one_end_to_the_other() {
    let mut track = TrackState::new("piano", Default::default(), 0);
    for step in 0..=200 {
        track.pan = step as f32 / 100.0 - 1.0;
        let [left, right] = channel_gains(&track);
        let power = left * left + right * right;
        // Two, because the middle is 1 in each channel.
        assert!((power - 2.0).abs() < 1e-5, "pan {}: {power}", track.pan);
    }
    // The ends are exact, and the middle leaves the samples alone.
    track.pan = -1.0;
    assert_eq!(channel_gains(&track), [std::f32::consts::SQRT_2, 0.0]);
    track.pan = 1.0;
    assert_eq!(channel_gains(&track), [0.0, std::f32::consts::SQRT_2]);
    track.pan = 0.0;
    assert_eq!(channel_gains(&track), [1.0, 1.0]);
}

#[test]
fn six_decibels_down_scales_every_sample_by_half_the_power() {
    let mut harness = playing();
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "gain_db": -6.0"#));
    let (left, right) = render(&mut harness, 8_000);
    // -6 dB is 10 to the power -0.3: 0.5012 of the samples, half the power.
    let expected = LEVEL * 0.501_187_2;
    assert!(
        (peak(&left[2_000..]) - expected).abs() < 1e-3,
        "{}",
        peak(&left)
    );
    assert_eq!(left, right);

    // And 6 dB up is the other way.
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "gain_db": 6.0"#));
    let (left, _) = render(&mut harness, 8_000);
    let expected = LEVEL * 1.995_262_3;
    assert!((peak(&left[2_000..]) - expected).abs() < 1e-3);
}

#[test]
fn a_muted_track_is_silent_and_unmuting_brings_it_back() {
    let mut harness = playing();
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "mute": true"#));
    let (left, right) = render(&mut harness, 8_000);
    let settled = 2_000..8_000;
    assert_eq!(peak(&left[settled.clone()]), 0.0);
    assert_eq!(peak(&right[settled]), 0.0);

    harness.write_and_apply(TRACK_FILE, &track_record(r#", "mute": false"#));
    let (left, _) = render(&mut harness, 8_000);
    assert_eq!(peak(&left[2_000..]), LEVEL);
}

#[test]
fn a_change_in_the_middle_of_a_render_ramps_and_takes_no_step_larger_than_the_bound() {
    let ramp_frames = RAMP_SECONDS * SAMPLE_RATE as f32;
    // The whole distance over the ramp, and one frame of slack for the block edges.
    let bound = LEVEL * std::f32::consts::SQRT_2 / ramp_frames * 1.001;

    for fields in [
        r#", "mute": true"#,
        r#", "gain_db": -60.0"#,
        r#", "pan": -1.0"#,
        r#", "gain_db": 6.0"#,
    ] {
        let mut harness = playing();
        let (before, _) = render(&mut harness, 2_000);
        assert_eq!(largest_step(&before), 0.0);
        harness.write_and_apply(TRACK_FILE, &track_record(fields));
        let (after, other) = render(&mut harness, 4_000);
        assert!(
            largest_step(&after) <= bound,
            "{fields}: step {} above {bound}",
            largest_step(&after)
        );
        assert!(largest_step(&other) <= bound, "{fields}: the other channel");
        // The ramp is over well inside the render: the last thousand frames are steady.
        assert_eq!(largest_step(&after[3_000..]), 0.0, "{fields}");
    }
}

#[test]
fn a_mixer_edit_is_one_undo_step_and_undo_brings_the_sound_back() {
    let mut harness = playing();
    let (before, _) = render(&mut harness, 2_000);
    harness.write_and_apply(
        TRACK_FILE,
        &track_record(r#", "gain_db": -12.0, "pan": 0.5"#),
    );
    let (changed, _) = render(&mut harness, 8_000);
    assert!(peak(&changed[2_000..]) < peak(&before));

    harness.project.undo().unwrap();
    let (back, right) = render(&mut harness, 8_000);
    assert_eq!(peak(&back[2_000..]), LEVEL);
    // Past the ramp back to the middle, where the two channels are one again.
    assert_eq!(back[2_000..], right[2_000..]);
    // The file is what it was: no mixer fields at all.
    let file = std::fs::read_to_string(harness.path(TRACK_FILE)).unwrap();
    assert!(
        file.contains(r#""gain_db": 0.0, "pan": 0.0, "mute": false"#),
        "{file}"
    );
}

#[test]
fn a_value_out_of_range_names_the_field_and_the_track_plays_on() {
    let mut harness = playing();
    render(&mut harness, 1_000);
    harness.write_and_apply(TRACK_FILE, &track_record(r#", "gain_db": 12.0"#));
    assert_eq!(
        harness.problems(),
        ["state/arrangement/piano/instance.json: state: gain_db must be from -60 to 6, not 12"]
    );
    let (left, _) = render(&mut harness, 2_000);
    assert_eq!(peak(&left), LEVEL);

    harness.write_and_apply(TRACK_FILE, &track_record(r#", "pan": -2.0"#));
    assert_eq!(
        harness.problems(),
        ["state/arrangement/piano/instance.json: state: pan must be from -1 to 1, not -2"]
    );
}
