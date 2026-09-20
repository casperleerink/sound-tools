//! A window on a temporary project, and the hands of a composer: a mouse that presses, moves
//! and releases at the place of a tick, a track or a pitch, and the keys.

use std::path::{Path, PathBuf};

use arrangement::view::layout::{HEADER_WIDTH, RULER_HEIGHT, TRACK_HEIGHT};
use arrangement::view::roll::{self, EDITOR_HEIGHT, KEY_HEIGHT};
use arrangement::view::{ArrangementView, NoteEditor, Timeline, TrackPanel};
use gpui::{
    AppContext, Entity, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualTestContext, point, px,
};
use runtime::window::{Shell, bind_keys};
use runtime::{OFFLINE, open_or_create, views};
use sound_core::{Engine, InstanceId, Project, Ticks};
use sound_notes::{Clip, Length, Note, Pitch, Velocity};
use sound_ui::{POLL_INTERVAL, Playhead, Session};
use tempfile::TempDir;

pub const BAR: u64 = 3840;
/// A sixteenth, the snap step.
pub const STEP: u64 = 240;
/// The height of the row with the project menu, above the main area.
pub const TOP_ROW: f32 = 48.;

pub struct Opened<'a> {
    pub folder: TempDir,
    pub engine: Engine,
    pub session: Entity<Session>,
    pub arrangement: Entity<ArrangementView>,
    pub timeline: Entity<Timeline>,
    pub cx: &'a mut VisualTestContext,
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub fn note(start: u64, length: u64, pitch: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

pub fn clip(start: u64, length: u64, notes: Vec<Note>) -> Clip {
    Clip {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        notes,
    }
}

/// Opens the window on a new default project in a temporary folder. `fill` adds to it first.
pub fn open_with(cx: &mut TestAppContext, fill: impl FnOnce(&mut Project)) -> Opened<'_> {
    let folder = tempfile::tempdir().unwrap();
    let (control, engine) = Engine::new(OFFLINE);
    let mut project = open_or_create(folder.path(), control).unwrap();
    fill(&mut project);
    open_project(cx, folder, project, engine)
}

/// Opens the window on a project that is open already.
pub fn open_project(
    cx: &mut TestAppContext,
    folder: TempDir,
    project: Project,
    engine: Engine,
) -> Opened<'_> {
    cx.update(sound_ui::init);
    let session = cx.new(|cx| Session::new(project, cx));
    cx.update(bind_keys);
    let (shell, cx) = cx.add_window_view({
        let session = session.clone();
        move |window, cx| Shell::new(session, views(), "Test device".into(), window, cx)
    });
    cx.run_until_parked();
    let main = shell
        .read_with(cx, |shell, _| shell.main_view().cloned())
        .unwrap();
    let arrangement = main.downcast::<ArrangementView>().ok().unwrap();
    let timeline = arrangement.read_with(cx, |view, _| view.timeline().clone());
    Opened {
        folder,
        engine,
        session,
        arrangement,
        timeline,
        cx,
    }
}

impl Opened<'_> {
    /// Lets the engine take what was sent to it and the poll timer see the result.
    pub fn settle(&mut self) {
        self.render(64);
        self.cx.executor().advance_clock(POLL_INTERVAL);
        self.cx.run_until_parked();
    }

    /// Runs the engine for `frames` and gives what it played, interleaved.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0_f32; frames * OFFLINE.channels];
        for buffer in output.chunks_mut(512 * OFFLINE.channels) {
            self.engine.process_block(buffer);
        }
        output
    }

    pub fn playhead(&mut self) -> Playhead {
        let session = self.session.clone();
        self.cx.read(|cx| *session.read(cx).playhead().read(cx))
    }

    pub fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.cx.run_until_parked();
    }

    /// A whole key press. GPUI clicks the focused control when enter comes up again, and
    /// `simulate_keystrokes` only sends the key down.
    pub fn press_enter(&mut self) {
        self.cx.simulate_keystrokes("enter");
        let keystroke = Keystroke::parse("enter").unwrap();
        self.cx.simulate_event(KeyUpEvent { keystroke });
        self.cx.run_until_parked();
    }

    pub fn project<R>(&mut self, read: impl FnOnce(&Project) -> R) -> R {
        let session = self.session.clone();
        self.cx.read(|cx| read(session.read(cx).project()))
    }

    /// Runs a project operation through the session, as the watcher or another view would.
    pub fn edit<R>(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<R, sound_core::ProjectError>,
    ) -> Option<R> {
        let session = self.session.clone();
        let result = self
            .cx
            .update(|_, cx| session.update(cx, |session, cx| session.edit(cx, operation)));
        self.cx.run_until_parked();
        result
    }

    pub fn clip(&mut self, clip: &str) -> Option<Clip> {
        let clip = id(clip);
        self.project(|project| {
            let instance = project.resolve::<Clip>(&clip)?;
            project.state(&instance).cloned()
        })
    }

    pub fn undo_label(&mut self) -> Option<String> {
        self.project(|project| project.undo_label().map(str::to_string))
    }

    pub fn redo_label(&mut self) -> Option<String> {
        self.project(|project| project.redo_label().map(str::to_string))
    }

    pub fn gesture_open(&mut self) -> bool {
        let session = self.session.clone();
        self.cx.read(|cx| session.read(cx).gesture_open())
    }

    pub fn notice(&mut self) -> Option<String> {
        let session = self.session.clone();
        self.cx
            .read(|cx| session.read(cx).notice().map(ToString::to_string))
    }

    /// A path in the project folder, canonical as the project knows it.
    pub fn path(&mut self, relative: &str) -> PathBuf {
        self.project(|project| project.root().join(relative))
    }

    /// The file of a clip as it is on disk. `None` when there is none.
    pub fn clip_file(&mut self, clip: &str) -> Option<String> {
        std::fs::read_to_string(self.path(&format!("state/{clip}.json"))).ok()
    }

    pub fn selected_clip(&mut self) -> Option<InstanceId> {
        let timeline = self.timeline.clone();
        self.cx
            .read(|cx| timeline.read(cx).selected_clip().cloned())
    }

    /// The place of a tick on a track row of the arrangement, in the middle of the row.
    pub fn at(&mut self, tick: u64, track: usize) -> Point<Pixels> {
        let timeline = self.timeline.clone();
        let viewport = self.cx.read(|cx| timeline.read(cx).viewport());
        point(
            px(HEADER_WIDTH + viewport.x_of(Ticks(tick))),
            px(TOP_ROW + RULER_HEIGHT + viewport.y_of(track) + TRACK_HEIGHT / 2.),
        )
    }

    /// The middle of the header of a track row of the arrangement.
    pub fn track_header(&mut self, track: usize) -> Point<Pixels> {
        let y = self.at(0, track).y;
        point(px(HEADER_WIDTH / 2.), y)
    }

    pub fn selected_track(&mut self) -> Option<InstanceId> {
        let timeline = self.timeline.clone();
        self.cx
            .read(|cx| timeline.read(cx).selected_track().cloned())
    }

    pub fn track_panel(&mut self) -> Option<Entity<TrackPanel>> {
        let arrangement = self.arrangement.clone();
        self.cx
            .read(|cx| arrangement.read(cx).track_panel().cloned())
    }

    /// The track that the open track panel shows.
    pub fn panel_track(&mut self) -> Option<InstanceId> {
        let panel = self.track_panel()?;
        Some(self.cx.read(|cx| panel.read(cx).track().id().clone()))
    }

    /// The middle of a control that names itself for tests: `knob-<id>` or `segment-<value>`.
    /// GPUI knows the bounds of what the last frame painted, and a cached view paints
    /// nothing, so this asks for a whole frame first.
    pub fn control(&mut self, selector: &'static str) -> Point<Pixels> {
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
        let bounds = self.cx.debug_bounds(selector);
        bounds
            .unwrap_or_else(|| panic!("nothing on screen is called {selector}"))
            .center()
    }

    pub fn editor(&mut self) -> Option<Entity<NoteEditor>> {
        let arrangement = self.arrangement.clone();
        self.cx.read(|cx| arrangement.read(cx).editor().cloned())
    }

    /// The clip that the open editor shows.
    pub fn editor_clip(&mut self) -> Option<InstanceId> {
        let editor = self.editor()?;
        Some(self.cx.read(|cx| editor.read(cx).clip().id().clone()))
    }

    pub fn selected_note(&mut self) -> Option<usize> {
        let editor = self.editor()?;
        self.cx.read(|cx| editor.read(cx).selected_note(cx))
    }

    fn editor_top(&mut self) -> f32 {
        let height = self.cx.update(|window, _| window.viewport_size().height);
        f32::from(height) - EDITOR_HEIGHT
    }

    /// The place of a project tick on the row of a pitch in the open note editor, in the
    /// middle of the row.
    pub fn in_editor(&mut self, tick: u64, pitch: u8) -> Point<Pixels> {
        let editor = self.editor().unwrap();
        let viewport = self.cx.read(|cx| editor.read(cx).viewport());
        let y = roll::y_of(&viewport, Pitch::new(pitch).unwrap()) + KEY_HEIGHT / 2.;
        point(
            px(HEADER_WIDTH + viewport.x_of(Ticks(tick))),
            px(self.editor_top() + RULER_HEIGHT + y),
        )
    }

    pub fn press(&mut self, position: Point<Pixels>) {
        self.press_times(position, 1);
    }

    fn press_times(&mut self, position: Point<Pixels>, click_count: usize) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    /// The button goes down where the pointer is, with no move before it: what arrives when
    /// the mouse up of a drag was lost and the next press comes.
    pub fn mouse_down(&mut self, position: Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    /// Several moves with the left button held, with no frame between them, as a fast mouse
    /// sends them between two frames of the screen.
    pub fn drag_through(&mut self, positions: &[Point<Pixels>]) {
        self.cx.update(|window, cx| {
            for position in positions {
                let event = MouseMoveEvent {
                    position: *position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                };
                window.dispatch_event(PlatformInput::MouseMove(event), cx);
            }
        });
        self.cx.run_until_parked();
    }

    /// A move with the left button held.
    pub fn drag_to(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
        self.cx.run_until_parked();
    }

    pub fn release(&mut self, position: Point<Pixels>) {
        self.release_times(position, 1);
    }

    fn release_times(&mut self, position: Point<Pixels>, click_count: usize) {
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
        self.cx.run_until_parked();
    }

    /// Press, move in two steps, release: the mouse never jumps in one event.
    pub fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.press(from);
        let half = point((from.x + to.x) / 2., (from.y + to.y) / 2.);
        self.drag_to(half);
        self.drag_to(to);
        self.release(to);
    }

    /// A scroll of the wheel or the trackpad at a place. A negative `dy` goes down.
    pub fn scroll(&mut self, position: Point<Pixels>, dx: f32, dy: f32) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            ..Default::default()
        });
        self.cx.run_until_parked();
    }

    /// Closes the window and the project, and gives the folder back for a second opening.
    pub fn close(self) -> TempDir {
        let Self {
            folder,
            engine,
            session,
            arrangement,
            timeline,
            cx,
        } = self;
        cx.update(|window, _| window.remove_window());
        drop((engine, arrangement, timeline));
        cx.run_until_parked();
        // The window held every view, and the views held the session. This was the last hold.
        let released = session.downgrade();
        drop(session);
        cx.cx.update(|_| {});
        cx.run_until_parked();
        let held = released.upgrade().is_some();
        assert!(
            !held,
            "something still holds the session and the project lock"
        );
        folder
    }

    pub fn click(&mut self, position: Point<Pixels>) {
        self.press(position);
        self.release(position);
    }

    pub fn double_click(&mut self, position: Point<Pixels>) {
        self.click(position);
        self.press_times(position, 2);
        self.release_times(position, 2);
    }
}

/// The loudest sample.
pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// Every record file under `state/` and `project.json`, by relative path, with its bytes.
pub fn files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(folder: &Path, root: &Path, found: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, found);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                found.push((relative, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = vec![(
        PathBuf::from("project.json"),
        std::fs::read(root.join("project.json")).unwrap(),
    )];
    walk(&root.join("state"), root, &mut found);
    found
}
