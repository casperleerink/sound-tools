//! The record control of the transport and the `r` key, with a simulated mouse and keyboard
//! and messages that a test sends through the MIDI input.

use midi::Played;
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::{Clip, Pedal, Pitch, Velocity};

use crate::support::{Opened, id, open_with};

fn on(pitch: u8, velocity: u8) -> Played {
    Played::On {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

fn off(pitch: u8) -> Played {
    Played::Off {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: 64,
    }
}

/// The default project: one track with a synth, nothing selected.
fn opened(cx: &mut gpui::TestAppContext) -> Opened<'_> {
    let mut opened = open_with(cx, |_| {});
    opened.settle();
    opened
}

/// The clip a recording made, when there is one.
fn take_clip(opened: &mut Opened<'_>) -> Option<Clip> {
    opened.clip("arrangement/track-1/take")
}

#[gpui::test]
fn the_record_button_records_what_is_played_into_a_clip_as_one_undo_step(
    cx: &mut gpui::TestAppContext,
) {
    let mut opened = opened(cx);
    let record = opened.control("record");
    opened.click(record);
    opened.settle();
    assert!(opened.is_recording());
    // Recording plays: a take needs the playhead to move.
    assert!(opened.playhead().playing);

    opened.play_midi(on(60, 88));
    opened.render(24_000);
    opened.settle();
    opened.play_midi(off(60));

    opened.click(record);
    opened.settle();
    assert!(!opened.is_recording());
    let clip = take_clip(&mut opened).expect("the take became a clip");
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].pitch.number(), 60);
    assert_eq!(clip.notes[0].velocity.value(), 88);
    assert_eq!(opened.undo_label(), Some("Record".to_string()));

    // One step: undo takes the whole clip away and leaves the raw take.
    let take_file = opened.path("assets/takes/arrangement/track-1/take.json");
    assert!(take_file.exists());
    opened.keys("cmd-z");
    assert_eq!(take_clip(&mut opened), None);
    assert!(take_file.exists(), "undo removed the raw take");
    assert_eq!(opened.undo_label(), None);

    opened.keys("shift-cmd-z");
    assert!(take_clip(&mut opened).is_some());
}

#[gpui::test]
fn the_r_key_starts_and_ends_a_recording(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("r");
    opened.settle();
    assert!(opened.is_recording());
    opened.play_midi(on(64, 100));
    opened.render(12_000);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(!opened.is_recording());
    let clip = take_clip(&mut opened).expect("the take became a clip");
    assert_eq!(clip.notes.len(), 1);
    // The key was still down: the note ends where the recording ended.
    assert_eq!(clip.notes[0].end(), clip.length.ticks());
}

/// A stop ends the take, so a composer who presses stop keeps what was played.
#[gpui::test]
fn a_stop_ends_the_take(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(60, 88));
    opened.render(12_000);
    opened.settle();
    let stop = opened.control("stop");
    opened.click(stop);
    opened.settle();
    assert!(!opened.is_recording());
    assert!(take_clip(&mut opened).is_some());
    assert_eq!(opened.playhead().tick, Ticks(0));
}

/// Nothing was played, so there is no clip and no file. An empty take is not music.
#[gpui::test]
fn a_recording_with_nothing_played_makes_no_clip_and_no_file(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("r");
    opened.settle();
    opened.render(12_000);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert_eq!(take_clip(&mut opened), None);
    assert!(!opened.path("assets/takes").exists());
    assert_eq!(opened.undo_label(), None);
}

/// The live input plays into the selected track, and a recording goes there too. With nothing
/// selected it is the first track, so a keyboard always sounds.
#[gpui::test]
fn the_keyboard_plays_into_the_selected_track(cx: &mut gpui::TestAppContext) {
    let mut opened = open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
    });
    opened.settle();
    assert_eq!(opened.selected_track(), None);

    // Nothing selected: the first track.
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(60, 88));
    opened.render(12_000);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(opened.clip("arrangement/track-1/take").is_some());
    assert_eq!(opened.clip("arrangement/track-2/take"), None);

    // Click the header of the second track, and the input follows it.
    let header = opened.track_header(1);
    opened.click(header);
    opened.settle();
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(64, 88));
    opened.render(12_000);
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(opened.clip("arrangement/track-2/take").is_some());
}

/// A take goes to the track it began on. Selecting another track while it runs moves neither
/// the sound nor the clip: the two would otherwise end up on different tracks.
#[gpui::test]
fn a_take_stays_on_the_track_it_began_on(cx: &mut gpui::TestAppContext) {
    let mut opened = open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
    });
    opened.settle();
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(60, 88));
    let header = opened.track_header(1);
    opened.click(header);
    opened.settle();
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
    opened.render(12_000);
    opened.settle();
    opened.play_midi(off(60));
    opened.keys("r");
    opened.settle();
    assert!(opened.clip("arrangement/track-1/take").is_some());
    assert_eq!(opened.clip("arrangement/track-2/take"), None);
}

/// The sustain pedal is recorded into the clip and shows in the file an agent reads.
#[gpui::test]
fn the_pedal_is_recorded_into_the_clip(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("r");
    opened.settle();
    opened.play_midi(Played::Pedal(Pedal::new(127).unwrap()));
    opened.play_midi(on(60, 88));
    opened.render(12_000);
    opened.settle();
    opened.play_midi(off(60));
    opened.render(12_000);
    opened.settle();
    opened.play_midi(Played::Pedal(Pedal::UP));
    opened.keys("r");
    opened.settle();

    let clip = take_clip(&mut opened).expect("the take became a clip");
    assert_eq!(clip.pedal.len(), 2);
    assert_eq!(clip.pedal[0].value.value(), 127);
    assert_eq!(clip.pedal[1].value, Pedal::UP);
    let file = opened.clip_file("arrangement/track-1/take").unwrap();
    assert!(file.contains(r#""pedal": ["#), "{file}");
}

/// A take over an existing clip is a new clip. Nothing is merged and nothing is overdubbed:
/// the clips overlap and both play, as the arrangement always allowed.
#[gpui::test]
fn a_take_over_an_existing_clip_is_a_new_clip(cx: &mut gpui::TestAppContext) {
    let mut opened = open_with(cx, |project| {
        let mut changes = Changes::new();
        let clip = crate::support::clip(0, 3840, vec![crate::support::note(0, 960, 48)]);
        changes.create(InstanceId::new("arrangement/track-1/part").unwrap(), clip);
        project.commit("Add clip", changes).unwrap();
    });
    opened.settle();
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(72, 88));
    opened.render(12_000);
    opened.settle();
    opened.play_midi(off(72));
    opened.keys("r");
    opened.settle();

    let existing = opened.clip("arrangement/track-1/part").unwrap();
    assert_eq!(existing.notes.len(), 1);
    assert_eq!(existing.notes[0].pitch.number(), 48);
    let take = take_clip(&mut opened).unwrap();
    assert_eq!(take.notes[0].pitch.number(), 72);
}
