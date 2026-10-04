//! The card of the built-in utility in the track rack, with a simulated mouse: it is added from
//! the control at the end of the rack, every edit of it is one undo step written once, and what
//! it does is heard.

use gpui::{TestAppContext, point, px};
use utility::view::UtilityView;
use utility::{Channels, UtilityState};

use crate::support::{self, Opened, id};

const UTILITY: &str = "arrangement/track-1/utility";
const UTILITY_FILE: &str = "state/arrangement/track-1/utility.json";

/// `Track 1` with a note that plays for four bars, and its panel open. The utility is
/// added the way a composer adds it: `Add effect`, then `Utility`.
fn open_panel(cx: &mut TestAppContext) -> Opened<'_> {
    let mut opened = support::open_with(cx, |project| {
        let mut changes = sound_core::Changes::new();
        let note = support::note(0, 4 * support::BAR, 64);
        let part = support::clip(0, 4 * support::BAR, vec![note]);
        changes.create(id("arrangement/track-1/part"), part);
        project.commit("Add clip", changes).unwrap();
        project.clear_history();
    });
    let header = opened.track_header(0);
    opened.click(header);
    let trigger = opened.control("add-effect");
    opened.click(trigger);
    let row = opened.control("menu-utility");
    opened.click(row);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Utility"));
    opened.project(|project| assert_eq!(project.problems(), []));
    opened
}

fn state(opened: &mut Opened<'_>) -> UtilityState {
    opened.project(|project| {
        let utility = project.resolve::<UtilityState>(&id(UTILITY)).unwrap();
        *project.state(&utility).unwrap()
    })
}

fn file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path(UTILITY_FILE)).unwrap()
}

/// What the track plays, left and right, after a few blocks for any glide.
fn heard(opened: &mut Opened<'_>) -> (Vec<f32>, Vec<f32>) {
    opened.settle();
    opened.cx.update(|_, cx| {
        let session = opened.session.clone();
        session.update(cx, |session, _| session.engine().play());
    });
    opened.settle();
    opened.render(4_800);
    let render = opened.render(12_000);
    let left = render.iter().step_by(2).copied().collect();
    let right = render.iter().skip(1).step_by(2).copied().collect();
    (left, right)
}

fn loudness(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample.abs()).sum()
}

#[gpui::test]
fn add_effect_puts_a_utility_with_its_card_on_the_track(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    assert_eq!(state(&mut opened), UtilityState::default());
    let panel = opened.track_panel().unwrap();
    let view = opened.cx.read(|cx| {
        let mut views = panel.read(cx).device_views();
        views.nth(1).unwrap().cloned()
    });
    assert!(view.unwrap().downcast::<UtilityView>().is_ok());
    // The shown controls, and none of the hidden ones.
    for shown in [
        "knob-gain_db",
        "knob-utility-pan",
        "knob-width",
        "toggle-utility-mute",
        "handle-gain-pan",
        "segment-stereo",
    ] {
        assert!(opened.find(shown).is_some(), "{shown}");
    }
    for hidden in [
        "toggle-bass_mono",
        "knob-bass_mono_hz",
        "toggle-invert_left",
        "toggle-invert_right",
    ] {
        assert_eq!(opened.find(hidden), None, "{hidden}");
    }

    // One undo takes it off again.
    opened.edit(|project| project.undo().map(|_| ()));
    assert!(!opened.path(UTILITY_FILE).exists());
}

#[gpui::test]
fn a_knob_drag_is_one_undo_step_written_once(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let before = file(&mut opened);
    let knob = opened.control("knob-width");
    opened.press(knob);
    opened.drag_to(point(knob.x, knob.y - px(20.)));
    // Heard during the drag, not written until it ends.
    let moving = state(&mut opened).width;
    assert!(moving > UtilityState::default().width);
    assert_eq!(file(&mut opened), before);
    opened.drag_to(point(knob.x, knob.y - px(40.)));
    opened.release(point(knob.x, knob.y - px(40.)));
    let after = state(&mut opened).width;
    assert!(after > moving);
    assert_eq!(opened.undo_label().as_deref(), Some("Change width"));
    assert!(file(&mut opened).contains(&format!("\"width\": {after:?}")));

    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), UtilityState::default());
    assert_eq!(opened.undo_label().as_deref(), Some("Add Utility"));
}

/// The handle moves pan sideways and gain up and down, in one gesture and one undo step.
#[gpui::test]
fn the_handle_drags_gain_and_pan_as_one_step(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let handle = opened.control("handle-gain-pan");
    opened.drag(handle, point(handle.x + px(20.), handle.y - px(10.)));
    let after = state(&mut opened);
    assert!(after.pan > 0.0 && after.gain_db > 0.0, "{after:?}");
    assert_eq!(opened.undo_label().as_deref(), Some("Change gain and pan"));

    // Far past the right and the top: pan and gain stop at their most.
    let handle = opened.control("handle-gain-pan");
    opened.drag(handle, point(handle.x + px(300.), handle.y - px(300.)));
    let after = state(&mut opened);
    assert_eq!(
        (after.pan, after.gain_db),
        (utility::PAN.max, utility::GAIN.max)
    );

    opened.edit(|project| project.undo().map(|_| ()));
    opened.edit(|project| project.undo().map(|_| ()));
    assert_eq!(state(&mut opened), UtilityState::default());
}

/// Expand shows bass mono and the inverts, is no edit, and is not saved. A switch and a segment
/// are one click and one step each.
#[gpui::test]
fn expand_shows_the_hidden_controls_and_a_switch_is_one_click(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-utility-expand");
    opened.click(expand);
    assert_eq!(opened.undo_label().as_deref(), Some("Add Utility"));
    for hidden in [
        "toggle-bass_mono",
        "knob-bass_mono_hz",
        "toggle-invert_left",
        "toggle-invert_right",
    ] {
        assert!(opened.find(hidden).is_some(), "{hidden}");
    }
    let bass_mono = opened.control("toggle-bass_mono");
    opened.click(bass_mono);
    assert!(state(&mut opened).bass_mono);
    assert_eq!(opened.undo_label().as_deref(), Some("Change bass mono"));
    assert!(file(&mut opened).contains(r#""bass_mono": true"#));

    let invert = opened.control("toggle-invert_right");
    opened.click(invert);
    assert!(state(&mut opened).invert_right);
    assert_eq!(opened.undo_label().as_deref(), Some("Change invert right"));

    let swap = opened.control("segment-swap");
    opened.click(swap);
    assert_eq!(state(&mut opened).channels, Channels::Swap);
    assert_eq!(opened.undo_label().as_deref(), Some("Change channels"));

    let expand = opened.control("card-utility-expand");
    opened.click(expand);
    assert_eq!(opened.find("toggle-bass_mono"), None);
}

/// What the card does is heard: the left upside down cancels the right of a synth that plays the
/// same in both, and mute silences it. The power icon bypasses the utility and brings it back.
#[gpui::test]
fn what_the_card_does_is_heard(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let (left, right) = heard(&mut opened);
    let level = loudness(&left);
    assert!(level > 1.0, "{level}");
    assert_eq!(left, right);

    let expand = opened.control("card-utility-expand");
    opened.click(expand);
    let invert = opened.control("toggle-invert_left");
    opened.click(invert);
    let (left, right) = heard(&mut opened);
    let sum: Vec<f32> = left.iter().zip(&right).map(|(l, r)| l + r).collect();
    assert_eq!(loudness(&sum), 0.0);
    assert!(loudness(&left) > 1.0);

    let mute = opened.control("toggle-utility-mute");
    opened.click(mute);
    assert!(state(&mut opened).mute);
    assert_eq!(opened.undo_label().as_deref(), Some("Change mute"));
    let (left, right) = heard(&mut opened);
    assert_eq!(loudness(&left) + loudness(&right), 0.0);

    let power = opened.control("card-utility-power");
    opened.click(power);
    assert_eq!(opened.undo_label().as_deref(), Some("Turn off Utility"));
    let (left, right) = heard(&mut opened);
    assert!(loudness(&left) > 1.0);
    assert_eq!(left, right);
}

/// An agent edits the file while the card is open: the card shows it at once.
#[gpui::test]
fn an_outside_edit_shows_on_the_card(cx: &mut TestAppContext) {
    let mut opened = open_panel(cx);
    let expand = opened.control("card-utility-expand");
    opened.click(expand);
    let path = opened.path(UTILITY_FILE);
    std::fs::write(
        &path,
        r#"{"tool": "utility", "state": {"invert_left": true, "width": 0.5}}"#,
    )
    .unwrap();
    opened.edit(|project| project.apply_outside_changes(&[path]).map(|_| ()));
    assert!(state(&mut opened).invert_left);
    // The toggle shows what the file says: a click turns the invert off, not on again.
    let invert = opened.control("toggle-invert_left");
    opened.click(invert);
    assert!(!state(&mut opened).invert_left);
    assert_eq!(state(&mut opened).width, 0.5);
    assert_eq!(opened.undo_label().as_deref(), Some("Change invert left"));
}
