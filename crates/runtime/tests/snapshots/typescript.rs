//! Pages of tools of the project, drawn by Bun:
//!
//! - `typescript-mixer.png`: the mixer example of `tooling/agent-eval/examples` as five tones play:
//!   meters, faders, a muted strip, the master's scope and the device output.
//! - `typescript-song-mixer.png`: the same song with its mixer open.
//! - `typescript-song-lanes.png`: the same song with the automation lane of its first track
//!   showing under it.
//! - `typescript-song.png`: the song of the DAW template (`templates/daw`), 12 tracks of 150 clips,
//!   as it plays: the SDK's transport on top, a clip selected, the cards of its track's devices
//!   under the timeline, another clip open in the note editor, the meters of the tracks.
//! - `typescript-daw.png`: a new project from the DAW template, as it opens.
//! - `typescript-gallery*.png`: the project `sdk-gallery`, one example of each component, look,
//!   overlay and path of the view SDK: with a div held pressed, its popover open, its dialog open.
//! - `typescript-piano-roll.png`: the piano roll of `tooling/agent-eval/examples`, with the
//!   notes of bar 2 selected by a marquee and held by the pointer two beats on and two rows up,
//!   as the window moves them before their code hears the drag.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui::{
    HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, point, px,
};
use sound_core::Ticks;

const GALLERY: [(&str, &str); 3] = [
    ("project.json", include_str!("../sdk-gallery/project.json")),
    (
        "state/gallery.json",
        include_str!("../sdk-gallery/state/gallery.json"),
    ),
    (
        "extensions/gallery.tsx",
        include_str!("../sdk-gallery/extensions/gallery.tsx"),
    ),
];
const PIANO_ROLL: &str = include_str!("../../../../tooling/agent-eval/examples/piano-roll.tsx");
const MIXER: &str = include_str!("../../../../tooling/agent-eval/examples/mixer.tsx");
const METER: &str = r#"{"tool": "meter", "state": {}}"#;
const SWAY: &str = include_str!("../../../../tooling/agent-eval/examples/sway.ts");

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &super::Save) -> Result<()> {
    assert!(
        sound_typescript::has_bun(),
        "Bun is installed, see tooling/fetch-bun.sh"
    );
    let opened = open_files(cx, &GALLERY)?;
    // The div "Press" held, then the popover and the dialog each opened by its button.
    let press = |opened: &super::Opened, cx: &mut HeadlessAppContext, at| {
        let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
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
        anyhow::Ok(())
    };
    press(&opened, cx, point(px(833.), px(119.)))?;
    wait_for_bun(cx);
    save(cx, opened.window.into(), "typescript-gallery")?;
    for (button, name) in [
        (446., "typescript-gallery-popover"),
        (526., "typescript-gallery-dialog"),
    ] {
        let at = point(px(button), px(406.));
        press(&opened, cx, at)?;
        let up = MouseUpEvent {
            position: at,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        };
        opened.event(PlatformInput::MouseUp(up), cx)?;
        wait_for_bun(cx);
        save(cx, opened.window.into(), name)?;
    }
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
    let roll = point(px(44.), px(48. + 22. + 1.));
    let at = |tick: f32, row: f32| roll + point(px(tick * 0.1), px((row - 40.) * 12.));
    let mouse = |opened: &super::Opened, cx: &mut HeadlessAppContext, event| {
        opened.event(event, cx).map(|_| wait_for_bun(cx))
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
    save(cx, opened.window.into(), "typescript-piano-roll")?;
    drop(opened);

    let mut opened = open_mixer(cx)?;
    // Half a second of sound in frames of the window, as when it plays, so the meters, their
    // peak lines and the level's history move as they do.
    play(&mut opened, cx)?;
    save(cx, opened.window.into(), "typescript-mixer")?;
    drop(opened);

    let mut files = crate::song::song_files(12, 150);
    // The fourth track, whose devices show, has a reverb in its chain and a utility turned off.
    files.retain(|(path, _)| path != "state/song/t03/chain/instance.json");
    for (path, record) in [
        (
            "chain/instance.json",
            r#"{"tool": "chain", "state": {"order": ["filter", "room", "trim"], "off": ["trim"]}}"#,
        ),
        ("chain/room.json", r#"{"tool": "reverb", "state": {}}"#),
        ("chain/trim.json", r#"{"tool": "utility", "state": {}}"#),
    ] {
        files.push((format!("state/song/t03/{path}"), record.to_string()));
    }
    // A take of the voice, a region on its row from the second bar.
    files.push((
        "state/song/audio-voice/take.json".to_string(),
        r#"{"tool": "region", "state": {"asset": "voice.wav", "start": 3840}}"#.to_string(),
    ));
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut opened = open_files_with(cx, &files, write_voice)?;
    // Where bar 1 starts on the first row: right of the headers, under the title row and the
    // ruler.
    let timeline = point(px(crate::song::HEADER + crate::song::LEAD), px(TOP));
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
    save(cx, opened.window.into(), "typescript-song")?;
    // The mixer's button at the right of the master's row, over the editor: a strip per track
    // and return, and the master's on the right, in place of the editor.
    click(&opened, cx, point(px(1449.), px(547.)), 1)?;
    play(&mut opened, cx)?;
    save(cx, opened.window.into(), "typescript-song-mixer")?;
    // A track's head shows its panel in place of the mixer, and the automation of the first
    // track's head: its lane of the cutoff shows under it.
    click(&opened, cx, point(px(60.), px(280.)), 1)?;
    click(
        &opened,
        cx,
        point(px(24.), px(TOP + crate::song::TOGGLES)),
        1,
    )?;
    play(&mut opened, cx)?;
    save(cx, opened.window.into(), "typescript-song-lanes")?;
    drop(opened);

    // As a new project of the template is: written as the menu writes it.
    let opened = open_files_with(cx, &[], |root| {
        runtime::templates::write(root, crate::song::daw())
    })?;
    save(cx, opened.window.into(), "typescript-daw")
}

/// Where a song's rows start: under the title row, the ruler and its line.
const TOP: f32 = 48.0 + 32.0 + 1.0;

/// Plays half a second in frames of the window, so meters and what eases move as they do.
fn play(opened: &mut super::Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    opened.play_from(Ticks(0), cx)
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
    let files: Vec<(String, String)> = (files.iter())
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    super::Opened::open_with(cx, &files, prepare)
}
