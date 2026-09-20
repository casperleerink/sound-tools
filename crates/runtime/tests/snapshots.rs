//! Renders the application window to PNGs with no visible window, and times its frames.
//! `cargo test -p runtime --test snapshots` writes to `$WINDOW_SNAPSHOT_DIR`, default
//! `<target>/window-snapshots`:
//!
//! - `default.png`: the default project.
//! - `piece.png`: three tracks with several clips, playing, one clip selected.
//! - `scale.png`: 100 tracks of 100 clips, scrolled to the middle.
//! - `menu.png`: the project menu, open, after one edit.
//!
//! The frame times it prints are those of `Window::draw` on the scale project: building and
//! painting the scene, not the GPU.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use arrangement::view::ArrangementView;
use arrangement::view::layout::{TRACK_HEIGHT, Viewport};
use arrangement::{Colour, TrackState};
use gpui::{AppContext, Entity, HeadlessAppContext, WindowHandle, px, size};
use instrument::SynthState;
use runtime::window::Shell;
use runtime::{OFFLINE, main_arrangement, open_or_create, views};
use sound_core::{Changes, Engine, Instance, InstanceId, Project, Ticks};
use sound_notes::{Clip, Length, Note, Pitch, Velocity};
use sound_ui::{Assets, Session};
use tempfile::TempDir;

const BAR: u64 = 3840;

struct Opened {
    _folder: TempDir,
    engine: Engine,
    session: Entity<Session>,
    window: WindowHandle<Shell>,
}

impl Opened {
    fn new(
        cx: &mut HeadlessAppContext,
        fill: impl FnOnce(&mut Project) -> Result<()>,
    ) -> Result<Self> {
        // The folder name is the project name in the window.
        let folder = tempfile::tempdir()?;
        let (control, engine) = Engine::new(OFFLINE);
        let mut project = open_or_create(&folder.path().join("Night Study"), control)?;
        fill(&mut project)?;
        let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
        let window = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
            let session = session.clone();
            cx.new(|cx| Shell::new(session, views(), "MacBook Pro Speakers".into(), window, cx))
        })?;
        cx.run_until_parked();
        Ok(Self {
            _folder: folder,
            engine,
            session,
            window,
        })
    }

    /// Lets the engine run for `frames` and the session see it, as the device and the poll
    /// timer do in the real window.
    fn advance(&mut self, frames: usize, cx: &mut HeadlessAppContext) {
        let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
        self.engine.process_block(&mut buffer);
        cx.update(|cx| self.session.update(cx, |session, cx| session.poll(cx)));
        cx.run_until_parked();
    }

    /// Seeks and plays. Making a large project leaves many edits waiting for the engine, and
    /// the transport waits behind them, so this runs the engine until it plays.
    fn play_from(&mut self, tick: Ticks, cx: &mut HeadlessAppContext) -> Result<()> {
        cx.update(|cx| {
            self.session.update(cx, |session, _| {
                session.engine().seek(tick);
                session.engine().play();
            })
        });
        for _ in 0..1000 {
            self.advance(64, cx);
            if cx.update(|cx| self.session.read(cx).playhead().read(cx).playing) {
                return Ok(());
            }
        }
        anyhow::bail!("the engine did not start to play")
    }

    fn timeline_view(
        &self,
        cx: &mut HeadlessAppContext,
    ) -> Result<Entity<arrangement::view::Timeline>> {
        let view = cx.update(|cx| self.window.read(cx).map(|shell| shell.main_view().cloned()))?;
        let view = view.context("the window shows no main view")?;
        let view = view
            .downcast::<ArrangementView>()
            .ok()
            .context("not the arrangement")?;
        Ok(cx.update(|cx| view.read(cx).timeline().clone()))
    }

    fn draw(&self, cx: &mut HeadlessAppContext) -> Result<Duration> {
        cx.update_window(self.window.into(), |_, window, cx| {
            let started = Instant::now();
            window.draw(cx).clear(cx);
            started.elapsed()
        })
    }
}

fn note(start: u64, length: u64, pitch: u8) -> Result<Note> {
    Ok(Note {
        start: Ticks(start),
        length: Length::new(Ticks(length))?,
        pitch: Pitch::new(pitch)?,
        velocity: Velocity::new(100)?,
    })
}

fn clip(start_bar: u64, bars: u64, notes: Vec<Note>) -> Result<Clip> {
    Ok(Clip {
        start: Ticks(start_bar * BAR),
        length: Length::new(Ticks(bars * BAR))?,
        notes,
    })
}

fn add_track(
    project: &mut Project,
    name: &str,
    colour: Colour,
    clips: Vec<Clip>,
) -> Result<Instance<TrackState>> {
    let arrangement = main_arrangement(project).context("no arrangement")?;
    let mut changes = Changes::new();
    let synth = SynthState::default();
    let track =
        arrangement::add_track(project, &mut changes, arrangement.id(), name, colour, synth)?;
    for (index, clip) in clips.into_iter().enumerate() {
        let id = track.id().child(&format!("clip-{index:03}"))?;
        changes.create(id, clip);
    }
    project.commit("Add track", changes)?;
    Ok(track)
}

/// A short piece: chords, a bass line and a melody with a gap, over twelve bars.
fn piece(project: &mut Project) -> Result<()> {
    let mut chords = Vec::new();
    for (bar, root) in [(0, 57), (1, 53), (2, 48), (3, 55)] {
        for interval in [0, 4, 7, 12] {
            chords.push(note(bar * BAR, BAR - 120, root + interval)?);
        }
    }
    let mut bass = Vec::new();
    for (bar, root) in [(0, 33), (1, 29), (2, 36), (3, 31)] {
        for eighth in 0..8 {
            let pitch = if eighth % 4 == 3 { root + 12 } else { root };
            bass.push(note(bar * BAR + eighth * 480, 360, pitch)?);
        }
    }
    let mut melody = Vec::new();
    let line = [
        76, 74, 72, 74, 76, 79, 76, 72, 69, 72, 74, 72, 71, 67, 69, 71,
    ];
    for (index, pitch) in line.into_iter().enumerate() {
        melody.push(note(index as u64 * 960, 720, pitch)?);
    }

    let track = main_arrangement(project)
        .and_then(|arrangement| arrangement::tracks(project, arrangement.id()).pop())
        .context("the default project has a track")?
        .0;
    let mut changes = Changes::new();
    for (index, start_bar) in [0, 4, 8].into_iter().enumerate() {
        let id = track.id().child(&format!("chords-{index}"))?;
        changes.create(id, clip(start_bar, 4, chords.clone())?);
    }
    project.commit("Add chords", changes)?;
    let bass_clips = vec![
        clip(0, 4, bass.clone())?,
        clip(4, 4, bass.clone())?,
        clip(8, 4, bass)?,
    ];
    add_track(project, "Bass", Colour::Peach, bass_clips)?;
    let melody_clips = vec![
        clip(4, 4, melody.clone())?,
        clip(10, 2, melody[..8].to_vec())?,
    ];
    add_track(
        project,
        "A melody with a name too long for its header",
        Colour::Mauve,
        melody_clips,
    )?;
    Ok(())
}

/// The scale project of the milestone: clip `c` of track `t` is one bar at bar `8c + t mod 8`.
fn scale(project: &mut Project) -> Result<()> {
    for track in 0..100_u64 {
        let pitch = 36 + (track % 40) as u8;
        let notes: Vec<Note> = (0..4)
            .map(|beat| note(beat * 960, 900, pitch + beat as u8 * 3))
            .collect::<Result<_>>()?;
        let clips = (0..100).map(|index| clip(index * 8 + track % 8, 1, notes.clone()));
        let colour = Colour::ALL[track as usize % Colour::ALL.len()];
        add_track(
            project,
            &format!("Track {}", track + 2),
            colour,
            clips.collect::<Result<_>>()?,
        )?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let out_dir = std::env::var("WINDOW_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("../window-snapshots"));
    std::fs::create_dir_all(&out_dir)?;

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(sound_ui::init);
    let save = |cx: &mut HeadlessAppContext, opened: &Opened, name: &str| -> Result<()> {
        let path = out_dir.join(format!("{name}.png"));
        cx.capture_screenshot(opened.window.into())?.save(&path)?;
        println!("wrote {}", path.display());
        Ok(())
    };

    let opened = Opened::new(&mut cx, |_| Ok(()))?;
    save(&mut cx, &opened, "default")?;

    let mut opened = Opened::new(&mut cx, piece)?;
    let timeline = opened.timeline_view(&mut cx)?;
    opened.play_from(Ticks(5 * BAR + 1440), &mut cx)?;
    let selected = InstanceId::new("arrangement/bass/clip-001")?;
    cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.select_clip(Some(selected), cx)));
    cx.run_until_parked();
    save(&mut cx, &opened, "piece")?;

    // The menu after an edit, so that undo has something to name.
    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            let arrangement = main_arrangement(session.project()).context("no arrangement")?;
            session.edit(cx, |project| runtime::add_track(project, &arrangement));
            anyhow::Ok(())
        })
    })?;
    let menu = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        anyhow::Ok(shell.project_menu().read(cx).menu().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "menu")?;
    drop(opened);

    let started = Instant::now();
    let mut opened = Opened::new(&mut cx, scale)?;
    println!("scale project made and opened in {:?}", started.elapsed());
    let timeline = opened.timeline_view(&mut cx)?;
    let middle = Viewport {
        scroll_x: 400.0 * 96.0,
        scroll_y: 44.0 * f64::from(TRACK_HEIGHT),
        ..Viewport::default()
    };
    cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(middle, cx)));
    opened.play_from(Ticks(405 * BAR), &mut cx)?;
    save(&mut cx, &opened, "scale")?;

    // Frames while playing: only the playhead moves, the timeline reuses what it painted.
    let mut playing = Vec::new();
    for _ in 0..200 {
        opened.advance(800, &mut cx);
        playing.push(opened.draw(&mut cx)?);
    }
    // Frames while scrolling: the timeline builds and paints its scene again.
    let mut scrolling = Vec::new();
    for step in 0..200 {
        let viewport = middle.scrolled(-(step as f32) * 7.0, -(step as f32) * 3.0);
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
        scrolling.push(opened.draw(&mut cx)?);
    }
    // Far zoomed out: every clip of every visible track is on screen.
    let far_out = middle.zoomed(0.05, 0.0);
    let mut zoomed_out = Vec::new();
    for step in 0..200 {
        let viewport = far_out.scrolled(-(step as f32), 0.0);
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
        zoomed_out.push(opened.draw(&mut cx)?);
    }
    for (what, mut times) in [
        ("playing", playing),
        ("scrolling", scrolling),
        ("scrolling, zoomed far out", zoomed_out),
    ] {
        times.sort();
        let mean = times.iter().sum::<Duration>() / times.len() as u32;
        let worst = times.last().copied().unwrap_or_default();
        println!(
            "frame while {what}: mean {mean:?}, median {:?}, worst {worst:?}",
            times[times.len() / 2]
        );
    }
    Ok(())
}
