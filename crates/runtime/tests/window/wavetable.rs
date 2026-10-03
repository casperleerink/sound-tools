//! The card of the Wavetable in the track panel, with a simulated mouse and keys: it is picked
//! as the instrument of a track, a drag on its wavetable moves the position as one undo step,
//! routes of its matrix are added, changed and removed one undo step each, and its tabs and the
//! switches of its sections change nothing that is saved.

use gpui::{TestAppContext, point, px};
use sound_core::Changes;
use wavetable::{Destination, Route, Source, WavetableState};

use crate::support::{self, Opened, id};

const SLOT: &str = "arrangement/track-1/instrument";

/// One track whose instrument is a default Wavetable, its panel open.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = Changes::new();
        changes.delete(&id(SLOT));
        project.commit("Remove synth", changes).unwrap();
        let mut changes = Changes::new();
        changes.create(id(SLOT), WavetableState::default());
        project.commit("Add wavetable", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    opened
}

fn state(opened: &mut Opened<'_>) -> WavetableState {
    opened.project(|project| {
        let wavetable = project.resolve::<WavetableState>(&id(SLOT)).unwrap();
        project.state(&wavetable).unwrap().clone()
    })
}

fn expand(opened: &mut Opened<'_>) {
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
}

/// Clicks the tab of a page of the expanded card: `Osc`, `Voice`, `Filter`, `Env`, `Lfo` or
/// `Matrix`.
fn show(opened: &mut Opened<'_>, page: &str) {
    let tab = opened.control(&format!("segment-{page}"));
    opened.click(tab);
}

#[gpui::test]
fn the_picker_puts_a_wavetable_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = support::open_with(cx, |project| project.clear_history());
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("instrument-picker");
    opened.click(trigger);
    let row = opened.control("menu-wavetable");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Choose Wavetable"));
    assert_eq!(state(&mut opened), WavetableState::default());
    assert_eq!(opened.project(|project| project.problems()), []);
    // The four knobs of the card and the table of its display; nothing of the sections.
    for shown in [
        "knob-osc-1-position",
        "knob-gain",
        "knob-filter-1-cutoff_hz",
        "knob-filter-1-resonance",
        "select-osc-1-table",
        "handle-osc-1-position-area",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("knob-osc-2-position"), None);
    // Expanded, one page at a time, the oscillators first.
    expand(&mut opened);
    for (page, shown) in [
        ("Osc", "knob-osc-2-position"),
        ("Osc", "knob-osc-1-detune_cents"),
        ("Voice", "knob-voicing-glide_seconds"),
        ("Filter", "knob-filter-2-cutoff_hz"),
        ("Env", "knob-amp-env-attack_seconds"),
        ("Lfo", "knob-lfo-1-rate_hz"),
        ("Matrix", "route-add"),
    ] {
        show(&mut opened, page);
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    assert_eq!(opened.find("knob-osc-2-position"), None);
    // Picked from the menu, not defaulted: undo gives the synth back.
    opened.keys("cmd-z");
    assert!(
        opened.project(|project| project.resolve::<WavetableState>(&id(SLOT)).is_none()),
        "the Wavetable is still there"
    );
}

/// A drag up and down anywhere on the wavetable moves the position, from where it is, and a
/// drag is one undo step under the name of the Position knob.
#[gpui::test]
fn a_drag_on_the_wavetable_moves_the_position_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = support::mark(&mut opened);
    let area = opened.control("handle-osc-1-position-area");
    // Off the middle, where the pointer is not on the line: a press never jumps.
    let from = area + point(px(40.), px(20.));
    opened.press(from);
    opened.drag_to(from - point(px(0.), px(10.)));
    opened.drag_to(from - point(px(0.), px(20.)));
    assert!(opened.gesture_open());
    opened.release(from - point(px(0.), px(20.)));
    assert!(!opened.gesture_open());
    let position = state(&mut opened).osc_1.position;
    // 20 pt of a display 118 pt tall whose frames rise 40 % of it from first to last.
    assert!(
        (position - (0.5 + 20. / (118. * 0.4))).abs() < 0.01,
        "{position}"
    );
    assert_eq!(
        state(&mut opened),
        WavetableState {
            osc_1: wavetable::state::Oscillator {
                position,
                ..WavetableState::default().osc_1
            },
            ..WavetableState::default()
        }
    );
    support::one_undo_step(&mut opened, "Change Osc 1 position", &before);
    // A double click puts the default back.
    opened.double_click(area);
    assert_eq!(state(&mut opened), WavetableState::default());

    // The table at the top of the display takes its own presses, and the display none of them.
    let before = support::mark(&mut opened);
    let tables = opened.control("select-osc-1-table");
    opened.click(tables);
    let organ = opened.control("menu-Organ");
    opened.click(organ);
    assert_eq!(state(&mut opened).osc_1.table, wavetable::Table::Organ);
    assert_eq!(state(&mut opened).osc_1.position, 0.5);
    support::one_undo_step(&mut opened, "Change Osc 1 table", &before);
}

/// Add, pick a source, drag the amount and remove: one undo step each, and four undos give
/// the matrix of before back.
#[gpui::test]
fn routes_are_added_changed_and_removed_one_undo_step_each(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    expand(&mut opened);
    show(&mut opened, "Matrix");
    let before = state(&mut opened).matrix;
    let routes = before.len();

    let mark = support::mark(&mut opened);
    let add = opened.control("route-add");
    opened.click(add);
    let added = Route {
        source: Source::Lfo1,
        destination: Destination::Osc1Position,
        amount: 0.,
    };
    assert_eq!(state(&mut opened).matrix[routes], added);
    support::one_undo_step(&mut opened, "Add route", &mark);

    let mark = support::mark(&mut opened);
    let source = opened.control(&format!("select-route-source-{routes}"));
    opened.click(source);
    let row = opened.control("menu-Env3");
    opened.click(row);
    assert_eq!(state(&mut opened).matrix[routes].source, Source::Env3);
    support::one_undo_step(&mut opened, "Change route source", &mark);

    let mark = support::mark(&mut opened);
    let amount = opened.control(&format!("slider-route-amount-{routes}"));
    opened.drag(amount, amount + point(px(14.), px(0.)));
    let dragged = state(&mut opened).matrix[routes].amount;
    // The dot follows the pointer: 14 pt of a line of 56 is half the range from the middle.
    assert!((dragged - 0.5).abs() < 0.02, "{dragged}");
    support::one_undo_step(&mut opened, "Change route amount", &mark);

    let mark = support::mark(&mut opened);
    let remove = opened.control(&format!("route-remove-{routes}"));
    opened.click(remove);
    assert_eq!(state(&mut opened).matrix, before);
    support::one_undo_step(&mut opened, "Remove route", &mark);

    for _ in 0..4 {
        opened.keys("cmd-z");
    }
    assert_eq!(state(&mut opened).matrix, before);
    assert_eq!(opened.undo_label(), None);
}

/// Which page, envelope and LFO the card shows is interface state: a tab or a switch writes
/// nothing and is no undo step, and the knobs follow it.
#[gpui::test]
fn the_tabs_and_the_switches_of_the_sections_change_no_record(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    expand(&mut opened);
    let before = support::mark(&mut opened);
    show(&mut opened, "Env");
    assert_eq!(opened.find("knob-osc-2-position"), None);
    assert!(opened.find("knob-amp-env-attack_seconds").is_some());
    let env_2 = opened.control("segment-Env2");
    opened.click(env_2);
    assert_eq!(opened.find("knob-amp-env-attack_seconds"), None);
    assert!(opened.find("knob-env-2-attack_seconds").is_some());
    show(&mut opened, "Lfo");
    let lfo_2 = opened.control("segment-Lfo2");
    opened.click(lfo_2);
    assert!(opened.find("knob-lfo-2-rate_hz").is_some());
    assert_eq!(opened.undo_label(), before.undo_label);
    assert_eq!(support::files(opened.folder.path()), before.files);
    assert_eq!(state(&mut opened), WavetableState::default());
}
