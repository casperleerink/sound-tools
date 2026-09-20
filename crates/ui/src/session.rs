//! The bridge between a live project and GPUI views.
//!
//! One [`Session`] entity owns the [`Project`] on the main thread. It polls the engine and the
//! file watcher from a timer, never from `render`, and tells views what changed. Views hold
//! the entity and typed `Instance<S>` handles, read the current state in `render`, and keep
//! no copy of saved state. `README.md` in this crate is the guide for view authors.

use std::fmt::Display;
use std::time::Duration;

use gpui::{Context, Entity, EventEmitter, SharedString, Task, prelude::*};
use sound_core::{EngineControl, EngineStatus, Project, ProjectError, ProjectEvent, Ticks};

/// How often the session polls. About one display frame, so the playhead moves smoothly.
/// A poll that finds nothing new notifies nobody, so a stopped project draws no frames.
pub const POLL_INTERVAL: Duration = Duration::from_millis(16);

/// Where the project plays. Its own entity, because it changes every frame during playback:
/// only what shows the playhead observes it, and everything else observes the [`Session`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Playhead {
    pub playing: bool,
    pub tick: Ticks,
}

pub struct Session {
    project: Project,
    playhead: Entity<Playhead>,
    notice: Option<SharedString>,
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

    /// Runs one project operation: `begin`, `publish`, `update`, `finish`, `cancel`, `commit`,
    /// `undo`, `redo`. Observers hear what it changed. An error goes to the notice and gives
    /// `None`, so no caller can drop one.
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
                self.notice = None;
                Some(value)
            }
            Err(error) => {
                self.notice = Some(error.to_string().into());
                None
            }
        }
    }

    /// The last error, for a quiet status surface. It stays until it is dismissed or an edit
    /// succeeds.
    pub fn notice(&self) -> Option<&SharedString> {
        self.notice.as_ref()
    }

    /// Shows an error that did not come from the project, for example from the system.
    pub fn report(&mut self, error: impl Display, cx: &mut Context<Self>) {
        let message = SharedString::from(error.to_string());
        if self.notice.as_ref() != Some(&message) {
            self.notice = Some(message);
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
