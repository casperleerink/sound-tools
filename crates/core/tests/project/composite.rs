//! The composite test tool: a parent that reads its children into one snapshot and routes
//! through an owned child, with no connection in `project.json`. The arrangement needs this
//! shape for tracks, clips and instruments.

use sound_core::{Changes, ProjectEvent};

use crate::tools::{Amplifier, BANK_RECORD, Bank, Harness, Level, id, level_record};

const AMPLIFIER_RECORD: &str = r#"{"tool": "test.amplifier", "state": {"gain": 2.0}}"#;

#[test]
fn children_from_outside_reach_the_parents_snapshot_in_one_engine_edit() {
    let mut harness = Harness::new();
    harness.write_and_apply("state/bank/instance.json", BANK_RECORD);
    assert!(harness.project.project_file().connections.is_empty());
    let batches = harness.batches();

    let paths = [
        harness.write("state/bank/a.json", &level_record(0.125)),
        harness.write("state/bank/b.json", &level_record(0.25)),
        harness.write("state/bank/output.json", AMPLIFIER_RECORD),
    ];
    harness.project.apply_outside_changes(&paths).unwrap();
    // Two levels, an output child and new routing: one batch, heard with no project.json edit.
    assert_eq!(harness.batches(), batches + 1);
    assert_eq!(harness.level(), 0.75);

    let bank = harness.project.resolve::<Bank>(&id("bank")).unwrap();
    let levels: Vec<f32> = harness
        .project
        .children::<Level>(bank.id())
        .map(|(_, level)| level.value)
        .collect();
    assert_eq!(levels, [0.125, 0.25]);
    assert_eq!(harness.project.children::<Amplifier>(bank.id()).count(), 1);
}

#[test]
fn a_child_edit_reaches_the_parent_and_only_names_the_child() {
    let mut harness = Harness::new();
    harness.write("state/bank/instance.json", BANK_RECORD);
    harness.write("state/bank/a.json", &level_record(0.125));
    let bank = harness.path("state/bank");
    harness.project.apply_outside_changes(&[bank]).unwrap();
    harness.project.drain_events();

    let level = harness.project.resolve::<Level>(&id("bank/a")).unwrap();
    let mut edit = harness.project.begin("Raise level");
    harness
        .project
        .update(&mut edit, &level, |state| state.value = 0.5)
        .unwrap();
    assert_eq!(harness.level(), 0.5);
    harness.project.finish(edit).unwrap();
    let events = harness.project.drain_events();
    assert_eq!(events, [ProjectEvent::Changed(id("bank/a"))]);
    assert!(events.iter().all(|event| match event {
        ProjectEvent::Changed(changed) => changed.is_inside(&id("bank")),
        _ => false,
    }));
}

#[test]
fn routing_follows_the_output_child_as_it_comes_and_goes() {
    let mut harness = Harness::new();
    harness.write("state/bank/instance.json", BANK_RECORD);
    harness.write("state/bank/a.json", &level_record(0.25));
    let bank = harness.path("state/bank");
    harness.project.apply_outside_changes(&[bank]).unwrap();
    assert_eq!(harness.level(), 0.25);

    harness.write_and_apply("state/bank/output.json", AMPLIFIER_RECORD);
    assert_eq!(harness.level(), 0.5);

    let output = harness.path("state/bank/output.json");
    std::fs::remove_file(&output).unwrap();
    harness.project.apply_outside_changes(&[output]).unwrap();
    assert_eq!(harness.level(), 0.25);

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.5);
}

#[test]
fn deleting_the_parent_folder_removes_children_and_processors() {
    let mut harness = Harness::new();
    harness.write("state/bank/instance.json", BANK_RECORD);
    harness.write("state/bank/a.json", &level_record(0.25));
    harness.write("state/bank/output.json", AMPLIFIER_RECORD);
    let bank = harness.path("state/bank");
    harness
        .project
        .apply_outside_changes(std::slice::from_ref(&bank))
        .unwrap();
    assert_eq!(harness.level(), 0.5);
    harness.project.drain_events();
    let batches = harness.batches();

    std::fs::remove_dir_all(&bank).unwrap();
    assert_eq!(harness.project.apply_outside_changes(&[bank]).unwrap(), 3);
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.batches(), batches + 1);
    assert_eq!(harness.project.instances().count(), 0);
    let mut deleted = harness.project.drain_events();
    deleted.sort_by_key(|event| format!("{event:?}"));
    assert_eq!(
        deleted,
        ["bank", "bank/a", "bank/output"].map(|it| ProjectEvent::Deleted(id(it)))
    );

    // The same processor names are free again: the old ones are gone from the graph.
    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.5);
    assert!(harness.path("state/bank/output.json").exists());
}

#[test]
fn a_parent_created_with_children_from_the_interface_is_one_batch() {
    let mut harness = Harness::new();
    let batches = harness.batches();
    let mut changes = Changes::new();
    let bank = changes.create(id("bank"), Bank { gain: 1.0 });
    changes.create(bank.id().child("output").unwrap(), Amplifier { gain: 0.5 });
    changes.create(bank.id().child("a").unwrap(), Level { value: 0.5 });
    harness.project.commit("Add bank", changes).unwrap();
    assert_eq!(harness.batches(), batches + 1);
    assert_eq!(harness.level(), 0.25);
}
