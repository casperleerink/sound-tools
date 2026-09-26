//! The track panel and the view of the synth in it, with a simulated mouse and keys. Every
//! test checks the project, the undo history and the file of the instrument.

use std::cell::Cell;
use std::rc::Rc;

use arrangement::TrackState;
use gpui::{Entity, TestAppContext, point, px, size};
use instrument::view::SynthView;
use instrument::{SynthState, Waveform};
use sound_core::Changes;

use crate::support::{self, BAR, Opened, clip, id, note};

const TRACK: &str = "arrangement/track-1";
const SYNTH: &str = "arrangement/track-1/instrument";
const SYNTH_FILE: &str = "state/arrangement/track-1/instrument.json";
const PART: &str = "arrangement/track-1/part";
const CUTOFF: &str = "knob-cutoff_hz";

/// Two tracks, each with the default synth. `part` on the first holds one low note for two
/// bars, so there is a sound to change.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement).unwrap();
        let mut changes = Changes::new();
        changes.create(id(PART), clip(0, 2 * BAR, vec![note(0, 2 * BAR, 45)]));
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    })
}

/// The same with the panel of the first track open, by a click on its header.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    assert_eq!(opened.panel_track(), Some(id(TRACK)));
    opened
}

/// Opens the envelope knobs of the synth card, behind its expand icon.
fn expand_synth(opened: &mut Opened<'_>) {
    let expand = opened.control("card-instrument-expand");
    opened.click(expand);
}

fn synth(opened: &mut Opened<'_>) -> Option<SynthState> {
    opened.project(|project| {
        let instance = project.resolve::<SynthState>(&id(SYNTH))?;
        project.state(&instance).copied()
    })
}

fn cutoff(opened: &mut Opened<'_>) -> f32 {
    synth(opened).unwrap().cutoff_hz
}

fn synth_file(opened: &mut Opened<'_>) -> Option<String> {
    std::fs::read_to_string(opened.path(SYNTH_FILE)).ok()
}

/// Whether each card of the rack holds a view, left to right.
fn cards(opened: &mut Opened<'_>) -> Vec<bool> {
    let panel = opened.track_panel().unwrap();
    opened.cx.read(|cx| {
        let views = panel.read(cx).device_views();
        views.map(|view| view.is_some()).collect()
    })
}

fn write_outside(opened: &mut Opened<'_>, record: &str) {
    let path = opened.path(SYNTH_FILE);
    std::fs::write(&path, record).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
}

/// How much of a sound is high: the mean step from sample to sample over the mean level. A
/// lower cutoff makes it smaller.
fn brightness(samples: &[f32]) -> f32 {
    let steps: f32 = samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .sum();
    let level: f32 = samples.iter().map(|sample| sample.abs()).sum();
    steps / level
}

#[gpui::test]
fn a_click_on_a_track_header_opens_the_panel_of_that_track_with_the_view_of_its_instrument(
    cx: &mut TestAppContext,
) {
    let mut opened = open(cx);
    assert!(opened.track_panel().is_none());
    let (first, second) = (opened.track_header(0), opened.track_header(1));
    opened.click(second);
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
    // The card holds the view that the instrument extension registered for the synth.
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.next().unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<SynthView>().is_ok());

    // Another header: the same panel shows the other track.
    opened.click(first);
    assert_eq!(opened.panel_track(), Some(id(TRACK)));
    assert_eq!(opened.track_panel().unwrap(), panel);
    assert_eq!(cards(&mut opened), [true]);
    // Selecting and looking are no edits.
    assert_eq!(opened.undo_label(), None);
    // Below the last track there is no header.
    let below = opened.track_header(2);
    opened.click(below);
    assert_eq!(opened.panel_track(), Some(id(TRACK)));
}

#[gpui::test]
fn opening_a_clip_swaps_the_panel_for_the_note_editor_and_a_header_click_swaps_back(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let on_clip = opened.at(BAR, 0);
    opened.double_click(on_clip);
    assert_eq!(opened.editor_clip(), Some(id(PART)));
    assert!(opened.track_panel().is_none());
    // The track stays selected while its clip is edited.
    assert_eq!(opened.selected_track(), Some(id(TRACK)));

    let header = opened.track_header(1);
    opened.click(header);
    assert!(opened.editor().is_none());
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));

    // Enter on the selected clip swaps again, and escape closes the editor.
    opened.click(on_clip);
    opened.keys("enter");
    assert!(opened.editor().is_some());
    assert!(opened.track_panel().is_none());
    opened.keys("escape");
    assert!(opened.editor().is_none());
    assert!(opened.track_panel().is_none());
}

/// Devices need less room than notes: the track panel is 216 pt and the note editor 352, and
/// the timeline above takes the rest.
#[gpui::test]
fn the_track_panel_and_the_note_editor_have_heights_of_their_own(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let panel = opened.bounds("track-panel").unwrap();
    assert_eq!(panel.size.height, px(216.));
    let window = opened.cx.update(|window, _| window.viewport_size());
    assert_eq!(panel.bottom(), window.height);

    let on_clip = opened.at(BAR, 0);
    opened.double_click(on_clip);
    assert!(opened.editor().is_some());
    let editor = opened.bounds("note-editor").unwrap();
    assert_eq!(editor.size.height, px(352.));
    assert_eq!(editor.bottom(), window.height);
    assert_eq!(opened.bounds("track-panel"), None);
}

#[gpui::test]
fn the_keys_select_a_track_open_its_panel_and_close_it(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    let header = opened.track_header(0);
    opened.click(header);
    // Escape closes the panel from the timeline, and the track stays selected.
    opened.keys("escape");
    assert!(opened.track_panel().is_none());
    assert_eq!(opened.selected_track(), Some(id(TRACK)));
    // Down selects the next track. Enter opens its panel, and the panel follows up and down.
    opened.keys("down");
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
    assert!(opened.track_panel().is_none());
    opened.keys("enter");
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));
    opened.keys("down");
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));
    opened.keys("up");
    assert_eq!(opened.panel_track(), Some(id(TRACK)));

    // Tab goes into the panel: the close control, the volume, the pan, mute and solo of the
    // track, the picker and the expand icon of the card, the waveform, then the first knob.
    for _ in 0..8 {
        opened.keys("tab");
    }
    opened.keys("right");
    assert_eq!(synth(&mut opened).unwrap().waveform, Waveform::Square);
    opened.keys("tab");
    opened.keys("down");
    assert!(cutoff(&mut opened) < 2_000.0);
    // Escape from inside the panel closes it, and the keys are those of the timeline again.
    opened.keys("escape");
    assert!(opened.track_panel().is_none());
    opened.keys("down");
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
    // A selected clip gets the arrows before the track does.
    let on_clip = opened.at(BAR, 0);
    opened.click(on_clip);
    opened.keys("down");
    assert!(opened.clip("arrangement/track-2/part").is_some());
    assert_eq!(opened.selected_track(), Some(id("arrangement/track-2")));
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_and_the_sound_follows_every_move(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    opened.keys("space");
    opened.settle();
    opened.render(24_000);
    let before = brightness(&opened.render(12_000));

    let knob = opened.control(CUTOFF);
    opened.press(knob);
    assert!(!opened.gesture_open());
    // Half of the travel down: three octaves and a bit, from 2 kHz to 63.2 Hz.
    opened.drag_to(knob + point(px(0.), px(50.)));
    assert!(opened.gesture_open());
    let half_way = cutoff(&mut opened);
    assert!(half_way < 2_000.0 && half_way > 63.2);
    opened.drag_to(knob + point(px(30.), px(100.)));
    assert_eq!(cutoff(&mut opened), 63.2);
    // The file waits for the end of the drag. The sound does not.
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"cutoff_hz\": 2000.0")
    );
    opened.settle();
    opened.render(24_000);
    let during = brightness(&opened.render(12_000));
    assert!(
        during < before * 0.75,
        "the sound did not get darker during the drag: {before} then {during}"
    );
    assert_eq!(opened.undo_label(), None);

    opened.release(knob + point(px(30.), px(100.)));
    assert!(!opened.gesture_open());
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"cutoff_hz\": 63.2")
    );
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    let expected = SynthState {
        cutoff_hz: 63.2,
        ..SynthState::default()
    };
    assert_eq!(synth(&mut opened), Some(expected));

    // One step: one undo puts all of it back, in the project and in the file.
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"cutoff_hz\": 2000.0")
    );
    assert_eq!(opened.undo_label(), None);
    opened.keys("shift-cmd-z");
    assert_eq!(cutoff(&mut opened), 63.2);
}

#[gpui::test]
fn a_knob_stops_at_the_ends_of_its_range_and_a_drag_there_and_back_is_no_step(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-400.)));
    assert_eq!(cutoff(&mut opened), 20_000.0);
    opened.drag_to(knob + point(px(0.), px(400.)));
    assert_eq!(cutoff(&mut opened), 20.0);
    // Back where the press was: the value of the press, not what the live value gives.
    opened.drag_to(knob);
    assert_eq!(cutoff(&mut opened), 2_000.0);
    opened.release(knob);
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn escape_during_a_knob_drag_puts_the_value_back_and_keeps_the_panel(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    expand_synth(&mut opened);
    let knob = opened.control("knob-sustain");
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(60.)));
    assert!(synth(&mut opened).unwrap().sustain < 0.7);
    opened.keys("escape");
    assert!(!opened.gesture_open());
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(opened.undo_label(), None);
    assert_eq!(opened.redo_label(), None);
    assert!(opened.track_panel().is_some());
    // The mouse is still down. Its moves and its release do nothing more.
    opened.drag_to(knob + point(px(0.), px(90.)));
    opened.release(knob + point(px(0.), px(90.)));
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label(), None);
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"sustain\": 0.7")
    );
}

#[gpui::test]
fn a_click_on_a_knob_without_a_move_is_no_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let file = synth_file(&mut opened);
    let knob = opened.control(CUTOFF);
    opened.click(knob);
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label(), None);
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(synth_file(&mut opened), file);
}

#[gpui::test]
fn a_double_click_on_a_knob_sets_its_default_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    expand_synth(&mut opened);
    let knob = opened.control("knob-release_seconds");
    opened.drag(knob, knob + point(px(0.), px(-50.)));
    assert!(synth(&mut opened).unwrap().release_seconds > 0.3);
    assert_eq!(opened.undo_label().as_deref(), Some("Change release"));

    opened.double_click(knob);
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert!(!opened.gesture_open());
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"release_seconds\": 0.3")
    );
    // The reset is a step of its own, after the drag.
    opened.keys("cmd-z");
    assert!(synth(&mut opened).unwrap().release_seconds > 0.3);
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(opened.undo_label(), None);

    // At the default already: nothing to do, and no step.
    opened.double_click(knob);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn the_arrow_keys_step_the_focused_knob_and_shift_makes_the_step_fine(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    opened.click(knob);
    opened.keys("up");
    // A fiftieth of the travel of three decades.
    assert_eq!(cutoff(&mut opened), 2_300.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"cutoff_hz\": 2300.0")
    );
    opened.keys("shift-up");
    assert_eq!(cutoff(&mut opened), 2_330.0);
    opened.keys("shift-down");
    opened.keys("left");
    assert_eq!(cutoff(&mut opened), 2_000.0);
    // Each key is a step of its own.
    for expected in [2_300.0, 2_330.0, 2_300.0, 2_000.0] {
        opened.keys("cmd-z");
        assert_eq!(cutoff(&mut opened), expected);
    }
    assert_eq!(opened.undo_label(), None);

    // A linear knob, and the end of a range: no step past it.
    let gain = opened.control("knob-gain");
    opened.click(gain);
    opened.keys("right");
    assert_eq!(synth(&mut opened).unwrap().gain, 0.17);
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));
    for _ in 0..10 {
        opened.keys("down");
    }
    assert_eq!(synth(&mut opened).unwrap().gain, 0.0);
    // The arrows of a focused knob are not those of the timeline.
    assert_eq!(opened.selected_track(), Some(id(TRACK)));
}

#[gpui::test]
fn the_waveform_control_switches_the_oscillator(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let (saw, square) = (
        opened.control("segment-saw"),
        opened.control("segment-square"),
    );
    opened.click(square);
    assert_eq!(synth(&mut opened).unwrap().waveform, Waveform::Square);
    assert_eq!(opened.undo_label().as_deref(), Some("Change waveform"));
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"waveform\": \"square\"")
    );
    // The keys of the focused control.
    opened.keys("left");
    assert_eq!(synth(&mut opened).unwrap().waveform, Waveform::Saw);
    opened.keys("left");
    assert_eq!(synth(&mut opened).unwrap().waveform, Waveform::Saw);
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened).unwrap().waveform, Waveform::Square);
    opened.click(saw);
    opened.keys("cmd-z");
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn an_outside_edit_of_the_instrument_shows_in_the_knob(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    opened.click(knob);
    write_outside(
        &mut opened,
        r#"{"tool": "instrument.synth", "state": {"cutoff_hz": 200.0}}"#,
    );
    assert_eq!(cutoff(&mut opened), 200.0);
    // The knob holds no value of its own: a key steps from what the file said.
    opened.keys("up");
    assert_eq!(cutoff(&mut opened), 230.0);

    // During a drag the last write wins: the file first, then the next mouse move, which
    // works from the value at the press.
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-20.)));
    assert_eq!(cutoff(&mut opened), 459.0);
    write_outside(
        &mut opened,
        r#"{"tool": "instrument.synth", "state": {"cutoff_hz": 5000.0, "gain": 0.3}}"#,
    );
    assert_eq!(cutoff(&mut opened), 5_000.0);
    assert!(opened.gesture_open());
    opened.drag_to(knob + point(px(0.), px(-40.)));
    assert_eq!(cutoff(&mut opened), 916.0);
    opened.release(knob + point(px(0.), px(-40.)));
    // What else the file changed is kept.
    assert_eq!(synth(&mut opened).unwrap().gain, 0.3);
    assert!(
        synth_file(&mut opened)
            .unwrap()
            .contains("\"cutoff_hz\": 916.0")
    );
    assert!(!opened.gesture_open());
}

#[gpui::test]
fn an_outside_delete_of_the_instrument_during_a_knob_drag_ends_the_gesture(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(40.)));
    assert!(opened.gesture_open());

    let path = opened.path(SYNTH_FILE);
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    assert!(!opened.gesture_open());
    assert_eq!(synth(&mut opened), None);
    // The panel stays on its track, and the card says that the slot is empty.
    assert_eq!(opened.panel_track(), Some(id(TRACK)));
    assert_eq!(cards(&mut opened), [false]);
    // The mouse is still down. Nothing of it reaches the project, and nothing is reported.
    opened.drag_to(knob + point(px(0.), px(80.)));
    opened.release(knob + point(px(0.), px(80.)));
    assert!(!opened.gesture_open());
    assert_eq!(opened.notice(), None);
    assert_eq!(synth_file(&mut opened), None);

    // The delete was the last write. Undo gives the synth back as it was before the drag,
    // and the card shows its view again.
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(cards(&mut opened), [true]);
}

#[gpui::test]
fn an_outside_delete_of_the_track_during_a_knob_drag_closes_the_panel_cleanly(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-resonance");
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-40.)));
    assert!(opened.gesture_open());

    let folder = opened.path("state/arrangement/track-1");
    std::fs::remove_dir_all(&folder).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[folder]));
    assert!(!opened.gesture_open());
    assert!(opened.track_panel().is_none());
    assert_eq!(opened.selected_track(), None);
    opened.drag_to(knob + point(px(0.), px(-80.)));
    opened.release(knob + point(px(0.), px(-80.)));
    assert!(!opened.gesture_open());
    assert_eq!(opened.notice(), None);

    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
}

#[gpui::test]
fn the_panel_closes_with_its_track_when_an_undo_takes_the_track_away(cx: &mut TestAppContext) {
    let mut opened = open(cx);
    opened.edit(|project| {
        let arrangement = runtime::main_arrangement(project).unwrap();
        runtime::add_track(project, &arrangement)
    });
    let header = opened.track_header(2);
    opened.click(header);
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-3")));
    opened.keys("cmd-z");
    assert!(opened.track_panel().is_none());
    assert_eq!(opened.selected_track(), None);
    // The focus was on the timeline and stays there: its keys still work.
    opened.keys("shift-cmd-z");
    let header = opened.track_header(0);
    opened.click(header);
    opened.keys("down");
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));
}

#[gpui::test]
fn a_slot_without_a_registered_view_shows_the_empty_card_and_follows_the_file(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    assert_eq!(cards(&mut opened), [true]);
    // Another tool in the slot, from outside. The tone has no view.
    write_outside(
        &mut opened,
        r#"{"tool": "tone", "state": {"frequency_hz": 220.0, "gain": 0.1}}"#,
    );
    assert_eq!(
        opened.project(|project| project.tool_of(&id(SYNTH))),
        Some("tone")
    );
    assert_eq!(cards(&mut opened), [false]);
    // And a synth again: the card gets a new view of it.
    write_outside(&mut opened, r#"{"tool": "instrument.synth", "state": {}}"#);
    assert_eq!(cards(&mut opened), [true]);
    let knob = opened.control(CUTOFF);
    opened.click(knob);
    opened.keys("up");
    assert_eq!(cutoff(&mut opened), 2_300.0);

    // A panel that opens on a track with no instrument at all.
    let path = opened.path("state/arrangement/track-2/instrument.json");
    std::fs::remove_file(&path).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    let header = opened.track_header(1);
    opened.click(header);
    assert_eq!(opened.panel_track(), Some(id("arrangement/track-2")));
    assert_eq!(cards(&mut opened), [false]);
}

#[gpui::test]
fn closing_the_panel_during_a_knob_drag_leaves_no_gesture_open(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control("knob-gain");
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-40.)));
    assert!(opened.gesture_open());
    let arrangement = opened.arrangement.clone();
    opened
        .cx
        .update(|window, cx| arrangement.update(cx, |view, cx| view.close_detail(window, cx)));
    opened.cx.run_until_parked();
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));
    opened.release(knob);
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));
    // Nothing of the panel holds the session after the window: the project closes.
    drop(arrangement);
    drop(opened.close());
}

/// Counts how often an entity tells its observers that it changed, which is what makes GPUI
/// render it again.
fn notifications<T: 'static>(opened: &mut Opened<'_>, entity: &Entity<T>) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    let counted = count.clone();
    opened.cx.update(|_, cx| {
        cx.observe(entity, move |_, _| counted.set(counted.get() + 1))
            .detach()
    });
    count
}

#[gpui::test]
fn a_press_with_a_sideways_move_or_a_drag_there_and_back_keeps_a_value_written_by_hand(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    // More digits than a knob gives.
    write_outside(
        &mut opened,
        r#"{"tool": "instrument.synth", "state": {"cutoff_hz": 1234.5}}"#,
    );
    let (file, label) = (synth_file(&mut opened), opened.undo_label());
    assert!(file.as_ref().unwrap().contains("1234.5"));

    let knob = opened.control(CUTOFF);
    opened.press(knob);
    opened.drag_to(knob + point(px(3.), px(0.)));
    assert!(!opened.gesture_open());
    opened.release(knob + point(px(3.), px(0.)));
    assert_eq!(cutoff(&mut opened), 1234.5);
    assert_eq!(synth_file(&mut opened), file);
    assert_eq!(opened.undo_label(), label);

    // Up, where the knob gives three digits, and back to the height of the press.
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-25.)));
    assert_eq!(cutoff(&mut opened), 2930.0);
    opened.drag_to(knob + point(px(-2.), px(0.)));
    assert_eq!(cutoff(&mut opened), 1234.5);
    opened.release(knob + point(px(-2.), px(0.)));
    assert!(!opened.gesture_open());
    assert_eq!(synth_file(&mut opened), file);
    assert_eq!(opened.undo_label(), label);
}

#[gpui::test]
fn mouse_moves_between_two_frames_end_where_the_pointer_is(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    opened.press(knob);
    // Away and back before the next frame: the knob of that frame still has the old value.
    opened.drag_through(&[knob + point(px(0.), px(-37.5)), knob]);
    assert_eq!(cutoff(&mut opened), 2_000.0);
    opened.release(knob);
    assert!(!opened.gesture_open());
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(opened.undo_label(), None);

    // And away, back and away again: the last one counts.
    opened.press(knob);
    let away = knob + point(px(0.), px(-37.5));
    opened.drag_through(&[away, knob, away]);
    assert_eq!(cutoff(&mut opened), 7_300.0);
    opened.release(away);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
}

#[gpui::test]
fn a_press_elsewhere_ends_a_knob_drag_whose_mouse_up_was_lost(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let (knob, other) = (opened.control(CUTOFF), opened.control("knob-gain"));
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-37.5)));
    assert_eq!(cutoff(&mut opened), 7_300.0);
    assert!(opened.gesture_open());

    // No mouse up arrives. The next thing is a press on another knob, and a drag of that.
    opened.mouse_down(other);
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    opened.drag_to(other + point(px(0.), px(-50.)));
    // Only the knob under the new press moves.
    assert_eq!(cutoff(&mut opened), 7_300.0);
    assert_eq!(synth(&mut opened).unwrap().gain, 0.4);
    opened.release(other + point(px(0.), px(-50.)));
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain"));
    assert!(!opened.gesture_open());
    opened.keys("cmd-z");
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
}

#[gpui::test]
fn a_knob_drag_does_not_draw_the_timeline_and_a_timeline_click_does_not_draw_the_synth(
    cx: &mut TestAppContext,
) {
    let mut opened = open_panel(cx);
    let knob = opened.control(CUTOFF);
    let timeline = opened.timeline.clone();
    let timeline_notified = notifications(&mut opened, &timeline);
    opened.press(knob);
    opened.drag_to(knob + point(px(0.), px(-30.)));
    opened.drag_to(knob + point(px(0.), px(-60.)));
    opened.release(knob + point(px(0.), px(-60.)));
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    // The instrument of a track shows nowhere in the timeline.
    assert_eq!(timeline_notified.get(), 0);

    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.next().unwrap().cloned()
    });
    let synth_view = view.unwrap().downcast::<SynthView>().ok().unwrap();
    let (synth_notified, panel_notified) = (
        notifications(&mut opened, &synth_view),
        notifications(&mut opened, &panel),
    );
    // Every knob hears every mouse up of the window. With no drag open it tells nobody.
    let empty = opened.at(6 * BAR, 1);
    opened.click(empty);
    opened.click(empty);
    assert_eq!(synth_notified.get(), 0);
    assert_eq!(panel_notified.get(), 0);
    // A clip change still reaches the timeline.
    let on_clip = opened.at(BAR, 0);
    opened.click(on_clip);
    opened.keys("right");
    assert!(timeline_notified.get() > 0);
}

#[gpui::test]
fn the_rack_scrolls_so_that_the_last_knob_is_reachable_in_a_narrow_window(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    opened.cx.simulate_resize(size(px(400.), px(800.)));
    opened.cx.run_until_parked();
    let window_right = px(400.);
    let gain = opened.control("knob-gain");
    assert!(gain.x > window_right, "the window is not narrow enough");

    // A wheel has no sideways scroll: its up and down moves a rack that only goes sideways.
    let in_rack = opened.control("instrument-picker");
    opened.scroll(in_rack, 0., -100.);
    let gain = opened.control("knob-gain");
    assert!(gain.x + px(32.) < window_right, "gain is at {gain:?}");
    opened.click(gain);
    opened.keys("up");
    assert_eq!(synth(&mut opened).unwrap().gain, 0.17);
    // And back.
    opened.scroll(gain, 100., 0.);
    assert!(opened.control("knob-gain").x > window_right);
}

/// The mixer strip in the header column: the volume, the pan knob and the mute toggle, which
/// edit the record of the track itself.
const VOLUME: &str = "volume-gain_db";
const PAN_KNOB: &str = "knob-pan";
const MUTE: &str = "toggle-mute";
const TRACK_FILE: &str = "state/arrangement/track-1/instance.json";

fn track(opened: &mut Opened<'_>) -> Option<TrackState> {
    opened.project(|project| {
        let instance = project.resolve::<TrackState>(&id(TRACK))?;
        project.state(&instance).cloned()
    })
}

fn track_file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(TRACK_FILE)).unwrap()
}

/// The two channels of a render, apart.
fn channels(interleaved: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let channel = |first: usize| interleaved.iter().skip(first).step_by(2).copied().collect();
    (channel(0), channel(1))
}

/// Plays the note of `part` and renders past the attack, so the level is steady.
fn playing(opened: &mut Opened<'_>) -> Vec<f32> {
    opened.keys("space");
    opened.settle();
    opened.render(24_000);
    opened.render(12_000)
}

#[gpui::test]
fn a_drag_of_the_volume_is_one_undo_step_and_the_level_follows_every_move(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let loud = support::peak(&playing(&mut opened));
    assert!(loud > 0.0);

    let knob = opened.control(VOLUME);
    opened.press(knob);
    // The thumb follows the pointer on the scale of the meter: 50 pt down from 0 dB, at 80 %
    // of its 113 pt, is -21.7 dB.
    opened.drag_to(knob + point(px(0.), px(50.)));
    assert!(opened.gesture_open());
    let gain = track(&mut opened).unwrap().gain_db;
    assert_eq!(gain, -21.7);
    // The file waits for the end of the drag. The sound does not.
    assert!(track_file(&mut opened).contains("\"gain_db\": 0.0"));
    opened.settle();
    opened.render(24_000);
    let quieter = support::peak(&opened.render(12_000));
    assert!(quieter < loud * 0.3, "{loud} then {quieter}");
    assert_eq!(opened.undo_label(), None);

    opened.release(knob + point(px(0.), px(50.)));
    assert!(!opened.gesture_open());
    assert_eq!(opened.undo_label().as_deref(), Some("Change volume"));
    assert!(track_file(&mut opened).contains(&format!("\"gain_db\": {gain:?}")));

    // One step: one undo puts it back, in the project, in the file and in the sound.
    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().gain_db, 0.0);
    assert!(track_file(&mut opened).contains("\"gain_db\": 0.0"));
    opened.settle();
    opened.render(24_000);
    // As loud as before, within what a later stretch of the same note differs by.
    let again = support::peak(&opened.render(12_000));
    assert!((again - loud).abs() < loud * 0.01, "{loud} then {again}");
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn the_pan_knob_moves_the_track_to_one_channel(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let (left, right) = channels(&playing(&mut opened));
    assert_eq!(left, right);

    // Past the end of the travel: hard left.
    let knob = opened.control(PAN_KNOB);
    opened.drag(knob, knob + point(px(0.), px(400.)));
    assert_eq!(track(&mut opened).unwrap().pan, -1.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change pan"));
    opened.settle();
    opened.render(24_000);
    let (left, right) = channels(&opened.render(12_000));
    assert_eq!(support::peak(&right), 0.0);
    assert!(support::peak(&left) > 0.0);

    // A double click puts it back in the middle, as its own step.
    opened.double_click(knob);
    assert_eq!(track(&mut opened).unwrap().pan, 0.0);
    opened.settle();
    opened.render(24_000);
    let (left, right) = channels(&opened.render(12_000));
    assert_eq!(left, right);
    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().pan, -1.0);
}

#[gpui::test]
fn the_mute_button_is_one_undo_step_and_silences_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let loud = support::peak(&playing(&mut opened));

    let mute = opened.control(MUTE);
    opened.click(mute);
    assert_eq!(track(&mut opened).unwrap().mute, true);
    assert_eq!(opened.undo_label().as_deref(), Some("Mute track"));
    assert!(track_file(&mut opened).contains("\"mute\": true"));
    opened.settle();
    opened.render(24_000);
    assert_eq!(support::peak(&opened.render(12_000)), 0.0);

    // The button is the way back too, and it is another step.
    let mute = opened.control(MUTE);
    opened.click(mute);
    assert_eq!(track(&mut opened).unwrap().mute, false);
    assert_eq!(opened.undo_label().as_deref(), Some("Unmute track"));
    opened.settle();
    opened.render(24_000);
    let again = support::peak(&opened.render(12_000));
    assert!((again - loud).abs() < loud * 0.01, "{loud} then {again}");

    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().mute, true);
    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().mute, false);
    assert_eq!(opened.undo_label(), None);
    assert!(track_file(&mut opened).contains("\"mute\": false"));

    // Tab reaches it after the volume and the pan, and enter is the click.
    let gain = opened.control(VOLUME);
    opened.click(gain);
    opened.keys("tab");
    opened.keys("tab");
    opened.press_enter();
    assert_eq!(track(&mut opened).unwrap().mute, true);
}

#[gpui::test]
fn an_outside_edit_of_the_mixer_shows_in_the_panel_and_undo_takes_it_back(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = track_file(&mut opened);
    let path = opened.path(TRACK_FILE);
    let record = r#"{"tool": "arrangement.track", "state": {"name": "Track 1", "gain_db": -12.0, "pan": 1.0, "mute": true}}"#;
    std::fs::write(&path, record).unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]));
    let changed = track(&mut opened).unwrap();
    assert_eq!(
        (changed.gain_db, changed.pan, changed.mute),
        (-12.0, 1.0, true)
    );

    // The controls hold no value of their own: a key steps from what the file said.
    let gain = opened.control(VOLUME);
    opened.click(gain);
    opened.keys("up");
    assert_eq!(track(&mut opened).unwrap().gain_db, -11.5);
    let pan = opened.control(PAN_KNOB);
    opened.click(pan);
    opened.keys("down");
    assert_eq!(track(&mut opened).unwrap().pan, 0.96);

    // Three steps back: the two keys and the file change. The file is what it was.
    for _ in 0..3 {
        opened.keys("cmd-z");
    }
    assert_eq!(track_file(&mut opened), before);
    assert_eq!(opened.undo_label(), None);
}

#[gpui::test]
fn the_bottom_of_the_volume_is_silence_and_the_file_says_minus_inf(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let volume = opened.control(VOLUME);
    opened.drag(volume, volume + point(px(0.), px(400.)));
    assert_eq!(track(&mut opened).unwrap().gain_db, f32::NEG_INFINITY);
    assert!(
        track_file(&mut opened).contains(r#""gain_db": "-inf""#),
        "{}",
        track_file(&mut opened)
    );
    // One step.
    assert_eq!(opened.undo_label().as_deref(), Some("Change volume"));
    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().gain_db, 0.0);
    assert_eq!(opened.undo_label(), None);
    opened.keys("shift-cmd-z");

    // From the bottom, a press that does not move and a drag further down change nothing and
    // make no step.
    let volume = opened.control(VOLUME);
    opened.drag(volume, volume);
    opened.drag(volume, volume + point(px(0.), px(200.)));
    assert_eq!(track(&mut opened).unwrap().gain_db, f32::NEG_INFINITY);
    assert!(!opened.gesture_open());
    opened.keys("cmd-z");
    assert_eq!(track(&mut opened).unwrap().gain_db, 0.0);
    assert_eq!(opened.undo_label(), None);

    // And a double click is 0 dB again, as its own step.
    opened.keys("shift-cmd-z");
    opened.double_click(volume);
    assert_eq!(track(&mut opened).unwrap().gain_db, 0.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change volume"));
}

/// Each handle of the envelope edits the field of its knob, under the name of its knob in the
/// history, and one drag is one undo step. The knob is how the keys reach the same value.
#[gpui::test]
fn a_handle_of_the_envelope_edits_what_its_knob_edits_as_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    expand_synth(&mut opened);
    let file = synth_file(&mut opened);

    // The attack peak, sideways.
    let peak = opened.control("handle-attack");
    opened.press(peak);
    opened.drag_to(peak + point(px(10.), px(0.)));
    opened.drag_to(peak + point(px(20.), px(0.)));
    assert!(opened.gesture_open());
    // The file waits for the end of the drag.
    assert_eq!(synth_file(&mut opened), file);
    opened.release(peak + point(px(20.), px(0.)));
    let attack = synth(&mut opened).unwrap().attack_seconds;
    assert!(attack > ATTACK_DEFAULT, "{attack}");
    assert_eq!(
        synth(&mut opened),
        Some(SynthState {
            attack_seconds: attack,
            ..SynthState::default()
        })
    );
    assert_eq!(opened.undo_label().as_deref(), Some("Change attack"));
    // The handle and the knob show one value: the handle moved to where the knob points, and
    // a key on the knob steps from what the handle set.
    let moved = opened.control("handle-attack");
    assert!((moved.x - (peak.x + px(20.))).abs() < px(1.), "{moved:?}");
    let knob = opened.control("knob-attack_seconds");
    opened.click(knob);
    opened.keys("down");
    let stepped = synth(&mut opened).unwrap().attack_seconds;
    assert!(stepped < attack && stepped > ATTACK_DEFAULT, "{stepped}");
    let back = opened.control("handle-attack");
    assert!(back.x < moved.x, "the handle did not follow the knob");
    // Two steps: the key and the drag.
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened).unwrap().attack_seconds, attack);
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
    assert_eq!(opened.undo_label(), None);
    assert_eq!(synth_file(&mut opened), file);

    // The corner after the decay moves two values, and its drag is still one step.
    let corner = opened.control("handle-decay");
    opened.drag(corner, corner + point(px(15.), px(-20.)));
    let state = synth(&mut opened).unwrap();
    assert!(state.decay_seconds > 0.2, "{state:?}");
    assert!(state.sustain > 0.7, "{state:?}");
    assert_eq!(
        opened.undo_label().as_deref(),
        Some("Change decay and sustain")
    );
    opened.keys("cmd-z");
    assert_eq!(synth(&mut opened), Some(SynthState::default()));

    // A double click resets what a handle moves.
    let end = opened.control("handle-release");
    opened.drag(end, end + point(px(-30.), px(0.)));
    assert!(synth(&mut opened).unwrap().release_seconds < 0.3);
    let end = opened.control("handle-release");
    opened.double_click(end);
    assert_eq!(synth(&mut opened), Some(SynthState::default()));
}

/// The default attack of a synth.
const ATTACK_DEFAULT: f32 = 0.005;
