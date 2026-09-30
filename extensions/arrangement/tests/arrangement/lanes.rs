//! The expression lanes of a clip on playback: a straight line sent once per block, only when
//! it moved, found again after a seek, and at rest where no clip moves it.
//!
//! The probe shows where one lane stands instead of the notes, so the level of a frame is the
//! value the instrument has there.

use sound_core::Ticks;

use crate::support::{Harness, Shows, TICK, amount, bend, clip, clip_json, level_changes, note};

const CLIP: &str = "state/arrangement/piano/clip-0.json";
/// The device buffer of the harness, which the engine splits into blocks of 64 frames.
const BUFFER: usize = 480;
const BLOCK: usize = 64;

/// Where every block of the first `frames` frames starts and ends, as the engine splits the
/// device buffers of the harness.
fn blocks(frames: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..frames).step_by(BUFFER).flat_map(move |buffer| {
        let end = (buffer + BUFFER).min(frames);
        (buffer..end)
            .step_by(BLOCK)
            .map(move |start| (start, (start + BLOCK).min(end)))
    })
}

/// The level changes a lane gives when every block sends where the lane stands at its last
/// tick, given by `value_at`, and only when that moved. Playing from tick 0.
fn expected(frames: usize, value_at: impl Fn(u64) -> f32) -> Vec<(usize, f32)> {
    let mut changes = Vec::new();
    let mut level = 0.0;
    for (start, end) in blocks(frames) {
        // The ticks of a block are those that land on one of its frames.
        let last_tick = end.div_ceil(TICK) as u64 - 1;
        let value = value_at(last_tick);
        if value != level {
            level = value;
            changes.push((start, value));
        }
    }
    changes
}

/// A bend from the middle up to 7680 over 1920 ticks, 4 per tick, in a clip of two bars.
fn rising_bend() -> Vec<sound_notes::Point<sound_notes::Bend>> {
    vec![bend(0, 0), bend(1920, 7680)]
}

fn rising(tick: u64) -> f32 {
    (tick.min(1920) * 4) as f32
}

#[test]
fn a_lane_plays_as_a_straight_line_one_value_per_block() {
    let mut lane = clip(0, 7680, vec![]);
    lane.bend = rising_bend();
    let mut harness = Harness::new().and_clips_showing(Shows::Bend, vec![lane]);
    let frames = 2400 * TICK;
    let output = harness.play(frames);
    let changes = level_changes(&output);
    assert_eq!(changes, expected(frames, rising));
    // One value per block while it moves, and nothing once it holds.
    assert_eq!(changes.last().map(|change| change.1), Some(7680.0));
    assert!(changes.windows(2).all(|pair| pair[1].0 - pair[0].0 >= 32));
}

#[test]
fn a_lane_goes_back_to_rest_where_its_clip_ends() {
    let mut held = clip(0, 960, vec![]);
    held.mod_wheel = vec![amount(0, 100)];
    let mut harness = Harness::new().and_clips_showing(Shows::ModWheel, vec![held]);
    let frames = 1920 * TICK;
    let output = harness.play(frames);
    let wanted = expected(frames, |tick| if tick < 960 { 100.0 } else { 0.0 });
    assert_eq!(level_changes(&output), wanted);
    assert_eq!(wanted.len(), 2);
}

/// Playing from the middle of a lane starts where the lane stands there: a bend that is
/// already up plays bent from the first block.
#[test]
fn a_seek_into_a_lane_arrives_with_its_value() {
    let mut lane = clip(0, 7680, vec![]);
    lane.bend = rising_bend();
    lane.pressure = vec![amount(0, 90)];
    for (shows, value) in [(Shows::Bend, 960.0 * 4.0 + 8.0), (Shows::Pressure, 90.0)] {
        let mut harness = Harness::new().and_clips_showing(shows, vec![lane.clone()]);
        harness.project.engine().seek(Ticks(960));
        let output = harness.play(64);
        // The first block ends on tick 962.
        assert_eq!(level_changes(&output), [(0, value)], "{shows:?}");
    }
}

#[test]
fn a_stop_puts_the_lanes_at_rest_and_playing_again_finds_them() {
    let mut held = clip(0, 3840, vec![]);
    held.bend = vec![bend(0, -3000)];
    let mut harness = Harness::new().and_clips_showing(Shows::Bend, vec![held]);
    let mut output = harness.play(BUFFER);
    harness.project.engine().stop();
    output.extend(harness.render(BUFFER));
    assert_eq!(level_changes(&output), [(0, -3000.0), (BUFFER, 0.0)]);
    let again = harness.play(BUFFER);
    assert_eq!(level_changes(&again), [(0, -3000.0)]);
}

/// Where clips with a lane overlap, the one that started last owns it, and a clip without
/// points in that lane takes nothing from the others.
#[test]
fn of_overlapping_clips_the_one_that_started_last_owns_a_lane() {
    let mut long = clip(0, 3840, vec![]);
    long.mod_wheel = vec![amount(0, 10)];
    let mut short = clip(960, 960, vec![]);
    short.mod_wheel = vec![amount(0, 30)];
    let without = clip(1440, 960, vec![note(0, 480, 60)]);
    let mut harness = Harness::new().and_clips_showing(Shows::ModWheel, vec![long, short, without]);
    let frames = 4800 * TICK;
    let output = harness.play(frames);
    let wanted = expected(frames, |tick| match tick {
        960..1920 => 30.0,
        0..3840 => 10.0,
        _ => 0.0,
    });
    assert_eq!(level_changes(&output), wanted);
    assert_eq!(wanted.len(), 4);
}

/// On one frame the lanes go before the notes, so a note starts where the lanes are. Also the
/// first note of a clip that starts inside a block: the block sends where the lane is going.
#[test]
fn a_note_starts_where_the_lanes_are() {
    for start in [0, 10] {
        let mut bent = clip(start, 3840, vec![note(0, 480, 60)]);
        bent.bend = vec![bend(0, 5000)];
        let mut harness = Harness::new().and_clips_showing(Shows::BendOfLastOn, vec![bent]);
        let output = harness.play(960 * TICK);
        let on = start as usize * TICK;
        assert_eq!(level_changes(&output), [(on, 5000.0)], "clip at {start}");
    }
}

/// A preview note starts where the lanes are too: on the first block after a play, the lanes
/// go out before it.
#[test]
fn a_preview_starts_where_the_lanes_are() {
    let mut bent = clip(0, 3840, vec![]);
    bent.bend = vec![bend(0, -2000)];
    let mut harness = Harness::new().and_clips_showing(Shows::BendOfLastOn, vec![bent]);
    harness.project.engine().play();
    let (pitch, velocity) = (
        sound_notes::Pitch::new(60).unwrap(),
        sound_notes::Velocity::new(100).unwrap(),
    );
    let track = crate::support::id("arrangement/piano");
    arrangement::preview_note(&mut harness.project, &track, pitch, velocity).unwrap();
    let output = harness.render(BUFFER);
    assert_eq!(level_changes(&output), [(0, -2000.0)]);
}

/// An edit that takes a lane away while it plays puts it at rest at the next block.
#[test]
fn an_edit_that_removes_a_lane_puts_it_at_rest() {
    let mut held = clip(0, 3840, vec![]);
    held.pressure = vec![amount(0, 64)];
    let mut harness = Harness::new().and_clips_showing(Shows::Pressure, vec![held]);
    let mut output = harness.play(BUFFER);
    harness.write_and_apply(CLIP, &clip_json(&clip(0, 3840, vec![])));
    output.extend(harness.render(BUFFER));
    assert_eq!(level_changes(&output), [(0, 64.0), (BUFFER, 0.0)]);
}

/// The runtime writes a long lane one point per line, as it writes notes. A clip without lanes
/// keeps the fields out of its file, and a lane written by hand is not written back.
#[test]
fn the_runtime_writes_a_long_lane_one_point_per_line() {
    let mut recorded = clip(0, 3840, vec![]);
    recorded.bend = (0..8)
        .map(|index| bend(index * 480, index as i16 * 1000))
        .collect();
    let harness = Harness::with_clips(vec![recorded, clip(3840, 3840, vec![])]);
    assert_eq!(harness.problems(), Vec::<String>::new());
    let text = std::fs::read_to_string(harness.path(CLIP)).unwrap();
    assert!(
        text.contains("\n      {\"tick\": 480, \"value\": 1000},\n"),
        "{text}"
    );
    let plain = std::fs::read_to_string(harness.path("state/arrangement/piano/clip-1.json"));
    let plain = plain.unwrap();
    assert!(!plain.contains("bend") && !plain.contains("mod_wheel") && !plain.contains("pressure"));

    let mut harness = Harness::new();
    harness.add_track("piano", 1.0);
    let mut written = clip(0, 3840, vec![]);
    written.mod_wheel = vec![amount(0, 1), amount(960, 127)];
    let json = clip_json(&written);
    assert_eq!(harness.write_and_apply(CLIP, &json), 1);
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(harness.path(CLIP)).unwrap(), json);
}
