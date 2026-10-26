//! The timeline of the DAW template in the scenes of the deleted arrangement's editing
//! snapshots, so the two compare side by side (`WINDOW_SNAPSHOT_ONLY=timeline`):
//!
//! - `look-selection.png`: the piece with tempo changes at bars 5 and 9, the bass's second and
//!   third clips and the melody's first chosen.
//! - `look-tempo.png`: the same with the tempo lane open and its change of bar 5 clicked.
//! - `look-loop.png`: the same with a loop dragged over bars 3 to 6.
//! - `look-snap.png`: the snap setting open in the corner.
//! - `look-rename.png`: the name field of the bass, open by a double click.
//! - `look-ruler-sections.png`: three sections, the chorus clicked.
//! - `look-ruler-menu.png`: the menu of a right click past the middle of bar 7, two rows down.
//! - `look-clip-menu.png`: the menu of a right click on a chosen clip.

use anyhow::Result;
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use serde_json::{Value, json};

use super::{at, away, click, piece_files, settle};
use crate::song::{ACROSS, BAR, HEADER, LEAD};

/// Where `tick` is on the top line of the ruler, `right` points after it.
fn in_ruler(tick: u64, right: f32) -> Point<Pixels> {
    point(
        px(HEADER + LEAD + tick as f32 * ACROSS + right),
        px(48. + 8.),
    )
}

/// The tempo toggle left of the ruler.
const TEMPO_TOGGLE: (f32, f32) = (20., 48. + 22.);

fn press(
    opened: &crate::Opened,
    at: Point<Pixels>,
    button: MouseButton,
    modifiers: Modifiers,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let moved = MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers,
    };
    opened.event(PlatformInput::MouseMove(moved), cx)?;
    let down = MouseDownEvent {
        position: at,
        modifiers,
        button,
        click_count: 1,
        first_mouse: false,
    };
    opened.event(PlatformInput::MouseDown(down), cx)?;
    let up = MouseUpEvent {
        position: at,
        modifiers,
        button,
        click_count: 1,
    };
    opened.event(PlatformInput::MouseUp(up), cx)?;
    settle(cx);
    Ok(())
}

fn drag(
    opened: &crate::Opened,
    from: Point<Pixels>,
    to: Point<Pixels>,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let modifiers = Modifiers::default();
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
    let up = MouseUpEvent {
        position: to,
        modifiers,
        button: MouseButton::Left,
        click_count: 1,
    };
    opened.event(PlatformInput::MouseUp(up), cx)?;
    settle(cx);
    Ok(())
}

/// The piece with `project` merged into its `project.json`.
fn piece_with(project: Value) -> Result<Vec<(String, String)>> {
    let mut files = piece_files(&[]);
    if let Some((_, file)) = files.iter_mut().find(|(path, _)| path == "project.json") {
        let mut merged: Value = serde_json::from_str(file)?;
        if let (Some(merged), Some(more)) = (merged.as_object_mut(), project.as_object()) {
            merged.extend(more.clone());
        }
        *file = merged.to_string();
    }
    Ok(files)
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &crate::Save) -> Result<()> {
    let tempo = json!({ "tempo_map": { "time_signature": "4/4", "tempo_changes": [
        { "tick": 0, "bpm": 120.0 }, { "tick": 4 * BAR, "bpm": 96.0 }, { "tick": 8 * BAR, "bpm": 132.5 },
    ] } });
    let mut opened = crate::Opened::open(cx, &piece_with(tempo.clone())?)?;
    opened.advance(512, cx);
    let cmd = Modifiers {
        platform: true,
        ..Modifiers::default()
    };
    click(&opened, at(4 * BAR + 960, 1.), 1, cx)?;
    press(&opened, at(8 * BAR + 960, 1.), MouseButton::Left, cmd, cx)?;
    press(&opened, at(4 * BAR + 960, 2.), MouseButton::Left, cmd, cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-selection")?;

    let toggle = point(px(TEMPO_TOGGLE.0), px(TEMPO_TOGGLE.1));
    click(&opened, toggle, 1, cx)?;
    click(
        &opened,
        in_ruler(4 * BAR, 0.) + point(px(0.), px(44. + 8.)),
        1,
        cx,
    )?;
    opened.advance(64, cx);
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-tempo")?;
    click(&opened, toggle, 1, cx)?;

    drag(&opened, in_ruler(2 * BAR, 0.), in_ruler(6 * BAR, 0.), cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-loop")?;

    click(&opened, point(px(HEADER - 40.), px(48. + 22.)), 1, cx)?;
    save(cx, opened.window.into(), "look-snap")?;
    opened.key("escape", cx)?;
    settle(cx);

    click(&opened, point(px(60.), px(80. + 64. + 18.)), 2, cx)?;
    save(cx, opened.window.into(), "look-rename")?;
    opened.key("escape", cx)?;
    settle(cx);
    drop(opened);

    let mut sections = tempo;
    sections["sections"] = json!([{ "bar": 1, "name": "Intro" }, { "bar": 5, "name": "Verse" }, { "bar": 9, "name": "Chorus" }]);
    let mut opened = crate::Opened::open(cx, &piece_with(sections)?)?;
    opened.advance(512, cx);
    settle(cx);
    click(&opened, in_ruler(8 * BAR, 30.), 1, cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-ruler-sections")?;

    press(
        &opened,
        in_ruler(6 * BAR + 2000, 0.),
        MouseButton::Right,
        Modifiers::default(),
        cx,
    )?;
    opened.key("down", cx)?;
    opened.key("down", cx)?;
    settle(cx);
    save(cx, opened.window.into(), "look-ruler-menu")?;
    opened.key("escape", cx)?;
    settle(cx);

    click(&opened, at(4 * BAR + 960, 1.), 1, cx)?;
    press(
        &opened,
        at(4 * BAR + 960, 1.),
        MouseButton::Right,
        Modifiers::default(),
        cx,
    )?;
    save(cx, opened.window.into(), "look-clip-menu")?;
    Ok(())
}
