//! A tool may say where its instances live. A record somewhere else is not loaded and the
//! problem says where it belongs, so an agent that forgot a folder gets a signal.

use sound_core::{Changes, ProjectError};

use crate::tools::{BANK_RECORD, BOOK_RECORD, Book, Harness, SHELF_RECORD, Shelf, id};

#[test]
fn a_record_outside_its_place_is_reported_with_where_it_belongs() {
    let mut harness = Harness::new();
    harness.write_and_apply("state/shelf/instance.json", SHELF_RECORD);
    harness.write_and_apply("state/bank/instance.json", BANK_RECORD);
    assert_eq!(
        harness.write_and_apply("state/shelf/novel.json", BOOK_RECORD),
        1
    );

    let cases = [
        (
            "state/lost.json",
            BOOK_RECORD,
            "not loaded: an instance of \"test.book\" belongs directly inside an instance of \"test.shelf\", not at the top of state/",
        ),
        (
            "state/bank/lost.json",
            BOOK_RECORD,
            "not loaded: an instance of \"test.book\" belongs directly inside an instance of \"test.shelf\", and its owner here is a \"test.bank\"",
        ),
        (
            "state/bank/shelf/instance.json",
            SHELF_RECORD,
            "not loaded: an instance of \"test.shelf\" belongs at the top of state/, not inside another instance",
        ),
    ];
    for (path, record, message) in cases {
        assert_eq!(harness.write_and_apply(path, record), 0, "{path}");
        assert_eq!(harness.problem_at(path).as_deref(), Some(message));
    }
    assert_eq!(harness.project.instances().count(), 3);

    // Moving the file to its place loads it and takes the problem away.
    let (from, to) = (
        harness.path("state/lost.json"),
        harness.path("state/shelf/lost.json"),
    );
    std::fs::rename(&from, &to).unwrap();
    assert_eq!(harness.apply_outside_changes(&[from, to]).unwrap(), 1);
    assert_eq!(harness.problem_at("state/lost.json"), None);
    assert!(harness.project.resolve::<Book>(&id("shelf/lost")).is_some());
}

#[test]
fn a_folder_that_arrives_whole_checks_places_inside_it_and_a_reopen_reports_the_same() {
    let mut harness = Harness::new();
    harness.write("state/shelf/instance.json", SHELF_RECORD);
    harness.write("state/shelf/novel.json", BOOK_RECORD);
    harness.write("state/stray.json", BOOK_RECORD);
    let state = harness.path("state");
    assert_eq!(harness.apply_outside_changes(&[state]).unwrap(), 2);
    assert!(harness.problem_at("state/stray.json").is_some());
    let harness = harness.reopen();
    assert_eq!(harness.project.instances().count(), 2);
    assert!(harness.problem_at("state/stray.json").is_some());
}

#[test]
fn an_interface_edit_outside_the_place_is_a_typed_error() {
    let mut harness = Harness::new();
    let mut changes = Changes::new();
    changes.create(id("lost"), Book {});
    let error = harness.project.commit("Add book", changes).unwrap_err();
    assert!(matches!(error, ProjectError::WrongPlace { .. }), "{error}");

    let mut changes = Changes::new();
    let shelf = changes.create(id("shelf"), Shelf {});
    changes.create(shelf.id().child("novel").unwrap(), Book {});
    harness.project.commit("Add shelf", changes).unwrap();
    assert_eq!(harness.project.instances().count(), 2);
}
