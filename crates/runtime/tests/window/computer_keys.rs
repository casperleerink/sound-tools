//! The computer keys playing the track a MIDI keyboard plays, with simulated keys.

use gpui::{KeyUpEvent, Keystroke};

use crate::support::{Opened, id, open_with, peak};

/// A new project with `Track 1` and its synth, its header clicked: the timeline has the focus,
/// where `a` shows the automation lanes of the track.
fn opened(cx: &mut gpui::TestAppContext) -> Opened<'_> {
    let mut opened = open_with(cx, |_| {});
    let header = opened.track_header(0);
    opened.click(header);
    opened.settle();
    opened
}

fn shows_lanes(opened: &mut Opened<'_>) -> bool {
    let timeline = opened.timeline.clone();
    opened
        .cx
        .read(|cx| timeline.read(cx).shows_lanes(&id("arrangement/track-1")))
}

fn keys_are_on(opened: &mut Opened<'_>) -> bool {
    let transport = opened.transport();
    opened
        .cx
        .read(|cx| transport.read(cx).computer_keys_are_on())
}

/// `keys` only sends a key going down.
fn key_up(opened: &mut Opened<'_>, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    opened.cx.simulate_event(KeyUpEvent { keystroke });
    opened.settle();
}

#[gpui::test]
fn a_key_holds_a_note_of_the_track_until_it_comes_up_and_does_nothing_else(
    cx: &mut gpui::TestAppContext,
) {
    let mut opened = opened(cx);
    opened.keys("secondary-k");
    assert!(keys_are_on(&mut opened));
    opened.settle();
    assert_eq!(peak(&opened.render(4_800)), 0.0);

    opened.keys("a");
    opened.settle();
    // Held for a second, past the decay of the synth.
    let held = opened.render(48_000);
    assert!(
        peak(&held[held.len() - 4_800..]) > 0.01,
        "the note did not hold"
    );
    assert!(
        !shows_lanes(&mut opened),
        "`a` also showed the automation lanes"
    );

    key_up(&mut opened, "a");
    opened.render(48_000);
    assert_eq!(
        peak(&opened.render(4_800)),
        0.0,
        "the note goes on sounding"
    );

    // Off again, `a` is the arrangement's.
    opened.keys("secondary-k");
    assert!(!keys_are_on(&mut opened));
    opened.keys("a");
    assert!(shows_lanes(&mut opened));
}

/// cmd going down ends the notes: macOS sends no key up while it is held.
#[gpui::test]
fn a_held_key_ends_when_cmd_goes_down(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("secondary-k");
    opened.settle();
    opened.keys("a");
    opened.settle();
    assert!(peak(&opened.render(4_800)) > 0.01);
    opened
        .cx
        .simulate_modifiers_change(gpui::Modifiers::command());
    opened.settle();
    opened.render(48_000);
    assert_eq!(peak(&opened.render(4_800)), 0.0, "a note goes on sounding");
}

#[gpui::test]
fn a_take_records_the_keys_and_space_and_r_stay_the_window_s(cx: &mut gpui::TestAppContext) {
    let mut opened = opened(cx);
    opened.keys("secondary-k");
    opened.keys("space");
    opened.settle();
    assert!(opened.playhead().playing);
    opened.keys("space");
    opened.settle();
    opened.keys("r");
    opened.settle();
    assert!(opened.is_recording());

    opened.keys("d");
    opened.render(12_000);
    opened.settle();
    key_up(&mut opened, "d");
    opened.keys("r");
    opened.settle();
    let clip = opened.clip("arrangement/track-1/take");
    let clip = clip.expect("the take became a clip");
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].pitch.number(), 64);
    assert!(opened.path("assets/takes/take-1.json").exists());
}
