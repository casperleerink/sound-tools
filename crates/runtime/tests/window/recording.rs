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
    let take_file = opened.path("assets/takes/take-1.json");
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

/// Record, undo, record again: the most common thing a pianist does. The second take must not
/// write over the first performance. Both clips take the id `take`, because undo frees it, so
/// a take named after its clip would have truncated the first file.
#[gpui::test]
fn a_second_take_after_an_undo_leaves_the_first_one_untouched(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    let record = |opened: &mut Opened<'_>, pitch: u8| {
        opened.keys("r");
        opened.settle();
        opened.play_midi(on(pitch, 88));
        opened.render(12_000);
        opened.settle();
        opened.play_midi(off(pitch));
        opened.keys("r");
        opened.settle();
    };
    record(&mut opened, 60);
    let first = opened.path("assets/takes/take-1.json");
    let before = std::fs::read_to_string(&first).unwrap();
    assert!(before.contains(r#""pitch":60"#), "{before}");
    assert_eq!(
        take_clip(&mut opened).unwrap().take.as_deref(),
        Some("take-1")
    );

    opened.keys("cmd-z");
    assert_eq!(take_clip(&mut opened), None);
    record(&mut opened, 64);

    // The clip has the same id as the first one had, and a take of its own.
    let second = opened.path("assets/takes/take-2.json");
    assert_eq!(
        take_clip(&mut opened).unwrap().take.as_deref(),
        Some("take-2")
    );
    assert!(second.exists());
    assert!(
        std::fs::read_to_string(&second)
            .unwrap()
            .contains(r#""pitch":64"#),
        "the second take holds the second performance"
    );
    assert_eq!(std::fs::read_to_string(&first).unwrap(), before);
}

/// A clip that is dragged to another track is a delete and a create, so it gets a new id. The
/// take travels with it, because it is a field of the record and not the path.
#[gpui::test]
fn a_clip_dragged_to_another_track_still_names_its_take(cx: &mut gpui::TestAppContext) {
    let mut opened = open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
    });
    opened.settle();
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(60, 88));
    opened.render(24_000);
    opened.settle();
    opened.play_midi(off(60));
    opened.keys("r");
    opened.settle();
    let clip = take_clip(&mut opened).expect("the take became a clip");
    assert_eq!(clip.take.as_deref(), Some("take-1"));

    // Drag it from the first track row to the second.
    let from = opened.at(clip.start.0 + 480, 0);
    let to = opened.at(clip.start.0 + 480, 1);
    opened.drag(from, to);
    opened.settle();
    assert_eq!(opened.clip("arrangement/track-1/take"), None);
    let moved = opened
        .clip("arrangement/track-2/take")
        .expect("the clip moved to the other track");
    assert_eq!(moved.take.as_deref(), Some("take-1"));
    assert_eq!(moved.notes, clip.notes);
    let file = opened.clip_file("arrangement/track-2/take").unwrap();
    assert!(file.contains(r#""take": "take-1""#), "{file}");
}

/// The track is deleted while the take runs, so no clip can be made. The performance is still
/// written: it is the only copy of what the composer played.
#[gpui::test]
fn a_take_whose_track_goes_away_is_still_written(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("r");
    opened.settle();
    opened.play_midi(on(60, 88));
    opened.render(12_000);
    opened.settle();

    // The track goes, from outside, as an agent would delete it.
    let folder = opened.path("state/arrangement/track-1");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    opened.settle();
    assert!(opened.project(|project| {
        project
            .resolve::<Clip>(&id("arrangement/track-1/take"))
            .is_none()
    }));

    opened.keys("r");
    opened.settle();
    assert!(!opened.is_recording());
    let take = opened.path("assets/takes/take-1.json");
    assert!(take.exists(), "the performance was lost with its track");
    let text = std::fs::read_to_string(&take).unwrap();
    assert!(text.contains(r#""pitch":60"#), "{text}");
    assert_eq!(take_clip(&mut opened), None);
}

/// A project where another track has latency: pressing record plays, the playhead waits for
/// that latency first, and the take must go on and be written. A play from rest is no seek, so
/// the transport does not end the take as it would on a jump.
#[gpui::test]
fn a_recording_goes_on_in_a_project_with_latency(cx: &mut gpui::TestAppContext) {
    let mut opened = crate::support::open_with_test_plugin(cx, |_| {});
    let state = test_plugin_support::save_state(test_plugin_support::SavedState {
        latency: 700,
        ..Default::default()
    });
    let asset = opened.path("assets/plugin-state/late.bin");
    std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
    std::fs::write(&asset, state).unwrap();
    let folder = opened.path("state/arrangement/late");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("instance.json"),
        r#"{"tool": "arrangement.track", "state": {"name": "Late", "order": 1}}"#,
    )
    .unwrap();
    std::fs::write(
        folder.join("instrument.json"),
        crate::support::test_plugin_record(plugin_host::PluginFormat::Clap, "late"),
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened.settle();

    let record = opened.control("record");
    opened.click(record);
    opened.settle();
    assert!(opened.is_recording());
    opened.render(2_048);
    opened.settle();
    let latency = opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().poll().unwrap().latency)
    });
    assert_eq!(latency, 700);
    opened.play_midi(on(60, 88));
    opened.render(24_000);
    opened.settle();
    opened.play_midi(off(60));
    opened.render(2_048);
    opened.settle();
    assert!(opened.is_recording(), "the take ended by itself");

    opened.click(record);
    opened.settle();
    let clip = take_clip(&mut opened).expect("the take became a clip");
    assert_eq!(clip.notes.len(), 1);
    assert!(opened.path("assets/takes/take-1.json").exists());
}
