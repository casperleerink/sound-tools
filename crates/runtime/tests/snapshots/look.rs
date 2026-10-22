//! The DAW template drawing what the Rust arrangement's snapshots drew, with the same content,
//! so the two can be compared side by side: `look-*.png`.
//!
//! - `look-empty.png`: a song with no tracks.
//! - `look-piece.png`: the piece playing at 6.3, the second clip of the bass chosen.
//! - `look-sections.png`: the piece with an intro and a verse named on the ruler.
//! - `look-editor.png`: the note editor on the first clip of the melody.
//! - `look-track-panel.png`: the panel of the bass, with a reverb after its synth.
//! - `look-lanes.png`: the lanes of the bass: its volume and the cutoff of a filter.
//! - `look-point.png`: a point added to the cutoff's lane at bar 7, chosen, with its readout.
//! - `look-master.png`: the master's panel, its limiter pushed 18 dB, four seconds in.
//! - `look-sidechain.png`: the panel of the bass with a filter and a compressor keyed by Track 1,
//!   expanded with its sidechain picker.
//! - `look-returns.png`, `look-sends.png`: the returns Space, a reverb, and Echo, open over the
//!   master with the panel of Space; folded, with the panel of the bass and its sends.
//! - `look-picker.png`, `look-effect-picker.png`: the panel of an empty track whose instrument is
//!   the test plugin, the menu of its card's title open, and the menu of Add effect.
//! - `look-audio.png`: the piece with a voice and a guitar as audio, playing at 6.3, the second
//!   region of the voice chosen, and the panel of the voice: its region's card, a compressor and
//!   a reverb.
//! - `look-armed.png`: the voice armed, its input level in its header and on the meter of its
//!   panel, stopped.
//! - `look-recording.png`: the voice and the guitar armed and recording from bar 5 over their
//!   regions, the takes growing to the playhead at 7.2 with their sound.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use serde_json::{Value, json};
use sound_core::Ticks;

use crate::song::{ACROSS, ARM, BAR, HEADER, LEAD, ROW, TOGGLES, daw};
use runtime::window::audio_input::{OpenInput, OpenedInput};
use sound_core::CaptureWriter;

#[path = "look_takes.rs"]
pub(crate) mod takes;
#[path = "look_timeline.rs"]
pub(crate) mod timeline;

/// Where a song's rows start: under the title row and the ruler, whose line is its last point.
const TOP: f32 = 48.0 + 32.0;

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
pub(crate) fn click(
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

/// The pointer off what a snapshot shows, as the arrangement's snapshots opened their menus
/// without one.
fn away(opened: &super::Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    let moved = MouseMoveEvent {
        position: point(px(1400.), px(80.)),
        pressed_button: None,
        modifiers: Modifiers::default(),
    };
    opened.event(PlatformInput::MouseMove(moved), cx)?;
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

/// The arm toggle of the audio track `row` from the top.
fn arm(row: f32) -> Point<Pixels> {
    point(px(ARM), px(TOP + row * ROW + TOGGLES))
}

/// The record button of the transport in the title bar.
const RECORD: (f32, f32) = (555., 24.);

/// What the input brings, as the arrangement's snapshots had it: the voice on the left and the
/// guitar on the right, at half.
#[derive(Clone, Default)]
struct SimulatedInput {
    writer: Arc<Mutex<Option<CaptureWriter>>>,
    written: Arc<AtomicU64>,
}

impl SimulatedInput {
    fn opener(&self) -> OpenInput {
        let input = self.clone();
        Arc::new(move || {
            let (writer, reader) = sound_core::capture(48_000, 2);
            *input.writer.lock().expect("the writer") = Some(writer);
            input.written.store(0, Ordering::Relaxed);
            Ok(OpenedInput {
                stream: None,
                reader,
                live: sound_core::live_input(48_000, 2).1,
                notice: None,
            })
        })
    }

    /// Writes the input up to engine frame `until`, captured as that frame sounds.
    fn write_until(&self, until: u64) {
        let mut writer = self.writer.lock().expect("the writer");
        let Some(writer) = writer.as_mut() else {
            return;
        };
        let first = self.written.load(Ordering::Relaxed);
        if until <= first {
            return;
        }
        let samples: Vec<f32> = (first..until)
            .flat_map(|frame| {
                let time = frame as f64 / 48_000.;
                [(voice(time) * 0.5) as f32, (strum(time) * 0.5) as f32]
            })
            .collect();
        writer.write(&samples, first * 1_000_000_000 / 48_000, 0);
        self.written.store(until, Ordering::Relaxed);
    }
}

/// `seconds` of polls of the window, the input arriving as the engine plays.
fn run_input(
    opened: &mut super::Opened,
    input: &SimulatedInput,
    seconds: f32,
    cx: &mut HeadlessAppContext,
) -> Result<()> {
    let poll = (48_000. * sound_ui::POLL_INTERVAL.as_secs_f32()) as usize;
    let step = |opened: &mut super::Opened, cx: &mut HeadlessAppContext| {
        opened.advance(poll, cx);
        input.write_until(opened.engine.frames());
        opened.poll_recording(cx)
    };
    let polls = (seconds * 1000. / 16.) as usize;
    for _ in 0..polls.saturating_sub(4) {
        step(opened, cx)?;
    }
    // The recorder writes on a thread of its own: its takes catch up, then the last polls show
    // the level of the input as it arrives.
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(10));
        opened.poll_recording(cx)?;
    }
    for _ in 0..polls.min(4) {
        step(opened, cx)?;
    }
    cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
    cx.run_until_parked();
    Ok(())
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

    let mut files = piece_files(&[]);
    if let Some((_, project)) = files.iter_mut().find(|(path, _)| path == "project.json") {
        let mut file: Value = serde_json::from_str(project)?;
        file["sections"] = json!([{ "bar": 1, "name": "Intro" }, { "bar": 5, "name": "Verse" }]);
        *project = file.to_string();
    }
    let mut opened = super::Opened::open(cx, &files)?;
    // The poll tells Bun the sections.
    opened.advance(512, cx);
    settle(cx);
    save(cx, opened.window.into(), "look-sections")?;
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
    // A click on the cutoff's lane adds a point there, chosen, with its readout.
    click(&opened, at(6 * BAR, 3.), 1, cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-point")?;
    // The row under the lanes of the bass adds one: its numbers by device.
    click(&opened, point(px(60.), px(TOP + 4. * ROW + 20.)), 1, cx)?;
    save(cx, opened.window.into(), "look-add-lane")?;
    drop(opened);

    let bus = |title: &str, colour: &str| json!({ "tool": "bus", "state": { "title": title, "colour": colour } });
    let send = |gain_db: f32| json!({ "tool": "fader", "state": { "gain_db": gain_db } });
    let fader = json!({ "tool": "fader", "state": {} });
    let reverb =
        json!({ "tool": "reverb", "state": { "decay_seconds": 2.4, "size": 0.6, "mix": 1.0 } });
    let opened = open(
        cx,
        &[
            ("ret-a/instance.json", bus("Space", "teal")),
            ("ret-a/mix.json", fader.clone()),
            (
                "ret-a/chain/instance.json",
                json!({ "tool": "chain", "state": { "order": ["reverb"] } }),
            ),
            ("ret-a/chain/reverb.json", reverb),
            ("ret-b/instance.json", bus("Echo", "mauve")),
            ("ret-b/mix.json", fader),
            (
                "ret-b/chain/instance.json",
                json!({ "tool": "chain", "state": {} }),
            ),
            ("bass/send-ret-a.json", send(-12.)),
            ("bass/send-ret-b.json", send(-18.)),
        ],
    )?;
    let bottom = super::WINDOW_HEIGHT - 40.;
    click(&opened, header(1.), 1, cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-sends")?;
    // The returns' row over the master row, open, and Space's row in it.
    let panel = 216. + 1.;
    click(&opened, point(px(60.), px(bottom - panel - 16.)), 1, cx)?;
    click(&opened, point(px(60.), px(bottom - panel - 60.)), 1, cx)?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-returns")?;
    drop(opened);

    // Track 1 alone, its instrument the test plugin.
    let plugin: serde_json::Value = serde_json::from_str(&super::test_plugin(
        plugin_host::PluginFormat::Clap,
        "test-tone",
    ))?;
    let files: Vec<(String, String)> = piece_files(&[("track-1/synth.json", plugin.clone())])
        .into_iter()
        // Track 1 alone and empty, as the arrangement's picker snapshots had it.
        .filter(|(path, _)| {
            !path.starts_with("state/song/bass")
                && !path.starts_with("state/song/melody")
                && !path.starts_with("state/song/track-1/clip-")
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
    drop(opened);

    // The same track with the test plugin twice after it, Warmth and Space, Space turned off.
    let chain =
        json!({ "tool": "chain", "state": { "order": ["warmth", "space"], "off": ["space"] } });
    let effects = [
        ("track-1/chain/instance.json", chain),
        ("track-1/chain/warmth.json", plugin.clone()),
        ("track-1/chain/space.json", plugin),
    ];
    let files: Vec<(String, String)> = files
        .into_iter()
        .filter(|(path, _)| !path.starts_with("state/song/track-1/chain/"))
        .chain(
            effects
                .into_iter()
                .map(|(path, record)| (format!("state/song/{path}"), record.to_string())),
        )
        .collect();
    let opened = super::Opened::open(cx, &files)?;
    click(&opened, header(0.), 1, cx)?;
    // Add effect after three cards of a plugin, 200 points each and 12 apart.
    click(
        &opened,
        point(px(ADD_EFFECT.0 + 2. * 212.), px(ADD_EFFECT.1)),
        1,
        cx,
    )?;
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-effect-picker")?;
    drop(opened);

    let mut opened = super::Opened::open_with(cx, &audio_files(), write_audio)?;
    click(&opened, at(4 * BAR + 3 * 960, 3.), 1, cx)?;
    // Half a second earlier than the arrangement's 6.3, which `play_from` plays.
    opened.play_from(Ticks(5 * BAR + 1510), cx)?;
    wait_for_waveforms(&mut opened, cx)?;
    save(cx, opened.window.into(), "look-audio")?;
    drop(opened);

    // The voice armed, stopped, with its panel open: the level of its input.
    let input = SimulatedInput::default();
    let mut opened =
        super::Opened::open_with_input(cx, &audio_files(), write_audio, Some(input.opener()))?;
    wait_for_waveforms(&mut opened, cx)?;
    click(&opened, arm(3.), 1, cx)?;
    click(&opened, header(3.), 1, cx)?;
    run_input(&mut opened, &input, 0.5, cx)?;
    save(cx, opened.window.into(), "look-armed")?;
    drop(opened);

    // Both armed, recording from bar 5 to 7.2 over their regions.
    let input = SimulatedInput::default();
    let mut opened =
        super::Opened::open_with_input(cx, &audio_files(), write_audio, Some(input.opener()))?;
    wait_for_waveforms(&mut opened, cx)?;
    click(&opened, arm(3.), 1, cx)?;
    click(&opened, arm(4.), 1, cx)?;
    click(&opened, header(3.), 1, cx)?;
    cx.update(|cx| (opened.session).update(cx, |session, _| session.engine().seek(Ticks(4 * BAR))));
    opened.advance(64, cx);
    settle(cx);
    click(&opened, point(px(RECORD.0), px(RECORD.1)), 1, cx)?;
    run_input(&mut opened, &input, 4.6, cx)?;
    save(cx, opened.window.into(), "look-recording")?;
    drop(opened);

    // The bass's filter and then a compressor under its peaks, keyed by Track 1 after its
    // effects and expanded, as the arrangement's sidechain snapshot had them.
    let filter = json!({ "tool": "filter", "state": { "cutoff_hz": 1200.0, "resonance": 0.3 } });
    let compressor = json!({ "tool": "compressor", "state": { "threshold_db": -30.0, "ratio": 4.0, "attack_ms": 10.0, "release_ms": 120.0 } });
    let chain = json!({ "tool": "chain", "state": { "order": ["filter", "comp"] } });
    let song = json!({ "tool": "song", "state": { "order": ["track-1", "bass", "melody"], "sidechains": [
        { "effect": "bass/chain/comp", "from": "track-1", "tap": "post_fx" },
    ] } });
    let mut opened = open(
        cx,
        &[
            ("bass/chain/instance.json", chain),
            ("bass/chain/filter.json", filter),
            ("bass/chain/comp.json", compressor),
            ("instance.json", song),
        ],
    )?;
    click(&opened, header(1.), 1, cx)?;
    let card = opened.card::<compressor::view::CompressorView>("song/bass/chain/comp", cx)?;
    // Half a second of the piece, and one look at what the compressor heard and did.
    opened.play_from(Ticks(0), cx)?;
    for _ in 0..25 {
        opened.advance(960, cx);
    }
    cx.update(|cx| {
        card.update(cx, |card, cx| {
            card.read_meters(cx);
            card.set_expanded(true, cx);
        })
    });
    away(&opened, cx)?;
    save(cx, opened.window.into(), "look-sidechain")?;
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

/// Lets the threads make every waveform the window asked for, and draws again.
fn wait_for_waveforms(opened: &mut super::Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    for _ in 0..300 {
        settle(cx);
        let making = cx.update(|cx| sound_ui::Waveforms::entity(cx).read(cx).making());
        if making == 0 {
            opened.advance(800, cx);
            cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
            settle(cx);
            return Ok(());
        }
    }
    anyhow::bail!("the waveforms were not made")
}

/// A 16-bit mono WAV file at 48 kHz, `seconds` long, of what `sound` gives at each time.
fn write_wav(path: &std::path::Path, seconds: f64, sound: impl Fn(f64) -> f64) -> Result<()> {
    std::fs::create_dir_all(path.parent().expect("a folder"))?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for frame in 0..(seconds * 48_000.) as u64 {
        let time = frame as f64 / 48_000.;
        writer.write_sample((sound(time).clamp(-1., 1.) * 32_000.) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

/// A noise that is the same every time.
fn noise(time: f64) -> f64 {
    let x = (time * 48_000.).floor();
    ((x * 12.9898).sin() * 43_758.545).fract() * 2. - 1.
}

/// Something like a voice: syllables in phrases, with gaps between the phrases.
fn voice(time: f64) -> f64 {
    let open = if (time * 0.5).fract() < 0.72 { 1. } else { 0. };
    let syllable = (time * std::f64::consts::PI * 4.3).sin().abs().powf(1.5);
    let swell = 0.55 + 0.45 * (time * 1.3).sin().abs();
    let tone = (time * 2. * std::f64::consts::PI * 180.).sin() * 0.6 + noise(time) * 0.4;
    tone * syllable * swell * open * 0.9
}

/// Something like a guitar: a strum on every eighth at 120 bpm, each dying away.
fn strum(time: f64) -> f64 {
    let since = time % 0.25;
    let accent = if ((time / 0.25) as u64).is_multiple_of(2) {
        1.
    } else {
        0.7
    };
    let tone = (time * 2. * std::f64::consts::PI * 220.).sin() * 0.5 + noise(time) * 0.5;
    tone * (-since * 14.).exp() * accent * 0.8
}

/// The files of the voice and the guitar, as the Rust arrangement's audio snapshots wrote them.
fn write_audio(root: &std::path::Path) -> Result<()> {
    let audio = root.join("assets/audio");
    write_wav(&audio.join("voice-take-1.wav"), 4.6, voice)?;
    write_wav(&audio.join("voice-take-3.wav"), 14.6, |time| {
        voice(time + 3.)
    })?;
    write_wav(&audio.join("voice-take-4.wav"), 6.5, |time| {
        voice(time + 7.)
    })?;
    write_wav(&audio.join("guitar.wav"), 24., strum)
}

/// The piece, and a voice and a guitar as audio tracks: three takes of the voice, the second
/// trimmed and faded, through a compressor and a reverb, and the guitar in two regions of one file.
fn audio_files() -> Vec<(String, String)> {
    let region = |asset: &str,
                  bar: f64,
                  from: f64,
                  to: Option<f64>,
                  gain_db: f64,
                  fades: (f64, f64)| {
        json!({ "tool": "region", "state": {
            "asset": asset, "start": (bar * BAR as f64) as u64, "file_start_seconds": from,
            "file_end_seconds": to, "gain_db": gain_db, "fade_in_ms": fades.0, "fade_out_ms": fades.1,
        } })
    };
    let tape = |title: &str, colour: &str, order: &[&str], channel: u32| {
        [
            (
                "instance.json",
                json!({ "tool": "tape", "state": { "colour": colour, "title": title } }),
            ),
            (
                "rec.json",
                json!({ "tool": "recorder", "state": { "channels": [channel] } }),
            ),
            (
                "chain/instance.json",
                json!({ "tool": "chain", "state": { "order": order } }),
            ),
            ("mix.json", json!({ "tool": "fader", "state": {} })),
        ]
    };
    let mut more: Vec<(String, Value)> = Vec::new();
    for (file, record) in tape("Voice", "mauve", &["compressor", "reverb"], 0) {
        more.push((format!("audio-voice/{file}"), record));
    }
    // The guitar on the right channel of the input, as the arrangement's recorded it.
    for (file, record) in tape("Guitar", "teal", &[], 1) {
        more.push((format!("audio-guitar/{file}"), record));
    }
    more.extend([
        ("audio-voice/chain/compressor.json".into(), json!({ "tool": "compressor", "state": { "threshold_db": -18.0, "ratio": 4.0 } })),
        ("audio-voice/chain/reverb.json".into(), json!({ "tool": "reverb", "state": { "decay_seconds": 2.4, "size": 0.6, "low_cut_hz": 200.0, "mix": 0.25 } })),
        ("audio-voice/take-0.json".into(), region("voice-take-1.wav", 1.5, 0., None, 0., (0., 0.))),
        ("audio-voice/take-1.json".into(), region("voice-take-3.wav", 4., 2.1, Some(10.1), -0.9, (150., 260.))),
        ("audio-voice/take-2.json".into(), region("voice-take-4.wav", 8.5, 0., None, 0., (0., 0.))),
        ("audio-guitar/take-0.json".into(), region("guitar.wav", 0., 0., Some(16.), 0., (0., 0.))),
        ("audio-guitar/take-1.json".into(), region("guitar.wav", 8., 16., None, 0., (0., 0.))),
        ("instance.json".into(), json!({ "tool": "song", "state": { "order": ["track-1", "bass", "melody", "audio-voice", "audio-guitar"] } })),
    ]);
    let more: Vec<(&str, Value)> = more
        .iter()
        .map(|(path, record)| (path.as_str(), record.clone()))
        .collect();
    piece_files(&more)
}
