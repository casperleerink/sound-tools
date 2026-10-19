//! The DAW template drawing what the Rust arrangement's snapshots drew, with the same content,
//! so the two can be compared side by side: `look-*.png`.
//!
//! - `look-empty.png`: a song with no tracks.
//! - `look-piece.png`: the piece playing at 6.3, the second clip of the bass chosen.
//! - `look-editor.png`: the note editor on the first clip of the melody.
//! - `look-track-panel.png`: the panel of the bass, with a reverb after its synth.
//! - `look-lanes.png`: the lanes of the bass: its volume and the cutoff of a filter.
//! - `look-master.png`: the master's panel, its limiter pushed 18 dB, four seconds in.
//! - `look-picker.png`, `look-effect-picker.png`: the panel of a track whose instrument is the
//!   test plugin, the menu of its card's title open, and the menu of Add effect.

use std::time::Duration;

use anyhow::Result;
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use serde_json::{Value, json};
use sound_core::Ticks;

use crate::song::{ACROSS, BAR, HEADER, LEAD, ROW, daw};

/// Where a song's rows start: under the title row, the ruler and its line.
const TOP: f32 = 48.0 + 32.0 + 1.0;

/// The title of the first card of a track's panel, and its Add effect after a plugin's card.
const PICKER: (f32, f32) = (230., 920. - 216. + 12. + 22.);
const ADD_EFFECT: (f32, f32) = (441., 920. - 216. + 12. + 16.);

/// The middle of `row` at `tick` on the timeline.
fn at(tick: u64, row: f32) -> Point<Pixels> {
    point(
        px(HEADER + LEAD + tick as f32 * ACROSS),
        px(TOP + (row + 0.5) * ROW),
    )
}

/// `clicks` clicks at `at`, and what Bun draws after them.
fn click(
    opened: &super::Opened,
    at: Point<Pixels>,
    clicks: usize,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    let moved = MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers,
    };
    opened.event(PlatformInput::MouseMove(moved), cx)?;
    for click_count in 1..=clicks {
        let down = MouseDownEvent {
            position: at,
            modifiers,
            button,
            click_count,
            first_mouse: false,
        };
        opened.event(PlatformInput::MouseDown(down), cx)?;
        let up = MouseUpEvent {
            position: at,
            modifiers,
            button,
            click_count,
        };
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
        .filter(|(path, _)| {
            !path.starts_with("state/song/track-1")
                && !path.starts_with("state/song/bass")
                && !path.starts_with("state/song/melody")
        })
        .collect();
    let opened = super::Opened::open(cx, &files)?;
    save(cx, opened.window.into(), "look-empty")?;
    drop(opened);

    let mut opened = open(cx, &[])?;
    click(&opened, at(4 * BAR + 960, 1.), 1, cx)?;
    opened.play_from(Ticks(5 * BAR + 1000), cx)?;
    settle(cx);
    save(cx, opened.window.into(), "look-piece")?;
    drop(opened);

    let opened = open(cx, &[])?;
    click(&opened, at(4 * BAR + 960, 2.), 2, cx)?;
    // Its sixth note, 79 from beat 6: the editor opens fitted, bar 5 eight points into its roll,
    // with 79 four and a half rows from the top of it.
    let across = (1470. - HEADER - 1. - 104.) / (4 * BAR) as f32;
    click(
        &opened,
        point(
            px(HEADER + 1. + 8. + 5160. * across),
            px(920. - 352. + 33. + 60.),
        ),
        1,
        cx,
    )?;
    save(cx, opened.window.into(), "look-editor")?;
    drop(opened);

    let reverb = json!({ "tool": "reverb", "state": { "decay_seconds": 2.4, "size": 0.6, "low_cut_hz": 200.0, "mix": 0.25 } });
    let chain = json!({ "tool": "chain", "state": { "order": ["reverb"] } });
    let opened = open(
        cx,
        &[
            ("bass/chain/reverb.json", reverb),
            ("bass/chain/instance.json", chain),
        ],
    )?;
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
    let opened = open(
        cx,
        &[
            ("bass/instance.json", bass),
            ("bass/chain/instance.json", chain),
            ("bass/chain/filter.json", filter),
            ("bass/lane-volume.json", volume),
            ("bass/lane-cutoff.json", cutoff),
        ],
    )?;
    click(&opened, point(px(32.), px(TOP + ROW + 41.)), 1, cx)?;
    save(cx, opened.window.into(), "look-lanes")?;
    drop(opened);

    // Track 1 alone, its instrument the test plugin.
    let plugin: serde_json::Value = serde_json::from_str(&super::test_plugin(
        plugin_host::PluginFormat::Clap,
        "test-tone",
    ))?;
    let files: Vec<(String, String)> = piece_files(&[("track-1/synth.json", plugin)])
        .into_iter()
        .filter(|(path, _)| {
            !path.starts_with("state/song/bass") && !path.starts_with("state/song/melody")
        })
        .map(|(path, text)| match path.as_str() {
            "state/song/instance.json" => (
                path,
                json!({ "tool": "song", "state": { "order": ["track-1"] } }).to_string(),
            ),
            _ => (path, text),
        })
        .collect();
    let opened = super::Opened::open(cx, &files)?;
    click(&opened, header(0.), 1, cx)?;
    click(&opened, point(px(PICKER.0), px(PICKER.1)), 1, cx)?;
    save(cx, opened.window.into(), "look-picker")?;
    opened.key("escape", cx)?;
    click(&opened, point(px(ADD_EFFECT.0), px(ADD_EFFECT.1)), 1, cx)?;
    save(cx, opened.window.into(), "look-effect-picker")?;
    drop(opened);

    let limiter = json!({ "tool": "limiter", "state": { "ceiling_db": 0, "lookahead_ms": 0, "gain_db": 18 } });
    let mut opened = open(cx, &[("limit.json", limiter)])?;
    opened.play_from(Ticks(0), cx)?;
    click(
        &opened,
        point(px(HEADER / 2.), px(super::WINDOW_HEIGHT - 20.)),
        1,
        cx,
    )?;
    // The card reads the limiter's meters on a timer, which a headless window runs not.
    let limiter = opened.card::<limiter::view::LimiterView>("song/limit", cx)?;
    for _ in 0..(3.7 * 60.) as usize {
        opened.advance(800, cx);
        cx.update(|cx| limiter.update(cx, |card, cx| card.read_meters(cx)));
    }
    cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
    settle(cx);
    save(cx, opened.window.into(), "look-master")?;
    Ok(())
}

/// The piece the Rust arrangement's snapshots drew, as the DAW template's tools: chords on
/// `track-1`, a bass line on `bass` and a melody with a gap on `melody`, over twelve bars, with
/// nothing in their chains, in a project of every bundled extension, as a new one is. `more`
/// adds files after the tracks.
fn piece_files(more: &[(&str, Value)]) -> Vec<(String, String)> {
    let note = |start: u64, length: u64, pitch: u64| json!({ "start": start, "length": length, "pitch": pitch, "velocity": 100 });
    let mut chords = Vec::new();
    for (bar, root) in [(0, 57), (1, 53), (2, 48), (3, 55)] {
        for interval in [0, 4, 7, 12] {
            chords.push(note(bar * BAR, BAR - 120, root + interval));
        }
    }
    let mut bass = Vec::new();
    for (bar, root) in [(0, 33), (1, 29), (2, 36), (3, 31)] {
        for eighth in 0..8 {
            let pitch = if eighth % 4 == 3 { root + 12 } else { root };
            bass.push(note(bar * BAR + eighth * 480, 360, pitch));
        }
    }
    let line = [
        76, 74, 72, 74, 76, 79, 76, 72, 69, 72, 74, 72, 71, 67, 69, 71,
    ];
    let melody: Vec<Value> = (line.iter().enumerate())
        .map(|(index, pitch)| note(index as u64 * 960, 720, *pitch))
        .collect();
    let clip = |bar: u64, bars: u64, notes: &[Value]| json!({ "tool": "timeline", "state": { "start": bar * BAR, "length": bars * BAR, "notes": notes } });
    let tracks = [
        (
            "track-1",
            "Track 1",
            "blue",
            vec![
                clip(0, 4, &chords),
                clip(4, 4, &chords),
                clip(8, 4, &chords),
            ],
        ),
        (
            "bass",
            "Bass",
            "peach",
            vec![clip(0, 4, &bass), clip(4, 4, &bass), clip(8, 4, &bass)],
        ),
        (
            "melody",
            "A melody with a name too long for its header",
            "mauve",
            vec![clip(4, 4, &melody), clip(10, 2, &melody[..8])],
        ),
    ];
    let project = json!({
        "format": 1,
        "extensions": runtime::bundled_extensions().unwrap_or_default(),
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [{ "from": { "instance": "song", "port": "audio" }, "to": { "device_output": 0 } }],
    });
    let names: Vec<&str> = tracks.iter().map(|(name, ..)| *name).collect();
    let mut files = vec![
        ("project.json".to_string(), project.to_string()),
        (
            "state/song/instance.json".to_string(),
            json!({ "tool": "song", "state": { "order": names } }).to_string(),
        ),
    ];
    // The template's tools and the song's own records: its master, limiter and the lane a solo
    // turns tracks down with.
    files.extend(
        (daw().files.iter())
            .filter(|(path, _)| {
                path.starts_with("extensions/")
                    || path.starts_with("state/song/")
                        && !path.starts_with("state/song/t00")
                        && !path.ends_with("song/instance.json")
            })
            .map(|(path, contents)| (path.to_string(), contents.to_string())),
    );
    for (name, title, colour, clips) in tracks {
        let folder = format!("state/song/{name}");
        let record = |tool: &str| json!({ "tool": tool, "state": {} }).to_string();
        files.push((
            format!("{folder}/instance.json"),
            json!({ "tool": "track", "state": { "colour": colour, "title": title } }).to_string(),
        ));
        files.push((format!("{folder}/synth.json"), record("instrument.synth")));
        files.push((format!("{folder}/chain/instance.json"), record("chain")));
        files.push((format!("{folder}/mix.json"), record("fader")));
        files.push((format!("{folder}/rec.json"), record("recorder")));
        for (index, clip) in clips.into_iter().enumerate() {
            files.push((format!("{folder}/clip-{index:03}.json"), clip.to_string()));
        }
    }
    for (path, record) in more {
        let path = format!("state/song/{path}");
        files.retain(|(each, _)| *each != path);
        files.push((path, record.to_string()));
    }
    files
}
