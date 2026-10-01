//! Outside groups of one request, or that follow each other closely, are one undo step,
//! because an agent writes the files of one request seconds apart. The times are given, so no
//! test sleeps.

use std::time::{Duration, Instant};

use sound_core::{Changes, GROUPING_WINDOW, OUTSIDE_UNDO_WINDOW};

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
        .apply_outside_changes_at(std::slice::from_ref(&path), start)
        .unwrap();
    // A minute later, an agent rewrites the record three times, ten seconds apart.
    for (index, value) in [0.2, 0.3, 0.4].into_iter().enumerate() {
        std::fs::write(&path, dc_record(value)).unwrap();
        let at = start + Duration::from_secs(60 + 10 * index as u64);
        harness
            .project
            .apply_outside_changes_at(std::slice::from_ref(&path), at)
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

#[test]
fn an_agent_that_takes_back_what_it_wrote_leaves_no_undo_step() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let path = harness.write("state/dc.json", &dc_record(0.1));
    harness
        .project
        .apply_outside_changes_at(std::slice::from_ref(&path), start)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    let later = start + Duration::from_secs(5);
    harness
        .project
        .apply_outside_changes_at(&[path], later)
        .unwrap();
    assert_eq!(harness.project.undo_label(), None);
}

const REQUEST: &str = "Add a bank";

/// Writes `state/<name>.json` and applies it as heard at `at`.
fn write_dc(harness: &mut Harness, name: &str, value: f32, at: Instant) {
    let path = harness.write(&format!("state/{name}.json"), &dc_record(value));
    harness
        .project
        .apply_outside_changes_at(&[path], at)
        .unwrap();
}

#[test]
fn groups_thirty_seconds_apart_in_one_request_are_one_step_with_its_label() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let gap = Duration::from_secs(30);
    harness.project.begin_request(REQUEST);
    write_bank(&mut harness, start, gap);
    harness.project.end_request_at(start + gap * 3);
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 0);
    assert_eq!(harness.project.undo().unwrap(), None);
}

#[test]
fn an_interface_edit_in_a_request_splits_it_into_two_steps_with_its_label() {
    let mut harness = Harness::new();
    let start = Instant::now();
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    let mut changes = Changes::new();
    changes.create(id("by-hand"), Dc { value: 0.2 });
    harness.project.commit("Add by hand", changes).unwrap();
    write_dc(&mut harness, "b", 0.3, start + Duration::from_secs(1));
    harness
        .project
        .end_request_at(start + Duration::from_secs(2));

    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 2);
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add by hand")
    );
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 0);
}

#[test]
fn a_write_heard_just_after_the_end_joins_the_request() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let end = start + Duration::from_secs(10);
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    harness.project.end_request_at(end);
    let late = Duration::from_millis(50);
    assert!(late < GROUPING_WINDOW);
    write_dc(&mut harness, "b", 0.2, end + late);

    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 0);
}

#[test]
fn a_group_heard_before_the_end_but_applied_after_it_joins_the_request() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let end = start + Duration::from_secs(10);
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    harness.project.end_request_at(end);
    write_dc(&mut harness, "b", 0.2, end - Duration::from_millis(30));

    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 0);
}

#[test]
fn an_undo_in_a_request_leaves_its_later_writes_a_new_step_with_its_label() {
    let mut harness = Harness::new();
    let start = Instant::now();
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    harness.project.undo().unwrap();
    write_dc(&mut harness, "b", 0.2, start + Duration::from_secs(1));
    harness
        .project
        .end_request_at(start + Duration::from_secs(2));

    assert_eq!(harness.project.undo_label(), Some(REQUEST));
    assert_eq!(harness.project.redo_label(), None);
    harness.project.undo().unwrap();
    assert_eq!(harness.project.instances().count(), 0);
    assert_eq!(harness.project.undo_label(), None);
}

#[test]
fn a_write_heard_well_after_the_end_is_a_step_of_its_own() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let end = start + Duration::from_secs(10);
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    harness.project.end_request_at(end);
    write_dc(&mut harness, "b", 0.2, end + Duration::from_millis(500));

    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.project.instances().count(), 1);
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
}

#[test]
fn after_a_request_ends_outside_groups_follow_the_window_again() {
    let mut harness = Harness::new();
    let start = Instant::now();
    let end = start + Duration::from_secs(1);
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "a", 0.1, start);
    harness.project.end_request_at(end);
    // Within the window of the request's last group, yet not part of the request.
    write_dc(&mut harness, "b", 0.2, end + Duration::from_secs(5));
    write_dc(&mut harness, "c", 0.3, end + Duration::from_secs(8));

    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.project.instances().count(), 1);
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(harness.project.instances().count(), 0);
}

#[test]
fn undo_of_a_request_brings_back_every_file_it_touched() {
    let mut harness = Harness::new();
    let start = Instant::now();
    write_dc(&mut harness, "dc", 0.1, start);
    // Two seconds later: without the request, this would join the step above.
    let begin = start + Duration::from_secs(2);
    harness.project.begin_request(REQUEST);
    write_dc(&mut harness, "dc", 0.4, begin);
    write_bank(
        &mut harness,
        begin + Duration::from_secs(1),
        Duration::from_secs(20),
    );
    harness
        .project
        .end_request_at(begin + Duration::from_secs(60));

    assert_eq!(harness.project.undo().unwrap().as_deref(), Some(REQUEST));
    assert_eq!(
        harness.read("state/dc.json"),
        "{\n  \"tool\": \"test.dc\",\n  \"state\": {\"value\": 0.1}\n}\n"
    );
    assert!(!harness.path("state/bank").exists());
    assert_eq!(harness.project.instances().count(), 1);
    assert_eq!(harness.project.undo_label(), Some("File change"));
}
