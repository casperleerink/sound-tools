//! Runs: the edits of code that edits as it goes, such as a control loop 30 times a second.
//! The times are given, so no test sleeps.

use std::time::{Duration, Instant};

use sound_core::{Changes, Instance, PortReference, RUN_LONGEST, RUN_QUIET, SavedConnection};

use crate::tools::{Dc, Harness, dc_record, id};

fn connected_dc(harness: &mut Harness) -> Instance<Dc> {
    let mut changes = Changes::new();
    let dc = changes.create(id("dc"), Dc { value: 0.0 });
    changes.connect(SavedConnection::to_device(
        PortReference::new(dc.id(), "out"),
        0,
    ));
    harness.project.commit("Add dc", changes).unwrap();
    dc
}

fn set(dc: &Instance<Dc>, value: f32) -> Changes {
    let mut changes = Changes::new();
    changes.set(dc, Dc { value });
    changes
}

/// Publishes `frames` edits 30 times a second from `start`, the values `from` and up in
/// thousandths, and hears each one play at once.
fn edit_each_frame(
    harness: &mut Harness,
    dc: &Instance<Dc>,
    (label, from): (&str, u32),
    start: Instant,
    frames: u32,
) {
    let frame = Duration::from_millis(33);
    for index in 0..frames {
        let value = (from + index) as f32 / 1000.0;
        let at = start + frame * index;
        harness
            .project
            .publish_run_at(&id("tool"), label, set(dc, value), at)
            .unwrap();
        assert_eq!(harness.level(), value);
        // What the engine gives back, so a long run does not fill its ring.
        harness.project.engine().poll().unwrap();
    }
}

#[test]
fn edits_each_frame_are_one_undo_step_written_once_when_they_stop() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    let saved = harness.read("state/dc.json");
    let start = Instant::now();
    edit_each_frame(&mut harness, &dc, ("Follow", 0), start, 30);
    // Live, and nothing written yet.
    assert_eq!(harness.read("state/dc.json"), saved);
    harness
        .project
        .finish_quiet_runs_at(start + Duration::from_millis(990))
        .unwrap();
    assert_eq!(harness.read("state/dc.json"), saved);

    let quiet = start + Duration::from_millis(33 * 29) + RUN_QUIET;
    harness.project.finish_quiet_runs_at(quiet).unwrap();
    assert!(harness.read("state/dc.json").contains("0.029"));
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some("Follow"));
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.read("state/dc.json"), saved);
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
}

#[test]
fn another_label_or_a_pause_starts_another_step() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    let start = Instant::now();
    edit_each_frame(&mut harness, &dc, ("One", 1), start, 3);
    edit_each_frame(
        &mut harness,
        &dc,
        ("Two", 10),
        start + Duration::from_millis(100),
        3,
    );
    let later = start + Duration::from_millis(100) + RUN_QUIET * 2;
    edit_each_frame(&mut harness, &dc, ("Two", 20), later, 3);
    let labels: Vec<Option<String>> = (0..3).map(|_| harness.project.undo().unwrap()).collect();
    assert_eq!(
        labels,
        [Some("Two".into()), Some("Two".into()), Some("One".into())]
    );
}

#[test]
fn a_run_that_goes_on_is_written_at_its_longest() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    let frames = (RUN_LONGEST.as_millis() / 33) as u32 + 10;
    edit_each_frame(&mut harness, &dc, ("Follow", 0), Instant::now(), frames);
    // The first run is written, the one after it is still open.
    assert!(
        harness
            .read("state/dc.json")
            .contains(&format!("{}", (frames - 10) as f32 / 1000.0))
    );
}

/// Undo takes back what the composer just saw, so it finishes the run first.
#[test]
fn an_undo_during_a_run_takes_the_run_back() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    edit_each_frame(&mut harness, &dc, ("Follow", 0), Instant::now(), 5);
    assert_eq!(harness.project.undo().unwrap().as_deref(), Some("Follow"));
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.project.redo().unwrap().as_deref(), Some("Follow"));
    assert_eq!(harness.level(), 0.004);
}

/// An agent's file change during a run is the state the run's step starts from, so undo of the
/// run never takes back what the agent wrote.
#[test]
fn a_file_change_during_a_run_stays_under_its_step() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    let start = Instant::now();
    edit_each_frame(&mut harness, &dc, ("Follow", 0), start, 3);
    harness.write_and_apply("state/dc.json", &dc_record(0.5));
    assert_eq!(harness.level(), 0.5);
    harness
        .project
        .publish_run_at(
            &id("tool"),
            "Follow",
            set(&dc, 0.75),
            start + Duration::from_millis(200),
        )
        .unwrap();
    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.5);
}

#[test]
fn a_run_left_open_is_written_when_the_project_closes() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness);
    edit_each_frame(&mut harness, &dc, ("Follow", 0), Instant::now(), 3);
    let harness = harness.reopen();
    assert!(harness.read("state/dc.json").contains("0.002"));
}
