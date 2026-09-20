//! Outside groups that follow each other closely are one undo step, because an agent writes
//! the files of one request seconds apart. The times are given, so no test sleeps.

use std::time::{Duration, Instant};

use sound_core::{Changes, OUTSIDE_UNDO_WINDOW};

use crate::tools::{BANK_RECORD, Dc, Harness, dc_record, id, level_record};

/// Writes a bank folder the way an agent does, one file per group, `gap` apart.
fn write_bank(harness: &mut Harness, start: Instant, gap: Duration) {
    let files = [
        ("state/bank/instance.json", BANK_RECORD.to_string()),
        ("state/bank/a.json", level_record(0.25)),
        ("state/bank/b.json", level_record(0.5)),
    ];
    for (index, (path, record)) in files.iter().enumerate() {
        let path = harness.write(path, record);
        let at = start + gap * index as u32;
        let changed = harness.project.apply_outside_changes_at(&[path], at);
        assert_eq!(changed.unwrap(), 1);
    }
}

#[test]
fn groups_three_seconds_apart_are_one_undo_step() {
    let mut harness = Harness::new();
    write_bank(&mut harness, Instant::now(), Duration::from_secs(3));
    assert_eq!(harness.level(), 0.75);

    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.project.instances().count(), 0);
    assert!(!harness.path("state/bank").join("instance.json").exists());
    assert_eq!(harness.project.undo().unwrap(), None);

    // Redo brings all three back, as one step too.
    harness.project.redo().unwrap();
    assert_eq!(harness.project.instances().count(), 3);
    assert_eq!(harness.level(), 0.75);
    assert_eq!(harness.project.redo().unwrap(), None);
}

#[test]
fn the_window_runs_from_the_last_group_and_keeps_the_oldest_before_side() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let path = harness.write("state/dc.json", &dc_record(0.1));
    harness
        .project
        .apply_outside_changes_at(&[path.clone()], start)
        .unwrap();
    // A minute later, an agent rewrites the record three times, ten seconds apart.
    for (index, value) in [0.2, 0.3, 0.4].into_iter().enumerate() {
        std::fs::write(&path, dc_record(value)).unwrap();
        let at = start + Duration::from_secs(60 + 10 * index as u64);
        harness
            .project
            .apply_outside_changes_at(&[path.clone()], at)
            .unwrap();
    }
    harness.project.undo().unwrap();
    let dc = harness.project.resolve::<Dc>(&id("dc")).unwrap();
    assert_eq!(harness.project.state(&dc).unwrap().value, 0.1);
    assert_eq!(
        harness.read("state/dc.json"),
        "{\n  \"tool\": \"test.dc\",\n  \"state\": {\"value\": 0.1}\n}\n"
    );
}

#[test]
fn groups_twenty_seconds_apart_are_separate_undo_steps() {
    assert!(OUTSIDE_UNDO_WINDOW < Duration::from_secs(20));
    let mut harness = Harness::new();
    write_bank(&mut harness, Instant::now(), Duration::from_secs(20));
    harness.project.undo().unwrap();
    assert_eq!(harness.project.instances().count(), 2);
    harness.project.undo().unwrap();
    assert_eq!(harness.project.instances().count(), 1);
    harness.project.undo().unwrap();
    assert_eq!(harness.project.instances().count(), 0);
}

#[test]
fn an_interface_edit_an_undo_or_a_redo_in_between_ends_the_step() {
    let start = Instant::now();
    let second = start + Duration::from_secs(3);

    // An interface edit in between.
    let mut harness = Harness::new();
    let first = harness.write("state/a.json", &dc_record(0.1));
    harness
        .project
        .apply_outside_changes_at(&[first], start)
        .unwrap();
    let mut changes = Changes::new();
    changes.create(id("by-hand"), Dc { value: 0.2 });
    harness.project.commit("Add by hand", changes).unwrap();
    let later = harness.write("state/b.json", &dc_record(0.3));
    harness
        .project
        .apply_outside_changes_at(&[later], second)
        .unwrap();
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.project.instances().count(), 2);
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add by hand")
    );
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.project.instances().count(), 0);

    // An undo and a redo in between: the step on top is an outside one again, but the
    // composer has acted on it, so what comes next is a new step.
    let mut harness = Harness::new();
    let first = harness.write("state/a.json", &dc_record(0.1));
    harness
        .project
        .apply_outside_changes_at(&[first], start)
        .unwrap();
    harness.project.undo().unwrap();
    harness.project.redo().unwrap();
    let later = harness.write("state/b.json", &dc_record(0.3));
    harness
        .project
        .apply_outside_changes_at(&[later], second)
        .unwrap();
    harness.project.undo().unwrap();
    assert_eq!(harness.project.instances().count(), 1);
}

#[test]
fn a_cleared_history_has_nothing_to_undo() {
    let mut harness = Harness::new();
    let mut changes = Changes::new();
    changes.create(id("dc"), Dc { value: 0.2 });
    harness.project.commit("Add", changes).unwrap();
    harness.project.clear_history();
    assert_eq!(harness.project.undo_label(), None);
    assert_eq!(harness.project.undo().unwrap(), None);
    assert_eq!(harness.project.instances().count(), 1);
}
