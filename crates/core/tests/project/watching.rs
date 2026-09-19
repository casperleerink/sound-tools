//! The real file watcher. Everything else calls `apply_outside_changes` with explicit paths.

use std::time::{Duration, Instant};

use crate::tools::{BANK_RECORD, Harness, level_record};

/// Polls until the project has applied `changes` changes. Returns in how many groups they
/// came. A burst is one group, unless a loaded machine delivers its events more than the
/// grouping window apart, so the test does not insist on one.
fn poll_until_applied(harness: &mut Harness, changes: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut applied, mut groups) = (0, 0);
    while applied < changes {
        let group = harness.project.poll().unwrap();
        applied += group;
        groups += usize::from(group > 0);
        assert!(
            Instant::now() < deadline,
            "the watcher reported {applied} of {changes}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(applied, changes);
    groups
}

#[test]
fn the_watcher_applies_a_burst_of_files_and_skips_own_writes() {
    let mut harness = Harness::new();
    harness.project.watch().unwrap();
    // The watcher needs a moment before it sees changes.
    std::thread::sleep(Duration::from_millis(200));

    harness.write("state/bank/instance.json", BANK_RECORD);
    for index in 0..8 {
        harness.write(
            &format!("state/bank/level-{index}.json"),
            &level_record(0.0625),
        );
    }
    let groups = poll_until_applied(&mut harness, 9);
    println!("the watcher delivered the burst in {groups} group(s)");
    assert_eq!(harness.level(), 0.5);

    // Undo deletes the nine files. The watcher sees that and must find nothing to apply.
    for _ in 0..groups {
        harness.project.undo().unwrap();
    }
    assert!(!harness.path("state/bank").exists());
    let quiet_until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < quiet_until {
        assert_eq!(harness.project.poll().unwrap(), 0);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.project.undo_label(), None);
    assert_eq!(harness.project.redo_label(), Some("File change"));

    // A deleted record is seen too.
    for _ in 0..groups {
        harness.project.redo().unwrap();
    }
    std::fs::remove_file(harness.path("state/bank/level-0.json")).unwrap();
    poll_until_applied(&mut harness, 1);
    assert_eq!(harness.level(), 0.4375);
}
