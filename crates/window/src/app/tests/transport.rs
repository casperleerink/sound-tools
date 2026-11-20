//! The transport of the title row and the notices, driven by key and label.

use std::path::PathBuf;

use rune::{Key, KeyEvent, Mods};
use sound_core::Ticks;

use super::*;

fn window() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    window.on_key = Some(on_key);
    redraw(&mut window);
    (scratch, window)
}

fn tempo(state: &State) -> f64 {
    let map = &state.project().project_file().tempo_map;
    map.change_at(Ticks(0)).bpm.bpm()
}

fn signature(state: &State) -> String {
    let map = &state.project().project_file().tempo_map;
    let signature = map.time_signatures().bar_at(Ticks(0)).signature;
    format!("{}/{}", signature.numerator(), signature.denominator())
}

fn undo_label(state: &State) -> Option<String> {
    state.project().undo_label()
}

fn chord(window: &mut rune::Runtime<State>, key: Key, shift: bool) {
    let mods = Mods {
        cmd: true,
        shift,
        ..Mods::default()
    };
    window.key(KeyEvent { key, mods });
    redraw(window);
}

fn typed(window: &mut rune::Runtime<State>, words: &str) {
    for letter in words.chars() {
        key(window, Key::Text(letter.into()));
    }
}

fn frames(window: &mut rune::Runtime<State>, seconds: f64) {
    for _ in 0..(seconds * 60.0) as usize {
        redraw(window);
    }
}

#[test]
fn play_pauses_where_it_is_and_stop_goes_back_to_the_start() {
    let (_scratch, mut window) = window();
    act(
        &mut window.state,
        json!({ "label": "Play", "action": "press" }),
    );
    frames(&mut window, 0.5);
    assert!(window.state.playing);
    act(
        &mut window.state,
        json!({ "label": "Pause", "action": "press" }),
    );
    frames(&mut window, 0.1);
    let paused = window.state.playhead;
    assert!(!window.state.playing && paused.0 > 0, "{paused:?}");
    frames(&mut window, 0.1);
    assert_eq!(window.state.playhead, paused);
    act(
        &mut window.state,
        json!({ "label": "Stop", "action": "press" }),
    );
    frames(&mut window, 0.1);
    assert_eq!(window.state.playhead, Ticks(0));

    // Space plays and pauses wherever the keys are.
    key(&mut window, Key::Text(" ".into()));
    frames(&mut window, 0.1);
    assert!(window.state.playing);
}

#[test]
fn a_drag_on_the_tempo_is_one_undo_step() {
    let (_scratch, mut window) = window();
    let (x, y) = window.find("tempo").unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    window.pointer_move(x, y - 10.0);
    redraw(&mut window);
    // Twenty points up: ten beats a minute faster.
    window.pointer_move(x, y - 20.0);
    redraw(&mut window);
    assert_eq!(tempo(&window.state), 130.0);
    assert_eq!(undo_label(&window.state), None);
    window.pointer_up(x, y - 20.0);
    redraw(&mut window);
    assert_eq!(
        undo_label(&window.state).as_deref(),
        Some("Change the tempo")
    );
    chord(&mut window, Key::Command('z'), false);
    assert_eq!(tempo(&window.state), 120.0);
}

/// A click types the tempo: Enter keeps it as one undo step, Escape leaves it.
#[test]
fn a_tempo_typed_is_one_undo_step_and_escape_leaves_it() {
    let (_scratch, mut window) = window();
    click(&mut window, "tempo");
    assert_eq!(focused(&window).as_deref(), Some("tempo-typed"));
    typed(&mut window, "95");
    key(&mut window, Key::Enter);
    assert_eq!(tempo(&window.state), 95.0);
    assert_eq!(
        undo_label(&window.state).as_deref(),
        Some("Change the tempo")
    );
    assert_eq!(focused(&window).as_deref(), Some("tempo"));

    // Enter on the tempo types it too.
    key(&mut window, Key::Enter);
    typed(&mut window, "140");
    key(&mut window, Key::Escape);
    assert_eq!(tempo(&window.state), 95.0);
    assert_eq!(focused(&window).as_deref(), Some("tempo"));
}

#[test]
fn an_agent_sets_the_tempo_and_the_time_signature_one_step_each() {
    let (_scratch, mut window) = window();
    let state = &mut window.state;
    act(
        state,
        json!({ "label": "Tempo", "action": "set", "value": 100 }),
    );
    act(
        state,
        json!({ "label": "Time signature", "action": "set", "value": "3/4" }),
    );
    assert_eq!((tempo(state), signature(state).as_str()), (100.0, "3/4"));
    let controls = controls::collect(state).controls;
    assert_eq!(control(&controls, "Tempo").value, Some(json!(100.0)));
    act(state, json!({ "action": "undo" }));
    assert_eq!((tempo(state), signature(state).as_str()), (100.0, "4/4"));
    act(state, json!({ "action": "undo" }));
    assert_eq!(tempo(state), 120.0);
}

#[test]
fn the_click_the_computer_keys_and_write_automation_are_no_edit() {
    let (_scratch, mut window) = window();
    let state = &mut window.state;
    for label in ["Click", "Computer keys", "Write automation"] {
        act(state, json!({ "label": label, "action": "toggle" }));
        let controls = controls::collect(state).controls;
        assert_eq!(control(&controls, label).checked, Some(true), "{label}");
    }
    let now = state.session.transport();
    assert!(now.click && now.keys && now.automation);
    assert_eq!(undo_label(state), None);

    chord(&mut window, Key::Command('k'), false);
    assert!(!window.state.session.transport().keys);
}

#[test]
fn record_shows_only_for_a_tool_that_records_and_is_lit_while_a_take_records() {
    let (_scratch, mut window) = window();
    let has_record = |state: &State| {
        let controls = controls::collect(state).controls;
        controls
            .into_iter()
            .find(|control| control.label == "Record")
    };
    assert!(has_record(&window.state).is_none());
    window.state.transport.records = true;
    window.state.session.set_recording(true);
    assert_eq!(has_record(&window.state).unwrap().checked, Some(true));
}

#[test]
fn undo_waits_for_the_drag_on_the_tempo_and_says_why() {
    let (_scratch, mut window) = window();
    act(
        &mut window.state,
        json!({ "label": "Tempo", "action": "set", "value": 100 }),
    );
    let (x, y) = window.find("tempo").unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    window.pointer_move(x, y - 20.0);
    redraw(&mut window);
    chord(&mut window, Key::Command('z'), false);
    assert_eq!(tempo(&window.state), 110.0);
    assert_eq!(
        window.state.session.notice(),
        Some("Let go of the drag first.")
    );
    // The drag ends as a step that works, which clears the notice.
    window.pointer_up(x, y - 20.0);
    redraw(&mut window);
    assert_eq!(window.state.session.notice(), None);
    chord(&mut window, Key::Command('z'), false);
    assert_eq!(tempo(&window.state), 100.0);

    // Nothing left to redo is no error.
    chord(&mut window, Key::Command('z'), true);
    chord(&mut window, Key::Command('z'), true);
    assert_eq!(tempo(&window.state), 110.0);
    assert_eq!(window.state.session.notice(), None);
}

#[test]
fn tab_reaches_the_transport_and_enter_types_the_tempo() {
    let (_scratch, mut window) = window();
    tab_to(&mut window, "play");
    tab_to(&mut window, "tempo");
    key(&mut window, Key::Enter);
    assert_eq!(focused(&window).as_deref(), Some("tempo-typed"));
}

#[test]
fn a_long_notice_fits_bottom_right_in_three_lines_and_a_dismiss_takes_it_away() {
    let (_scratch, mut window) = window();
    let long = "a problem that goes on and on, ".repeat(20);
    window.state.session.report(&long);
    frames(&mut window, 0.5);
    let (size, (notice, _)) = ((1470.0, 920.0), window.presented("notice-error").unwrap());
    let [x, y, w, h] = notice;
    // 24 pt in from the bottom right, 400 pt wide at most, three lines of 20 pt and padding.
    assert_eq!((x + w, y + h), (size.0 - 24.0, size.1 - 24.0), "{notice:?}");
    assert!(w <= 400.0, "{notice:?}");
    assert!((60.0..=74.0).contains(&h), "{notice:?}");

    // A short one is as wide as its words.
    window.state.session.dismiss_notice();
    window.state.session.report("short");
    redraw(&mut window);
    frames(&mut window, 0.5);
    let [_, _, w, h] = window.presented("notice-error").unwrap().0;
    assert!(w < 200.0 && h < 40.0, "{w} {h}");

    // A notice of the system stays after an edit that works; the dismiss takes it away.
    act(
        &mut window.state,
        json!({ "label": "Tempo", "action": "set", "value": 100 }),
    );
    assert_eq!(window.state.session.notice(), Some("short"));
    click(&mut window, "notice-dismiss");
    assert_eq!(window.state.session.notice(), None);
    assert!(window.find("notice-error").is_none());
}

/// Pictures of the title row to look at, which no run of the app can show yet: a tool that
/// records and a take that records, and a notice. `SNAPS=<folder> cargo nextest run -p
/// sound-window --run-ignored only title_row_pictures`.
#[test]
#[ignore = "writes pictures to look at"]
fn title_row_pictures() {
    let Ok(folder) = std::env::var("SNAPS") else {
        return;
    };
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    state.transport.records = true;
    state.session.set_recording(true);
    state
        .session
        .report("The plugin “Valhalla Room” did not load: it is not on this computer any more.");
    let png = PathBuf::from(folder).join("recording-notice.png");
    let spec = rune::AppSpec {
        min_size: Some(MIN_SIZE),
        poll: Some(poll),
        setup: Some(setup),
        on_key: Some(on_key),
        fonts: &crate::FONTS,
        families: ("Inter Display", "ui-monospace"),
        ..rune::AppSpec::new("song", (1470.0, 920.0), || unreachable!(), view)
    };
    rune::App::new(spec)
        .args(["--snap".to_string(), png.display().to_string()])
        .init(move || state)
        .run();
    assert!(png.exists());
}
