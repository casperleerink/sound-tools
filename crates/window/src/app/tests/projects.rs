//! The projects of the window, as an agent reaches them by `--act`: New project, Save as
//! template, Open project, and what an empty project shows.

use crate::shell::{Dialog, Projects};

use super::*;
use crate::app::MAIN_MODIFIER;

/// The window of a new project in `scratch`, keeping templates in its `support`.
fn kept(scratch: &tempfile::TempDir) -> State {
    let mut state = opened(&scratch.path().join("song"));
    let here = state.project().root().to_path_buf();
    state.shell = Projects::new(Some(scratch.path().join("support")), true, Some(here));
    state
}

fn saved(scratch: &tempfile::TempDir) -> Vec<String> {
    let saved = runtime::templates::saved_in(&scratch.path().join("support/templates"));
    saved.unwrap().into_iter().map(|saved| saved.name).collect()
}

fn keys(state: &State) -> Vec<String> {
    let controls = controls::collect(state).controls.into_iter();
    controls.map(|control| control.key).collect()
}

#[test]
fn save_as_template_keeps_tools_or_content_and_asks_before_it_replaces() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    act(&mut state, json!({ "action": "app.save-as-template" }));
    let name = control(&controls::collect(&state).controls, "Name")
        .value
        .clone();
    assert_eq!(name, Some(json!("song")));
    act(
        &mut state,
        json!({ "key": "template-name", "action": "commit", "value": "Beats" }),
    );
    act(
        &mut state,
        json!({ "label": "Tools and content", "action": "press" }),
    );
    act(&mut state, json!({ "label": "Save", "action": "press" }));
    assert_eq!(saved(&scratch), ["Beats"]);
    assert!(state.shell.dialog.is_none());

    act(&mut state, json!({ "action": "app.save-as-template" }));
    act(
        &mut state,
        json!({ "key": "template-name", "action": "commit", "value": "Bare" }),
    );
    assert!(
        !keys(&state).contains(&"save".to_string()),
        "the app's name is refused"
    );
    act(
        &mut state,
        json!({ "key": "template-name", "action": "commit", "value": "Beats" }),
    );
    act(&mut state, json!({ "label": "Replace", "action": "press" }));
    assert_eq!(saved(&scratch), ["Beats"]);
}

#[test]
fn a_new_project_goes_next_to_this_one_and_bare_opens_empty() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    act(&mut state, json!({ "action": "app.new-project" }));
    act(&mut state, json!({ "label": "Bare", "action": "press" }));
    act(
        &mut state,
        json!({ "key": "project-name", "action": "commit", "value": "Sketch" }),
    );
    act(&mut state, json!({ "label": "Create", "action": "press" }));
    let here = state.project().root().to_path_buf();
    let sketch = here.parent().unwrap().join("Sketch");
    assert_eq!(state.shell.opened.as_deref(), Some(sketch.as_path()));
    let sketched = opened(&sketch);
    assert_eq!(
        sketched.project().instances().count(),
        0,
        "Bare is an empty project"
    );
}

#[test]
fn open_project_refuses_a_folder_of_other_files() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    let papers = scratch.path().join("Papers");
    std::fs::create_dir(&papers).unwrap();
    std::fs::write(papers.join("tax.pdf"), "").unwrap();
    let step = json!({ "action": "app.open-project", "value": papers });
    let refused = try_act(&mut state, step).unwrap_err().to_string();
    assert!(
        refused.contains("has files in it and is not a Sound Tools project"),
        "{refused}"
    );
    let song = scratch.path().join("song");
    act(
        &mut state,
        json!({ "action": "app.open-project", "value": song }),
    );
    assert_eq!(state.shell.opened.as_deref(), Some(song.as_path()));
}

#[test]
fn an_empty_project_starts_from_a_template_in_place() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("empty");
    runtime::templates::write(&folder, runtime::templates::named("Bare").unwrap()).unwrap();
    let mut state = opened(&folder);
    state.shell = Projects::new(None, true, Some(folder));
    let mut window = laid_out(state);
    redraw(&mut window);
    assert!(window.find("empty-project").is_some());
    assert!(
        window.find("start-from-1").is_none(),
        "only the DAW, not Bare"
    );
    let (x, y) = window.find("start-from-0").unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    window.pointer_up(x, y);
    redraw(&mut window);
    assert_eq!(window.state.session.notice(), None);
    let root = window.state.project().root();
    assert!(root.join("extensions/song.tsx").is_file());
}

#[test]
fn cmd_n_opens_the_new_project_and_escape_closes_it() {
    let scratch = new_project();
    let state = kept(&scratch);
    let mut window = laid_out(state);
    window.key(rune::KeyEvent {
        key: rune::Key::Command('n'),
        mods: MAIN_MODIFIER,
    });
    redraw(&mut window);
    assert!(matches!(window.state.shell.dialog, Some(Dialog::New(_))));
    assert!(window.find("new-project-dialog").is_some());
    key(&mut window, rune::Key::Escape);
    assert!(window.state.shell.dialog.is_none());
}

/// A template of the tools only of a song opens as an empty song, with no problem, that
/// offers its first track.
#[test]
fn a_project_from_the_tools_only_of_a_song_opens_its_empty_song() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    act(&mut state, json!({ "action": "app.save-as-template" }));
    act(
        &mut state,
        json!({ "key": "template-name", "action": "commit", "value": "Bare song" }),
    );
    act(
        &mut state,
        json!({ "label": "Tools only", "action": "press" }),
    );
    act(&mut state, json!({ "label": "Save", "action": "press" }));
    let saved = runtime::templates::saved_in(&scratch.path().join("support/templates")).unwrap();
    let folder = scratch.path().join("bare");
    saved[0].write(&folder).unwrap();
    let mut state = opened(&folder);
    let tracks = |state: &State| {
        let instances = state.project().instances();
        instances.filter(|(_, tool)| *tool == "track").count()
    };
    assert_eq!(tracks(&state), 0);
    act(
        &mut state,
        json!({ "label": "Add an instrument track", "action": "press" }),
    );
    assert_eq!(tracks(&state), 1);
    assert_eq!(state.project().problems(), []);
}
