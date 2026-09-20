//! An agent with only file access adds a part and a whole track while the project plays.

use crate::support::{Harness, TICK, clip, clip_json, id, level_changes, note};

const BAR: usize = 3840 * TICK;

/// Two tracks that play: the piano a long note per bar, the pad one note over four bars.
fn two_tracks() -> Harness {
    let bars = (0..4).map(|bar| note(bar * 3840, 3000, 60 + bar as u8));
    let mut harness = Harness::with_clips(vec![clip(0, 4 * 3840, bars.collect())]);
    harness.add_track("pad", 200.0);
    let pad = clip(0, 4 * 3840, vec![note(0, 4 * 3840, 40)]);
    harness.write_and_apply("state/arrangement/pad/long.json", &clip_json(&pad));
    harness
}

/// What one track plays, from the sum of all tracks. The scales are 1, 200 and 40000: a level
/// stays below 200, and the sum stays a whole number that an `f32` holds exactly.
fn part(output: &[f32], scale: u32) -> Vec<f32> {
    let part = |sample: &f32| ((*sample as u64 / u64::from(scale)) % 200) as f32;
    output.iter().map(part).collect()
}

#[test]
fn a_clip_file_written_during_playback_sounds_in_its_bars_and_undo_removes_it() {
    let mut quiet = two_tracks();
    let expected = quiet.play(4 * BAR);

    let mut harness = two_tracks();
    let mut output = harness.play(BAR);
    // A part in bars 3 and 4 of the pad, written while bar 2 is about to play.
    let part_clip = clip(
        2 * 3840,
        2 * 3840,
        vec![note(0, 960, 52), note(3840, 960, 55)],
    );
    let file = "state/arrangement/pad/agent-part.json";
    assert_eq!(harness.write_and_apply(file, &clip_json(&part_clip)), 1);
    assert!(harness.problems().is_empty());
    output.extend(harness.render(3 * BAR));

    // The piano plays exactly what it plays without the edit.
    assert_eq!(part(&output, 1), part(&expected, 1));
    assert_eq!(
        level_changes(&part(&output, 200)),
        [
            (0, 40.0),
            (2 * BAR, 92.0),
            (2 * BAR + 960 * TICK, 40.0),
            (3 * BAR, 95.0),
            (3 * BAR + 960 * TICK, 40.0),
            // The last frame of the render is the last tick of the pad note.
        ]
    );

    // One undo step takes the part away: the file is gone and the bars play as before.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert!(!harness.path(file).exists());
    harness.project.engine().seek(sound_core::Ticks(0));
    assert_eq!(harness.render(4 * BAR), expected);
}

#[test]
fn a_track_folder_written_during_playback_adds_the_track_and_deleting_it_removes_it() {
    let mut quiet = two_tracks();
    let expected = quiet.play(4 * BAR);

    let mut harness = two_tracks();
    let mut output = harness.play(BAR);
    let folder = "state/arrangement/bass";
    let bass_clip = clip(2 * 3840, 3840, vec![note(0, 1920, 36)]);
    for (file, contents) in [
        (
            "instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "Bass", "colour": "green", "order": 2}}"#.to_string(),
        ),
        (
            "instrument.json",
            r#"{"tool": "test.probe", "state": {"scale": 40000.0}}"#.to_string(),
        ),
        ("line.json", clip_json(&bass_clip)),
    ] {
        let path = harness.path(folder).join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    assert_eq!(harness.apply(&[folder]), 3);
    assert!(harness.problems().is_empty());
    output.extend(harness.render(2 * BAR));

    // The other tracks never stop.
    assert_eq!(part(&output, 1), part(&expected[..3 * BAR], 1));
    assert_eq!(part(&output, 200), part(&expected[..3 * BAR], 200));
    assert_eq!(
        level_changes(&part(&output, 40_000)),
        [(2 * BAR, 36.0), (2 * BAR + 1920 * TICK, 0.0)]
    );

    // Undo removes the whole folder as one step, redo writes it again.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert!(!harness.path(folder).join("instance.json").exists());
    assert!(
        harness
            .project
            .resolve::<arrangement::TrackState>(&id("arrangement/bass"))
            .is_none()
    );
    harness.project.redo().unwrap();
    assert!(harness.path(folder).join("line.json").exists());

    // Deleting the folder from outside removes the track, also as one step.
    std::fs::remove_dir_all(harness.path(folder)).unwrap();
    assert_eq!(harness.apply(&[folder]), 3);
    harness.project.engine().seek(sound_core::Ticks(0));
    assert_eq!(harness.render(4 * BAR), expected);
    harness.project.undo().unwrap();
    assert!(harness.path(folder).join("instrument.json").exists());
    harness.project.engine().seek(sound_core::Ticks(0));
    let restored = harness.render(4 * BAR);
    assert_eq!(
        level_changes(&part(&restored, 40_000)),
        [(2 * BAR, 36.0), (2 * BAR + 1920 * TICK, 0.0)]
    );
}

#[test]
fn moving_a_clip_file_to_another_track_moves_its_notes_to_the_other_instrument() {
    let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(960, 960, 60)])]);
    harness.add_track("bass", 1000.0);
    assert_eq!(
        level_changes(&harness.play(BAR)),
        [(960 * TICK, 60.0), (1920 * TICK, 0.0)]
    );

    let (from, to) = (
        "state/arrangement/piano/clip-0.json",
        "state/arrangement/bass/clip-0.json",
    );
    std::fs::rename(harness.path(from), harness.path(to)).unwrap();
    assert_eq!(harness.apply(&[from, to]), 2);
    harness.project.engine().seek(sound_core::Ticks(0));
    assert_eq!(
        level_changes(&harness.render(BAR)),
        [(960 * TICK, 60_000.0), (1920 * TICK, 0.0)]
    );

    // One step back: the file and the notes are with the piano again.
    harness.project.undo().unwrap();
    assert!(harness.path(from).exists() && !harness.path(to).exists());
    harness.project.engine().seek(sound_core::Ticks(0));
    assert_eq!(
        level_changes(&harness.render(BAR)),
        [(960 * TICK, 60.0), (1920 * TICK, 0.0)]
    );
}
