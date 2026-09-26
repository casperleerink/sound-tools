//! Renders the application window to PNGs with no visible window, and times its frames.
//! `cargo test -p runtime --test snapshots` writes to `$WINDOW_SNAPSHOT_DIR`, default
//! `<target>/window-snapshots`. The window is 1470 x 920 points at scale 2, the screen of a
//! 13 inch MacBook Air without the menu bar, which is the laptop the design is for:
//!
//! - `default.png`: the default project.
//! - `piece.png`: three tracks with several clips, playing, one clip selected.
//! - `transport-click-off.png`: the same with the transport in focus, the click off.
//! - `transport-click-on.png`: the same with the click on.
//! - `transport-recording.png`: the same while it records.
//! - `notices.png`: an error from an edit and a file that is not live, bottom-left.
//! - `scale.png`: 100 tracks of 100 clips, scrolled to the middle.
//! - `menu.png`: the project menu, open, after one edit.
//! - `fit-action.png`: the project menu over a recorded take, with `Fit tempo to take`.
//! - `fitted.png`: the same project after the fit, with the steadiness control in the
//!   transport and the clip of the take on the grid the playing made.
//! - `fit-steady.png`: the same with the steadiness at 60 %.
//! - `fit-recording.png`: the same while it records: tempo, steadiness and record together.
//! - `editor.png`: the note editor open on the selected clip, one note selected.
//! - `editor-focus.png`: the same with the focus from the keyboard, and the editor scrolled.
//! - `track-panel.png`: the track panel open on the bass, with a sound that is not the default.
//! - `track-panel-focus.png`: the same after tab went to the cutoff knob.
//! - `track-panel-synth-effects.png`: the synth with five effects after it, which is wider
//!   than the rack has room for on this screen, so its right edge fades. The test checks
//!   the fade in the pixels.
//! - `track-panel-filter.png`: the synth and the built-in filter after it.
//! - `track-panel-filter-expanded.png`: the same with the filter expanded: slope and LFO.
//! - `track-panel-empty.png`: the panel of a track whose instrument is a tool with no view.
//! - `track-panel-plugin.png`: the panel of a track whose instrument is a CLAP plugin.
//! - `track-panel-picker.png`: the same with the instrument picker open.
//! - `track-panel-plugin-vst3.png`: the panel of a track whose instrument is a VST 3 plugin.
//! - `track-panel-missing.png`: the panel of a track whose plugin this machine does not have.
//! - `track-panel-effects.png`: the rack with an instrument and two effects, and the control
//!   that adds one at the end of it.
//! - `track-panel-effect-picker.png`: the same with that control open.
//! - `track-panel-effect-missing.png`: an effect whose plugin this machine does not have.
//! - `track-panel-picker-disabled.png`: the picker of a project that does not enable the
//!   plugin host, where every plugin says what the one edit is.
//! - `fit-action-disabled.png`: the project menu of a project made before the fit existed,
//!   where the fit says what the one edit is.
//!
//! The frame times it prints are those of one update and the `Window::draw` it causes on the
//! scale project: rendering, layout and painting into the scene, not the GPU. The drag times
//! are those of one real mouse move on that project: the publish into the gesture, every
//! project event it causes and the frame after it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use arrangement::view::layout::{HEADER_WIDTH, RULER_HEIGHT, TRACK_HEIGHT, Viewport};
use arrangement::view::roll::{self, EDITOR_HEIGHT, KEY_HEIGHT};
use arrangement::view::{ArrangementView, NoteEditor};
use arrangement::{Colour, TrackState};
use filter::FilterState;
use filter::view::FilterView;
use gpui::{
    AppContext, Entity, HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, WindowHandle, point, px, size,
};
use instrument::SynthState;
use plugin_host::{PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand};
use runtime::window::Shell;
use runtime::{OFFLINE, main_arrangement, open_or_create_with, views};
use sound_core::{Changes, Engine, Instance, InstanceId, Project, Ticks};
use sound_notes::{Clip, Length, Note, Pitch, Velocity};
use sound_ui::{Assets, Session};
use tempfile::TempDir;

#[path = "projects/generated_take.rs"]
mod generated_take;

const BAR: u64 = 3840;
/// The window in points.
const WINDOW_WIDTH: f32 = 1470.;
const WINDOW_HEIGHT: f32 = 920.;

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
        // A plugin host that looks only in a folder of this project, with the repository's
        // own test plugin in it, so that the picker holds the same plugins everywhere.
        let root = folder.path().join("Night Study");
        let plugins = test_plugin_host(&root);
        let mut project = open_or_create_with(&root, control, plugins.clone())?;
        fill(&mut project)?;
        let plugins = plugins.downgrade();
        let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
        let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
            let (session, plugins) = (session.clone(), plugins.clone());
            cx.new(|cx| {
                let name = "MacBook Pro Speakers".into();
                Shell::new(session, views(plugins), name, window, cx)
            })
        })?;
        cx.run_until_parked();
        Ok(Self {
            _folder: folder,
            engine,
            session,
            window,
        })
    }

    /// A project whose `project.json` does not enable the plugin host, as one made before
    /// step 4a has. Its content is that of the default project.
    fn without_plugin_host(cx: &mut HeadlessAppContext) -> Result<Self> {
        Self::without_extensions(cx, r#"["arrangement", "instrument", "tone"]"#, |_| Ok(()))
    }

    /// A project that enables only these extensions, with the default content plus whatever
    /// `fill` writes into it. For the controls that offer something a project cannot load.
    fn without_extensions(
        cx: &mut HeadlessAppContext,
        extensions: &str,
        fill: impl FnOnce(&mut Project) -> Result<()>,
    ) -> Result<Self> {
        let folder = tempfile::tempdir()?;
        let root = folder.path().join("Night Study");
        let write = |relative: &str, contents: &str| -> Result<()> {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().context("a parent folder")?)?;
            std::fs::write(path, contents)?;
            Ok(())
        };
        write(
            "project.json",
            &format!(
                r#"{{"format": 1, "extensions": {extensions},
                "tempo_map": {{"time_signature": "4/4", "tempo_changes": [{{"tick": 0, "bpm": 120.0}}]}},
                "connections": []}}"#
            ),
        )?;
        write(
            "state/arrangement/instance.json",
            r#"{"tool": "arrangement", "state": {}}"#,
        )?;
        write(
            "state/arrangement/track-1/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "Track 1", "order": 1}}"#,
        )?;
        write(
            "state/arrangement/track-1/instrument.json",
            r#"{"tool": "instrument.synth", "state": {}}"#,
        )?;
        let (control, engine) = Engine::new(OFFLINE);
        let plugins = test_plugin_host(&root);
        let mut project = open_or_create_with(&root, control, plugins.clone())?;
        fill(&mut project)?;
        let plugins = plugins.downgrade();
        let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
        let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
            let (session, plugins) = (session.clone(), plugins.clone());
            cx.new(|cx| {
                let name = "MacBook Pro Speakers".into();
                Shell::new(session, views(plugins), name, window, cx)
            })
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
    /// timer do in the real window. Returns how long the poll and the frame it caused took:
    /// with test support GPUI draws a window as soon as an update leaves it dirty.
    fn advance(&mut self, frames: usize, cx: &mut HeadlessAppContext) -> Duration {
        let mut buffer = vec![0.0_f32; frames * OFFLINE.channels];
        self.engine.process_block(&mut buffer);
        let started = Instant::now();
        cx.update(|cx| self.session.update(cx, |session, cx| session.poll(cx)));
        cx.run_until_parked();
        started.elapsed()
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

    fn arrangement_view(&self, cx: &mut HeadlessAppContext) -> Result<Entity<ArrangementView>> {
        let view = cx.update(|cx| self.window.read(cx).map(|shell| shell.main_view().cloned()))?;
        let view = view.context("the window shows no main view")?;
        view.downcast::<ArrangementView>()
            .ok()
            .context("not the arrangement")
    }

    fn timeline_view(
        &self,
        cx: &mut HeadlessAppContext,
    ) -> Result<Entity<arrangement::view::Timeline>> {
        let view = self.arrangement_view(cx)?;
        Ok(cx.update(|cx| view.read(cx).timeline().clone()))
    }

    /// Selects a clip and opens the note editor for it, as enter does.
    fn open_editor(
        &self,
        clip: &InstanceId,
        cx: &mut HeadlessAppContext,
    ) -> Result<Entity<NoteEditor>> {
        let view = self.arrangement_view(cx)?;
        let timeline = self.timeline_view(cx)?;
        let instance = cx.update(|cx| self.session.read(cx).project().resolve::<Clip>(clip));
        let instance = instance.context("no such clip")?;
        cx.update_window(self.window.into(), |_, window, cx| {
            timeline.update(cx, |timeline, cx| {
                timeline.select_clip(Some(clip.clone()), cx)
            });
            view.update(cx, |view, cx| view.open_editor(instance, window, cx));
        })?;
        cx.run_until_parked();
        let editor = cx.update(|cx| view.read(cx).editor().cloned());
        editor.context("the editor did not open")
    }

    /// A real click on the header of a track row, which selects the track and opens its
    /// panel. The focus is then on the timeline, from the mouse, so no ring shows.
    fn click_track_header(&self, row: f32, cx: &mut HeadlessAppContext) -> Result<()> {
        let position = point(
            px(HEADER_WIDTH / 2.),
            px(48. + RULER_HEIGHT + TRACK_HEIGHT * (row + 0.5)),
        );
        self.drag(position, point(px(0.), px(0.)), 0, cx)?;
        let view = self.arrangement_view(cx)?;
        let open = cx.update(|cx| view.read(cx).track_panel().is_some());
        anyhow::ensure!(open, "the track panel did not open");
        Ok(())
    }

    /// One key, and a frame, as the screen draws one between two keys: a focus ring follows
    /// the focus that a frame sees.
    fn key(&self, keys: &str, cx: &mut HeadlessAppContext) -> Result<()> {
        let keystroke = gpui::Keystroke::parse(keys)?;
        let key_down = PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
        cx.update_window(self.window.into(), |_, window, cx| {
            window.dispatch_event(key_down, cx);
        })?;
        cx.run_until_parked();
        cx.capture_screenshot(self.window.into())?;
        Ok(())
    }

    /// One real mouse event, and the frame it causes. Returns how long both took.
    fn mouse(&self, event: PlatformInput, cx: &mut HeadlessAppContext) -> Result<Duration> {
        let started = Instant::now();
        cx.update_window(self.window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
        })?;
        cx.run_until_parked();
        Ok(started.elapsed())
    }

    /// A drag with the left button from `from`, one mouse move per step of `step`, and the
    /// time of each move. The button comes up where the drag ends.
    fn drag(
        &self,
        from: Point<Pixels>,
        step: Point<Pixels>,
        moves: usize,
        cx: &mut HeadlessAppContext,
    ) -> Result<Vec<Duration>> {
        let modifiers = Modifiers::default();
        let moved = |position, pressed_button| {
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button,
                modifiers,
            })
        };
        self.mouse(moved(from, None), cx)?;
        self.mouse(
            PlatformInput::MouseDown(MouseDownEvent {
                position: from,
                modifiers,
                button: MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        )?;
        let mut position = from;
        let mut times = Vec::new();
        for _ in 0..moves {
            position += step;
            times.push(self.mouse(moved(position, Some(MouseButton::Left)), cx)?);
        }
        self.mouse(
            PlatformInput::MouseUp(MouseUpEvent {
                position,
                modifiers,
                button: MouseButton::Left,
                click_count: 1,
            }),
            cx,
        )?;
        Ok(times)
    }
}

/// A plugin host that scans one folder inside the project, with the repository's own test
/// plugin in it. No plugin of this machine is ever listed, so the picker looks the same in CI.
fn test_plugin_host(root: &std::path::Path) -> Plugins {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    test_vst3_plugin::install_into(&folder);
    let scanner = ScanCommand::new(
        env!("CARGO_BIN_EXE_runtime"),
        [std::ffi::OsString::from(plugin_host::SCAN_ARGUMENT)],
    );
    Plugins::new(vec![folder], scanner, ScanCache::none())
}

/// Puts a plugin record in the `instrument` slot of a track, as picking one does.
fn set_plugin(
    project: &mut Project,
    track: &str,
    format: PluginFormat,
    plugin_id: &str,
    state_asset: &str,
) -> Result<()> {
    let slot = InstanceId::new(&format!("arrangement/{track}/instrument"))?;
    let mut changes = Changes::new();
    changes.create(
        slot,
        PluginRecord::new(format, plugin_id, state_asset).context("a plugin record")?,
    );
    project.commit("Choose a plugin", changes)?;
    Ok(())
}

/// Puts a plugin in each of these effect slots of a track, in this order. One group each,
/// which is what an interface does: `add_effect` reads the track record of the project and not
/// the group being built.
fn set_effects(project: &mut Project, track: &str, names: &[&str]) -> Result<()> {
    let id = InstanceId::new(&format!("arrangement/{track}"))?;
    for name in names {
        let track = project
            .resolve::<TrackState>(&id)
            .context("the track is not there")?;
        let mut changes = Changes::new();
        let slot = arrangement::add_effect(project, &mut changes, &track, name)?;
        let record =
            PluginRecord::new(PluginFormat::Clap, test_clap_plugin::PLUGIN_ID, slot.name());
        changes.create(slot, record.context("a plugin record")?);
        project.commit(&format!("Add {name}"), changes)?;
    }
    Ok(())
}

/// Puts a filter after the instrument of a track, as `Add effect` does.
fn add_filter(project: &mut Project, track: &str) -> Result<()> {
    let id = InstanceId::new(&format!("arrangement/{track}"))?;
    let track = project
        .resolve::<TrackState>(&id)
        .context("the track is not there")?;
    let mut changes = Changes::new();
    let slot = arrangement::add_effect(project, &mut changes, &track, "Filter")?;
    let sound = FilterState {
        cutoff_hz: 1_200.0,
        resonance: 0.3,
        ..FilterState::default()
    };
    changes.create(slot, sound);
    project.commit("Add Filter", changes)?;
    Ok(())
}

fn print_times(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
    let worst = times.last().copied().unwrap_or_default();
    let median = times.get(times.len() / 2).copied().unwrap_or_default();
    println!("{what}: mean {mean:?}, median {median:?}, worst {worst:?}");
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
    Ok(Clip::new(
        Ticks(start_bar * BAR),
        Length::new(Ticks(bars * BAR))?,
        notes,
    ))
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

/// A project with one track and the clip of a recorded take, as a recording leaves it.
fn recorded(project: &mut Project) -> Result<()> {
    let take = generated_take::generated_take(8);
    let name = take.write(project.assets())?;
    let clock = project.clock().clone();
    let mut clip = take
        .clip(|time_us| clock.tick_at_micros(time_us))
        .context("a take with notes in it")?;
    clip.take = Some(name);
    let track = main_arrangement(project)
        .and_then(|arrangement| arrangement::tracks(project, arrangement.id()).pop())
        .context("the default project has a track")?
        .0;
    let mut changes = Changes::new();
    changes.create(track.id().child("take")?, clip);
    project.commit("Record", changes)?;
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
    cx.update(runtime::window::bind_keys);
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

    // The transport with the tempo and the click, off and on. The click is a switch on the
    // engine, not project state: nothing is written and there is no undo step.
    save(&mut cx, &opened, "transport-click-off")?;
    let transport = cx.update(|cx| anyhow::Ok(opened.window.read(cx)?.transport().clone()))?;
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_click(cx)));
    cx.run_until_parked();
    anyhow::ensure!(
        cx.update(|cx| transport.read(cx).click_is_on()),
        "the click did not come on"
    );
    save(&mut cx, &opened, "transport-click-on")?;

    // Recording: the record control in red, next to play and stop. Nothing is played into it,
    // so the take is empty and it makes no clip and no file.
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_recording(cx)));
    cx.run_until_parked();
    anyhow::ensure!(
        cx.update(|cx| transport.read(cx).is_recording()),
        "the recording did not start"
    );
    save(&mut cx, &opened, "transport-recording")?;
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_recording(cx)));
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_click(cx)));
    cx.run_until_parked();

    // The two notices, bottom-left: an edit that failed, and a file that is not live, here
    // the record of a plugin this machine does not have.
    let notices = Opened::new(&mut cx, |project| {
        piece(project)?;
        set_plugin(
            project,
            "bass",
            PluginFormat::Clap,
            "com.example.nowhere",
            "bass",
        )
    })?;
    // A clip with a note after its end, which the clip tool refuses.
    let wrong = clip(0, 1, vec![note(2 * BAR, 480, 60)?])?;
    let id = InstanceId::new("arrangement/bass/clip-wrong")?;
    cx.update(|cx| {
        notices.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.create(id, wrong);
                project.commit("Add clip", changes)
            })
        })
    });
    anyhow::ensure!(
        cx.update(|cx| notices.session.read(cx).notice().is_some()),
        "the edit did not fail"
    );
    cx.run_until_parked();
    save(&mut cx, &notices, "notices")?;
    drop(notices);

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

    // A recorded take: the project menu offers to fit the tempo to it once its clip is
    // selected, and the transport grows a steadiness control once the fit is there.
    let mut opened = Opened::new(&mut cx, recorded)?;
    let timeline = opened.timeline_view(&mut cx)?;
    let take_clip = InstanceId::new("arrangement/track-1/take")?;
    cx.update(|cx| {
        timeline.update(cx, |timeline, cx| {
            timeline.select_clip(Some(take_clip.clone()), cx)
        })
    });
    cx.run_until_parked();
    let menu = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        anyhow::Ok(shell.project_menu().read(cx).menu().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "fit-action")?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.close(window, cx));
    })?;

    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            let clip = session
                .project()
                .resolve::<Clip>(&take_clip)
                .context("the take clip")?;
            let clip = session.project().state(&clip).context("its state")?.clone();
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                fit_tempo::fit_take(project, &mut changes, &clip)?;
                project.commit(fit_tempo::FIT_LABEL, changes)
            });
            anyhow::Ok(())
        })
    })?;
    opened.play_from(Ticks(5 * BAR), &mut cx)?;
    let transport = cx.update(|cx| anyhow::Ok(opened.window.read(cx)?.transport().clone()))?;
    anyhow::ensure!(
        cx.update(|cx| transport.read(cx).shown_steadiness(cx)) == Some(0.0),
        "the transport shows no steadiness"
    );
    save(&mut cx, &opened, "fitted")?;

    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                fit_tempo::set_steadiness(project, &mut changes, 0.6);
                project.commit(fit_tempo::STEADINESS_LABEL, changes)
            });
        })
    });
    cx.run_until_parked();
    save(&mut cx, &opened, "fit-steady")?;
    let transport = cx.update(|cx| anyhow::Ok(opened.window.read(cx)?.transport().clone()))?;
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_recording(cx)));
    cx.run_until_parked();
    save(&mut cx, &opened, "fit-recording")?;
    cx.update(|cx| transport.update(cx, |pill, cx| pill.toggle_recording(cx)));
    cx.run_until_parked();
    drop(opened);

    // A project made before the fit existed: the action is at 40 % and says the one edit that
    // brings it within reach, exactly as an instrument the project cannot load does.
    let extensions = r#"["arrangement", "instrument", "plugin-host", "tone"]"#;
    let opened = Opened::without_extensions(&mut cx, extensions, recorded)?;
    let timeline = opened.timeline_view(&mut cx)?;
    let take_clip = InstanceId::new("arrangement/track-1/take")?;
    cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.select_clip(Some(take_clip), cx)));
    cx.run_until_parked();
    let menu = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        anyhow::Ok(shell.project_menu().read(cx).menu().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "fit-action-disabled")?;
    drop(opened);

    // The note editor on the melody, one note selected, stopped at the start of the clip.
    let opened = Opened::new(&mut cx, piece)?;
    let melody =
        InstanceId::new("arrangement/a-melody-with-a-name-too-long-for-its-header/clip-000")?;
    let editor = opened.open_editor(&melody, &mut cx)?;
    cx.update(|cx| editor.update(cx, |editor, cx| editor.select_note(Some(5), cx)));
    cx.run_until_parked();
    save(&mut cx, &opened, "editor")?;
    // The bass, low in the pitch range, as the editor looks after tab gave it the focus.
    let bass = InstanceId::new("arrangement/bass/clip-001")?;
    opened.open_editor(&bass, &mut cx)?;
    // Away and back with the keys, so the focus is one from the keyboard and shows its ring.
    for keys in ["shift-tab", "tab"] {
        opened.key(keys, &mut cx)?;
    }
    save(&mut cx, &opened, "editor-focus")?;

    // The track panel takes the place of the editor. The bass has a sound of its own, so the
    // knobs are not all where the defaults are.
    let bass_synth = InstanceId::new("arrangement/bass/instrument")?;
    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            let synth = session.project().resolve::<SynthState>(&bass_synth);
            let synth = synth.context("the bass has no synth")?;
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let sound = SynthState {
                    waveform: instrument::Waveform::Square,
                    cutoff_hz: 480.0,
                    resonance: 0.4,
                    decay_seconds: 0.35,
                    sustain: 0.25,
                    release_seconds: 0.12,
                    ..SynthState::default()
                };
                changes.set(&synth, sound);
                project.commit("Change sound", changes)
            });
            anyhow::Ok(())
        })
    })?;
    opened.click_track_header(1., &mut cx)?;
    save(&mut cx, &opened, "track-panel")?;
    // From the timeline, tab goes to the close control, the volume, the pan and mute, then the
    // picker and the expand icon of the card, the waveform and the cutoff.
    for _ in 0..8 {
        opened.key("tab", &mut cx)?;
    }
    save(&mut cx, &opened, "track-panel-focus")?;
    drop(opened);

    // The synth with five effects: more than the rack has room for on the screen of the
    // laptop, so it fades at its right edge.
    let opened = Opened::new(&mut cx, |project| {
        piece(project)?;
        set_effects(project, "bass", &["Warmth", "Space", "Air", "Echo", "Room"])
    })?;
    opened.click_track_header(1., &mut cx)?;
    save(&mut cx, &opened, "track-panel-synth-effects")?;
    // The last card runs under the right edge of the window, and there the rack fades to the
    // window colour: dark at the edge, the card itself 40 pt in, in the blank of its body.
    let image = cx.capture_screenshot(opened.window.into())?;
    let red_at = |x: f32| {
        let y = WINDOW_HEIGHT - 216. + 12. + 32. + 60.;
        image.get_pixel((x * 2.) as u32, (y * 2.) as u32).0[0]
    };
    let (edge, card) = (red_at(WINDOW_WIDTH - 2.), red_at(WINDOW_WIDTH - 45.));
    anyhow::ensure!(
        edge < 22 && card > 24,
        "no fade at the right edge of the rack: {edge} at the edge, {card} on the card"
    );
    drop(opened);

    // The built-in filter after the synth, with the values of the mockup, then expanded with
    // the slope and the LFO.
    let opened = Opened::new(&mut cx, |project| {
        piece(project)?;
        add_filter(project, "bass")
    })?;
    opened.click_track_header(1., &mut cx)?;
    save(&mut cx, &opened, "track-panel-filter")?;
    let view = opened.arrangement_view(&mut cx)?;
    let card = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let card = panel.read(cx).device_views().nth(1).flatten().cloned();
        let card = card.context("the filter has no card")?;
        card.downcast::<FilterView>()
            .map_err(|_| anyhow::anyhow!("the second card is not the filter"))
    })?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    cx.run_until_parked();
    save(&mut cx, &opened, "track-panel-filter-expanded")?;
    drop(opened);

    // A track whose instrument is a tool that has no view: the tone.
    let opened = Opened::new(&mut cx, |project| {
        let slot = InstanceId::new("arrangement/track-1/instrument")?;
        let mut changes = Changes::new();
        changes.delete(&slot);
        project.commit("Remove synth", changes)?;
        let mut changes = Changes::new();
        changes.create(slot, tone::ToneState::default());
        project.commit("Add tone", changes)?;
        Ok(())
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-empty")?;
    drop(opened);

    // A track that plays a CLAP plugin: the name of the plugin on the card, with the control
    // that opens the plugin's own window. Then the picker of that card, open.
    let opened = Opened::new(&mut cx, |project| {
        set_plugin(
            project,
            "track-1",
            PluginFormat::Clap,
            test_clap_plugin::PLUGIN_ID,
            "test-tone",
        )
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-plugin")?;
    let view = opened.arrangement_view(&mut cx)?;
    let picker = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let picker = panel.read(cx).pickers().next().cloned();
        picker.context("the card has no picker")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        picker.update(cx, |picker, cx| picker.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "track-panel-picker")?;
    drop(opened);

    // The same for a VST 3 plugin. Its card is the CLAP one: the plugin's name and the control
    // that opens its own window, which it has had since step 5b.
    let opened = Opened::new(&mut cx, |project| {
        set_plugin(
            project,
            "track-1",
            PluginFormat::Vst3,
            test_vst3_plugin::PLUGIN_ID,
            "test-tone",
        )
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-plugin-vst3")?;
    drop(opened);

    // A project that does not enable the plugin host, as one made before step 4a has: every
    // plugin is shown and cannot be taken, with the one edit that would put it within reach.
    let opened = Opened::without_plugin_host(&mut cx)?;
    opened.click_track_header(0., &mut cx)?;
    let view = opened.arrangement_view(&mut cx)?;
    let picker = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let picker = panel.read(cx).pickers().next().cloned();
        picker.context("the card has no picker")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        picker.update(cx, |picker, cx| picker.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "track-panel-picker-disabled")?;
    drop(opened);

    // A plugin this machine does not have: the card says so and names the id, and the record
    // is left exactly as it is.
    let opened = Opened::new(&mut cx, |project| {
        set_plugin(
            project,
            "track-1",
            PluginFormat::Clap,
            "com.example.nowhere",
            "piano",
        )
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-missing")?;
    drop(opened);

    // The rack with an instrument and two effects after it, in the order of the record, with
    // the control that adds one at the end. Then that control, open, with what this machine
    // offers as an effect.
    let opened = Opened::new(&mut cx, |project| {
        set_plugin(
            project,
            "track-1",
            PluginFormat::Clap,
            test_clap_plugin::PLUGIN_ID,
            "test-tone",
        )?;
        set_effects(project, "track-1", &["Warmth", "Space"])
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-effects")?;
    let view = opened.arrangement_view(&mut cx)?;
    let add = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        anyhow::Ok(panel.read(cx).add_effect_control().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        add.update(cx, |add, cx| add.open(window, cx));
    })?;
    cx.run_until_parked();
    save(&mut cx, &opened, "track-panel-effect-picker")?;
    drop(opened);

    // An effect this machine does not have: the card says so, the rest of the chain plays.
    let opened = Opened::new(&mut cx, |project| {
        set_plugin(
            project,
            "track-1",
            PluginFormat::Clap,
            test_clap_plugin::PLUGIN_ID,
            "test-tone",
        )?;
        set_effects(project, "track-1", &["Warmth"])?;
        let slot = InstanceId::new("arrangement/track-1/warmth")?;
        let mut changes = Changes::new();
        let record = PluginRecord::new(PluginFormat::Clap, "com.example.nowhere", "warmth");
        changes.create(slot, record.context("a plugin record")?);
        project.commit("A plugin this machine does not have", changes)?;
        Ok(())
    })?;
    opened.click_track_header(0., &mut cx)?;
    save(&mut cx, &opened, "track-panel-effect-missing")?;
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

    // The transport reads this after every change, so it has to stay small.
    for _ in 0..3 {
        let started = Instant::now();
        let end = cx.update(|cx| opened.session.read(cx).project().end());
        println!(
            "Project::end over 10,000 clips: {:?} ({end:?})",
            started.elapsed()
        );
        let started = Instant::now();
        let end = cx.update(|cx| {
            let project = opened.session.read(cx).project();
            main_arrangement(project).and_then(|a| arrangement::end(project, a.id()))
        });
        println!("arrangement::end: {:?} ({end:?})", started.elapsed());
    }

    // Frames while playing: only the playhead moves, the timeline reuses what it painted.
    let mut playing = Vec::new();
    for _ in 0..200 {
        playing.push(opened.advance(800, &mut cx));
    }
    // Frames while scrolling: the timeline builds and paints its scene again.
    let mut scrolling = Vec::new();
    for step in 0..200 {
        let viewport = middle.scrolled(-(step as f32) * 7.0, -(step as f32) * 3.0);
        let started = Instant::now();
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
        scrolling.push(started.elapsed());
    }
    // Far zoomed out: every clip of every visible track is on screen.
    let far_out = middle.zoomed(0.05, 0.0);
    let mut zoomed_out = Vec::new();
    for step in 0..200 {
        let viewport = far_out.scrolled(-(step as f32), 0.0);
        let started = Instant::now();
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(viewport, cx)));
        zoomed_out.push(started.elapsed());
    }
    print_times("frame while playing", playing);
    print_times("frame while scrolling", scrolling);
    print_times("frame while scrolling, zoomed far out", zoomed_out);

    // A drag of a clip with the real mouse events, a snap step per move: 100 moves to the
    // right on its track, then 8 moves down over other tracks. Stopped, so that only the drag
    // is measured.
    cx.update(|cx| {
        opened
            .session
            .update(cx, |session, _| session.engine().stop())
    });
    opened.advance(64, &mut cx);
    cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.set_viewport(middle, cx)));
    cx.run_until_parked();
    // Row 46 is track 45 of the scale project, after the track of the default project. Its
    // clip 50 is at bar 8 * 50 + 45 % 8 = 405, which is on screen.
    let top = 48.0 + RULER_HEIGHT;
    let row_46 = top + middle.y_of(46) + TRACK_HEIGHT / 2.;
    let on_clip = point(
        px(HEADER_WIDTH + middle.x_of(Ticks(405 * BAR + BAR / 2))),
        px(row_46),
    );
    let in_time = opened.drag(on_clip, point(px(6.), px(0.)), 100, &mut cx)?;
    print_times("clip drag in time, per mouse move", in_time);
    let moved_to = on_clip + point(px(600.), px(0.));
    let across = opened.drag(moved_to, point(px(0.), px(TRACK_HEIGHT)), 8, &mut cx)?;
    print_times("clip drag across tracks, per mouse move", across);
    let label = cx.update(|cx| {
        opened
            .session
            .read(cx)
            .project()
            .undo_label()
            .map(str::to_string)
    });
    anyhow::ensure!(
        label.as_deref() == Some("Move clip"),
        "the drag did not move a clip"
    );

    // A drag of a note in the editor of a clip of that project, a semitone per move.
    let clip = InstanceId::new("arrangement/track-60/clip-050")?;
    let editor = opened.open_editor(&clip, &mut cx)?;
    let viewport = cx.update(|cx| editor.read(cx).viewport());
    let state = cx.update(|cx| {
        let project = opened.session.read(cx).project();
        let instance = project.resolve::<Clip>(&clip)?;
        project.state(&instance).cloned()
    });
    let state = state.context("the clip has no state")?;
    let first = state.notes.first().context("the clip has no notes")?;
    let editor_top = WINDOW_HEIGHT - EDITOR_HEIGHT + RULER_HEIGHT;
    let on_note = point(
        px(HEADER_WIDTH + viewport.x_of(state.start + first.start) + 40.),
        px(editor_top + roll::y_of(&viewport, first.pitch) + KEY_HEIGHT / 2.),
    );
    let mut note_moves = opened.drag(on_note, point(px(12.), px(-KEY_HEIGHT)), 10, &mut cx)?;
    let back = on_note + point(px(120.), px(-10. * KEY_HEIGHT));
    note_moves.extend(opened.drag(back, point(px(-12.), px(KEY_HEIGHT)), 10, &mut cx)?);
    print_times("note drag, per mouse move", note_moves);
    let label = cx.update(|cx| {
        opened
            .session
            .read(cx)
            .project()
            .undo_label()
            .map(str::to_string)
    });
    anyhow::ensure!(
        label.as_deref() == Some("Move note"),
        "the drag did not move a note"
    );
    save(&mut cx, &opened, "scale-editor")?;

    // The publish alone, without the frame: the record, the behaviour of its track with a new
    // snapshot of its 100 clips, and the engine batch.
    let instance = cx.update(|cx| opened.session.read(cx).project().resolve::<Clip>(&clip));
    let instance = instance.context("no such clip")?;
    let mut publishes = Vec::new();
    for step in 1..=100_u64 {
        publishes.push(cx.update(|cx| {
            opened.session.update(cx, |session, cx| {
                if step == 1 {
                    session.begin_gesture("Move clip", cx);
                }
                let started = Instant::now();
                session.gesture(cx, |project, edit| {
                    project.update(edit, &instance, |clip| {
                        clip.start = state.start + Ticks(step * 240)
                    })
                });
                started.elapsed()
            })
        }));
        opened.advance(64, &mut cx);
    }
    cx.update(|cx| {
        opened
            .session
            .update(cx, |session, cx| session.cancel_gesture(cx))
    });
    print_times("publish of one clip move", publishes);
    Ok(())
}
