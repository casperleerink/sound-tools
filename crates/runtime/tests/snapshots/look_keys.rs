//! The DAW template worked with the keys alone, no pointer (`WINDOW_SNAPSHOT_ONLY=keys`): the
//! piece with a return, Space, and a picture after each step, `keys-NN-*.png`. Each shows where
//! the keys are: the lavender ring of the control that has the focus, a text field with its
//! caret, or what is chosen.
//!
//! - `keys-01-add-track.png`: tab to Add track, enter: a fourth track.
//! - `keys-02-choose-track.png`: shift-tab to its head, enter: its panel.
//! - `keys-03-add-clip.png`: shift-c: a clip at the playhead, chosen.
//! - `keys-04-open-clip.png`: cmd-down: its notes, none yet.
//! - `keys-05-add-note.png`: n: a note, chosen, which the head says.
//! - `keys-06-type-pitch.png`: tab to its pitch, enter: the field to type it in, `e4` typed.
//! - `keys-07-pitch-typed.png`: enter: the note is E4, and its pitch keeps the keys.
//! - `keys-08-four-notes.png`: n, enter, a pitch and enter, three times more: G4, A4 and F4.
//! - `keys-09-type-length.png`: tab, tab to the length of the last, enter, `1/4` typed.
//! - `keys-10-type-velocity.png`: enter, tab to its velocity, enter, `64` typed.
//! - `keys-11-velocity-typed.png`: enter: its bar in the lane is half as tall.
//! - `keys-12-effect-menu.png`: escape twice closes the notes; tab to Add effect, enter, `sat`
//!   typed: the menu keeps the saturator, highlighted.
//! - `keys-13-add-effect.png`: enter: a saturator after the synth, chosen, as the fill around
//!   its card says.
//! - `keys-14-second-effect.png`: enter, `util`, enter: a utility after it, chosen.
//! - `keys-15-move-earlier.png`: alt-left: the utility before the saturator.
//! - `keys-16-type-send.png`: tab to the dial of what the track sends into Space, enter, `-12` typed.
//! - `keys-17-send-typed.png`: enter: it sends -12 dB.

use anyhow::{Context as _, Result};
use gpui::{HeadlessAppContext, KeyUpEvent, Keystroke, Modifiers, PlatformInput};
use serde_json::json;

use super::{open, settle};

/// The lavender of a focus ring.
const RING: [u8; 3] = [0xa9, 0xb1, 0xff];

/// A part of the window in points: its left, its top, its width and its height.
type Part = (f32, f32, f32, f32);

/// One key as a hand presses it: down, what it types into a text field, and up, as enter
/// clicks what has the focus when it comes up. Then the frame that draws the focus it gave.
fn stroke(opened: &crate::Opened, keystroke: Keystroke, cx: &mut HeadlessAppContext) -> Result<()> {
    cx.update_window(opened.window.into(), |_, window, cx| {
        window.dispatch_keystroke(keystroke.clone(), cx);
    })?;
    cx.run_until_parked();
    opened.event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx)?;
    settle(cx);
    cx.capture_screenshot(opened.window.into())?;
    Ok(())
}

/// The keys of `keys`, one after the other: `"shift-tab enter"`.
fn keys(opened: &crate::Opened, keys: &str, cx: &mut HeadlessAppContext) -> Result<()> {
    for key in keys.split(' ') {
        stroke(opened, Keystroke::parse(key)?, cx)?;
    }
    Ok(())
}

/// `text` typed into the field that has the keys, a letter at a time.
fn typed(opened: &crate::Opened, text: &str, cx: &mut HeadlessAppContext) -> Result<()> {
    for letter in text.chars() {
        let keystroke = Keystroke {
            modifiers: Modifiers::default(),
            key: letter.to_string(),
            key_char: Some(letter.to_string()),
        };
        stroke(opened, keystroke, cx)?;
    }
    Ok(())
}

/// The points of `part` that have the colour of a focus ring now.
fn ring_points(
    opened: &crate::Opened,
    part: Part,
    cx: &mut HeadlessAppContext,
) -> Result<Vec<(u32, u32)>> {
    let picture = cx.capture_screenshot(opened.window.into())?;
    let (left, top, width, height) = part;
    let [x0, y0, x1, y1] = [left, top, left + width, top + height].map(|at| (at * 2.) as u32);
    let points = (y0..y1.min(picture.height()))
        .flat_map(|y| (x0..x1.min(picture.width())).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let pixel = picture.get_pixel(*x, *y).0;
            (0..3).all(|part| pixel[part].abs_diff(RING[part]) < 16)
        });
    Ok(points.collect())
}

/// Presses `key`, tab or shift-tab, until the focus is in `part`: a ring that was not there
/// before the key shows in it. So the walk is right however many tab stops the cards on the
/// way have.
fn tab_to(
    opened: &crate::Opened,
    key: &str,
    part: Part,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    for _ in 0..200 {
        let before = ring_points(opened, part, cx)?;
        keys(opened, key, cx)?;
        let after = ring_points(opened, part, cx)?;
        // More than a caret has.
        if after.iter().filter(|point| !before.contains(point)).count() > 60 {
            return Ok(());
        }
    }
    anyhow::bail!("{key} did not reach {part:?}")
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &crate::Save) -> Result<()> {
    let reverb =
        json!({ "tool": "reverb", "state": { "decay_seconds": 2.4, "size": 0.6, "mix": 1.0 } });
    let opened = open(
        cx,
        &[
            (
                "ret-a/instance.json",
                json!({ "tool": "bus", "state": { "title": "Space", "colour": "teal" } }),
            ),
            ("ret-a/mix.json", json!({ "tool": "fader", "state": {} })),
            (
                "ret-a/chain/instance.json",
                json!({ "tool": "chain", "state": { "order": ["reverb"] } }),
            ),
            ("ret-a/chain/reverb.json", reverb),
        ],
    )?;
    let window = opened.window.into();
    let mut step = 0;
    let mut shot = |cx: &mut HeadlessAppContext, name: &str| {
        step += 1;
        save(cx, window, &format!("keys-{step:02}-{name}"))
    };
    // The heads of the tracks are 64 points each under the ruler, and Add track is under them.
    let row = |row: f32| (0., super::TOP + row * super::ROW, 176., super::ROW);

    tab_to(&opened, "tab", row(3.), cx)?;
    keys(&opened, "enter", cx)?;
    shot(cx, "add-track")?;
    tab_to(&opened, "shift-tab", row(3.), cx)?;
    keys(&opened, "enter", cx)?;
    shot(cx, "choose-track")?;
    keys(&opened, "shift-c", cx)?;
    shot(cx, "add-clip")?;
    keys(&opened, "cmd-down", cx)?;
    shot(cx, "open-clip")?;
    keys(&opened, "n", cx)?;
    shot(cx, "add-note")?;

    // The values of the chosen note, in the head of the editor: its pitch is the first.
    let editor = crate::WINDOW_HEIGHT - 352.;
    tab_to(&opened, "tab", (900., editor, 570., 32.), cx)?;
    keys(&opened, "enter", cx)?;
    typed(&opened, "e4", cx)?;
    shot(cx, "type-pitch")?;
    keys(&opened, "enter", cx)?;
    shot(cx, "pitch-typed")?;
    for pitch in ["g4", "a4", "f4"] {
        keys(&opened, "n enter", cx)?;
        typed(&opened, pitch, cx)?;
        keys(&opened, "enter", cx)?;
    }
    shot(cx, "four-notes")?;
    keys(&opened, "tab tab enter", cx)?;
    typed(&opened, "1/4", cx)?;
    shot(cx, "type-length")?;
    keys(&opened, "enter tab enter", cx)?;
    typed(&opened, "64", cx)?;
    shot(cx, "type-velocity")?;
    keys(&opened, "enter", cx)?;
    shot(cx, "velocity-typed")?;

    // Escape lets go of the note, and again closes the notes: the panel of the track is back.
    keys(&opened, "escape escape", cx)?;
    let panel = crate::WINDOW_HEIGHT - 216.;
    tab_to(&opened, "tab", (176., panel, 1100., 60.), cx).context("Add effect")?;
    keys(&opened, "enter", cx)?;
    typed(&opened, "sat", cx)?;
    shot(cx, "effect-menu")?;
    keys(&opened, "enter", cx)?;
    shot(cx, "add-effect")?;
    keys(&opened, "enter", cx)?;
    typed(&opened, "util", cx)?;
    keys(&opened, "enter", cx)?;
    shot(cx, "second-effect")?;
    keys(&opened, "alt-left", cx)?;
    shot(cx, "move-earlier")?;

    // The sends of the track, at the right of its panel: the dial has the keys for what it sends.
    tab_to(&opened, "tab", (1312., panel + 44., 40., 32.), cx).context("the send")?;
    keys(&opened, "enter", cx)?;
    typed(&opened, "-12", cx)?;
    shot(cx, "type-send")?;
    keys(&opened, "enter", cx)?;
    shot(cx, "send-typed")?;
    Ok(())
}
