//! The History tab, driven by key, pointer and label on a new project with real edits: a press
//! or the keys write a point's files back, the menu renames, keeps, merges and folds away, and
//! going elsewhere waits for a drag or a take.

use rune::{Key, KeyEvent, Mods};
use runtime::session::Drag;
use sound_core::{InstanceId, Line, PointId};

use super::super::panel::LeftPanel;
use super::*;

const SYNTH: &str = "song/t00/synth";

/// The window of a new project without the agent, its History tab open.
fn history(scratch: &tempfile::TempDir) -> rune::Runtime<State> {
    let mut state = opened(&scratch.path().join("song"));
    state.panel = LeftPanel::new(None, None);
    state.set_panel_open(true);
    let mut window = laid_out(state);
    window.on_key = Some(on_key);
    redraw(&mut window);
    window
}

/// One point: the synth's cutoff, under `label`.
fn edit(window: &mut rune::Runtime<State>, label: &str, cutoff: f64) -> PointId {
    let synth = InstanceId::new(SYNTH).unwrap();
    let session = &mut window.state.session;
    session
        .set(label, &synth, &[("cutoff_hz", cutoff)])
        .unwrap();
    redraw(window);
    current(window)
}

fn current(window: &rune::Runtime<State>) -> PointId {
    window.state.project().history().unwrap().current()
}

fn cutoff(window: &rune::Runtime<State>) -> Option<f64> {
    record(&window.state, "song/t00/synth.json")["state"]["cutoff_hz"].as_f64()
}

fn points(window: &rune::Runtime<State>) -> usize {
    let history = window.state.project().history().unwrap();
    history.points(1000).unwrap().len()
}

fn line(window: &rune::Runtime<State>, point: PointId) -> Line {
    let history = window.state.project().history().unwrap();
    history.line_of(point).unwrap().unwrap().clone()
}

fn go_to(window: &mut rune::Runtime<State>, point: PointId) {
    assert!(window.state.session.go_to(point));
    redraw(window);
}

fn row(point: PointId) -> String {
    format!("history-row-{}", point.short())
}

fn card(start: PointId) -> String {
    format!("history-card-{}", start.short())
}

fn shows(window: &rune::Runtime<State>, key: &str) -> bool {
    window.find(key).is_some()
}

/// Rows glide to where they go: a pointer finds them once they are there.
fn settle(window: &mut rune::Runtime<State>) {
    for _ in 0..30 {
        window.frame(0.1);
    }
}

fn press(window: &mut rune::Runtime<State>, key: &str) {
    settle(window);
    click(window, key);
}

fn right_click(window: &mut rune::Runtime<State>, key: &str) {
    settle(window);
    let (x, y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    window.secondary_down(x, y);
    window.secondary_up(x, y);
    redraw(window);
}

fn double_click(window: &mut rune::Runtime<State>, key: &str) {
    settle(window);
    let (x, y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    for _ in 0..2 {
        window.pointer_down(x, y);
        window.pointer_up(x, y);
        window.frame(0.05);
    }
    redraw(window);
}

fn type_text(window: &mut rune::Runtime<State>, words: &str) {
    window.key(KeyEvent {
        key: Key::Text(words.into()),
        mods: Mods::default(),
    });
    redraw(window);
}

/// Picks the item of the open menu that says `label`.
fn pick(window: &mut rune::Runtime<State>, label: &str) {
    let controls = chrome_controls(&window.state);
    let key = control(&controls, label).key.clone();
    press(window, &key);
}

/// Version 1 to `first`, and version 2 from the start to `second`, back on `first`.
fn two_versions(window: &mut rune::Runtime<State>) -> (PointId, PointId, PointId) {
    let start = current(window);
    let first = edit(window, "Change the cutoff", 600.0);
    go_to(window, start);
    let second = edit(window, "Change the cutoff", 900.0);
    go_to(window, first);
    (start, first, second)
}

#[test]
fn a_press_on_a_row_writes_its_files_back_and_makes_no_point() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let start = current(&window);
    let before = cutoff(&window);
    let changed = edit(&mut window, "Change the cutoff", 600.0);
    let made = points(&window);

    press(&mut window, &row(start));
    assert_eq!(current(&window), start);
    assert_eq!(cutoff(&window), before);

    press(&mut window, &row(changed));
    assert_eq!(current(&window), changed);
    assert_eq!(cutoff(&window), Some(600.0));
    assert_eq!(points(&window), made);
}

#[test]
fn tab_reaches_the_point_you_are_at_the_arrows_move_and_enter_goes() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let start = current(&window);
    let changed = edit(&mut window, "Change the cutoff", 600.0);
    tab_to(&mut window, &row(changed));

    // The one below the newest is the start.
    key(&mut window, Key::Down);
    assert_eq!(focused(&window), Some(row(start)));
    key(&mut window, Key::Enter);
    assert_eq!(current(&window), start);
    key(&mut window, Key::Up);
    key(&mut window, Key::Enter);
    assert_eq!(current(&window), changed);
}

#[test]
fn the_menu_renames_and_folds_away_and_folded_away_versions_show_again() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let (_, first, second) = two_versions(&mut window);

    right_click(&mut window, &row(first));
    pick(&mut window, "Rename");
    type_text(&mut window, "Longer tail");
    key(&mut window, Key::Enter);
    assert_eq!(line(&window, first).name.as_deref(), Some("Longer tail"));
    assert!(line(&window, first).named_by_hand);

    let second_card = card(line(&window, second).start);
    right_click(&mut window, &second_card);
    pick(&mut window, "Fold away");
    assert!(line(&window, second).set_aside);
    assert!(!shows(&window, &second_card));
    press(&mut window, "history-folded-away");
    assert!(shows(&window, &second_card));
    right_click(&mut window, &second_card);
    pick(&mut window, "Bring back");
    assert!(!line(&window, second).set_aside);
}

#[test]
fn a_card_goes_to_its_latest_point_a_double_click_renames_and_its_from_goes_back() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let (_, first, second) = two_versions(&mut window);
    let second_card = card(line(&window, second).start);

    press(&mut window, &second_card);
    assert_eq!(current(&window), second);

    // Now open, at the top.
    double_click(&mut window, &second_card);
    type_text(&mut window, "Dry");
    key(&mut window, Key::Enter);
    assert_eq!(line(&window, second).name.as_deref(), Some("Dry"));
    assert_eq!(focused(&window), Some(second_card.clone()));

    let start = line(&window, second).start;
    press(&mut window, &format!("history-from-{}", start.short()));
    assert_eq!(current(&window), first);
}

#[test]
fn escape_lets_a_rename_go_and_a_press_elsewhere_keeps_it() {
    let scratch = new_project();
    let mut window = history(&scratch);
    edit(&mut window, "Change the cutoff", 600.0);
    let open = line(&window, current(&window)).start;
    let title = line(&window, open).title();

    double_click(&mut window, &card(open));
    type_text(&mut window, "Gone");
    key(&mut window, Key::Escape);
    assert_eq!(line(&window, open).title(), title);
    assert!(!shows(&window, "history-rename"));
    assert_eq!(focused(&window), Some(card(open)));

    double_click(&mut window, &card(open));
    type_text(&mut window, "Kept");
    press(&mut window, "track-t00");
    redraw(&mut window);
    assert_eq!(line(&window, open).name.as_deref(), Some("Kept"));

    // An empty name gives the naming back.
    double_click(&mut window, &card(open));
    key(&mut window, Key::Backspace);
    key(&mut window, Key::Enter);
    assert_eq!(line(&window, open).title(), title);
}

#[test]
fn merge_brings_another_version_into_this_one_as_one_new_point() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let (_, first, second) = two_versions(&mut window);
    let into = line(&window, first).title();

    let second_card = card(line(&window, second).start);
    right_click(&mut window, &second_card);
    let controls = chrome_controls(&window.state);
    control(
        &controls,
        &format!("Merge into \u{201c}{into}\u{201d}, keep mine"),
    );
    pick(&mut window, &format!("Merge into \u{201c}{into}\u{201d}"));
    let history = window.state.project().history().unwrap();
    let merged = history.parents(history.current()).unwrap();
    assert_eq!(merged, [first, second]);
    assert_eq!(cutoff(&window), Some(900.0));
}

#[test]
fn going_elsewhere_waits_for_a_drag_and_a_take_and_says_why() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let (start, first, second) = two_versions(&mut window);

    let mut drag = Drag::new("Change the cutoff");
    let synth = InstanceId::new(SYNTH).unwrap();
    (window.state.session)
        .play(&mut drag, &synth, &[("cutoff_hz", 700.0)])
        .unwrap();
    press(&mut window, &row(start));
    assert_eq!(
        window.state.session.notice(),
        Some("Let go of the drag first.")
    );
    assert!(shows(&window, "notices"), "the notice shows");
    window.state.session.finish(drag);
    let here = current(&window);

    window.state.session.set_recording(true);
    let (second_card, into) = (
        card(line(&window, second).start),
        line(&window, first).title(),
    );
    right_click(&mut window, &second_card);
    pick(&mut window, &format!("Merge into \u{201c}{into}\u{201d}"));
    assert_eq!(window.state.session.notice(), Some("Stop recording first."));
    assert_eq!(current(&window), here);
}

#[test]
fn work_taken_back_shows_between_changes_and_a_press_goes_there() {
    let scratch = new_project();
    let mut window = history(&scratch);
    edit(&mut window, "Change the cutoff", 600.0);
    edit(&mut window, "Change the cutoff", 700.0);
    let undone_tip = edit(&mut window, "Change the cutoff", 800.0);
    window.state.session.undo();
    window.state.session.undo();
    edit(&mut window, "Change the resonance", 900.0);

    let controls = chrome_controls(&window.state);
    let taken_back = control(&controls, "2 changes taken back").key.clone();
    let help = window
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&taken_back));
    let help = format!("{:?}", help.unwrap());
    assert!(help.contains("This undo keeps work as a version"), "{help}");
    press(&mut window, &taken_back);
    assert_eq!(current(&window), undone_tip);
}

#[test]
fn a_long_line_shows_six_changes_then_fifty_more_each_time() {
    let scratch = new_project();
    let mut window = history(&scratch);
    for step in 0..9 {
        edit(&mut window, "Change the cutoff", 500.0 + f64::from(step));
    }
    let changes = |window: &rune::Runtime<State>| {
        let controls = chrome_controls(&window.state);
        (controls.iter())
            .filter(|control| control.key.starts_with("history-row-"))
            .count()
    };
    assert_eq!(changes(&window), 6);
    press(&mut window, "history-more");
    assert_eq!(changes(&window), 10);
    assert!(!shows(&window, "history-more"));
}

#[test]
fn an_edit_while_the_tab_shows_comes_in_on_top_as_where_you_are() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let before = edit(&mut window, "Change the cutoff", 600.0);
    let after = edit(&mut window, "Change the resonance", 700.0);
    let controls = chrome_controls(&window.state);
    let here = control(&controls, "Change the resonance");
    assert_eq!((here.key.clone(), here.checked), (row(after), Some(true)));
    assert_eq!(control(&controls, "Change the cutoff").checked, Some(false));
    // Undo moves the mark down a row.
    window.state.undo().unwrap();
    redraw(&mut window);
    let controls = chrome_controls(&window.state);
    assert_eq!(control(&controls, "Change the cutoff").checked, Some(true));
    assert_eq!(current(&window), before);
}

#[test]
fn an_agent_goes_renames_and_merges_by_label() {
    let scratch = new_project();
    let mut window = history(&scratch);
    let (_, first, second) = two_versions(&mut window);
    let state = &mut window.state;
    let second_title = {
        let history = state.project().history().unwrap();
        history.line_of(second).unwrap().unwrap().title()
    };
    act(state, json!({ "label": second_title, "action": "open" }));
    act(state, json!({ "label": "Rename", "action": "press" }));
    act(
        state,
        json!({ "label": "Name", "action": "commit", "value": "Bright" }),
    );
    act(state, json!({ "label": "Bright", "action": "press" }));
    assert_eq!(state.project().history().unwrap().current(), second);
    let open = state
        .project()
        .history()
        .unwrap()
        .line_of(first)
        .unwrap()
        .unwrap()
        .start;
    let first_card = card(open);
    act(state, json!({ "key": first_card, "action": "open" }));
    act(
        state,
        json!({ "label": "Merge into \u{201c}Bright\u{201d}", "action": "press" }),
    );
    let history = state.project().history().unwrap();
    assert_eq!(history.parents(history.current()).unwrap(), [second, first]);

    let why = try_act(
        state,
        json!({ "label": "Show more changes", "action": "press" }),
    );
    assert!(why.is_err());
}
