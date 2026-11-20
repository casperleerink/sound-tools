//! The start state and the new project, driven by key and label.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use rune::{Key, KeyEvent, Mods};

use super::start::{Start, on_key, poll, view};
use super::{Dialog, Host};
use crate::app::MAIN_MODIFIER;

/// The start state with `support` as the machine's support folder.
fn window(support: &Path) -> rune::Runtime<Start> {
    let fonts = rune::text::TextSystem::new(&crate::FONTS, ("Inter Display", "ui-monospace"));
    let env = rune::Env::new(true, 1.0);
    let start = Start::new(Some(support.to_path_buf()), false, None);
    let mut window = rune::Runtime::with_text(start, view, (760.0, 560.0), env, fonts);
    window.on_key = Some(on_key);
    window.frame(0.0);
    // The app's poll, which puts the keys on the first choice.
    window.update(|start, tx| {
        poll(start, 0.0, tx);
    });
    window.frame(1.0 / 60.0);
    window
}

fn press(window: &mut rune::Runtime<Start>, key: Key) {
    window.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    window.frame(1.0 / 60.0);
}

fn chord(window: &mut rune::Runtime<Start>, c: char) {
    let mods = MAIN_MODIFIER;
    window.key(KeyEvent {
        key: Key::Command(c),
        mods,
    });
    window.frame(1.0 / 60.0);
}

fn type_text(window: &mut rune::Runtime<Start>, text: &str) {
    for letter in text.chars() {
        press(window, Key::Text(letter.into()));
    }
}

fn focused(window: &rune::Runtime<Start>) -> Option<String> {
    let elements = window.elements();
    (elements.into_iter())
        .find(|element| element.focused)
        .and_then(|element| element.key)
}

fn tab_to(window: &mut rune::Runtime<Start>, key: &str) {
    for _ in 0..30 {
        if focused(window).as_deref() == Some(key) {
            return;
        }
        press(window, Key::Tab);
    }
    panic!("Tab never reaches {key}");
}

/// A project of the DAW in `folder`.
fn project(folder: &Path) -> PathBuf {
    runtime::templates::write(folder, &crate::app::TEMPLATE).unwrap();
    dunce::canonicalize(folder).unwrap()
}

/// What the element `key` says.
fn said(window: &rune::Runtime<Start>, key: &str) -> String {
    let elements = window.elements().into_iter();
    let element = elements
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key));
    element
        .and_then(|element| element.text)
        .unwrap_or_else(|| panic!("no {key}"))
}

fn opened(window: &rune::Runtime<Start>) -> Option<&Path> {
    window.state.shell().opened.as_deref()
}

#[test]
fn enter_opens_the_last_project_and_remembers_it_first() {
    let scratch = tempfile::tempdir().unwrap();
    let support = scratch.path().join("support");
    let (song, demo) = (
        project(&scratch.path().join("song")),
        project(&scratch.path().join("demo")),
    );
    runtime::app::remember_project_in(&support, &demo).unwrap();
    runtime::app::remember_project_in(&support, &song).unwrap();
    let mut window = window(&support);
    window.frame(0.1);
    assert!(
        window.find("recent-1").is_some(),
        "both projects are listed"
    );
    press(&mut window, Key::Enter);
    assert_eq!(opened(&window), Some(song.as_path()));
    press(&mut window, Key::Down);
    press(&mut window, Key::Enter);
    assert_eq!(opened(&window), Some(demo.as_path()));
    assert_eq!(runtime::app::recent_projects_in(&support), [demo, song]);
}

#[test]
fn a_folder_of_other_files_is_refused_in_plain_words() {
    let scratch = tempfile::tempdir().unwrap();
    let papers = scratch.path().join("Papers");
    std::fs::create_dir(&papers).unwrap();
    std::fs::write(papers.join("tax.pdf"), "").unwrap();
    let mut window = window(&scratch.path().join("support"));
    chord(&mut window, 'o');
    let (kind, options) = window.file_dialog().unwrap();
    assert_eq!(
        (kind, options.title.as_str()),
        (rune::FileDialogKind::Folder, "Open a project")
    );
    window.resolve_file_dialog(Some(papers.clone())).unwrap();
    window.frame(0.1);
    assert_eq!(opened(&window), None);
    let shown = said(&window, "start-message");
    assert!(
        shown.starts_with("“Papers” has files in it and is not a Sound Tools project."),
        "{shown}"
    );
    assert_eq!(std::fs::read_dir(&papers).unwrap().count(), 1);

    let gone = scratch.path().join("gone");
    chord(&mut window, 'o');
    window.resolve_file_dialog(Some(gone.clone())).unwrap();
    window.frame(0.1);
    assert_eq!(
        said(&window, "start-message"),
        format!("{} is not there anymore.", gone.display())
    );
    assert!(!gone.exists());
}

#[test]
fn new_project_lists_the_templates_keys_pick_and_escape_makes_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let mut window = window(&scratch.path().join("support"));
    chord(&mut window, 'n');
    let labels: Vec<String> = (window.state.shell().controls().into_iter())
        .filter(|control| control.role == "radio")
        .map(|control| control.label)
        .collect();
    assert_eq!(labels, ["DAW (new window)", "Bare"]);
    assert_eq!(focused(&window).as_deref(), Some("input"));
    tab_to(&mut window, "template-1");
    press(&mut window, Key::Text(" ".into()));
    let picked = (window.state.shell().controls().into_iter())
        .find(|control| control.checked == Some(true))
        .unwrap();
    assert_eq!(picked.label, "Bare");
    press(&mut window, Key::Escape);
    assert!(window.state.shell().dialog.is_none());
    assert!(window.find("new-project-dialog").is_none());
    assert_eq!(focused(&window).as_deref(), Some("choose-folder"));
}

#[test]
fn a_new_project_takes_a_free_name_and_opens_where_it_was_made() {
    let scratch = tempfile::tempdir().unwrap();
    let music = dunce::canonicalize(scratch.path()).unwrap();
    project(&music.join("Night drive"));
    let mut window = window(&music.join("support"));
    chord(&mut window, 'n');
    // In the folder of the last project, else home: here, pick it in the folder panel.
    let place = window.find("project-place").unwrap();
    window.pointer_down(place.0, place.1);
    window.pointer_up(place.0, place.1);
    window.frame(0.1);
    window.resolve_file_dialog(Some(music.clone())).unwrap();
    window.frame(0.1);
    type_text(&mut window, "Night drive");
    press(&mut window, Key::Enter);
    let note = said(&window, "new-project-note");
    assert_eq!(note, "“Night drive” has files in it. Pick another name.");
    assert_eq!(opened(&window), None);
    for _ in 0.."Night drive".len() {
        press(&mut window, Key::Backspace);
    }
    type_text(&mut window, "Day/off");
    let note = said(&window, "new-project-note");
    assert_eq!(
        note,
        "“Day/off” cannot be the name of a project. Pick another name."
    );
    for _ in 0..4 {
        press(&mut window, Key::Backspace);
    }
    type_text(&mut window, " off");
    press(&mut window, Key::Enter);
    let made = music.join("Day off");
    assert_eq!(opened(&window), Some(made.as_path()));
    assert!(made.join(runtime::PROJECT_FILE).is_file());
    assert!(matches!(window.state.shell().dialog, None::<Dialog>));
}

#[test]
fn the_new_project_names_its_place_as_open_recent_does() {
    let scratch = tempfile::tempdir().unwrap();
    let mut window = window(&scratch.path().join("support"));
    chord(&mut window, 'n');
    assert_eq!(said(&window, "project-place-words"), "In ~");
}
