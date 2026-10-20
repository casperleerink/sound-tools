//! The note editor and the audio regions of the DAW template in the scenes of the deleted
//! arrangement's snapshots, the button still down, so the two compare side by side
//! (`WINDOW_SNAPSHOT_ONLY=takes`):
//!
//! - `look-velocity.png`: the melody's first clip in the editor, velocities of its own as a take
//!   has them, its fifth to seventh notes chosen.
//! - `look-marquee.png`: a rectangle over the middle of the same clip.
//! - `look-fade.png`: the fade in of the voice's last region dragged in by 0.42 of a bar.
//! - `look-gain.png`: its gain dragged down by 6 dB.
//! - `look-take-notes.png`: the keyboard recording on the first track from bar 5, a phrase, its
//!   last key still held at 7.2.

use anyhow::{Context, Result};
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use midi::Played;
use serde_json::{Value, json};
use sound_core::Ticks;
use sound_notes::{Pitch, Velocity};

use super::{
    RECORD, TOP, at, audio_files, away, click, piece_files, settle, wait_for_waveforms, write_audio,
};
use crate::song::{BAR, HEADER, ROW};

/// The pointer pressed at `from` with `modifiers` and moved to `to`, the button still down.
fn hold(
    opened: &crate::Opened,
    from: Point<Pixels>,
    to: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let moved = MouseMoveEvent {
        position: from,
        pressed_button: None,
        modifiers,
    };
    opened.event(PlatformInput::MouseMove(moved), cx)?;
    let down = MouseDownEvent {
        position: from,
        modifiers,
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    };
    opened.event(PlatformInput::MouseDown(down), cx)?;
    for step in [0.25, 0.5, 1.0] {
        let moved = MouseMoveEvent {
            position: from + (to - from) * step,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        };
        opened.event(PlatformInput::MouseMove(moved), cx)?;
    }
    settle(cx);
    Ok(())
}

/// A press and its release where it pressed, with `modifiers`.
fn tap(
    opened: &crate::Opened,
    at: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    hold(opened, at, at, modifiers, cx)?;
    let up = MouseUpEvent {
        position: at,
        modifiers,
        button: MouseButton::Left,
        click_count: 1,
    };
    opened.event(PlatformInput::MouseUp(up), cx)?;
    settle(cx);
    Ok(())
}

/// The melody's first clip, from bar 5, as the editor opens on it: fitted to its four bars, with
/// pitch 79 four and a half rows from the top of its roll, as `look-editor.png` has it.
fn in_editor(tick: u64, pitch: u64) -> Point<Pixels> {
    let across = (1470. - HEADER - 1. - 104.) / (4 * BAR) as f32;
    let row = 4.5 + (79. - pitch as f32);
    point(
        px(HEADER + 1. + 8. + tick as f32 * across),
        px(920. - 352. + 33. + row * 12.),
    )
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &crate::Save) -> Result<()> {
    // Velocities as the arrangement's scene gave them: 40 and on by 37, under 127.
    let mut files = piece_files(&[]);
    let mut melody = Vec::new();
    for (path, text) in &mut files {
        if path == "state/song/melody/clip-000.json" {
            let mut record: Value = serde_json::from_str(text)?;
            let notes = record["state"]["notes"]
                .as_array_mut()
                .into_iter()
                .flatten();
            for (index, note) in notes.enumerate() {
                note["velocity"] = json!(40 + (index * 37) % 87);
            }
            melody = record["state"]["notes"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            *text = record.to_string();
        }
    }
    let opened = crate::Opened::open(cx, &files)?;
    click(&opened, at(4 * BAR + 960, 2.), 2, cx)?;
    let middle = |note: &Value| {
        in_editor(
            note["start"].as_u64().unwrap_or(0) + 200,
            note["pitch"].as_u64().unwrap_or(0),
        )
    };
    tap(&opened, middle(&melody[4]), Modifiers::default(), cx)?;
    for index in [5, 6] {
        tap(&opened, middle(&melody[index]), Modifiers::command(), cx)?;
    }
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-velocity")?;
    hold(
        &opened,
        in_editor(BAR + 200, 82),
        in_editor(2 * BAR + 1900, 70),
        Modifiers::default(),
        cx,
    )?;
    save(cx, opened.window.into(), "look-marquee")?;
    drop(opened);

    // The voice's last region, from bar 9.5: chosen, then a handle at its top dragged.
    let (row, start) = (3., (8.5 * BAR as f64) as u64);
    let handle =
        |tick: u64, right: f32| point(at(tick, row).x + px(right), px(TOP + row * ROW + 11.));
    let poses = [
        (
            "look-fade",
            handle(start, 5.),
            point(px(96. * 0.42), px(0.)),
        ),
        (
            "look-gain",
            handle(start + (1.625 * BAR as f64) as u64, 0.),
            point(px(0.), px(200. / 72. * 6.)),
        ),
    ];
    for (name, from, by) in poses {
        let mut opened = crate::Opened::open_with(cx, &audio_files(), write_audio)?;
        wait_for_waveforms(&mut opened, cx)?;
        click(&opened, at(start + BAR, row), 1, cx)?;
        hold(&opened, from, from + by, Modifiers::default(), cx)?;
        save(cx, opened.window.into(), name)?;
    }

    let mut opened = crate::Opened::open(cx, &piece_files(&[]))?;
    cx.update(|cx| (opened.session).update(cx, |session, _| session.engine().seek(Ticks(4 * BAR))));
    opened.advance(64, cx);
    settle(cx);
    click(&opened, point(px(RECORD.0), px(RECORD.1)), 1, cx)?;
    let keyboard = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        anyhow::Ok(shell.live_input().read(cx).midi_input())
    })?;
    let keyboard = keyboard.context("the keyboard")?;
    let poll = (48_000. * sound_ui::POLL_INTERVAL.as_secs_f32()) as usize;
    // A key every eighth of a second and a half, each held for a third of that.
    let phrase = [60, 64, 67, 72, 71, 67, 65, 62, 64, 69, 72, 76, 74, 71, 67];
    let velocity = Velocity::new(100).context("a velocity")?;
    for index in 0..280 {
        let (key, step) = (index / 18, index % 18);
        if let Some(pitch) = phrase.get(key).and_then(|pitch| Pitch::new(*pitch).ok()) {
            match step {
                0 => _ = keyboard.send(Played::On { pitch, velocity }),
                6 if key + 1 < phrase.len() => {
                    _ = keyboard.send(Played::Off { pitch, velocity: 0 })
                }
                _ => {}
            }
        }
        opened.advance(poll, cx);
        opened.poll_recording(cx)?;
    }
    cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
    cx.run_until_parked();
    save(cx, opened.window.into(), "look-take-notes")?;
    Ok(())
}
