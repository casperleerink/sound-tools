//! Exporting by key and label: the dialog and its kept choice, the save panel, and what an
//! export that ends says. A stand-in program renders here; `tests/export.rs` renders for real.

use std::path::PathBuf;

use rune::FileDialogKind;

use super::*;
use crate::app::frame::poll;

/// A new project of the DAW that keeps its export choice in `support/`.
fn exporting() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    state.export.kept = Some(scratch.path().join("support/export-settings.json"));
    (scratch, laid_out(state))
}

/// A frame as the app runs it, which opens the save panel the frame before asked for.
fn polled(window: &mut rune::Runtime<State>) {
    let seconds = window.state.last + 1.0 / 60.0;
    window.update(|state, tx| {
        poll(state, seconds, tx);
    });
    window.frame(1.0 / 60.0);
}

fn press(window: &mut rune::Runtime<State>, label: &str) {
    act(
        &mut window.state,
        json!({ "label": label, "action": "press" }),
    );
}

/// The labels of the chosen choices of the export dialog, top to bottom.
fn chosen(state: &State) -> Vec<String> {
    let controls = controls::collect(state).controls.into_iter();
    controls
        .filter(|control| control.key.starts_with("export-") && control.checked == Some(true))
        .map(|control| control.label)
        .collect()
}

/// Opens the dialog and exports with the choice it shows into `file`.
fn export_to(window: &mut rune::Runtime<State>, file: PathBuf) {
    act(&mut window.state, json!({ "action": "app.export" }));
    press(window, "Export…");
    polled(window);
    window.resolve_file_dialog(Some(file)).unwrap();
    window.frame(1.0 / 60.0);
}

#[test]
fn the_export_dialog_starts_from_the_last_choice_and_a_wav_keeps_its_depth_through_mp3() {
    let (scratch, mut window) = exporting();
    std::fs::create_dir_all(scratch.path().join("support")).unwrap();
    std::fs::write(window.state.export.kept.clone().unwrap(), "not settings").unwrap();
    act(&mut window.state, json!({ "action": "app.export" }));
    assert_eq!(
        chosen(&window.state),
        ["WAV", "48 kHz", "32-bit float", "As it plays"],
        "unreadable, so the defaults"
    );
    press(&mut window, "16-bit");
    press(&mut window, "MP3");
    assert_eq!(chosen(&window.state), ["MP3", "48 kHz", "As it plays"]);
    press(&mut window, "WAV");
    press(&mut window, "Streaming");
    assert_eq!(
        chosen(&window.state),
        ["WAV", "48 kHz", "16-bit", "Streaming"]
    );
    press(&mut window, "Export…");
    assert!(chosen(&window.state).is_empty(), "Export closes it");

    act(&mut window.state, json!({ "action": "app.export" }));
    assert_eq!(
        chosen(&window.state),
        ["WAV", "48 kHz", "16-bit", "Streaming"]
    );
    press(&mut window, "Cancel");
    assert!(chosen(&window.state).is_empty());
}

#[test]
fn the_save_panel_opens_next_to_the_project_and_refuses_another_ending() {
    let (scratch, mut window) = exporting();
    act(&mut window.state, json!({ "action": "app.export" }));
    press(&mut window, "Export…");
    polled(&mut window);
    let (kind, options) = window.file_dialog().expect("a save panel");
    let next_to = window.state.project().root().parent().map(PathBuf::from);
    assert_eq!(
        (
            kind,
            options.file_name.as_deref(),
            options.directory.clone()
        ),
        (FileDialogKind::Save, Some("song.wav"), next_to)
    );
    window
        .resolve_file_dialog(Some(scratch.path().join("song.mp3")))
        .unwrap();
    window.frame(1.0 / 60.0);
    assert_eq!(
        window.state.session.notice(),
        Some("The name must end in .wav.")
    );
    assert_eq!(window.state.session.rendering(), None);
}

#[test]
fn a_project_where_nothing_plays_to_an_end_has_nothing_to_export() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("empty");
    runtime::templates::write(&folder, runtime::templates::named("Bare").unwrap()).unwrap();
    let mut state = opened(&folder);
    act(&mut state, json!({ "action": "app.export" }));
    assert_eq!(
        state.session.notice(),
        Some(
            "Nothing in the project plays to an end, such as a clip, so there is nothing to export."
        )
    );
    assert!(chosen(&state).is_empty());
}

/// A program in place of this one's `--render`, which says `body` and does nothing else.
#[cfg(unix)]
pub(super) fn stand_in(folder: &std::path::Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = folder.join("render");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn an_export_that_ends_says_what_it_left_out_and_one_at_a_time() {
    let (scratch, mut window) = exporting();
    window.state.renders.program = stand_in(
        scratch.path(),
        "echo 'progress: 50'\necho 'note: It is quieter than streaming plays it.'\necho 'error: “Room” did not load'",
    );
    let file = scratch.path().join("song.wav");
    export_to(&mut window, file.clone());
    assert_eq!(
        window.state.session.rendering().map(|(notice, _)| notice),
        Some("Exporting song.wav")
    );
    act(&mut window.state, json!({ "action": "app.export" }));
    assert_eq!(
        window.state.session.notice(),
        Some("Another render is running: try again once it is done.")
    );
    assert!(chosen(&window.state).is_empty());

    polled(&mut window);
    assert_eq!(window.state.session.rendering(), None);
    let labels: Vec<String> = (controls::collect(&window.state).controls.into_iter())
        .filter(|control| control.key.starts_with("exported-"))
        .map(|control| control.label)
        .collect();
    assert_eq!(labels, [super::super::export::REVEAL, "OK"]);
    press(&mut window, super::super::export::REVEAL);
    assert_eq!(window.state.export.revealed, Some(file));
}

#[cfg(unix)]
#[test]
fn an_export_that_fails_says_why_in_the_corner() {
    let (scratch, mut window) = exporting();
    window.state.renders.program =
        stand_in(scratch.path(), "echo 'Error: the disk is full' >&2\nexit 1");
    export_to(&mut window, scratch.path().join("song.wav"));
    polled(&mut window);
    assert_eq!(
        window.state.session.notice(),
        Some("The export failed: the disk is full")
    );
}
