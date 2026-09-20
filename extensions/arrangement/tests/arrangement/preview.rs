//! The preview note: it sounds at once, also while the project is stopped, and its off always
//! comes, from the sequencer and not from the interface.

use arrangement::{PREVIEW_SECONDS, preview_note};
use sound_core::EngineConfig;
use sound_notes::{Pitch, Velocity};

use crate::support::{Harness, SAMPLE_RATE, TICK, clip, id, level_changes, note};

const PREVIEW_FRAMES: usize = (SAMPLE_RATE as f32 * PREVIEW_SECONDS) as usize;

fn preview(harness: &mut Harness, pitch: u8) {
    let (pitch, velocity) = (Pitch::new(pitch).unwrap(), Velocity::new(100).unwrap());
    preview_note(
        &mut harness.project,
        &id("arrangement/piano"),
        pitch,
        velocity,
    )
    .unwrap();
}

#[test]
fn a_preview_sounds_while_the_project_is_stopped_and_ends_by_itself() {
    let mut harness = Harness::with_clips(vec![]);
    preview(&mut harness, 64);
    let output = harness.render(SAMPLE_RATE as usize);
    // The off lands on the first block start after the time is over.
    let off = level_changes(&output)[1].0;
    assert_eq!(level_changes(&output), [(0, 64.0), (off, 0.0)]);
    assert!((PREVIEW_FRAMES..PREVIEW_FRAMES + 64 + 32).contains(&off));
}

#[test]
fn a_new_preview_ends_the_one_before_it() {
    let mut harness = Harness::with_clips(vec![]);
    let mut output = Vec::new();
    // A fast drag over five pitches, one per device buffer.
    for pitch in 60..65 {
        preview(&mut harness, pitch);
        output.extend(harness.render(480));
    }
    output.extend(harness.render(SAMPLE_RATE as usize));
    let changes = level_changes(&output);
    let levels: Vec<f32> = changes.iter().map(|(_, level)| *level).collect();
    // Never two at once: the probe would show the sum of their pitches.
    assert_eq!(levels, [60.0, 61.0, 62.0, 63.0, 64.0, 0.0]);
}

#[test]
fn two_previews_in_one_block_leave_nothing_sounding() {
    let mut harness = Harness::with_clips(vec![]);
    preview(&mut harness, 60);
    preview(&mut harness, 72);
    let output = harness.render(SAMPLE_RATE as usize);
    let levels: Vec<f32> = level_changes(&output).iter().map(|(_, l)| *l).collect();
    assert_eq!(levels, [72.0, 0.0]);
}

#[test]
fn a_preview_leaves_a_note_of_the_timeline_with_its_pitch_alone() {
    // The note sounds for 1920 ticks. The preview of the same pitch ends inside it.
    let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(0, 1920, 60)])]);
    let mut output = harness.play(480);
    preview(&mut harness, 60);
    output.extend(harness.render(1920 * TICK));
    // The probe counts two holders of pitch 60, then the one off of the note ends both.
    assert_eq!(
        level_changes(&output),
        [(0, 60.0), (480, 120.0), (1920 * TICK, 0.0)]
    );
}

#[test]
fn a_stop_ends_a_preview_with_everything_else() {
    let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(0, 1920, 48)])]);
    let mut output = harness.play(480);
    preview(&mut harness, 72);
    output.extend(harness.render(480));
    harness.project.engine().stop();
    output.extend(harness.render(SAMPLE_RATE as usize));
    let levels: Vec<f32> = level_changes(&output).iter().map(|(_, l)| *l).collect();
    assert_eq!(levels, [48.0, 120.0, 0.0]);
}

#[test]
fn a_preview_off_that_does_not_fit_comes_in_a_later_block() {
    // Room for one event per block: the off of the first preview takes the block, so the
    // second preview is dropped and counted, never stuck.
    let config = EngineConfig {
        event_capacity: 1,
        ..EngineConfig::new(SAMPLE_RATE, 1)
    };
    let mut harness = Harness::with_config(config).and_clips(vec![]);
    preview(&mut harness, 60);
    let mut output = harness.render_with_status(480).0;
    preview(&mut harness, 61);
    output.extend(harness.render_with_status(SAMPLE_RATE as usize).0);
    assert_eq!(output.last(), Some(&0.0));
    let levels: Vec<f32> = level_changes(&output).iter().map(|(_, l)| *l).collect();
    assert_eq!(levels.first(), Some(&60.0));
    assert!(!levels.contains(&121.0));
}

#[test]
fn a_preview_on_a_missing_track_is_an_error_and_not_a_panic() {
    let mut harness = Harness::with_clips(vec![]);
    let (pitch, velocity) = (Pitch::new(60).unwrap(), Velocity::new(100).unwrap());
    let sent = preview_note(
        &mut harness.project,
        &id("arrangement/gone"),
        pitch,
        velocity,
    );
    assert!(sent.is_err());
}
