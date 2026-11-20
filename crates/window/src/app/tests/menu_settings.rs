//! The project menu, the app's keys and Settings, driven by key and label.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rune::{Key, KeyEvent, Mods};
use sound_core::DeviceChoice;

use super::super::keys::Keys;
use super::super::menu::{Pick, picks};
use super::super::settings::{DeviceList, Section};
use super::*;
use crate::shell::Projects;

fn window_of(state: State) -> rune::Runtime<State> {
    let mut window = laid_out(state);
    window.on_key = Some(on_key);
    redraw(&mut window);
    window
}

/// The window of a new project in `scratch`, which keeps the composer's keys and the projects
/// of the machine in `scratch/support`.
fn kept(scratch: &tempfile::TempDir) -> State {
    let mut state = opened(&scratch.path().join("song"));
    let support = scratch.path().join("support");
    let here = state.project().root().to_path_buf();
    state.shell = Projects::new(Some(support.clone()), true, Some(here));
    state.keys = Keys::open(Some(runtime::app::shortcuts_file(&support))).0;
    state
}

/// `c` with cmd, and shift when `shift`.
fn chord(window: &mut rune::Runtime<State>, c: char, shift: bool) {
    let mods = Mods {
        cmd: true,
        shift,
        ..Mods::default()
    };
    window.key(KeyEvent {
        key: Key::Command(c),
        mods,
    });
    redraw(window);
}

fn labels(state: &State) -> Vec<String> {
    let controls = controls::collect(state).controls.into_iter();
    controls.map(|control| control.label).collect()
}

#[test]
fn the_menu_has_the_old_entries_in_their_order_and_the_keys_the_composer_gives_them() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    let shown: Vec<(Pick, String)> = picks(&state)
        .into_iter()
        .map(|(pick, label, _)| (pick, label))
        .collect();
    let mut expected = vec![
        (Pick::Export, "Export audio…"),
        (Pick::ExportSelection, "Export selection…"),
        (Pick::Undo, "Undo"),
        (Pick::Redo, "Redo"),
        (Pick::NewProject, "New project…"),
        (Pick::SaveAsTemplate, "Save as template…"),
        (Pick::OpenProject, "Open project…"),
        (Pick::OpenRecent, "Open recent"),
        (
            Pick::Reveal,
            if cfg!(target_os = "macos") {
                "Reveal project folder"
            } else {
                "Show project folder"
            },
        ),
        (Pick::Terminal, "Open terminal in project folder"),
    ];
    if cfg!(unix) {
        expected.push((Pick::InstallTool, "Install command line tool"));
    }
    expected.push((Pick::Settings, "Settings…"));
    let expected: Vec<(Pick, String)> = expected
        .into_iter()
        .map(|(pick, label)| (pick, label.into()))
        .collect();
    assert_eq!(shown, expected);

    // Undo says what it takes back, with the key it has now.
    act(
        &mut state,
        json!({ "label": "Mute Track 2", "action": "toggle" }),
    );
    let undo = |state: &State| {
        picks(state)
            .into_iter()
            .find(|(pick, ..)| *pick == Pick::Undo)
            .unwrap()
    };
    let (_, label, key) = undo(&state);
    assert!(
        label.starts_with("Undo ") && label.len() > "Undo ".len(),
        "{label}"
    );
    assert_eq!(key, Some(super::super::keys::key_label("cmd-z")));
    state
        .keys
        .keymap
        .set_keys("app.undo", vec!["cmd-u".into()])
        .unwrap();
    assert_eq!(undo(&state).2, Some(super::super::keys::key_label("cmd-u")));
}

#[test]
fn the_menu_opens_by_key_moves_by_arrows_and_letters_and_a_pick_closes_it() {
    let scratch = new_project();
    let mut window = window_of(kept(&scratch));
    tab_to(&mut window, "project-menu");
    key(&mut window, Key::Text(" ".into()));
    assert!(window.state.menu.open);
    assert!(labels(&window.state).contains(&"Settings…".to_string()));

    // Escape closes it and gives the keys back to the name.
    key(&mut window, Key::Escape);
    assert!(!window.state.menu.open);
    assert_eq!(focused(&window).as_deref(), Some("project-menu"));

    key(&mut window, Key::Text(" ".into()));
    assert!(window.state.menu.open);
    key(&mut window, Key::Down);
    // A letter goes to the entry it starts.
    key(&mut window, Key::Text("s".into()));
    let at = focused(&window).unwrap();
    let entry = controls::collect(&window.state)
        .controls
        .into_iter()
        .find(|control| control.key == at);
    assert_eq!(
        entry.map(|entry| entry.label).as_deref(),
        Some("Save as template…")
    );
    key(&mut window, Key::Enter);
    assert!(!window.state.menu.open);
    assert!(
        window.state.shell.dialog.is_some(),
        "the pick saves as a template"
    );
}

#[test]
fn open_recent_shows_the_recent_projects_beside_it_and_a_pick_opens_one() {
    let scratch = new_project();
    let support = scratch.path().join("support");
    let sketch = scratch.path().join("sketch");
    runtime::templates::write(&sketch, &TEMPLATE).unwrap();
    let sketch = dunce::canonicalize(sketch).unwrap();
    runtime::app::remember_project_in(&support, &sketch).unwrap();
    let mut window = window_of(kept(&scratch));
    tab_to(&mut window, "project-menu");
    key(&mut window, Key::Text(" ".into()));
    while !focused(&window).is_some_and(|at| {
        controls::collect(&window.state)
            .controls
            .iter()
            .any(|control| control.key == at && control.label == "Open recent")
    }) {
        key(&mut window, Key::Down);
    }
    key(&mut window, Key::Right);
    let sub = focused(&window).unwrap();
    assert!(sub.ends_with("-sub-item0"), "{sub}");
    assert!(labels(&window.state).contains(&"sketch".to_string()));
    key(&mut window, Key::Left);
    assert!(!labels(&window.state).contains(&"sketch".to_string()));
    assert!(window.state.menu.open);

    act(
        &mut window.state,
        json!({ "label": "Open recent", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "label": "sketch", "action": "press" }),
    );
    assert_eq!(window.state.shell.opened.as_deref(), Some(sketch.as_path()));
    assert!(!window.state.menu.open);
}

#[test]
fn an_agent_opens_the_menu_and_picks_by_label_and_an_entry_that_is_off_does_nothing() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    act(
        &mut state,
        json!({ "key": "project-menu", "action": "press" }),
    );
    let off = try_act(&mut state, json!({ "label": "Undo", "action": "press" }));
    assert!(off.is_err(), "nothing to undo yet");
    act(
        &mut state,
        json!({ "label": "Settings…", "action": "press" }),
    );
    assert!(!state.menu.open);
    assert_eq!(
        state.settings.page.as_ref().map(|page| page.section),
        Some(Section::Audio)
    );
}

fn device(id: &str, name: &str) -> DeviceChoice {
    DeviceChoice {
        id: id.into(),
        name: name.into(),
    }
}

/// Settings lists the devices of the machine each time it opens, so an interface plugged in
/// since is offered. Never on a timer: listing can take a while, and on Linux it opens each
/// device.
#[test]
fn settings_lists_the_devices_each_time_it_opens_and_escape_gives_the_keys_back() {
    let scratch = new_project();
    let outputs = Arc::new(Mutex::new(vec![device("speakers", "Speakers")]));
    let listed = Arc::new(AtomicUsize::new(0));
    let mut state = kept(&scratch);
    state.settings.list = Arc::new({
        let (outputs, listed) = (outputs.clone(), listed.clone());
        move || {
            listed.fetch_add(1, Ordering::Relaxed);
            DeviceList {
                outputs: outputs.lock().unwrap().clone(),
                inputs: Vec::new(),
            }
        }
    });
    let mut window = window_of(state);
    tab_to(&mut window, "track-t00");
    assert_eq!(listed.load(Ordering::Relaxed), 0);

    chord(&mut window, ',', false);
    assert_eq!(listed.load(Ordering::Relaxed), 1);
    assert_eq!(focused(&window).as_deref(), Some("settings-back"));
    assert!(labels(&window.state).contains(&"Output: Speakers".to_string()));
    // The project under it is not there to act on.
    assert!(!labels(&window.state).contains(&"Mute Track 2".to_string()));

    key(&mut window, Key::Escape);
    assert!(window.state.settings.page.is_none());
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));
    outputs
        .lock()
        .unwrap()
        .push(device("interface", "Interface"));
    for _ in 0..60 {
        redraw(&mut window);
    }
    assert_eq!(listed.load(Ordering::Relaxed), 1);

    chord(&mut window, ',', false);
    assert_eq!(listed.load(Ordering::Relaxed), 2);
    assert!(labels(&window.state).contains(&"Output: Interface".to_string()));
}

#[test]
fn a_picked_device_is_kept_for_the_machine_and_a_missing_one_says_what_plays() {
    let scratch = new_project();
    let file = scratch.path().join("support/audio-devices.json");
    let mut state = kept(&scratch);
    state.settings = super::super::settings::Settings::new(
        Arc::new(|| DeviceList {
            outputs: vec![
                device("speakers", "Speakers"),
                device("interface", "Interface"),
            ],
            inputs: vec![device("mic", "Microphone")],
        }),
        Some(file.clone()),
        "Speakers".into(),
    );
    act(&mut state, json!({ "action": "app.settings" }));
    let note = |state: &State, label: &str| {
        control(&controls::collect(state).controls, label)
            .value
            .clone()
    };
    assert_eq!(
        note(&state, "Output: System default"),
        Some(json!("Speakers"))
    );

    act(
        &mut state,
        json!({ "label": "Output: Interface", "action": "press" }),
    );
    act(
        &mut state,
        json!({ "label": "Input: Microphone", "action": "press" }),
    );
    let kept = runtime::app::ChosenDevices::read(&file);
    assert_eq!(
        (kept.output.as_deref(), kept.input.as_deref()),
        (Some("interface"), Some("mic"))
    );
    assert_eq!(
        control(&controls::collect(&state).controls, "Output: Interface").checked,
        Some(true)
    );
    // The interface is gone, so the speakers play.
    assert_eq!(
        note(&state, "Output: Interface"),
        Some(json!("Not available. Playing on Speakers"))
    );
}

#[test]
fn a_key_pressed_on_an_action_becomes_its_key_and_the_file_keeps_it_as_the_gpui_app_did() {
    let scratch = new_project();
    let file = runtime::app::shortcuts_file(&scratch.path().join("support"));
    let mut window = window_of(kept(&scratch));
    chord(&mut window, ',', false);
    act(
        &mut window.state,
        json!({ "label": "Shortcuts", "action": "press" }),
    );
    redraw(&mut window);
    tab_to(&mut window, "shortcut-app.new_project");

    // Escape leaves it as it was, Tab moves on.
    key(&mut window, Key::Enter);
    assert_eq!(
        window
            .state
            .settings
            .page
            .as_ref()
            .unwrap()
            .waiting
            .as_deref(),
        Some("app.new_project")
    );
    key(&mut window, Key::Escape);
    assert!(
        window.state.settings.page.is_some(),
        "Escape ended the wait, not the page"
    );
    assert_eq!(window.state.keys.keymap.keys("app.new_project"), ["cmd-n"]);
    key(&mut window, Key::Enter);
    key(&mut window, Key::Tab);
    assert_eq!(window.state.settings.page.as_ref().unwrap().waiting, None);
    assert_ne!(
        focused(&window).as_deref(),
        Some("shortcut-app.new_project")
    );

    // Shift-Tab back to the row.
    for _ in 0..2 {
        window.key(KeyEvent {
            key: Key::Tab,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        });
        redraw(&mut window);
    }
    assert_eq!(
        focused(&window).as_deref(),
        Some("shortcut-app.new_project")
    );
    key(&mut window, Key::Text(" ".into()));
    chord(&mut window, 'p', true);
    assert_eq!(
        window.state.keys.keymap.keys("app.new_project"),
        ["cmd-shift-p"]
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written, json!({ "app.new_project": ["cmd-shift-p"] }));

    // The new key does it, and the menu shows it.
    key(&mut window, Key::Escape);
    chord(&mut window, 'p', true);
    assert!(window.state.shell.dialog.is_some());
    let shown = picks(&window.state)
        .into_iter()
        .find(|(pick, ..)| *pick == Pick::NewProject);
    assert_eq!(
        shown.unwrap().2,
        Some(super::super::keys::key_label("cmd-shift-p"))
    );
}

#[test]
fn a_key_file_of_the_gpui_app_works_and_a_key_two_actions_share_is_said_in_plain_words() {
    let scratch = new_project();
    let file = runtime::app::shortcuts_file(&scratch.path().join("support"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        r#"{ "app.settings": ["cmd-shift-s"], "app.export": ["cmd-n"] }"#,
    )
    .unwrap();
    let mut window = window_of(kept(&scratch));
    chord(&mut window, 's', true);
    assert!(
        window.state.settings.page.is_some(),
        "the old name of Settings keeps its key"
    );

    act(
        &mut window.state,
        json!({ "label": "Shortcuts", "action": "press" }),
    );
    redraw(&mut window);
    let shared = window.find("shortcut-hint-app.export");
    assert!(shared.is_some(), "the export says it shares cmd-n");

    // A tool's action on undo's key does nothing, and says so.
    let tool = window
        .state
        .keys
        .keymap
        .groups()
        .find(|group| group.name != "app")
        .cloned();
    if let Some(tool) = tool {
        let id = sound_typescript::shortcuts::id(&tool.name, &tool.actions[0].name);
        act(
            &mut window.state,
            json!({ "key": format!("shortcut-{id}"), "action": "set", "value": "cmd-z" }),
        );
        redraw(&mut window);
        assert!(window.find(&format!("shortcut-hint-{id}")).is_some());
    }

    // Back to the usual keys, one, then all.
    act(
        &mut window.state,
        json!({ "label": "Settings…: back to its usual key", "action": "press" }),
    );
    assert_eq!(window.state.keys.keymap.keys("app.settings"), ["cmd-,"]);
    act(
        &mut window.state,
        json!({ "label": "Back to the usual keys", "action": "press" }),
    );
    assert_eq!(
        window.state.keys.keymap.keys("app.export"),
        Vec::<String>::new()
    );
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written, json!({}));
}

#[test]
fn the_search_finds_actions_by_their_words_or_their_key() {
    let scratch = new_project();
    let mut state = kept(&scratch);
    act(&mut state, json!({ "action": "app.settings" }));
    act(
        &mut state,
        json!({ "label": "Shortcuts", "action": "press" }),
    );
    act(
        &mut state,
        json!({ "label": "Find", "action": "commit", "value": "agent" }),
    );
    let rows: Vec<String> = (controls::collect(&state).controls.into_iter())
        .filter(|control| control.key.starts_with("shortcut-app."))
        .map(|control| control.label)
        .collect();
    assert_eq!(rows, ["Go to the agent", "Stop the agent"]);
}
