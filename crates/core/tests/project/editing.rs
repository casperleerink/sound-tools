//! Interface edits: gestures, creation and deletion, undo and redo, and writing.

use std::os::unix::fs::PermissionsExt;

use sound_core::{Changes, PortReference, ProjectError, ProjectEvent, SavedConnection};

use crate::tools::{Amplifier, BANK_OUTPUT, Bank, Dc, Harness, Level, dc_record, id};

fn connected_dc(harness: &mut Harness, name: &str, value: f32) -> sound_core::Instance<Dc> {
    let mut changes = Changes::new();
    let dc = changes.create(id(name), Dc { value });
    changes.connect(SavedConnection::to_device(
        PortReference::new(dc.id(), "out"),
        0,
    ));
    harness.project.commit("Add dc", changes).unwrap();
    dc
}

#[test]
fn a_gesture_publishes_live_and_writes_once_at_the_end() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let saved = harness.read("state/dc.json");
    assert_eq!(
        saved,
        "{\n  \"tool\": \"test.dc\",\n  \"state\": {\"value\": 0.25}\n}\n"
    );
    harness.project.drain_events();

    let mut edit = harness.project.begin("Drag value");
    for value in [0.3, 0.4, 0.5] {
        harness
            .project
            .update(&mut edit, &dc, |state| state.value = value)
            .unwrap();
        // The processor follows during the gesture. The file does not.
        assert_eq!(harness.level(), value);
        assert_eq!(harness.read("state/dc.json"), saved);
    }
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
    harness.project.finish(edit).unwrap();
    assert!(harness.read("state/dc.json").contains("0.5"));
    assert_eq!(
        harness.project.drain_events(),
        vec![ProjectEvent::Changed(id("dc")); 3]
    );

    // One undo step for the whole gesture.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Drag value")
    );
    assert_eq!(harness.level(), 0.25);
    assert_eq!(harness.read("state/dc.json"), saved);
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
}

#[test]
fn cancel_restores_the_state_from_before_the_gesture() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let mut edit = harness.project.begin("Drag value");
    harness
        .project
        .update(&mut edit, &dc, |state| state.value = 0.75)
        .unwrap();
    assert_eq!(harness.level(), 0.75);
    harness.project.cancel(edit).unwrap();
    assert_eq!(harness.level(), 0.25);
    assert_eq!(harness.project.state(&dc), Some(&Dc { value: 0.25 }));
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
    assert_eq!(harness.project.redo_label(), None);
}

#[test]
fn a_file_edit_during_a_gesture_follows_last_write_wins() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let mut edit = harness.project.begin("Drag value");
    harness
        .project
        .update(&mut edit, &dc, |state| state.value = 0.3)
        .unwrap();

    // The file edit applies at once and does not end the gesture.
    harness.write_and_apply("state/dc.json", &dc_record(0.9));
    assert_eq!(harness.level(), 0.9);
    // The next drag update is the later write.
    harness
        .project
        .update(&mut edit, &dc, |state| state.value = 0.4)
        .unwrap();
    assert_eq!(harness.level(), 0.4);
    harness.project.finish(edit).unwrap();
    assert!(harness.read("state/dc.json").contains("0.4"));

    // The gesture undoes to where it began, over the file edit.
    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.25);
    assert!(harness.read("state/dc.json").contains("0.25"));
}

#[test]
fn an_invalid_interface_edit_is_rejected() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let mut edit = harness.project.begin("Too loud");
    let error = harness
        .project
        .update(&mut edit, &dc, |state| state.value = 2.0)
        .unwrap_err();
    assert!(
        matches!(error, ProjectError::InvalidState { .. }),
        "{error}"
    );
    harness.project.finish(edit).unwrap();
    assert_eq!(harness.level(), 0.25);
    assert_eq!(harness.project.undo_label(), Some("Add dc"));

    let mut changes = Changes::new();
    changes.create(id("nobody/child"), Dc { value: 0.0 });
    let error = harness.project.commit("Orphan", changes).unwrap_err();
    assert!(matches!(error, ProjectError::MissingParent(_)), "{error}");
}

#[test]
fn a_failed_write_leaves_the_previous_file_complete() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let saved = harness.read("state/dc.json");
    let state_folder = harness.path("state");
    let read_only = std::fs::Permissions::from_mode(0o555);
    let writable = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(&state_folder, read_only).unwrap();

    let mut edit = harness.project.begin("Drag value");
    harness
        .project
        .update(&mut edit, &dc, |state| state.value = 0.5)
        .unwrap();
    let error = harness.project.finish(edit).unwrap_err();
    std::fs::set_permissions(&state_folder, writable).unwrap();

    assert!(matches!(error, ProjectError::Storage(_)), "{error}");
    assert_eq!(harness.read("state/dc.json"), saved);
    assert_eq!(std::fs::read_dir(&state_folder).unwrap().count(), 1);
    let problem = harness.problem_at("state/dc.json").unwrap();
    assert!(problem.starts_with("not written"), "{problem}");
    // The edit is live and undoable. The next write brings the file up to date.
    assert_eq!(harness.level(), 0.5);
    harness.project.undo().unwrap();
    harness.project.redo().unwrap();
    assert!(harness.read("state/dc.json").contains("0.5"));
    assert_eq!(harness.project.problems(), []);
}

#[test]
fn creating_and_deleting_from_the_interface_is_undoable() {
    let mut harness = Harness::new();
    let mut changes = Changes::new();
    let bank = changes.create(id("bank"), Bank { gain: 1.0 });
    changes.create(bank.id().child("a").unwrap(), Level { value: 0.25 });
    harness.project.commit("Add bank", changes).unwrap();
    // An instance with children is a folder, so deleting the folder deletes all of it.
    assert!(harness.path("state/bank/instance.json").exists());
    assert!(harness.path("state/bank/a.json").exists());
    assert_eq!(harness.level(), 0.25);

    let mut changes = Changes::new();
    changes.delete(bank.id());
    harness.project.commit("Delete bank", changes).unwrap();
    assert_eq!(harness.level(), 0.0);
    assert!(!harness.path("state/bank").exists());
    assert_eq!(harness.project.instances().count(), 0);

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.25);
    assert!(harness.path("state/bank/a.json").exists());
    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.0);
    assert!(!harness.path("state/bank").exists());
    harness.project.redo().unwrap();
    assert_eq!(harness.level(), 0.25);
}

#[test]
fn the_tool_decides_the_form_of_the_record_for_good() {
    let mut harness = Harness::new();
    let mut changes = Changes::new();
    let bank = changes.create(id("bank"), Bank { gain: 0.5 });
    changes.create(id("dc"), Dc { value: 0.0 });
    harness.project.commit("Add", changes).unwrap();
    // An owner is a folder before it has any child. A tool that owns nothing is a file.
    assert!(harness.path("state/bank/instance.json").exists());
    assert!(!harness.path("state/bank.json").exists());
    assert!(harness.path("state/dc.json").exists());

    let mut changes = Changes::new();
    let output = bank.id().child(BANK_OUTPUT).unwrap();
    changes.create(output, Amplifier { gain: 2.0 });
    changes.create(bank.id().child("a").unwrap(), Level { value: 0.5 });
    harness.project.commit("Add children", changes).unwrap();
    assert!(harness.path("state/bank/instance.json").exists());
    assert!(!harness.path("state/bank.json").exists());
    assert_eq!(harness.level(), 0.5);

    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.project.instances().count(), 4);
    assert_eq!(harness.level(), 0.5);
}

#[test]
fn a_tool_that_owns_no_children_cannot_get_one() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let mut changes = Changes::new();
    changes.create(dc.id().child("a").unwrap(), Level { value: 0.5 });
    let error = harness.project.commit("Add child", changes).unwrap_err();
    assert!(
        matches!(
            error,
            ProjectError::ParentOwnsNoChildren {
                parent_tool: "test.dc",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(harness.project.instances().count(), 1);

    // The same from outside is a problem that names the reason.
    harness.write_and_apply("state/dc/a.json", &crate::tools::level_record(0.5));
    let problem = harness.problem_at("state/dc/a.json").unwrap();
    assert!(problem.contains("owns no children"), "{problem}");
    assert_eq!(harness.project.instances().count(), 1);
}

#[test]
fn undo_does_not_write_over_a_file_that_took_the_id() {
    let mut harness = Harness::new();
    connected_dc(&mut harness, "dc", 0.25);
    let path = harness.path("state/dc.json");
    std::fs::remove_file(&path).unwrap();
    harness
        .apply_outside_changes(std::slice::from_ref(&path))
        .unwrap();

    // An agent puts a record of a tool this runtime does not know at the same id.
    let unknown = r#"{"tool": "other.thing", "state": {}}"#;
    harness.write_and_apply("state/dc.json", unknown);
    assert_eq!(harness.project.undo_label(), Some("File change"));
    let error = harness.project.undo().unwrap_err();
    assert!(matches!(error, ProjectError::IdTaken(_)), "{error}");
    assert_eq!(harness.read("state/dc.json"), unknown);
    assert_eq!(harness.project.instances().count(), 0);
    // The step is dropped, so undo is not stuck on it.
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
}

#[test]
fn cancel_brings_back_an_instance_whose_file_was_never_removed() {
    let mut harness = Harness::new();
    let dc = connected_dc(&mut harness, "dc", 0.25);
    let mut edit = harness.project.begin("Delete");
    let mut changes = Changes::new();
    changes.delete(dc.id());
    harness.project.publish(&mut edit, changes).unwrap();
    assert_eq!(harness.level(), 0.0);
    // The file is still there, and it is the runtime's own: no `IdTaken`.
    harness.project.cancel(edit).unwrap();
    assert_eq!(harness.level(), 0.25);
}

#[test]
fn an_interface_edit_lands_on_top_of_an_outside_project_file_not_yet_delivered() {
    let mut harness = Harness::new();
    connected_dc(&mut harness, "first", 0.25);
    let mut changes = Changes::new();
    let second = changes.create(id("second"), Dc { value: 0.125 });
    let third = changes.create(id("third"), Dc { value: 0.0625 });
    harness.project.commit("Add two", changes).unwrap();

    // An agent connects `second`. The watcher has not delivered it yet.
    let connections = ["first", "second"]
        .map(crate::tools::dc_to_device)
        .join(", ");
    harness.write("project.json", &crate::tools::project_file(&connections));
    // Inside that window the interface connects `third`.
    let mut changes = Changes::new();
    changes.connect(SavedConnection::to_device(
        PortReference::new(third.id(), "out"),
        0,
    ));
    harness.project.commit("Connect third", changes).unwrap();

    assert_eq!(harness.project.project_file().connections.len(), 3);
    assert_eq!(harness.level(), 0.4375);
    let saved = harness.read("project.json");
    assert!(
        saved.contains("second") && saved.contains("third"),
        "{saved}"
    );
    // Two steps: the file change, then the interface edit.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Connect third")
    );
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.level(), 0.25);
    drop(second);
}

#[test]
fn a_project_file_that_did_not_load_is_not_written_over() {
    let mut harness = Harness::new();
    connected_dc(&mut harness, "dc", 0.25);
    let broken = crate::tools::project_file("").replace("120.0", "5.0");
    harness.write_and_apply("project.json", &broken);
    assert!(harness.problem_at("project.json").is_some());

    // The edit applies live. The broken file stays for correction, and the problem says so.
    let mut changes = Changes::new();
    let tempo_map = serde_json::from_str(
        r#"{"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 90.0}]}"#,
    )
    .unwrap();
    changes.set_tempo_map(tempo_map);
    harness.project.commit("Tempo", changes).unwrap();
    assert_eq!(
        harness.project.project_file().tempo_map.tempo_changes()[0]
            .bpm
            .bpm(),
        90.0
    );
    assert_eq!(harness.read("project.json"), broken);
    let problem = harness.problem_at("project.json").unwrap();
    assert!(problem.contains("tempo_map"), "{problem}");
    assert!(problem.contains("live but not written"), "{problem}");

    // Once the file loads, it is the later write, and writing works again.
    let fixed = crate::tools::project_file(&crate::tools::dc_to_device("dc"));
    harness.write_and_apply("project.json", &fixed);
    assert_eq!(harness.project.problems(), []);
    assert_eq!(
        harness.project.project_file().tempo_map.tempo_changes()[0]
            .bpm
            .bpm(),
        120.0
    );
    harness.project.undo().unwrap();
    assert!(harness.read("project.json").contains("90.0"));
}

#[test]
fn deleting_an_instance_removes_its_saved_connections_in_the_same_step() {
    let mut harness = Harness::new();
    connected_dc(&mut harness, "keep", 0.125);
    let dc = connected_dc(&mut harness, "dc", 0.25);
    assert_eq!(harness.level(), 0.375);

    let mut changes = Changes::new();
    changes.delete(dc.id());
    harness.project.commit("Delete dc", changes).unwrap();
    assert_eq!(harness.project.project_file().connections.len(), 1);
    assert!(!harness.read("project.json").contains("\"dc\""));
    assert_eq!(harness.level(), 0.125);

    harness.project.undo().unwrap();
    assert_eq!(harness.project.project_file().connections.len(), 2);
    assert_eq!(harness.level(), 0.375);
}
