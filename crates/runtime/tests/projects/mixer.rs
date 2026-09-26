//! The mixer of a track through the whole runtime: a project written before it existed, and
//! gain, pan and mute from a file while the real synth plays.

use std::path::PathBuf;

use sound_core::Ticks;

use crate::support::{BAR, Harness, clip, difference};

const PIANO: &str = "state/arrangement/piano/instance.json";

/// The two channels of an interleaved render, apart.
fn split(interleaved: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let channel = |first: usize| interleaved.iter().skip(first).step_by(2).copied().collect();
    (channel(0), channel(1))
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
}

/// Every file under the project folder, by relative path, with its bytes.
fn files(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                found.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// The track record a project of the first milestone holds: no gain, no pan, no mute.
#[test]
fn a_project_from_before_the_mixer_opens_unchanged_and_plays_on_both_channels() {
    let mut harness = Harness::piece();
    let record = std::fs::read_to_string(harness.path(PIANO)).unwrap();
    assert!(!record.contains("gain_db"), "{record}");
    let before = files(harness.project.root());
    let first = harness.play(2 * BAR);

    let mut reopened = harness.reopen();
    assert_eq!(reopened.project.problems(), []);
    // Opening and closing wrote nothing: every file is byte for byte what it was.
    assert_eq!(files(reopened.project.root()), before);
    let again = reopened.play(2 * BAR);
    assert_eq!(difference(&first, &again), None);

    // It plays in the middle: both channels, the same samples.
    let (left, right) = split(&first);
    assert!(peak(&left) > 0.05, "{}", peak(&left));
    assert_eq!(left, right);
}

#[test]
fn gain_pan_and_mute_from_a_file_apply_live_through_the_synth() {
    let mut harness = Harness::piece();
    let plain = harness.play(BAR);
    assert!(peak(&plain) > 0.05);
    // The piece is four bars long, so each stage starts at the beginning again. The mixer
    // keeps its gains through a seek: only the notes start again.
    let from_the_start = |harness: &mut Harness| {
        harness.project.engine().seek(Ticks(0));
        split(&harness.render(2 * BAR))
    };

    // The piano hard left and 6 dB down, written by an agent while the project plays.
    let record = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "gain_db": -6.0, "pan": -1.0}}"#;
    harness.write_and_apply(PIANO, record);
    let (left, right) = from_the_start(&mut harness);
    // The pad is still in the middle, so the right channel keeps only the pad.
    let (settled_left, settled_right) = (&left[BAR / 2..], &right[BAR / 2..]);
    assert!(
        peak(settled_left) > peak(settled_right),
        "the piano is left"
    );

    // Muting the pad as well leaves the right channel silent: only the piano, hard left.
    let pad =
        r#"{"tool": "arrangement.track", "state": {"name": "pad", "order": 2, "mute": true}}"#;
    harness.write_and_apply("state/arrangement/pad/instance.json", pad);
    let (left, right) = from_the_start(&mut harness);
    assert_eq!(peak(&right[BAR / 2..]), 0.0);
    assert!(peak(&left[BAR / 2..]) > 0.0);

    // Two undo steps take both back, and the file with them.
    harness.project.undo().unwrap();
    harness.project.undo().unwrap();
    let record = std::fs::read_to_string(harness.path(PIANO)).unwrap();
    assert!(
        record.contains(r#""gain_db": 0.0, "pan": 0.0, "mute": false"#),
        "{record}"
    );
    let (left, right) = from_the_start(&mut harness);
    assert_eq!(left[BAR / 2..], right[BAR / 2..]);
    assert!(peak(&left[BAR / 2..]) > 0.05);
}

/// The piece with both synths at full gain: its chords sum far over full scale.
fn loud_piece() -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 15360, 48),
        (0, 15360, 55),
        (0, 15360, 64),
        (0, 15360, 67),
    ];
    harness.write_track("piano", 1, 1.0, &[("chords", clip(0, 15360, &chords))]);
    harness.write_track(
        "pad",
        2,
        1.0,
        &[("long", clip(0, 15360, &[(0, 15360, 72)]))],
    );
    assert_eq!(harness.project.problems(), []);
    harness
}

#[test]
fn a_project_that_clipped_renders_under_the_ceiling_and_the_meter_says_so() {
    let mut clipping = loud_piece();
    clipping.bypass_limiter();
    let before = clipping.play(2 * BAR);
    let mut limited = loud_piece();
    let after = limited.play(2 * BAR);
    println!(
        "sample peak without the limiter {:.4} ({:+.2} dBFS), with it {:.6} ({:+.4} dBFS)",
        peak(&before),
        20.0 * peak(&before).log10(),
        peak(&after),
        20.0 * peak(&after).log10()
    );
    assert!(peak(&before) > 1.5, "{}", peak(&before));
    assert!(peak(&after) <= 1.0, "{}", peak(&after));
    // The master meter took exactly the peak of the render.
    let arrangement = sound_core::InstanceId::new("arrangement").unwrap();
    let master = arrangement::master_peaks(&limited.project, &arrangement).unwrap();
    let (left, right) = split(&after);
    assert_eq!(master.take(), [peak(&left), peak(&right)]);
    // And the device output, which the transport shows, is the same here: the master is all
    // this project plays.
    let output = limited.project.engine().output_peaks().take();
    assert_eq!(output, [peak(&left), peak(&right)]);
}

/// A project of before the master: its arrangement record says nothing, as every record of
/// the first two milestones did. It opens with the limiter on, and no file is written for it.
#[test]
fn a_project_from_before_the_master_opens_unchanged_and_renders_under_the_ceiling() {
    let folder = tempfile::tempdir().unwrap();
    let old = [
        (
            "project.json",
            r#"{"format": 1, "extensions": ["arrangement", "instrument"], "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}, "connections": []}"#.to_string(),
        ),
        (
            "state/arrangement/instance.json",
            r#"{"tool": "arrangement", "state": {}}"#.to_string(),
        ),
        (
            "state/arrangement/piano/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1}}"#.to_string(),
        ),
        ("state/arrangement/piano/instrument.json", crate::support::synth(1.0)),
        (
            "state/arrangement/piano/chords.json",
            clip(0, 15360, &[(0, 15360, 48), (0, 15360, 55), (0, 15360, 64), (0, 15360, 67)]),
        ),
    ];
    for (path, body) in &old {
        crate::support::write(folder.path(), path, body);
    }
    let before = files(folder.path());
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    let render = harness.play(2 * BAR);
    let (left, right) = split(&render);
    // The chord at full gain is over full scale, and the limiter holds it.
    assert!(peak(&left) <= 1.0 && peak(&right) <= 1.0, "{}", peak(&left));
    assert!(peak(&left) > 0.99, "{}", peak(&left));
    // Opening, playing and closing wrote no record: every one of them is byte for byte what it
    // was. Only the generated files are new.
    let reopened = harness.reopen();
    let after = files(reopened.project.root());
    for (path, bytes) in &before {
        let now = after.iter().find(|(other, _)| other == path);
        assert_eq!(now.map(|(_, now)| now), Some(bytes), "{}", path.display());
    }
}
