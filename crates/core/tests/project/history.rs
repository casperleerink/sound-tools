//! The history of the folder: points, the lines they form, and what plain git reads of it.

use std::process::Command;

use sound_core::{Changes, HISTORY_FOLDER, Instance, PointId};

use crate::tools::{Dc, Harness, dc_record, id};

fn add_dc(harness: &mut Harness) -> Instance<Dc> {
    let mut changes = Changes::new();
    let dc = changes.create(id("dc"), Dc { value: 0.0 });
    harness.project.commit("Add dc", changes).unwrap();
    dc
}

fn set(harness: &mut Harness, dc: &Instance<Dc>, value: f32) {
    let mut changes = Changes::new();
    changes.set(dc, Dc { value });
    harness.project.commit("Set", changes).unwrap();
}

fn current(harness: &Harness) -> PointId {
    harness.project.history().unwrap().current()
}

/// Whether each line is a version, rather than undone work.
fn versions(harness: &Harness) -> Vec<bool> {
    let history = harness.project.history().unwrap();
    history
        .lines()
        .iter()
        .map(|line| line.is_version())
        .collect()
}

fn git(harness: &Harness, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(harness.path(HISTORY_FOLDER))
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn plain_git_reads_every_point() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    set(&mut harness, &dc, 0.5);
    harness.project.undo().unwrap();
    set(&mut harness, &dc, 0.25);

    git(&harness, &["fsck", "--strict", "--no-dangling"]);
    let log = git(&harness, &["log", "--all", "--format=%s"]);
    // The undone point stays findable.
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["Set", "Set", "Add dc", "Project start"]
    );
    let file = git(&harness, &["show", "HEAD:state/dc.json"]);
    assert_eq!(file, harness.read("state/dc.json"));
}

#[test]
fn undo_and_redo_make_no_point_and_bring_back_the_files_byte_for_byte() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    let written = harness.read("state/dc.json");
    let points = harness
        .project
        .history()
        .unwrap()
        .points(100)
        .unwrap()
        .len();

    assert_eq!(harness.project.undo().unwrap().as_deref(), Some("Add dc"));
    assert!(!harness.path("state/dc.json").exists());
    assert!(harness.project.state(&dc).is_none());
    assert_eq!(harness.project.redo().unwrap().as_deref(), Some("Add dc"));
    assert_eq!(harness.read("state/dc.json"), written);
    assert_eq!(
        harness
            .project
            .history()
            .unwrap()
            .points(100)
            .unwrap()
            .len(),
        points
    );
}

#[test]
fn a_change_after_a_small_undo_continues_the_line_and_marks_the_undone_work() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    set(&mut harness, &dc, 0.5);
    harness.project.undo().unwrap();
    set(&mut harness, &dc, 0.25);
    assert_eq!(versions(&harness), [true, false]);

    let undone = harness.project.history().unwrap().lines()[1].start;
    harness.project.keep_as_version(undone).unwrap();
    assert_eq!(versions(&harness), [true, true]);
}

#[test]
fn a_change_after_undoing_ten_points_starts_a_version() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    for step in 0..10 {
        set(&mut harness, &dc, step as f32 / 10.);
    }
    for _ in 0..10 {
        harness.project.undo().unwrap();
    }
    set(&mut harness, &dc, 0.95);
    assert_eq!(versions(&harness), [true, true]);
    let history = harness.project.history().unwrap();
    assert_eq!(history.lines()[1].start, current(&harness));
}

#[test]
fn undo_that_keeps_a_version_and_going_back_start_a_version() {
    for keep in [true, false] {
        let mut harness = Harness::new();
        let dc = add_dc(&mut harness);
        set(&mut harness, &dc, 0.5);
        let start = current(&harness);
        if keep {
            harness.project.undo_keeping_version().unwrap();
        } else {
            let points = harness.project.history().unwrap().points(10).unwrap();
            harness.project.go_to(points[1].id).unwrap();
            harness.project.go_to(start).unwrap();
            harness.project.go_to(points[1].id).unwrap();
            // Going around is free.
            assert_eq!(
                harness.project.history().unwrap().points(10).unwrap().len(),
                points.len()
            );
        }
        set(&mut harness, &dc, 0.25);
        assert_eq!(versions(&harness), [true, true], "keep: {keep}");
    }
}

#[test]
fn a_change_from_undone_work_makes_it_a_version() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    set(&mut harness, &dc, 0.5);
    let undone = current(&harness);
    harness.project.undo().unwrap();
    set(&mut harness, &dc, 0.25);
    assert_eq!(versions(&harness), [true, false]);

    harness.project.go_to(undone).unwrap();
    set(&mut harness, &dc, 0.75);
    assert_eq!(versions(&harness), [true, true]);
}

#[test]
fn outside_changes_close_together_are_one_point_and_one_taken_back_is_none() {
    let mut harness = Harness::new();
    add_dc(&mut harness);
    let before = current(&harness);
    let added = harness.read("state/dc.json");
    let path = harness.write("state/dc.json", &dc_record(0.5));
    let at = harness.now;
    harness
        .project
        .apply_outside_changes_at(std::slice::from_ref(&path), at)
        .unwrap();
    std::fs::write(&path, dc_record(0.75)).unwrap();
    let later = at + std::time::Duration::from_secs(5);
    harness
        .project
        .apply_outside_changes_at(std::slice::from_ref(&path), later)
        .unwrap();
    let points = harness.project.history().unwrap().points(100).unwrap();
    assert_eq!(points[1].id, before);

    // The agent writes back what was there: no point is left.
    std::fs::write(&path, added).unwrap();
    let last = later + std::time::Duration::from_secs(5);
    harness
        .project
        .apply_outside_changes_at(&[path], last)
        .unwrap();
    assert_eq!(current(&harness), before);
}

#[test]
fn merge_brings_over_what_only_the_other_version_changed() {
    for keep_mine in [false, true] {
        let mut harness = Harness::new();
        let dc = add_dc(&mut harness);
        let split = current(&harness);
        // The other version changes `dc` and adds a file.
        set(&mut harness, &dc, 0.5);
        harness.write_and_apply("state/other.json", &dc_record(0.125));
        let other = harness.project.history().unwrap().lines()[0].start;
        harness.project.go_to(split).unwrap();
        set(&mut harness, &dc, 0.25);
        let mine = harness.project.history().unwrap().lines()[1].start;
        assert_eq!(current(&harness), mine);

        harness.project.merge(other, keep_mine).unwrap();
        let points = harness.project.history().unwrap().points(1).unwrap();
        assert_eq!(points[0].parents.len(), 2);
        assert!(harness.path("state/other.json").exists());
        let value = harness.project.state(&dc).unwrap().value;
        assert_eq!(value, if keep_mine { 0.25 } else { 0.5 });
    }
}

#[test]
fn a_name_given_by_hand_stays_until_cleared() {
    let mut harness = Harness::new();
    add_dc(&mut harness);
    let line = harness.project.history().unwrap().lines()[0].clone();
    harness
        .project
        .rename_version(line.start, Some("Mine".into()))
        .unwrap();
    harness
        .project
        .name_version(line.start, "Found".into(), line.tip)
        .unwrap();
    assert_eq!(
        harness.project.history().unwrap().lines()[0].title(),
        "Mine"
    );

    harness.project.rename_version(line.start, None).unwrap();
    assert_eq!(
        harness.project.history().unwrap().lines()[0].title(),
        "Version 1"
    );
    harness
        .project
        .name_version(line.start, "Found".into(), line.tip)
        .unwrap();
    assert_eq!(
        harness.project.history().unwrap().lines()[0].title(),
        "Found"
    );
}

#[test]
fn the_tools_code_is_kept_and_what_the_runtime_writes_is_not() {
    let mut harness = Harness::new();
    harness.write("extensions/song.ts", "export const a = 1;");
    harness.write("extensions/node_modules/x/index.js", "x");
    harness.write("extensions/.editor-swap", "x");
    harness.write("assets/takes/one.wav", "x");
    let extensions = harness.path("extensions");
    harness
        .project
        .apply_outside_changes_at(&[extensions], harness.now)
        .unwrap();

    let files = git(&harness, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert_eq!(
        files.lines().collect::<Vec<_>>(),
        ["extensions/song.ts", "project.json"]
    );
}

#[test]
fn a_folder_changed_while_closed_is_one_point_on_reopen() {
    let mut harness = Harness::new();
    add_dc(&mut harness);
    let harness = harness.reopen();
    let before = current(&harness);
    let harness = harness.reopen();
    assert_eq!(current(&harness), before);

    harness.write("state/dc.json", &dc_record(0.5));
    harness.write("state/other.json", &dc_record(0.25));
    let harness = harness.reopen();
    let points = harness.project.history().unwrap().points(2).unwrap();
    assert_eq!(points[1].id, before);
}

#[test]
fn a_version_undone_whole_before_a_change_is_undone_work() {
    let mut harness = Harness::new();
    let dc = add_dc(&mut harness);
    set(&mut harness, &dc, 0.5);
    let points = harness.project.history().unwrap().points(10).unwrap();
    harness.project.go_to(points[1].id).unwrap();
    set(&mut harness, &dc, 0.25);
    assert_eq!(versions(&harness), [true, true]);

    harness.project.undo().unwrap();
    set(&mut harness, &dc, 0.75);
    assert_eq!(versions(&harness), [true, false, true]);
}

#[test]
fn a_version_folds_away_once_merged_or_thirty_points_old() {
    for merge in [true, false] {
        let mut harness = Harness::new();
        let dc = add_dc(&mut harness);
        let split = current(&harness);
        set(&mut harness, &dc, 0.5);
        harness.project.go_to(split).unwrap();
        set(&mut harness, &dc, 0.25);
        let folded = |harness: &Harness| {
            let history = harness.project.history().unwrap();
            let lines = history.lines();
            lines
                .iter()
                .map(|line| history.folded_away(line).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(folded(&harness), [false, false]);
        if merge {
            let other = harness.project.history().unwrap().lines()[0].start;
            harness.project.merge(other, false).unwrap();
        } else {
            for step in 0..28 {
                set(&mut harness, &dc, step as f32 / 100.);
            }
            assert_eq!(folded(&harness), [false, false]);
            set(&mut harness, &dc, 1.0);
        }
        assert_eq!(folded(&harness), [true, false], "merge: {merge}");
    }
}
