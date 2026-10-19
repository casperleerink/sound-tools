//! The DAW template drawing what the Rust arrangement's snapshots drew, with the same content,
//! so the two can be compared side by side: `look-*.png`.
//!
//! - `look-empty.png`: a song with no tracks.
//! - `look-piece.png`: the piece playing at 6.3, the second clip of the bass chosen.
//! - `look-editor.png`: the note editor on the first clip of the melody.
//! - `look-track-panel.png`: the panel of the bass, with a reverb after its synth.
//! - `look-lanes.png`: the lanes of the bass: its volume and the cutoff of a filter.
//! - `look-master.png`: the master's panel, its limiter pushed 18 dB, four seconds in.

use std::time::Duration;

use anyhow::Result;
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use serde_json::json;
use sound_core::Ticks;

use crate::song::{ACROSS, BAR, HEADER, LEAD, ROW, piece_files};

/// Where a song's rows start: under the title row, the ruler and its line.
const TOP: f32 = 48.0 + 32.0 + 1.0;

/// The middle of `row` at `tick` on the timeline.
fn at(tick: u64, row: f32) -> Point<Pixels> {
    point(
        px(HEADER + LEAD + tick as f32 * ACROSS),
        px(TOP + (row + 0.5) * ROW),
    )
}

/// `clicks` clicks at `at`, and what Bun draws after them.
fn click(opened: &super::Opened, at: Point<Pixels>, clicks: usize, cx: &mut HeadlessAppContext) -> Result<()> {
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    let moved = MouseMoveEvent { position: at, pressed_button: None, modifiers };
    opened.event(PlatformInput::MouseMove(moved), cx)?;
    for click_count in 1..=clicks {
        let down = MouseDownEvent { position: at, modifiers, button, click_count, first_mouse: false };
        opened.event(PlatformInput::MouseDown(down), cx)?;
        let up = MouseUpEvent { position: at, modifiers, button, click_count };
        opened.event(PlatformInput::MouseUp(up), cx)?;
    }
    settle(cx);
    Ok(())
}

/// Bun answers in real time, on a thread of its own.
fn settle(cx: &mut HeadlessAppContext) {
    for _ in 0..15 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn open(cx: &mut HeadlessAppContext, more: &[(&str, serde_json::Value)]) -> Result<super::Opened> {
    super::Opened::open(cx, &piece_files(more))
}

/// A track header's name, `row` from the top.
fn header(row: f32) -> Point<Pixels> {
    point(px(60.), px(TOP + row * ROW + 18.))
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &super::Save) -> Result<()> {
    let files: Vec<(String, String)> = piece_files(&[])
        .into_iter()
        .filter(|(path, _)| !path.starts_with("state/song/track-1") && !path.starts_with("state/song/bass") && !path.starts_with("state/song/melody"))
        .collect();
    let opened = super::Opened::open(cx, &files)?;
    save(cx, opened.window.into(), "look-empty")?;
    drop(opened);

    let mut opened = open(cx, &[])?;
    click(&opened, at(4 * BAR + 960, 1.), 1, cx)?;
    opened.play_from(Ticks(5 * BAR + 1000), cx)?;
    save(cx, opened.window.into(), "look-piece")?;
    drop(opened);

    let opened = open(cx, &[])?;
    click(&opened, at(4 * BAR + 960, 2.), 2, cx)?;
    // Its sixth note, 79 from beat 6: the editor opens fitted, bar 5 eight points into its roll,
    // with 79 four and a half rows from the top of it.
    let across = (1470. - HEADER - 1. - 104.) / (4 * BAR) as f32;
    click(&opened, point(px(HEADER + 1. + 8. + 5160. * across), px(920. - 352. + 33. + 60.)), 1, cx)?;
    save(cx, opened.window.into(), "look-editor")?;
    drop(opened);

    let reverb = json!({ "tool": "reverb", "state": { "decay_seconds": 2.4, "size": 0.6, "low_cut_hz": 200.0, "mix": 0.25 } });
    let chain = json!({ "tool": "chain", "state": { "order": ["reverb"] } });
    let opened = open(cx, &[("bass/chain/reverb.json", reverb), ("bass/chain/instance.json", chain)])?;
    click(&opened, header(1.), 1, cx)?;
    save(cx, opened.window.into(), "look-track-panel")?;
    drop(opened);

    let filter = json!({ "tool": "filter", "state": {} });
    let chain = json!({ "tool": "chain", "state": { "order": ["filter"] } });
    let bass = json!({ "tool": "track", "state": { "colour": "peach", "title": "Bass", "automation": [
        { "lane": "lane-volume", "target": "mix.gain_db" },
        { "lane": "lane-cutoff", "target": "chain/filter.cutoff_hz" },
    ] } });
    let volume = json!({ "tool": "lane", "state": { "points": [
        { "tick": 0, "value": -100 }, { "tick": 2 * BAR, "value": 0 }, { "tick": 10 * BAR, "value": 0 }, { "tick": 12 * BAR - 1, "value": -24 },
    ] } });
    let cutoff = json!({ "tool": "lane", "state": { "scale": "log", "points": [
        { "tick": 4 * BAR, "value": 200 }, { "tick": 8 * BAR, "value": 8000 }, { "tick": 9 * BAR, "value": 8000 }, { "tick": 11 * BAR, "value": 400 },
    ] } });
    let opened = open(cx, &[
        ("bass/instance.json", bass),
        ("bass/chain/instance.json", chain),
        ("bass/chain/filter.json", filter),
        ("bass/lane-volume.json", volume),
        ("bass/lane-cutoff.json", cutoff),
    ])?;
    click(&opened, point(px(32.), px(TOP + ROW + 41.)), 1, cx)?;
    save(cx, opened.window.into(), "look-lanes")?;
    drop(opened);

    let limiter = json!({ "tool": "limiter", "state": { "ceiling_db": 0, "lookahead_ms": 0, "gain_db": 18 } });
    let mut opened = open(cx, &[("limit.json", limiter)])?;
    opened.play_from(Ticks(0), cx)?;
    click(&opened, point(px(HEADER / 2.), px(super::WINDOW_HEIGHT - 20.)), 1, cx)?;
    for _ in 0..(3.7 * 60.) as usize {
        opened.advance(800, cx);
    }
    cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
    settle(cx);
    save(cx, opened.window.into(), "look-master")?;
    Ok(())
}
