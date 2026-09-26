//! The bridge between a live project and GPUI views.
//!
//! One [`Session`] entity owns the [`Project`] on the main thread. It polls the engine and the
//! file watcher from a timer, never from `render`, and tells views what changed. Views hold
//! the entity and typed `Instance<S>` handles, read the current state in `render`, and keep
//! no copy of saved state. `README.md` in this crate is the guide for view authors.

use std::fmt::Display;
use std::time::Duration;

use gpui::{Context, Entity, EventEmitter, SharedString, Task, prelude::*};
use sound_core::{
    EngineControl, EngineStatus, InstanceId, Project, ProjectEdit, ProjectError, ProjectEvent,
    Ticks,
};

/// How often the session polls. About one display frame, so the playhead moves smoothly.
/// A poll that finds nothing new notifies nobody, so a stopped project draws no frames.
pub const POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Where the project plays. Its own entity, because it changes every frame during playback:
/// only what shows the playhead observes it, and everything else observes the [`Session`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Playhead {
    pub playing: bool,
    pub tick: Ticks,
    /// Jumps of the project position since the engine started: one per seek and per stop. A
    /// view that keeps the last value knows whether the position jumped or moved with
    /// playback, which it cannot tell from the tick alone.
    pub jumps: u64,
}

/// Where a notice came from decides what clears it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum NoticeSource {
    /// A failed edit. The next edit that works clears it.
    Edit,
    /// The engine, the watcher or the system. It is reported once, so only a dismissal or a
    /// newer notice clears it. A drag publishes sixty good edits a second.
    System,
}

pub struct Session {
    project: Project,
    playhead: Entity<Playhead>,
    notice: Option<(NoticeSource, SharedString)>,
    /// The open gesture, see [`Self::begin_gesture`].
    gesture: Option<ProjectEdit>,
    /// What the composer is working on, see [`Self::select`].
    selected: Option<InstanceId>,
    /// The clip the composer is working on, see [`Self::select_clip`].
    selected_clip: Option<InstanceId>,
    /// A stopped engine fails every poll. It is reported once.
    engine_stopped: bool,
    _polling: Task<()>,
}

/// Every project event is emitted, so a view can refresh only for the ids it shows:
/// `cx.subscribe(&session, |view, _, event, cx| ...)`. The session also notifies once per
/// group of events and when the notice changes, for views that refresh on anything.
impl EventEmitter<ProjectEvent> for Session {}

impl Session {
    /// Takes the project and starts polling. Events from opening the project are dropped:
    /// no view exists yet that could have shown something older.
    pub fn new(mut project: Project, cx: &mut Context<Self>) -> Self {
        project.drain_events();
        let polling = cx.spawn(async move |session, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if session.update(cx, |session, cx| session.poll(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            project,
            playhead: cx.new(|_| Playhead::default()),
            notice: None,
            gesture: None,
            selected: None,
            selected_clip: None,
            engine_stopped: false,
            _polling: polling,
        }
    }

    /// Read the current state from here when rendering.
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn playhead(&self) -> &Entity<Playhead> {
        &self.playhead
    }

    /// The instance the composer is working on, such as the track whose header was clicked
    /// last. It is interface state: nothing is saved and there is no undo step.
    ///
    /// The view that owns a selection publishes it here, and anything outside that view reads
    /// it. Live MIDI input plays into the instrument of the selected track, and the window
    /// wires that, so the extension that owns the tracks and the one that reads the keyboard
    /// need nothing of each other.
    pub fn selected(&self) -> Option<&InstanceId> {
        self.selected.as_ref()
    }

    pub fn select(&mut self, instance: Option<InstanceId>, cx: &mut Context<Self>) {
        if self.selected != instance {
            self.selected = instance;
            cx.notify();
        }
    }

    /// The clip the composer has selected, next to [`Self::selected`], which is a track. It is
    /// interface state too, and the view that owns the selection publishes it here.
    ///
    /// Two fields and not one, because the two are read for different things and both are
    /// wanted at once: a keyboard plays into the instrument of the selected track while the
    /// project menu offers to fit the tempo to the take of the selected clip.
    pub fn selected_clip(&self) -> Option<&InstanceId> {
        self.selected_clip.as_ref()
    }

    pub fn select_clip(&mut self, clip: Option<InstanceId>, cx: &mut Context<Self>) {
        if self.selected_clip != clip {
            self.selected_clip = clip;
            cx.notify();
        }
    }

    /// The transport: `play`, `pause`, `stop` and `seek`. The result shows in the
    /// [`Playhead`] after the next poll. Change the tempo map through [`Self::edit`].
    pub fn engine(&mut self) -> &mut EngineControl {
        self.project.engine()
    }

    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.playhead.read(cx).playing {
            self.engine().pause();
        } else {
            self.engine().play();
        }
    }

    /// Runs one project operation, such as a `commit`. Observers hear what it changed. An
    /// error goes to the notice and gives `None`, so no caller can drop one. A drag goes
    /// through [`Self::begin_gesture`], and undo and redo through [`Self::undo`].
    pub fn edit<R>(
        &mut self,
        cx: &mut Context<Self>,
        operation: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Option<R> {
        let result = operation(&mut self.project);
        self.emit_events(cx);
        // Always, not only after events: finishing an edit changes the undo label and no record.
        cx.notify();
        match result {
            Ok(value) => {
                if matches!(self.notice, Some((NoticeSource::Edit, _))) {
                    self.notice = None;
                }
                Some(value)
            }
            Err(error) => {
                self.notice = Some((NoticeSource::Edit, error.to_string().into()));
                None
            }
        }
    }

    /// Opens the one gesture of this session: a drag, from mouse down to mouse up. The session
    /// keeps the edit, so a view cannot leave one open by losing it, and [`Self::undo`] and
    /// [`Self::redo`] do nothing until it ends. A gesture that is still open is finished first.
    pub fn begin_gesture(&mut self, label: &str, cx: &mut Context<Self>) {
        self.finish_gesture(cx);
        self.gesture = Some(self.project.begin(label));
    }

    pub fn gesture_open(&self) -> bool {
        self.gesture.is_some()
    }

    /// Publishes into the open gesture, per mouse move: `project.update(edit, ..)` or
    /// `project.publish(edit, ..)`. `None` when no gesture is open, or on an error, which goes
    /// to the notice.
    pub fn gesture<R>(
        &mut self,
        cx: &mut Context<Self>,
        publish: impl FnOnce(&mut Project, &mut ProjectEdit) -> Result<R, ProjectError>,
    ) -> Option<R> {
        let mut edit = self.gesture.take()?;
        let result = self.edit(cx, |project| publish(project, &mut edit));
        self.gesture = Some(edit);
        result
    }

    /// Ends the gesture as one undo step and writes the files. Nothing without a gesture.
    pub fn finish_gesture(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.gesture.take() {
            self.edit(cx, |project| project.finish(edit));
        }
    }

    /// Ends the gesture and applies the state from before it. Nothing without a gesture.
    pub fn cancel_gesture(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.gesture.take() {
            self.edit(cx, |project| project.cancel(edit));
        }
    }

    /// Undo for keys and menus. Ignored while a gesture is open: undo in the middle of a drag
    /// would be overwritten by the next mouse move and leave a step that ends nowhere.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() {
            self.edit(cx, Project::undo);
        }
    }

    /// Redo, ignored while a gesture is open, like [`Self::undo`].
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() {
            self.edit(cx, Project::redo);
        }
    }

    /// Runs the behaviours of these instances again, with the records they already have.
    ///
    /// It is not an edit: nothing is written and there is no undo step. It is for a service
    /// outside the project that can do more now than it could before, so far only the plugin
    /// host: its scan has found a plugin a record was waiting for, or a VST 3 plugin asked to be
    /// unloaded and loaded again (`kReloadComponent`).
    pub fn rebind(&mut self, instances: &[InstanceId], cx: &mut Context<Self>) {
        for id in instances {
            if let Err(error) = self.project.rebind(id) {
                self.report(error, cx);
            }
        }
        if self.emit_events(cx) {
            cx.notify();
        }
    }

    /// The last error, for a quiet status surface. The notice of a failed edit stays until
    /// an edit succeeds. Any other stays until it is dismissed.
    pub fn notice(&self) -> Option<&SharedString> {
        self.notice.as_ref().map(|(_, message)| message)
    }

    /// Shows an error that did not come from an edit: the engine, the watcher, the system.
    pub fn report(&mut self, error: impl Display, cx: &mut Context<Self>) {
        let notice = (NoticeSource::System, SharedString::from(error.to_string()));
        if self.notice.as_ref() != Some(&notice) {
            self.notice = Some(notice);
            cx.notify();
        }
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice.take().is_some() {
            cx.notify();
        }
    }

    /// One poll of the engine and the watcher. The timer calls it. Tests and headless
    /// snapshots call it to skip the wait.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        match self.project.engine().poll() {
            Ok(status) => self.follow(&status, cx),
            Err(error) if !self.engine_stopped => {
                self.engine_stopped = true;
                self.report(error, cx);
            }
            Err(_) => {}
        }
        if let Err(error) = self.project.poll() {
            self.report(error, cx);
        }
        if self.emit_events(cx) {
            cx.notify();
        }
    }

    fn follow(&mut self, status: &EngineStatus, cx: &mut Context<Self>) {
        let now = Playhead {
            playing: status.playing,
            tick: status.playhead_tick,
            jumps: status.jumps,
        };
        self.playhead.update(cx, |playhead, cx| {
            if *playhead != now {
                *playhead = now;
                cx.notify();
            }
        });
    }

    /// Whether there was anything to emit.
    fn emit_events(&mut self, cx: &mut Context<Self>) -> bool {
        let events = self.project.drain_events();
        let any = !events.is_empty();
        for event in events {
            cx.emit(event);
        }
        any
    }
}
