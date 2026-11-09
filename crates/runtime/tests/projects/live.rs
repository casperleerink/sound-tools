//! Outside edits during playback, heard through the real synth. Renders are compared sample
//! by sample with a run that has no edit.

use sound_core::Ticks;

use crate::support::{BAR, CHORDS, Harness, clip, difference, synth};

#[test]
fn a_new_folder_is_an_empty_project_of_every_extension() {
    let harness = Harness::new();
    assert_eq!(harness.project.instances().count(), 0);
    let project_file = harness.project.project_file();
    assert_eq!(project_file.tempo_map, sound_core::TempoMap::default());
    assert_eq!(project_file.connections, []);
    let extensions = runtime::bundled_extensions().unwrap();
    assert_eq!(project_file.extensions, extensions);
    assert_eq!(harness.project.undo_label(), None);
}

#[test]
fn a_folder_with_hidden_files_only_becomes_a_new_project() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".git")).unwrap();
    std::fs::write(folder.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(folder.path().join(".DS_Store"), [0_u8; 4]).unwrap();
    let harness = Harness::open(folder);
    assert!(harness.path("project.json").exists());
    assert!(harness.path(".git/HEAD").exists());
}

#[test]
fn a_folder_with_files_and_no_project_file_is_refused_and_left_as_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let write = |path: &str, contents: &str| crate::support::write(folder.path(), path, contents);
    write("CLAUDE.md", "# my rules\n");
    write("AGENTS.md", "mine\n");
    write("agent-docs/mine.md", "mine\n");
    write("extensions/tool.ts", "export {};\n");
    write("state/drone.json", r#"{"tool": "tone", "state": {}}"#);
    write("main.rs", "fn main() {}\n");
    let before = everything_in(folder.path());

    let (control, _engine) = sound_core::Engine::new(runtime::OFFLINE);
    let plugins = plugin_host::Plugins::new(
        Vec::new(),
        crate::plugin_hosts::scanner(),
        plugin_host::ScanCache::none(),
    );
    let Err(error) = runtime::open_or_create_with(folder.path(), control, plugins) else {
        panic!("a folder of someone's files opened as a project");
    };
    assert!(
        error.to_string().contains("is not a Sound Tools project"),
        "{error}"
    );
    assert_eq!(everything_in(folder.path()), before);
}

/// Every file and folder under `folder`, with the bytes of each file.
fn everything_in(folder: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.push((path.clone(), None));
            found.extend(everything_in(&path));
        } else {
            found.push((path.clone(), Some(std::fs::read(&path).unwrap())));
        }
    }
    found.sort();
    found
}

#[test]
fn a_timeline_written_during_playback_sounds_in_its_bars_and_disturbs_nothing_else() {
    let expected = Harness::piece().play(5 * BAR);

    // The part goes into a rack with no notes yet, in bars 3 and 4, written while bar 1 plays.
    let part = clip(7680, 7680, &[(0, 1920, 36), (3840, 1920, 43)]);
    let file = "state/empty/agent-part.json";
    let run = |gain: f32| {
        let (piano, chords) = (synth(0.15), clip(0, 15360, &CHORDS));
        let (pad, long) = (synth(0.1), clip(0, 15360, &[(0, 15360, 72)]));
        let empty = synth(gain);
        let mut harness = Harness::racks(&[
            ("piano", &[("instrument", &piano), ("chords", &chords)]),
            ("pad", &[("instrument", &pad), ("long", &long)]),
            ("empty", &[("instrument", &empty)]),
        ]);
        let mut output = harness.play(BAR / 2);
        assert_eq!(harness.write_and_apply(file, &part), 1);
        assert_eq!(harness.project.problems(), []);
        output.extend(harness.render(5 * BAR - BAR / 2));
        (harness, output)
    };

    // With a silent synth on the new part, every sample of the other racks is the same:
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
                harness.write_and_apply("state/pad/dragged.json", &dragged);
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
