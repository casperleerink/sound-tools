//! The open project of one window and every rule of working on it that no view should repeat:
//! gestures and undo, notices, the transport state that is never saved, the selection and the
//! render that runs. Plain data in and out; a thread asks for a poll through the [`Waker`].

mod gesture;
mod render;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::ops::Range;
use std::sync::Arc;

use plugin_host::Plugins;
use sound_core::{
    EngineStatus, InputEndpoint, InstanceId, PlayedLanes, PointId, Project, ProjectEdit,
    ProjectError, ProjectEvent, Ticks,
};
pub use sound_typescript::next::Waker;
use sound_typescript::next::{Clock, Transport};
use sound_typescript::touch;

pub use gesture::{Drag, Held};
pub use render::Rendered;

pub struct Session {
    project: Project,
    plugins: Plugins,
    waker: Waker,
    /// What the engine said at the last poll.
    status: EngineStatus,
    /// A stopped engine fails every poll. It is reported once.
    engine_stopped: bool,
    notice: Option<(Source, String)>,
    /// The open gesture and its number, see [`Drag`].
    gesture: Option<(u64, ProjectEdit)>,
    gestures: u64,
    writer: touch::Writer,
    transport: TransportState,
    click: Option<metronome::Click>,
    selected: Option<InstanceId>,
    span: Option<(Ticks, Ticks)>,
    recording: bool,
    render: Option<render::Running>,
}

/// Where a notice came from decides what clears it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Source {
    /// A failed edit: the next edit that works clears it.
    Edit,
    /// The engine, the watcher or the system. Said once, so only a dismissal or a newer notice
    /// clears it: a drag publishes sixty good edits a second.
    System,
}

/// What the composer chose for how the piece plays. Interface state: never saved, no undo step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportState {
    pub looped: Loop,
    pub click: bool,
    /// Whether the computer keys play notes.
    pub keys: bool,
    /// Whether a hand on a number a lane moves writes into the lane while the piece plays.
    pub automation: bool,
}

/// The part of the timeline that plays again and again while it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loop {
    pub range: Range<Ticks>,
    pub on: bool,
}

/// What a poll found.
#[derive(Debug, Default)]
pub struct Polled {
    pub events: Vec<ProjectEvent>,
    /// What the window shows besides the records changed: the notice, playing, the render.
    pub redraw: bool,
    /// The render that ended, see [`Session::render`].
    pub rendered: Option<anyhow::Result<Rendered>>,
}

impl Session {
    /// Takes the project, with the click attached and off, the loop off over bars 1 to 4.
    /// Events of opening are dropped: no view could have shown anything older.
    pub fn new(mut project: Project, plugins: Plugins, waker: Waker) -> Self {
        project.drain_events();
        let signatures = project.project_file().tempo_map.time_signatures();
        let bar_five = signatures.bar(5).map_or(Ticks(0), |bar| bar.start);
        let click = metronome::Click::attach(project.engine());
        let mut session = Self {
            project,
            plugins,
            waker,
            status: EngineStatus::default(),
            engine_stopped: false,
            notice: None,
            gesture: None,
            gestures: 0,
            writer: touch::Writer::default(),
            transport: TransportState {
                looped: Loop {
                    range: Ticks(0)..bar_five,
                    on: false,
                },
                click: false,
                keys: false,
                automation: false,
            },
            click: None,
            selected: None,
            span: None,
            recording: false,
            render: None,
        };
        match click {
            Ok(click) => session.click = Some(click),
            Err(error) => session.report(error),
        }
        session
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn status(&self) -> EngineStatus {
        self.status
    }

    /// One poll: the engine, the folder, the background work of the extensions, the open
    /// gesture, then what changed in the project. The window calls it every frame.
    pub fn poll(&mut self) -> Polled {
        let before = (
            self.notice.clone(),
            self.status.playing,
            self.rendering_now(),
        );
        match self.project.engine().poll() {
            Ok(status) => self.status = status,
            Err(error) if !self.engine_stopped => {
                self.engine_stopped = true;
                self.report(error);
            }
            Err(_) => {}
        }
        if let Err(error) = self.project.poll() {
            self.report(error);
        }
        let (problems, errors) = crate::tick(&mut self.project, &self.plugins);
        problems.iter().for_each(|problem| self.report(problem));
        errors.iter().for_each(|error| self.report(error));
        for notice in self.plugins.take_notices() {
            self.report(format!("plugin scan: {notice}"));
        }
        self.follow(false);
        let events = self.project.drain_events();
        self.finish_if_deleted(&events);
        let rendered = self.take_rendered();
        let after = (
            self.notice.clone(),
            self.status.playing,
            self.rendering_now(),
        );
        Polled {
            events,
            redraw: before != after || rendered.is_some(),
            rendered,
        }
    }

    /// Runs one project operation, such as a `commit`. An error goes to the notice and gives
    /// `None`; one that works clears the notice of an edit that failed.
    pub fn edit<R>(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Option<R> {
        match operation(&mut self.project) {
            Ok(value) => {
                if matches!(self.notice, Some((Source::Edit, _))) {
                    self.notice = None;
                }
                Some(value)
            }
            Err(error) => {
                self.set_notice(Source::Edit, error.to_string());
                None
            }
        }
    }

    /// Work on the project that is no edit, for the services outside it: nothing is written,
    /// there is no undo step and the notice of an edit stays.
    pub fn background<R>(&mut self, work: impl FnOnce(&mut Project) -> R) -> R {
        work(&mut self.project)
    }

    /// Whether it went: else the notice says why. So do redo, go to and merge.
    pub fn undo(&mut self) -> bool {
        self.go_elsewhere(Project::undo)
    }

    /// Undo that keeps what it takes back as a version.
    pub fn undo_keeping_version(&mut self) -> bool {
        self.go_elsewhere(Project::undo_keeping_version)
    }

    pub fn redo(&mut self) -> bool {
        self.go_elsewhere(Project::redo)
    }

    pub fn go_to(&mut self, point: PointId) -> bool {
        self.go_elsewhere(|project| project.go_to(point))
    }

    /// Merges the version that begins at `start` into the current point.
    pub fn merge(&mut self, start: PointId, keep_mine: bool) -> bool {
        self.go_elsewhere(|project| project.merge(start, keep_mine))
    }

    /// Writes other files over the folder, unless a drag is open, whose next move would write
    /// over them, or a take records, which would land in a project that is gone.
    fn go_elsewhere<R>(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> bool {
        let waits = match (self.gesture.is_some(), self.recording) {
            (true, _) => "Let go of the drag first.",
            (false, true) => "Stop recording first.",
            (false, false) => return self.edit(operation).is_some(),
        };
        self.set_notice(Source::Edit, waits.to_string());
        false
    }

    /// Whether a take records now, which undo waits for.
    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    /// Every file change until [`Self::end_request`] is one point named `label`: an agent's
    /// request.
    pub fn begin_request(&mut self, label: &str) {
        self.project.begin_request(label);
    }

    pub fn end_request(&mut self) {
        self.project.end_request();
    }

    /// The last thing that went wrong, for the corner of the window.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_ref().map(|(_, message)| message.as_str())
    }

    /// Shows what went wrong outside an edit: the engine, the watcher, the system. It stays
    /// until dismissed or replaced.
    pub fn report(&mut self, error: impl Display) {
        self.set_notice(Source::System, error.to_string());
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// A new notice is also a line on stderr, which the app's log keeps.
    fn set_notice(&mut self, source: Source, message: String) {
        let notice = (source, message);
        if self.notice.as_ref() != Some(&notice) {
            eprintln!("notice: {}", notice.1);
            self.notice = Some(notice);
        }
    }

    pub fn transport(&self) -> &TransportState {
        &self.transport
    }

    pub fn set_loop(&mut self, looped: Loop) {
        if self.transport.looped != looped {
            let range = looped.on.then(|| looped.range.clone());
            self.project.engine().set_loop(range);
            self.transport.looped = looped;
        }
    }

    /// Nothing when the click could not be attached, which the notice said.
    pub fn set_click(&mut self, on: bool) {
        let Some(click) = &mut self.click else {
            return;
        };
        if on == self.transport.click {
            return;
        }
        match click.set_on(self.project.engine(), on) {
            Ok(()) => self.transport.click = on,
            Err(error) => self.report(error),
        }
    }

    pub fn set_computer_keys(&mut self, on: bool) {
        self.transport.keys = on;
    }

    pub fn set_writes_automation(&mut self, on: bool) {
        self.transport.automation = on;
    }

    /// Where the piece plays and how, as code is told it.
    pub fn clock(&self) -> Clock {
        let status = self.status;
        let tick = status.playhead_tick;
        let tempo_map = &self.project.project_file().tempo_map;
        let at = tempo_map.time_signatures().bar_beat_of(tick);
        let TransportState {
            looped,
            click,
            keys,
            automation,
        } = &self.transport;
        Clock {
            time: status.frames as f64 / f64::from(self.project.clock().sample_rate()),
            transport: Transport {
                playing: status.playing,
                tick: tick.0,
                bar: at.bar,
                beat: at.beat,
                bpm: tempo_map.change_at(tick).bpm.bpm(),
                repeat: looped
                    .on
                    .then_some([looped.range.start.0, looped.range.end.0]),
                click: *click,
                keys: *keys,
                automation: *automation,
            },
        }
    }

    /// What the composer works on and the stretch of the piece it covers, as the code that
    /// owns the selection says. Interface state: never saved, no undo step.
    pub fn select(&mut self, instance: Option<InstanceId>, span: Option<(Ticks, Ticks)>) {
        (self.selected, self.span) = (instance, span);
    }

    /// The selected instance, while it is in the project.
    pub fn selected(&self) -> Option<&InstanceId> {
        let selected = self.selected.as_ref()?;
        self.project.tool_of(selected).map(|_| selected)
    }

    /// The selected stretch, for an export; none when it is empty or nothing is selected.
    pub fn selected_span(&self) -> Option<(Ticks, Ticks)> {
        self.selected()?;
        self.span.filter(|(from, to)| from < to)
    }

    /// Where live notes go: see [`crate::notes_inputs`].
    pub fn live_notes(&self) -> Vec<InputEndpoint> {
        crate::notes_inputs(&self.project, self.selected())
    }

    /// The numbers a lane moves of each instance of `shown`, which a hand does not change, by
    /// [`held`].
    pub fn held(&self, shown: &BTreeSet<InstanceId>) -> BTreeMap<InstanceId, Vec<String>> {
        held(&self.project, shown, self.writes_now())
    }

    /// Whether a hand on a number a lane moves writes into the lane now.
    fn writes_now(&self) -> bool {
        self.transport.automation && self.status.playing
    }
}

/// A turn of a plugin's knob that is still open is written as one point, and so is an open drag.
impl Drop for Session {
    fn drop(&mut self) {
        self.finish_open();
        for error in self.plugins.end_turns(&mut self.project) {
            eprintln!("error: {error}");
        }
    }
}

/// The numbers a lane moves of each instance of `shown` and of each instance inside it, as code
/// names them: `cutoff_hz` of its own, `t01/mix.gain_db` of one inside. While `writing` (write
/// automation on and the piece playing), a number whose lane a hand writes into is free.
pub fn held(
    project: &Project,
    shown: &BTreeSet<InstanceId>,
    writing: bool,
) -> BTreeMap<InstanceId, Vec<String>> {
    let mut holds: BTreeMap<InstanceId, Vec<String>> = BTreeMap::new();
    let mut values = Vec::new();
    for (inner, _) in project.instances() {
        let Some(lanes) = project.lanes(inner) else {
            continue;
        };
        values.clear();
        numbers_held(lanes, writing, &mut values);
        for owner in std::iter::once(inner.clone()).chain(inner.ancestors()) {
            if !shown.contains(&owner) {
                continue;
            }
            let path = match owner == *inner {
                true => String::new(),
                false => format!("{}.", &inner.as_str()[owner.as_str().len() + 1..]),
            };
            let held = holds.entry(owner).or_default();
            held.extend(values.iter().map(|(name, _)| format!("{path}{name}")));
        }
    }
    for held in holds.values_mut() {
        held.sort();
        held.dedup();
    }
    holds.retain(|_, held| !held.is_empty());
    holds
}

/// The numbers `lanes` move, into `values`. While `writing`, a lane a hand writes into is free.
fn numbers_held(lanes: &dyn PlayedLanes, writing: bool, values: &mut Vec<(Arc<str>, f32)>) {
    lanes.values_at(Ticks(0), values);
    values.retain(|(name, _)| !(writing && lanes.source(name).is_some()));
}
