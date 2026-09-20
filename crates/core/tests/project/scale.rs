//! A project of ordinary size: 100 parents with 100 small child records each.

use std::time::Instant;

use crate::tools::{Harness, level_record};

const PARENTS: usize = 100;
const CHILDREN: usize = 100;

/// Run with `cargo nextest run -p sound-core --run-ignored only scale --no-capture`. It
/// writes 10,100 files, which is too slow for every CI run.
#[test]
#[ignore = "writes 10,100 files; run it locally and read the printed times"]
fn ten_thousand_child_records_open_and_one_edit_applies_live() {
    let harness = Harness::new();
    let started = Instant::now();
    for parent in 0..PARENTS {
        let gain = if parent == 0 { 1.0 } else { 0.0 };
        let record = format!(r#"{{"tool": "test.bank", "state": {{"gain": {gain}}}}}"#);
        harness.write(&format!("state/bank-{parent:03}/instance.json"), &record);
        for child in 0..CHILDREN {
            let path = format!("state/bank-{parent:03}/level-{child:03}.json");
            harness.write(&path, &level_record(0.0));
        }
    }
    println!("generate: {:?}", started.elapsed());

    let started = Instant::now();
    let mut harness = harness.reopen();
    println!(
        "open {} records: {:?}",
        PARENTS * (CHILDREN + 1),
        started.elapsed()
    );
    assert_eq!(
        harness.project.instances().count(),
        PARENTS * (CHILDREN + 1)
    );
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.level(), 0.0);

    let path = harness.write("state/bank-000/level-050.json", &level_record(0.5));
    let started = Instant::now();
    assert_eq!(harness.apply_outside_changes(&[path]).unwrap(), 1);
    println!("apply one outside record edit: {:?}", started.elapsed());
    assert_eq!(harness.level(), 0.5);

    let started = Instant::now();
    harness.project.undo().unwrap();
    println!("undo it, with the file write: {:?}", started.elapsed());
    assert_eq!(harness.level(), 0.0);

    let folder = harness.path("state/bank-099");
    let started = Instant::now();
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        harness.apply_outside_changes(&[folder]).unwrap(),
        CHILDREN + 1
    );
    println!(
        "apply one deleted folder of {CHILDREN} children: {:?}",
        started.elapsed()
    );

    // Undo writes the folder again: 101 files, each a temporary file and a rename.
    let started = Instant::now();
    harness.project.undo().unwrap();
    println!(
        "undo it, which writes {} files: {:?}",
        CHILDREN + 1,
        started.elapsed()
    );
    assert!(harness.path("state/bank-099/level-099.json").exists());
}
