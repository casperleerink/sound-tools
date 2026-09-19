//! Changes that come from outside: record edits, new files and folders, deletions, moves and
//! `project.json`.

use sound_core::{
    Changes, Engine, EngineConfig, PortReference, Project, ProjectError, ProjectEvent,
    SavedConnection, Ticks,
};

use crate::tools::{
    BANK_RECORD, Dc, Harness, SAMPLE_RATE, dc_record, dc_to_device, id, level_record, project_file,
    registry,
};

/// A project with one connected `test.dc` at 0.25.
fn one_dc() -> Harness {
    let mut harness = Harness::new();
    let mut changes = Changes::new();
    let dc = changes.create(id("dc"), Dc { value: 0.25 });
    changes.connect(SavedConnection::to_device(
        PortReference::new(dc.id(), "out"),
        0,
    ));
    harness.project.commit("Add dc", changes).unwrap();
    harness.project.drain_events();
    harness
}

#[test]
fn an_outside_record_edit_applies_live_as_one_undo_step() {
    let mut harness = one_dc();
    assert_eq!(harness.level(), 0.25);
    let batches = harness.batches();

    assert_eq!(harness.write_and_apply("state/dc.json", &dc_record(0.5)), 1);
    assert_eq!(harness.level(), 0.5);
    assert_eq!(harness.batches(), batches + 1);
    assert_eq!(
        harness.project.drain_events(),
        [ProjectEvent::Changed(id("dc"))]
    );
    assert_eq!(harness.project.undo_label(), Some("File change"));

    // Undo restores the state and rewrites the file.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("File change")
    );
    assert_eq!(harness.level(), 0.25);
    assert!(harness.read("state/dc.json").contains("0.25"));
    harness.project.redo().unwrap();
    assert_eq!(harness.level(), 0.5);
    assert!(harness.read("state/dc.json").contains("0.5"));
}

#[test]
fn the_runtimes_own_writes_do_not_apply_again() {
    let mut harness = one_dc();
    let dc = harness.project.resolve::<Dc>(&id("dc")).unwrap();
    let mut edit = harness.project.begin("Louder");
    harness
        .project
        .update(&mut edit, &dc, |state| state.value = 0.75)
        .unwrap();
    harness.project.finish(edit).unwrap();
    harness.project.undo().unwrap();
    harness.project.drain_events();
    let batches = harness.batches();

    // The watcher reports the files the runtime wrote itself.
    let paths = [harness.path("state/dc.json"), harness.path("project.json")];
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 0);
    assert_eq!(harness.project.drain_events(), []);
    assert_eq!(harness.batches(), batches);
    assert_eq!(harness.project.redo_label(), Some("Louder"));
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
}

#[test]
fn a_rewrite_with_other_spacing_is_no_change() {
    let mut harness = one_dc();
    let compact = r#"{"tool":"test.dc","state":{"value":0.25}}"#;
    assert_eq!(harness.write_and_apply("state/dc.json", compact), 0);
    assert_eq!(harness.project.undo_label(), Some("Add dc"));
    // The file is the agent's. The runtime does not write it back in its own layout.
    assert_eq!(harness.read("state/dc.json"), compact);
}

#[test]
fn a_new_record_file_creates_an_instance_and_undo_deletes_it() {
    let mut harness = one_dc();
    harness.write("state/second.json", &dc_record(0.125));
    let project_path = harness.write(
        "project.json",
        &project_file(&[dc_to_device("dc"), dc_to_device("second")].join(", ")),
    );
    let batches = harness.batches();
    let paths = [harness.path("state/second.json"), project_path];
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 2);

    // The record and its connection arrive together: one batch, one undo step.
    assert_eq!(harness.level(), 0.375);
    assert_eq!(harness.batches(), batches + 1);
    assert_eq!(
        harness.project.drain_events(),
        [
            ProjectEvent::Created(id("second")),
            ProjectEvent::ProjectFileChanged
        ]
    );
    assert!(harness.project.resolve::<Dc>(&id("second")).is_some());

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.25);
    assert!(!harness.path("state/second.json").exists());
    assert!(!harness.read("project.json").contains("second"));
    assert!(harness.project.resolve::<Dc>(&id("second")).is_none());

    harness.project.redo().unwrap();
    assert_eq!(harness.level(), 0.375);
    assert!(harness.path("state/second.json").exists());
    assert!(harness.read("project.json").contains("second"));
}

#[test]
fn a_connection_that_arrives_before_its_instance_waits_for_it() {
    let mut harness = one_dc();
    let connections = [dc_to_device("dc"), dc_to_device("late")].join(", ");
    harness.write_and_apply("project.json", &project_file(&connections));
    let problem = harness.problem_at("project.json").unwrap();
    assert!(problem.contains("connections[1]"), "{problem}");
    assert!(problem.contains("\"late\""), "{problem}");
    assert_eq!(harness.level(), 0.25);

    harness.write_and_apply("state/late.json", &dc_record(0.5));
    assert_eq!(harness.level(), 0.75);
    assert_eq!(harness.project.problems(), []);
}

#[test]
fn deleting_a_record_file_deletes_the_instance_and_its_connections() {
    let mut harness = one_dc();
    std::fs::remove_file(harness.path("state/dc.json")).unwrap();
    let paths = [harness.path("state/dc.json")];
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 2);
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.project.instances().count(), 0);
    assert!(!harness.read("project.json").contains("\"dc\""));

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.25);
    assert!(harness.path("state/dc.json").exists());
    assert!(harness.read("project.json").contains("\"dc\""));
}

#[test]
fn a_whole_folder_with_children_arrives_as_one_step() {
    let mut harness = Harness::new();
    harness.write("state/bank/instance.json", BANK_RECORD);
    harness.write("state/bank/a.json", &level_record(0.25));
    harness.write("state/bank/b.json", &level_record(0.5));
    let batches = harness.batches();

    // macOS reports a moved-in folder as the one folder path.
    let paths = [harness.path("state/bank")];
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 3);
    assert_eq!(harness.level(), 0.75);
    assert_eq!(harness.batches(), batches + 1);
    let created: Vec<_> = harness.project.drain_events();
    assert_eq!(
        created,
        ["bank", "bank/a", "bank/b"].map(|it| ProjectEvent::Created(id(it)))
    );

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.0);
    assert!(!harness.path("state/bank").exists());
    harness.project.redo().unwrap();
    assert_eq!(harness.level(), 0.75);
    assert!(harness.path("state/bank/instance.json").exists());
    assert!(harness.path("state/bank/b.json").exists());
}

#[test]
fn several_files_that_arrive_together_are_one_undo_step() {
    let mut harness = Harness::new();
    harness.write_and_apply("state/bank/instance.json", BANK_RECORD);
    let paths: Vec<_> = (0..8)
        .map(|index| {
            harness.write(
                &format!("state/bank/level-{index}.json"),
                &level_record(0.0625),
            )
        })
        .collect();
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 8);
    assert_eq!(harness.level(), 0.5);

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.0);
    assert_eq!(harness.project.instances().count(), 1);
    assert_eq!(harness.project.undo_label(), Some("File change"));
}

#[test]
fn moving_a_record_to_another_owner_is_one_step() {
    let mut harness = Harness::new();
    harness.write("state/left/instance.json", BANK_RECORD);
    harness.write("state/left/part.json", &level_record(0.5));
    harness.write(
        "state/right/instance.json",
        r#"{"tool": "test.bank", "state": {"gain": 0.5}}"#,
    );
    let state = harness.path("state");
    harness.project.apply_outside_changes(&[state]).unwrap();
    assert_eq!(harness.level(), 0.5);

    let (from, to) = (
        harness.path("state/left/part.json"),
        harness.path("state/right/part.json"),
    );
    std::fs::rename(&from, &to).unwrap();
    assert_eq!(
        harness.project.apply_outside_changes(&[from, to]).unwrap(),
        2
    );
    assert_eq!(harness.level(), 0.25);

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.5);
    assert!(harness.path("state/left/part.json").exists());
    assert!(!harness.path("state/right/part.json").exists());
}

#[test]
fn invalid_files_leave_the_live_state_and_name_the_field() {
    let mut harness = one_dc();
    let cases = [
        (
            r#"{"tool": "test.dc", "state": {"value": "loud"}}"#,
            "state.value",
        ),
        (
            r#"{"tool": "test.dc", "state": {"value": 3.0}}"#,
            "value must be from -1 to 1",
        ),
        (
            r#"{"tool": "test.dc", "state": {"value": 0.5, "extra": 1}}"#,
            "unknown field `extra`",
        ),
        (r#"{"tool": "test.dc", "state": {"value": 0.5}"#, "EOF"),
    ];
    for (contents, expected) in cases {
        assert_eq!(harness.write_and_apply("state/dc.json", contents), 0);
        let problem = harness.problem_at("state/dc.json").unwrap();
        assert!(problem.contains(expected), "{problem}");
        assert_eq!(harness.level(), 0.25);
        // The file stays for correction.
        assert_eq!(harness.read("state/dc.json"), contents);
    }
    assert_eq!(harness.project.undo_label(), Some("Add dc"));

    harness.write_and_apply("state/dc.json", &dc_record(0.5));
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.level(), 0.5);
}

#[test]
fn records_of_unknown_tools_stay_untouched() {
    let mut harness = one_dc();
    let unknown = r#"{"tool": "other.thing", "state": {"anything": [1, 2]}}"#;
    harness.write("state/thing/instance.json", unknown);
    harness.write("state/thing/child.json", &dc_record(0.5));
    let paths = [harness.path("state/thing")];
    assert_eq!(harness.project.apply_outside_changes(&paths).unwrap(), 0);
    let problem = harness.problem_at("state/thing/instance.json").unwrap();
    assert!(
        problem.contains("unknown tool \"other.thing\""),
        "{problem}"
    );
    assert!(harness.problem_at("state/thing/child.json").is_some());

    // The id is taken, so the interface cannot write over the record.
    let mut changes = Changes::new();
    changes.create(id("thing"), Dc { value: 0.0 });
    let error = harness.project.commit("Overwrite", changes).unwrap_err();
    assert!(matches!(error, ProjectError::IdTaken(_)), "{error}");

    let harness = harness.reopen();
    assert_eq!(harness.read("state/thing/instance.json"), unknown);
    assert!(harness.problem_at("state/thing/instance.json").is_some());
    assert_eq!(harness.project.instances().count(), 1);
}

#[test]
fn a_folder_without_a_record_is_reported_until_the_record_arrives() {
    let mut harness = Harness::new();
    harness.write_and_apply("state/bank/a.json", &level_record(0.5));
    assert!(harness.problem_at("state/bank/a.json").is_some());
    assert_eq!(harness.project.instances().count(), 0);

    // The record of the folder arrives later. Its children load with it.
    assert_eq!(
        harness.write_and_apply("state/bank/instance.json", BANK_RECORD),
        2
    );
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.level(), 0.5);
}

#[test]
fn project_file_changes_apply_live() {
    let mut harness = one_dc();
    harness.project.engine().play();
    harness.level();

    // Disconnect and halve the tempo in one outside edit.
    let slow = project_file("").replace("120.0", "60.0");
    assert_eq!(harness.write_and_apply("project.json", &slow), 1);
    assert_eq!(harness.level(), 0.0);
    let clock = harness.project.engine().clock().clone();
    assert_eq!(clock.frame_of(Ticks(960)).0, u64::from(SAMPLE_RATE));
    assert_eq!(
        harness.project.drain_events(),
        [ProjectEvent::ProjectFileChanged]
    );

    harness.project.undo().unwrap();
    assert_eq!(harness.level(), 0.25);
    let clock = harness.project.engine().clock().clone();
    assert_eq!(clock.frame_of(Ticks(960)).0, u64::from(SAMPLE_RATE / 2));
    assert!(harness.read("project.json").contains("120.0"));
}

#[test]
fn an_invalid_project_file_leaves_the_live_state() {
    let mut harness = one_dc();
    let broken = project_file(&dc_to_device("dc")).replace("120.0", "5.0");
    assert_eq!(harness.write_and_apply("project.json", &broken), 0);
    let problem = harness.problem_at("project.json").unwrap();
    assert!(problem.contains("tempo_map"), "{problem}");
    assert_eq!(harness.level(), 0.25);
    assert_eq!(harness.read("project.json"), broken);
}

#[test]
fn a_cycle_in_project_json_is_rejected_whole() {
    let mut harness = Harness::new();
    for name in ["first", "second"] {
        let record = r#"{"tool": "test.amplifier", "state": {"gain": 1.0}}"#;
        harness.write_and_apply(&format!("state/{name}.json"), record);
    }
    let link = |from: &str, to: &str| {
        format!(
            r#"{{"from": {{"instance": "{from}", "port": "out"}}, "to": {{"input": {{"instance": "{to}", "port": "in"}}}}}}"#
        )
    };
    let cyclic = project_file(&[link("first", "second"), link("second", "first")].join(", "));
    let path = harness.write("project.json", &cyclic);
    let error = harness.project.apply_outside_changes(&[path]).unwrap_err();
    assert!(matches!(error, ProjectError::Graph(_)), "{error}");
    assert!(harness.project.project_file().connections.is_empty());
    assert!(
        harness
            .problem_at("project.json")
            .unwrap()
            .contains("cycle")
    );

    let fixed = project_file(&link("first", "second"));
    assert_eq!(harness.write_and_apply("project.json", &fixed), 1);
    assert_eq!(harness.project.problems(), []);
}

#[test]
fn reopening_restores_instances_children_connections_and_tempo() {
    let mut harness = one_dc();
    harness.write("state/bank/instance.json", BANK_RECORD);
    harness.write("state/bank/a.json", &level_record(0.125));
    harness.write(
        "state/bank/output.json",
        r#"{"tool": "test.amplifier", "state": {"gain": 2.0}}"#,
    );
    let bank = harness.path("state/bank");
    harness.project.apply_outside_changes(&[bank]).unwrap();
    let mut changes = Changes::new();
    let tempo_map = serde_json::from_str(
        r#"{"time_signature": "3/4", "tempo_changes": [{"tick": 0, "bpm": 90.0}]}"#,
    )
    .unwrap();
    changes.set_tempo_map(tempo_map);
    harness.project.commit("Tempo", changes).unwrap();
    assert_eq!(harness.level(), 0.5);
    let before: Vec<_> = harness
        .project
        .instances()
        .map(|(id, tool)| (id.clone(), tool))
        .collect();
    let project_file = harness.project.project_file().clone();

    let mut harness = harness.reopen();
    let after: Vec<_> = harness
        .project
        .instances()
        .map(|(id, tool)| (id.clone(), tool))
        .collect();
    assert_eq!(after, before);
    assert_eq!(after.len(), 4);
    assert_eq!(*harness.project.project_file(), project_file);
    assert_eq!(harness.level(), 0.5);
    assert_eq!(
        harness.project.engine().clock().tempo_map(),
        &project_file.tempo_map
    );
    // History is for the session only, and loading is not a step.
    assert_eq!(harness.project.undo_label(), None);
    assert_eq!(harness.project.drain_events(), []);
}

#[test]
fn a_second_open_of_the_same_folder_fails_with_a_typed_error() {
    let harness = one_dc();
    let open_again = || {
        let (control, _engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
        Project::open(harness.folder.path(), registry(), control)
    };
    assert!(matches!(open_again(), Err(ProjectError::AlreadyOpen(_))));

    // Reading next to the running project works, and cannot write.
    let (control, _engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let mut reader = Project::open_read_only(harness.folder.path(), registry(), control).unwrap();
    assert_eq!(reader.instances().count(), 1);
    let error = reader.commit("Nothing", Changes::new()).unwrap_err();
    assert!(matches!(error, ProjectError::ReadOnly), "{error}");

    let folder = harness.folder.path().to_path_buf();
    let Harness {
        project,
        folder: _keep,
        ..
    } = harness;
    drop(project);
    let (control, _engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    assert!(Project::open(&folder, registry(), control).is_ok());
}
