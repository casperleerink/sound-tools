//! Pages of tools of the project, drawn by Bun:
//!
//! - `typescript-mixer.png`: the mixer of `tooling/agent-eval/examples` as five tones play:
//!   meters, faders, a muted strip, the master's scope and the device output.
//! - `typescript-song.png`: the song of `tooling/agent-eval/examples`, 12 tracks of 150 clips,
//!   as it plays: the SDK's transport on top, a clip selected, the cards of its track's devices
//!   under the timeline, another clip open in the note editor, the meters of the tracks.
//! - `typescript-page.png`: the knob, steps, meter and pad of the SDK, made of shapes, beside
//!   a small piano roll whose notes and labels are shapes of a space.
//! - `typescript-piano-roll.png`: the piano roll of `tooling/agent-eval/examples`, with the
//!   notes of bar 2 selected by a marquee and held by the pointer two beats on and two rows up,
//!   as the window moves them before their code hears the drag.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui::{
    AppContext, HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, point, px, size,
};
use runtime::window::{DeviceAccess, Shell};
use runtime::{OFFLINE, views};
use sound_core::{Engine, Ticks};
use sound_ui::Session;

use super::{WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::plugin_hosts::test_plugin_host;

const DEMO: &str = r##"import { h, number, Knob, Label, Line, Meter, Pad, pattern, Rect, Space, Steps, tool } from "./sdk";

const notes = [[0, 60], [480, 64], [960, 67], [1440, 72], [1920, 67], [2400, 64], [2880, 62], [3360, 60]];

tool({
  name: "demo",
  title: "Demo",
  when: "A snapshot",
  doc: "Every element of the SDK.",
  state: {
    cutoff: number({ min: 100, max: 8000, default: 1200, unit: "hz" }),
    level: number({ min: -30, max: 6, default: -6, unit: "db" }),
    steps: pattern({ length: 8, min: 0, max: 1, default: 0 }),
  },
  inputs: { bend: number({ min: 0, max: 1, default: 0.3 }), drive: number({ min: 0, max: 1, default: 0.7 }) },
  sound: () => 0,
  view: ({ now }) =>
    h("div", { style: { direction: "row", gap: 24 } },
      h("div", { style: { gap: 16 } },
        h("div", { style: { direction: "row", gap: 8 } },
          h(Knob, { value: "cutoff", label: "Cutoff", min: 100, max: 8000, default: 1200, unit: "hz" }),
          h(Knob, { value: "level", label: "Level", min: -30, max: 6, default: -6, unit: "db" }),
          h(Meter, { value: "level", label: "Out" }),
        ),
        h(Steps, { path: "steps" }),
        h(Pad, { x: "bend", y: "drive", size: 120 }),
      ),
      h(Space, { key: "roll", width: 640, height: 320, background: "#121317", radius: 6, unit: { x: 0.15, y: 12 }, extent: { x: [0, 3840], y: [0, 36] } },
        Array.from({ length: 4 }, (_, beat) => h(Line, { from: [beat * 960, 0], to: [beat * 960, 36], color: "#292c34", width: 1 })),
        notes.map(([start, pitch], index) => h(Rect, {
          x: start, y: 84 - pitch, width: 440, height: 0.85, radius: 3,
          color: index === 2 ? "#e9ebef" : "#a9b1ff", hover: "#e9ebef", cursor: "grab",
        })),
        notes.map(([start, pitch], index) => h(Label, { x: start + 30, y: 84 - pitch + 0.05, text: String(pitch), size: 9, color: "#121317" })),
        h(Line, { from: [now.tick.plus(1200), 0], to: [now.tick.plus(1200), 36], color: "#7ee0a0", width: 2 }),
      ),
    ),
});
"##;

const PIANO_ROLL: &str = include_str!("../../../../tooling/agent-eval/examples/piano-roll.tsx");
const MIXER: &str = include_str!("../../../../tooling/agent-eval/examples/mixer.tsx");
const METER: &str = r#"{"tool": "meter", "state": {}}"#;
const SWAY: &str = include_str!("../../../../tooling/agent-eval/examples/sway.ts");

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &super::Opened, &str) -> Result<()>,
) -> Result<()> {
    if !sound_typescript::has_bun() {
        println!("Bun is not installed: no snapshot of a page of a tool of the project");
        return Ok(());
    }
    let demo = r#"{"tool": "demo", "state": {"steps": [1, 0, 0, 1, 0, 1, 0, 0], "cutoff": 2400}}"#;
    let opened = open_page(cx, "demo", demo, "extensions/demo.ts", DEMO)?;
    save(cx, &opened, "typescript-page")?;
    drop(opened);

    let record = serde_json::json!({ "tool": "piano-roll", "state": { "notes": melody() } });
    let opened = open_page(
        cx,
        "piano-roll",
        &record.to_string(),
        "extensions/piano-roll.tsx",
        PIANO_ROLL,
    )?;
    // The roll's top left: the page's padding, the title bar and the ruler; the keys.
    let roll = point(px(24. + 44.), px(48. + 24. + 22. + 1.));
    let at = |tick: f32, row: f32| roll + point(px(tick * 0.1), px((row - 40.) * 12.));
    let mouse = |opened: &super::Opened, cx: &mut HeadlessAppContext, event| {
        opened.mouse(event, cx).map(|_| wait_for_bun(cx))
    };
    let moved = |position: Point<Pixels>, pressed_button| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button,
            modifiers: Modifiers::default(),
        })
    };
    let down = |position| {
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        })
    };
    // A marquee over bar 2, between rows 44 and 56.
    let (from, to) = (at(3800., 44.), at(7600., 56.));
    mouse(&opened, cx, moved(from, None))?;
    mouse(&opened, cx, down(from))?;
    mouse(&opened, cx, moved(to, Some(MouseButton::Left)))?;
    mouse(
        &opened,
        cx,
        PlatformInput::MouseUp(MouseUpEvent {
            position: to,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        }),
    )?;
    // The first note of bar 2, on 76: row 51.
    let note = at(3840. + 100., 51.5);
    mouse(&opened, cx, moved(note, None))?;
    mouse(&opened, cx, down(note))?;
    let held = note + point(px(192.), px(-24.));
    mouse(&opened, cx, moved(held, Some(MouseButton::Left)))?;
    save(cx, &opened, "typescript-piano-roll")?;
    drop(opened);

    let mut opened = open_mixer(cx)?;
    // Half a second of sound in frames of the window, as when it plays, so the meters, their
    // peak lines and the level's history move as they do.
    play(&mut opened, cx)?;
    save(cx, &opened, "typescript-mixer")?;
    drop(opened);

    let mut files = crate::song::song_files(12, 150);
    // The fourth track, whose devices show, has a reverb in its chain and a utility turned off.
    files.retain(|(path, _)| path != "state/song/t03/chain/instance.json");
    for (path, record) in [
        ("chain/instance.json", r#"{"tool": "chain", "state": {"order": ["room", "trim"], "off": ["trim"]}}"#),
        ("chain/room.json", r#"{"tool": "reverb", "state": {}}"#),
        ("chain/trim.json", r#"{"tool": "utility", "state": {}}"#),
    ] {
        files.push((format!("state/song/t03/{path}"), record.to_string()));
    }
    // A take of the voice, a region on its row from the second bar.
    files.push((
        "state/song/voice/take.json".to_string(),
        r#"{"tool": "region", "state": {"asset": "voice.wav", "start": 3840}}"#.to_string(),
    ));
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut opened = open_files_with(cx, &files, write_voice)?;
    // The timeline's top left: the page's padding, the title bar and the ruler; the headers.
    let timeline = point(px(24. + crate::song::HEADER), px(48. + 24. + 22.));
    let lane = |tick: u64, row: f32| {
        timeline
            + point(
                px(tick as f32 * crate::song::ACROSS),
                px(row * crate::song::ROW),
            )
    };
    let click = |opened: &super::Opened, cx: &mut HeadlessAppContext, at, clicks| {
        for click_count in 1..=clicks {
            let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
            mouse(opened, cx, moved(at, None))?;
            let down = MouseDownEvent {
                position: at,
                modifiers,
                button,
                click_count,
                first_mouse: false,
            };
            mouse(opened, cx, PlatformInput::MouseDown(down))?;
            let up = MouseUpEvent {
                position: at,
                modifiers,
                button,
                click_count,
            };
            mouse(opened, cx, PlatformInput::MouseUp(up))?;
        }
        anyhow::Ok(())
    };
    // A clip of the second track opens below; the second of the fourth is selected, which shows
    // the devices of its track. The transport of the page is above the rows.
    click(&opened, cx, lane(crate::song::BAR / 2, 2.5), 2)?;
    click(&opened, cx, lane(3 * crate::song::BAR + 960, 4.5), 1)?;
    play(&mut opened, cx)?;
    save(cx, &opened, "typescript-song")
}

/// Plays half a second in frames of the window, so meters and what eases move as they do.
fn play(opened: &mut super::Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    opened.play_from(Ticks(0), cx)?;
    for _ in 0..30 {
        opened.advance(800, cx);
        cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
        cx.run_until_parked();
    }
    Ok(())
}

/// The mixer of `tooling/agent-eval/examples` with five strips of tones, a quieter one muted and
/// one turned down, and the pan of the first swinging.
fn open_mixer(cx: &mut HeadlessAppContext) -> Result<super::Opened> {
    let project = r#"{"format": 1, "extensions": ["tone", "utility"],
        "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
        "connections": [{"from": {"instance": "mixer", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    let strips = [
        ("bass", 55.0, 0.5, "{}"),
        ("keys", 220.0, 0.2, r#"{"gain_db": -9}"#),
        ("lead", 440.0, 0.15, r#"{"pan": 0.4}"#),
        ("pad", 330.0, 0.08, r#"{"gain_db": 4.5}"#),
        ("perc", 880.0, 0.1, r#"{"mute": true}"#),
    ];
    let mut files: Vec<(String, String)> = vec![
        ("project.json".into(), project.into()),
        (
            "state/mixer/instance.json".into(),
            r#"{"tool": "mixer", "state": {}}"#.into(),
        ),
        (
            "state/mixer/master.json".into(),
            r#"{"tool": "utility", "state": {}}"#.into(),
        ),
        (
            "state/mixer/sway.json".into(),
            r#"{"tool": "sway", "state": {"rate": 2}}"#.into(),
        ),
        ("extensions/mixer.tsx".into(), MIXER.into()),
        ("extensions/sway.ts".into(), SWAY.into()),
    ];
    for (name, frequency, gain, mix) in strips {
        let tone = serde_json::json!({ "tool": "tone", "state": { "frequency_hz": frequency, "gain": gain } });
        files.push((format!("state/mixer/{name}.json"), tone.to_string()));
        files.push((
            format!("state/mixer/{name}-mix.json"),
            format!(r#"{{"tool": "utility", "state": {mix}}}"#),
        ));
        files.push((format!("state/mixer/{name}-meter.json"), METER.to_string()));
    }
    files.push(("state/mixer/master-meter.json".into(), METER.to_string()));
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    open_files(cx, &files)
}

/// Two bars of melody over chords, on the sixteenths.
fn melody() -> Vec<serde_json::Value> {
    let tune = [
        72, 74, 76, 79, 77, 76, 74, 72, 76, 79, 81, 79, 77, 76, 74, 76,
    ];
    let chords = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]];
    let mut notes: Vec<serde_json::Value> = (tune.iter().enumerate())
        .map(|(step, pitch)| {
            let start = step * 480;
            let velocity = 60 + (step * 37) % 60;
            serde_json::json!({ "start": start, "length": 420, "pitch": pitch, "velocity": velocity })
        })
        .collect();
    for (bar, chord) in chords.iter().enumerate() {
        for pitch in chord {
            let start = bar * 1920;
            notes.push(serde_json::json!({ "start": start, "length": 1800, "pitch": pitch, "velocity": 70 }));
        }
    }
    notes
}

/// Bun answers in real time, on a thread of its own.
fn wait_for_bun(cx: &mut HeadlessAppContext) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(300) {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A window on a project of one tool at its top, `name`, whose code is `code` at `file`.
fn open_page(
    cx: &mut HeadlessAppContext,
    name: &str,
    record: &str,
    file: &str,
    code: &str,
) -> Result<super::Opened> {
    let project = r#"{"format": 1, "extensions": [],
        "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
        "connections": []}"#;
    let record_path = format!("state/{name}.json");
    open_files(
        cx,
        &[
            ("project.json", project),
            (&record_path, record),
            (file, code),
        ],
    )
}

/// A window on a project of these files.
fn open_files(cx: &mut HeadlessAppContext, files: &[(&str, &str)]) -> Result<super::Opened> {
    open_files_with(cx, files, |_| Ok(()))
}

/// Four seconds of a voice that swells and fades every half second, in `assets/audio/voice.wav`.
fn write_voice(root: &Path) -> Result<()> {
    let path = root.join("assets/audio/voice.wav");
    std::fs::create_dir_all(path.parent().context("a parent folder")?)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec)?;
    for frame in 0..4 * 48_000 {
        let seconds = f64::from(frame) / 48_000.0;
        let swell = (seconds * std::f64::consts::TAU).sin().abs();
        let tone = (seconds * 220.0 * std::f64::consts::TAU).sin();
        writer.write_sample((0.6 * swell * tone) as f32)?;
    }
    writer.finalize()?;
    Ok(())
}

/// [`open_files`], with `prepare` writing more into the project folder first.
fn open_files_with(
    cx: &mut HeadlessAppContext,
    files: &[(&str, &str)],
    prepare: impl FnOnce(&Path) -> Result<()>,
) -> Result<super::Opened> {
    let folder = tempfile::tempdir()?;
    let root = folder.path().join("Night Study");
    for (relative, contents) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().context("a parent folder")?)?;
        std::fs::write(path, contents)?;
    }
    prepare(&root)?;
    cx.allow_parking();
    let (control, engine) = Engine::new(OFFLINE);
    let plugins = test_plugin_host(&root);
    let (project, extensions) = runtime::open_with_extensions(&root, control, plugins.clone())?;
    let extensions = extensions.context("the tools of the project")?;
    let plugins = plugins.downgrade();
    let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
    let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
        let session = session.clone();
        let (mut views, mut devices) = views(plugins.clone());
        sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
        cx.new(|cx| {
            let name = "MacBook Pro Speakers".into();
            Shell::with_device(
                session,
                (views, devices),
                name,
                DeviceAccess::default(),
                window,
                cx,
            )
        })
    })?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(super::Opened {
        _folder: folder,
        engine,
        plugins,
        session,
        window,
    })
}
