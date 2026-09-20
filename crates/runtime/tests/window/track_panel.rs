//! The track panel and the view of the synth in it, with a simulated mouse and keys. Every
//! test checks the project, the undo history and the file of the instrument.

use gpui::{TestAppContext, point, px};
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

    // Tab goes into the panel: the close control, the waveform, then the first knob.
    opened.keys("tab");
    opened.keys("tab");
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
    opened.drag_to(knob + point(px(0.), px(40.)));
    assert!(opened.gesture_open());
    let half_way = cutoff(&mut opened);
    assert!(half_way < 2_000.0 && half_way > 63.2);
    opened.drag_to(knob + point(px(30.), px(80.)));
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

    opened.release(knob + point(px(30.), px(80.)));
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
    opened.drag_to(knob + point(px(0.), px(-16.)));
    assert_eq!(cutoff(&mut opened), 459.0);
    write_outside(
        &mut opened,
        r#"{"tool": "instrument.synth", "state": {"cutoff_hz": 5000.0, "gain": 0.3}}"#,
    );
    assert_eq!(cutoff(&mut opened), 5_000.0);
    assert!(opened.gesture_open());
    opened.drag_to(knob + point(px(0.), px(-32.)));
    assert_eq!(cutoff(&mut opened), 916.0);
    opened.release(knob + point(px(0.), px(-32.)));
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
