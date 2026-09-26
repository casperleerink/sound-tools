//! Outside edits during playback, heard through the real synth. Renders are compared sample
//! by sample with a run that has no edit.

use sound_core::Ticks;

use crate::support::{BAR, Harness, clip, difference};

#[test]
fn the_default_project_is_a_small_musical_template() {
    let harness = Harness::new();
    let instances: Vec<(String, &str)> = harness
        .project
        .instances()
        .map(|(id, tool)| (id.to_string(), tool))
        .collect();
    assert_eq!(
        instances,
        [
            ("arrangement".to_string(), "arrangement"),
            ("arrangement/track-1".to_string(), "arrangement.track"),
            (
                "arrangement/track-1/instrument".to_string(),
                "instrument.synth"
            ),
        ]
    );
    let project_file = harness.project.project_file();
    assert_eq!(
        project_file.extensions,
        [
            "arrangement",
            "filter",
            "fit-tempo",
            "instrument",
            "plugin-host",
            "tone"
        ]
    );
    assert_eq!(project_file.tempo_map, sound_core::TempoMap::default());
    assert_eq!(project_file.connections, []);
    assert!(
        harness
            .path("state/arrangement/track-1/instrument.json")
            .exists()
    );
    // Making the default content is nothing to undo.
    assert_eq!(harness.project.undo_label(), None);
}

#[test]
fn a_folder_without_a_project_file_becomes_the_default_project_whatever_else_is_in_it() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".git")).unwrap();
    std::fs::write(folder.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(folder.path().join(".DS_Store"), [0_u8; 4]).unwrap();
    let harness = Harness::open(folder);
    assert_eq!(harness.project.instances().count(), 3);
    assert!(harness.path(".git/HEAD").exists());

    // An existing project is left as it is, also one with no instances.
    let mut harness = harness;
    let mut changes = sound_core::Changes::new();
    changes.delete(&sound_core::InstanceId::new("arrangement").unwrap());
    harness.project.commit("Delete all", changes).unwrap();
    let harness = harness.reopen();
    assert_eq!(harness.project.instances().count(), 0);

    // Records without a project file are someone's work, not a new project.
    let folder = tempfile::tempdir().unwrap();
    let drone = r#"{"tool": "tone", "state": {"frequency_hz": 110.0, "gain": 0.1}}"#;
    crate::support::write(folder.path(), "state/drone.json", drone);
    let harness = Harness::open(folder);
    let ids: Vec<String> = harness
        .project
        .instances()
        .map(|(id, _)| id.to_string())
        .collect();
    assert_eq!(ids, ["drone"]);
}

#[test]
fn a_clip_file_written_during_playback_sounds_in_its_bars_and_disturbs_nothing_else() {
    let expected = Harness::piece().play(5 * BAR);

    // The part goes on the empty first track, in bars 3 and 4, written while bar 1 plays.
    let part = clip(7680, 7680, &[(0, 1920, 36), (3840, 1920, 43)]);
    let file = "state/arrangement/track-1/agent-part.json";
    let run = |gain: f32| {
        let mut harness = Harness::piece();
        let synth = crate::support::synth(gain);
        harness.write_and_apply("state/arrangement/track-1/instrument.json", &synth);
        let mut output = harness.play(BAR / 2);
        assert_eq!(harness.write_and_apply(file, &part), 1);
        assert_eq!(harness.project.problems(), []);
        output.extend(harness.render(5 * BAR - BAR / 2));
        (harness, output)
    };

    // With a silent synth on the new part, every sample of the other tracks is the same:
    // the edit, its compile and its snapshot swap disturb nothing.
    let (_, silent) = run(0.0);
    assert_eq!(difference(&expected, &silent), None);

    // With an audible synth the render differs only where the part sounds: from the start
    // of bar 3 until the release of its last note is over, before the end of bar 4.
    let (mut harness, audible) = run(0.15);
    let (first, last) = difference(&expected, &audible).unwrap();
    assert!((2 * BAR..2 * BAR + 64).contains(&first), "{first}");
    assert!((3 * BAR + BAR / 2..4 * BAR).contains(&last), "{last}");

    // Undo removes the part as one step.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert!(!harness.path(file).exists());
    harness.project.engine().stop();
    harness.render(BAR);
    assert_eq!(difference(&harness.play(5 * BAR), &expected), None);
}

#[test]
fn a_track_folder_written_during_playback_adds_a_track_without_stopping_the_others() {
    let expected = Harness::piece().play(5 * BAR);
    let run = |gain: f32| {
        let mut harness = Harness::piece();
        let mut output = harness.play(BAR / 2);
        let line = clip(7680, 3840, &[(0, 1920, 36)]);
        harness.write_track("bass", 3, gain, &[("line", line)]);
        assert_eq!(harness.project.problems(), []);
        output.extend(harness.render(5 * BAR - BAR / 2));
        (harness, output)
    };
    let (_, silent) = run(0.0);
    assert_eq!(difference(&expected, &silent), None);

    let (mut harness, audible) = run(0.15);
    let (first, last) = difference(&expected, &audible).unwrap();
    assert!((2 * BAR..2 * BAR + 64).contains(&first), "{first}");
    assert!((2 * BAR + BAR / 2..3 * BAR).contains(&last), "{last}");

    // Deleting the folder removes the track, and one undo brings the three records back.
    let folder = harness.path("state/arrangement/bass");
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(harness.apply(std::slice::from_ref(&folder)), 3);
    harness.project.engine().stop();
    harness.render(BAR);
    assert_eq!(difference(&harness.play(5 * BAR), &expected), None);
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert!(folder.join("line.json").exists());
    harness.project.engine().stop();
    harness.render(BAR);
    assert_eq!(difference(&harness.play(5 * BAR), &audible), None);
}

#[test]
fn snapshot_swaps_during_a_held_note_do_not_change_a_sample_of_it() {
    let run = |edits: bool| {
        let mut harness = Harness::piece();
        harness.project.engine().play();
        let mut output = Vec::new();
        // A drag of a note in bar 5, one new snapshot per 512 frames, while the pad holds
        // its long note and the piano changes chords.
        for step in 0..(4 * BAR / 512) as u64 {
            if edits {
                let dragged = clip(15360, 3840, &[(step, 480, 60)]);
                harness.write_and_apply("state/arrangement/pad/dragged.json", &dragged);
            }
            output.extend(harness.render(512));
        }
        output
    };
    assert_eq!(difference(&run(false), &run(true)), None);
}

#[test]
fn closing_and_reopening_restores_the_piece() {
    let mut harness = Harness::piece();
    harness.write_and_apply(
        "project.json",
        &std::fs::read_to_string(harness.path("project.json"))
            .unwrap()
            .replace("120.0", "96.5"),
    );
    let first = harness.play(3 * BAR);
    assert!(first.iter().any(|sample| sample.abs() > 0.05));

    let mut reopened = harness.reopen();
    assert_eq!(reopened.project.problems(), []);
    assert_eq!(difference(&reopened.play(3 * BAR), &first), None);

    // A seek back to the start, after the tails are over, plays the same again.
    reopened.project.engine().pause();
    reopened.render(BAR);
    reopened.project.engine().seek(Ticks(0));
    assert_eq!(difference(&reopened.play(3 * BAR), &first), None);
}
