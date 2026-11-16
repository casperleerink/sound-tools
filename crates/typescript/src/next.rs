//! The tools of `extensions/` for the new window, with no GPUI: Bun runs their views of
//! `sdk/view.ts`, a plain function of the records each, and the actions on them. The window
//! calls [`Tools::poll`] at the start of each frame, and [`Tools::run_loops`] every
//! [`FRAME_INTERVAL`]; nothing here waits for Bun but [`Tools::settle`], which a run without a
//! window calls before each frame so the same project always draws the same.
//!
//! Any change in `extensions/`, a crash or a hang of Bun starts a new Bun, as in the old window.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::RecommendedWatcher;
use serde_json::{Map, Value, json};
use sound_core::{GROUPING_WINDOW, InstanceId, Problem, Project, ProjectEdit, ProjectEvent, Ticks};
use sound_notes::{NOTES_INPUT, NoteFeed, NoteFeedUpdate, Performed, Pitch, Velocity};
use sound_signals::{Signals, SignalsUpdate};

pub use crate::bun::Waker;
use crate::bun::{
    Answer, Bun, Clock, Event, Played, RecordEdit, Request, Runs, Skipped, Telling, Transport,
    TransportOp, record_key,
};
use crate::tools::{Choice, Field, ToolInfo, feed_child, feed_processor};
use crate::window::{changes_of, control_update, every_instance, find, played, told_to_bun, watch};
use crate::{Extensions, FOLDER, Launched};

/// The file of a project's design tokens, in `extensions/`. A project that has one is a project
/// of the new window: it gets `sdk/view.ts` and its agent docs.
pub const THEME: &str = "theme.json";

/// Whether the project in `folder` is one of the new window, see [`THEME`].
pub fn has_views(folder: &Path) -> bool {
    folder.join(FOLDER).join(THEME).is_file()
}

/// How long [`Tools::settle`] waits for Bun at most, so a view in an endless loop cannot stop
/// a run without a window. Bun calls code hung after 2 s itself.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

/// How often the control loops run, as in the old window.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Why an action on a view did nothing while no Bun runs.
const NOT_RUNNING: &str = "the code of the project does not run now, so this did nothing";

/// What [`Tools::run_loops`] did with a step of the control loops.
#[derive(Debug, PartialEq)]
pub enum Looped {
    Sent,
    /// Bun still runs the last step: the time keeps for the next.
    Busy,
    /// No Bun runs, or no instance has a control loop: no time keeps.
    Nothing,
}

/// What the window shows of an instance: the tree of its view, or why there is none.
pub enum Drawn {
    View {
        root: Rc<Value>,
        /// The parts a `memo` built, by key, with the number of the Bun and of the build.
        memos: HashMap<String, ((u64, u64), Rc<Value>)>,
    },
    /// Its tool has no view of `sdk/view.ts`, or is no tool of the project.
    None,
    Failed(String),
}

pub struct Tools {
    folder: PathBuf,
    opening: u64,
    skipped: Skipped,
    bun: Option<Arc<Bun>>,
    /// A new Bun that loads on a thread of its own, and whether the folder changed meanwhile.
    starting: Option<(mpsc::Receiver<Result<Option<Launched>, String>>, bool)>,
    tools: Vec<Rc<ToolInfo>>,
    problems: Vec<Problem>,
    watch_problem: Option<Problem>,
    _watcher: Option<RecommendedWatcher>,
    changes: mpsc::Receiver<()>,
    /// When the folder last changed, while a restart waits for it to be quiet.
    changed_at: Option<Instant>,
    changed_records: BTreeSet<InstanceId>,
    shown: BTreeSet<InstanceId>,
    told_shown: bool,
    drawn: HashMap<InstanceId, Drawn>,
    /// Why a view does not draw, by instance, until it draws again.
    view_problems: BTreeMap<InstanceId, Problem>,
    /// The edit of the action on a view that runs now: one step of undo, finished once it ran.
    acting: Option<ProjectEdit>,
    /// How many actions went to Bun that it did not say it ran.
    acts: usize,
    /// Whether a gesture over several elements is held, such as a paint: its actions are one
    /// step of undo, which ends once it is let go and they ran.
    held: bool,
    /// Why actions on views did nothing, oldest first, for whoever asked: `--act`, or a notice.
    failed_actions: Vec<String>,
    /// Whether Bun runs a frame of the control loops now.
    ticking: bool,
    told_transport: Option<Transport>,
    /// Where the code of the running Bun played: an instance's own voices and controls, or the
    /// `notes` of its child, by path. Released when that Bun ends.
    performed_on: BTreeSet<(InstanceId, Option<String>)>,
    /// How many times Buns loaded, or none could: the window reads its design tokens again.
    loads: u64,
    /// How many trees came, for a test that a frame runs no view.
    view_runs: u64,
    waker: Waker,
}

impl Tools {
    /// The tools of `extensions` for the window of `project`. `watched`: a change in
    /// `extensions/` starts a new Bun; a test tells it with [`Tools::code_changed`] instead.
    pub fn start(extensions: Extensions, project: &Project, watched: bool, waker: Waker) -> Self {
        let Extensions {
            folder,
            bun,
            opening,
            tools,
            skipped,
            problems,
        } = extensions;
        let (send_changes, changes) = mpsc::channel();
        let (watcher, watch_problem) = match watched {
            true => watching(&folder, send_changes, waker.clone()),
            false => (None, None),
        };
        let mut this = Self {
            folder,
            opening,
            skipped,
            bun: None,
            starting: None,
            tools: tools.into_iter().map(Rc::new).collect(),
            problems,
            watch_problem,
            _watcher: watcher,
            changes,
            changed_at: None,
            changed_records: every_instance(project),
            shown: BTreeSet::new(),
            told_shown: true,
            drawn: HashMap::new(),
            view_problems: BTreeMap::new(),
            acting: None,
            acts: 0,
            held: false,
            failed_actions: Vec::new(),
            ticking: false,
            told_transport: None,
            performed_on: BTreeSet::new(),
            loads: 0,
            view_runs: 0,
            waker,
        };
        if let Some(bun) = bun {
            this.listen(bun);
        }
        this
    }

    /// Hears `bun` from now on: what it says wakes the window.
    fn listen(&mut self, bun: Arc<Bun>) {
        bun.wake_with(self.waker.clone());
        self.bun = Some(bun);
        self.told_shown = false;
    }

    /// What the window shows of `id`, once a Bun said. `None` while one may still say: a Bun
    /// runs or starts.
    pub fn drawn(&self, id: &InstanceId) -> Option<&Drawn> {
        self.drawn.get(id)
    }

    /// Whether a view not drawn yet may still come, see [`Tools::drawn`].
    pub fn drawing(&self) -> bool {
        self.bun.is_some() || self.starting.is_some()
    }

    /// How many times Buns loaded: the design tokens may have changed with the code.
    pub fn loads(&self) -> u64 {
        self.loads
    }

    pub fn view_runs(&self) -> u64 {
        self.view_runs
    }

    /// What is wrong with `extensions/`.
    pub fn problems(&self) -> impl Iterator<Item = &Problem> {
        (self.problems.iter())
            .chain(&self.watch_problem)
            .chain(self.view_problems.values())
    }

    /// The instances whose views the window shows now.
    pub fn show(&mut self, instances: BTreeSet<InstanceId>) {
        if instances != self.shown {
            self.drawn.retain(|id, _| instances.contains(id));
            self.shown = instances;
            self.told_shown = false;
        }
    }

    /// Runs `action` of the element `key` of the view of `instance`, as a pointer or an agent
    /// does: what it changes is one step of undo. Why it did nothing comes from
    /// [`Tools::take_failed_actions`] once Bun ran it, or here when no Bun runs.
    pub fn act(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) -> Result<(), String> {
        let request = Request::Act {
            instance,
            key,
            action,
            value,
        };
        match &self.bun {
            Some(bun) if bun.send(&request) => {
                self.acts += 1;
                Ok(())
            }
            _ => Err(NOT_RUNNING.to_string()),
        }
    }

    /// Holds a gesture over several elements, such as a paint, or lets it go: the actions while
    /// it is held are one step of undo.
    pub fn hold(&mut self, project: &mut Project, held: bool) {
        self.held = held;
        if !held && self.acts == 0 {
            self.finish_action(project);
        }
    }

    /// The step of undo of the actions that ran ends.
    fn finish_action(&mut self, project: &mut Project) {
        if let Some(edit) = self.acting.take()
            && let Err(error) = project.finish(edit)
        {
            self.failed_actions.push(error.to_string());
        }
    }

    /// Why actions on views did nothing since the last call.
    pub fn take_failed_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failed_actions)
    }

    /// The fields of `tool`, a tool of the project, as a JSON Schema of its record: what a card
    /// with no view of its own shows a control for.
    pub fn schema(&self, tool: &str) -> Option<Value> {
        let info = find(&self.tools, tool)?;
        let properties: Map<String, Value> = (info.fields.0.iter())
            .filter_map(|(name, field)| Some((name.clone(), field_schema(field)?)))
            .collect();
        Some(json!({ "x-title": info.title, "properties": properties }))
    }

    /// Whether a new Bun waits for the folder to be quiet: the window polls again soon. One that
    /// loads wakes the window itself.
    pub fn waits(&self) -> bool {
        self.changed_at.is_some()
    }

    /// Whether a tool of an instance of `project` has a control loop: the window runs them every
    /// [`FRAME_INTERVAL`].
    pub fn loops(&self, project: &Project) -> bool {
        self.bun.is_some() && (project.instances()).any(|(_, tool)| self.ticks(tool))
    }

    fn ticks(&self, tool: &str) -> bool {
        find(&self.tools, tool).is_some_and(|info| info.tick)
    }

    /// One step of the control loop of every instance whose tool has one, `dt` seconds after
    /// the last, while the project plays at `tick` or stands there, at `time` seconds of engine
    /// time; also tells Bun the clock when what its code follows of it changed. Not while the
    /// last step runs.
    pub fn run_loops(
        &mut self,
        project: &Project,
        playing: bool,
        tick: Ticks,
        time: f64,
        dt: f32,
    ) -> Looped {
        let Some(bun) = self.bun.clone() else {
            return Looped::Nothing;
        };
        let tempo_map = &project.project_file().tempo_map;
        let at = tempo_map.time_signatures().bar_beat_of(tick);
        let clock = Clock {
            time,
            transport: Transport {
                playing,
                tick: tick.0,
                bar: at.bar,
                beat: at.beat,
                bpm: tempo_map.change_at(tick).bpm.bpm(),
                ..Transport::default()
            },
        };
        let told = self.told_transport.map(|told| told.followed());
        if told != Some(clock.transport.followed()) && bun.send(&Request::Clock { clock }) {
            self.told_transport = Some(clock.transport);
        }
        if self.ticking {
            return Looped::Busy;
        }
        let instances: Vec<Played> = (project.instances())
            .filter(|(_, tool)| self.ticks(tool))
            .map(|(id, _)| played(project, id))
            .collect();
        if instances.is_empty() {
            return Looped::Nothing;
        }
        // Its code reads the records as they are now.
        self.tell(project);
        self.ticking = bun.send(&Request::Frame {
            dt,
            instances,
            clock,
        });
        match self.ticking {
            true => Looped::Sent,
            false => Looped::Nothing,
        }
    }

    /// A change in `extensions/`, as the watcher says it.
    pub fn code_changed(&mut self) {
        self.changed_at = Some(Instant::now() - GROUPING_WINDOW);
    }

    /// Hears Bun and the folder, applies what code changed, and tells Bun what changed of the
    /// records. `events` are the project's since the last poll. Whether what it shows changed.
    pub fn poll(&mut self, project: &mut Project, events: &[ProjectEvent]) -> bool {
        for event in events {
            if let ProjectEvent::Created(id)
            | ProjectEvent::Changed(id)
            | ProjectEvent::Deleted(id) = event
            {
                self.changed_records.insert(id.clone());
            }
        }
        while self.changes.try_recv().is_ok() {
            self.changed_at = Some(Instant::now());
        }
        let mut changed = self.launched(project);
        if let Some(events) = self.bun.as_ref().map(|bun| bun.events()) {
            while let Ok(event) = events.try_recv() {
                changed |= self.heard(event, project);
            }
        }
        // Until it is quiet: an editor saves in several writes, an agent several files.
        if self
            .changed_at
            .is_some_and(|at| at.elapsed() >= GROUPING_WINDOW)
        {
            self.changed_at = None;
            self.skipped.clear();
            self.restart(project);
        }
        let (shown, problems) = (&self.shown, self.view_problems.len());
        self.view_problems.retain(|id, _| shown.contains(id));
        if self.view_problems.len() != problems {
            self.report(project);
        }
        self.tell(project);
        changed
    }

    /// Waits until Bun did all it was told and the window heard it, and polls, until nothing
    /// is left: what a run without a window does before each frame. Whether what it shows
    /// changed, or that Bun did not finish.
    pub fn settle(&mut self, project: &mut Project) -> Result<bool, String> {
        let started = Instant::now();
        let mut changed = false;
        loop {
            let left = SETTLE_TIMEOUT.saturating_sub(started.elapsed());
            let starting = match &mut self.starting {
                Some((launched, _)) => match launched.recv_timeout(left) {
                    Ok(launched) => {
                        self.starting = None;
                        changed |= self.loaded(launched, project);
                        true
                    }
                    Err(_) => false,
                },
                None => false,
            };
            // What it says for what it was told is queued before it says it is done.
            while self.bun.as_ref().is_some_and(|bun| !bun.is_done())
                && started.elapsed() < SETTLE_TIMEOUT
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            let events = project.drain_events();
            let polled = self.poll(project, &events);
            changed |= polled;
            let idle = self.bun.as_ref().is_none_or(|bun| bun.is_idle());
            if !polled && !starting && idle && self.starting.is_none() {
                return Ok(changed);
            }
            if started.elapsed() >= SETTLE_TIMEOUT {
                return Err(format!(
                    "the code of the project did not finish within {SETTLE_TIMEOUT:?}"
                ));
            }
        }
    }

    /// Ends Bun, if one runs, and starts a new one on the files as they are now.
    fn restart(&mut self, project: &mut Project) {
        if let Some((_, again)) = &mut self.starting {
            *again = true;
            return;
        }
        let old = self.bun.take();
        self.performer_gone(project);
        let (folder, opening, skipped) = (self.folder.clone(), self.opening, self.skipped.clone());
        let (send, launched) = mpsc::channel();
        let wake = self.waker.clone();
        let started = std::thread::Builder::new()
            .name("bun-start".into())
            .spawn(move || {
                if let Some(old) = old {
                    old.end();
                }
                send.send(crate::launch((&folder, opening), skipped, true))
                    .ok();
                wake();
            });
        match started {
            Ok(_) => self.starting = Some((launched, false)),
            Err(error) => self.problems.push(crate::folder_problem(error.to_string())),
        }
    }

    /// A new Bun that loaded, if one did.
    fn launched(&mut self, project: &mut Project) -> bool {
        let Some((receiver, again)) = &self.starting else {
            return false;
        };
        let Ok(launched) = receiver.try_recv() else {
            return false;
        };
        let again = *again;
        self.starting = None;
        if again {
            // Its tools are older than the folder.
            drop(launched);
            self.restart(project);
            return false;
        }
        self.loaded(launched, project)
    }

    /// Defines the tools of a new Bun again, and shows its views.
    fn loaded(
        &mut self,
        launched: Result<Option<Launched>, String>,
        project: &mut Project,
    ) -> bool {
        self.changed_records = every_instance(project);
        self.loads += 1;
        self.end_actions(project, "the code changed while it ran");
        let Launched {
            bun,
            sounds,
            skipped,
            mut problems,
        } = match launched {
            Ok(Some(launched)) => launched,
            Ok(None) => {
                self.forget_views();
                self.problems.clear();
                self.report(project);
                return true;
            }
            Err(message) => {
                self.forget_views();
                self.problems = vec![crate::folder_problem(message)];
                self.report(project);
                return true;
            }
        };
        self.skipped = skipped;
        let tools = crate::define(sounds, &mut problems, |tools| {
            let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
            match project.define_json_tools(tools) {
                Ok(refused) => (refused.into_iter())
                    .map(|(name, error)| (name, error.to_string()))
                    .collect(),
                Err(error) => names
                    .into_iter()
                    .map(|name| (name, error.to_string()))
                    .collect(),
            }
        });
        self.tools = tools.into_iter().map(Rc::new).collect();
        self.problems = problems;
        self.report(project);
        self.listen(bun);
        true
    }

    /// The Bun that ran the actions sent is gone: what they changed is one step, and those it
    /// did not say it ran did nothing.
    fn end_actions(&mut self, project: &mut Project, why: &str) {
        self.finish_action(project);
        let unanswered = std::mem::take(&mut self.acts);
        self.failed_actions
            .extend(std::iter::repeat_n(why.to_string(), unanswered));
        (self.ticking, self.told_transport) = (false, None);
    }

    /// No Bun draws the views now.
    fn forget_views(&mut self) {
        self.drawn.clear();
        self.view_problems.clear();
    }

    fn report(&self, project: &mut Project) {
        let problems = self.problems().cloned().collect();
        project.set_problems_in(&format!("{FOLDER}/"), problems);
    }

    /// One thing Bun said. Whether what the window shows changed.
    fn heard(&mut self, event: Event, project: &mut Project) -> bool {
        match event {
            Event::Tree {
                instance,
                tree,
                error,
            } => {
                if !self.shown.contains(&instance) {
                    return false;
                }
                let before = self.drawn.remove(&instance);
                let problem = (error.as_ref()).and_then(|error| {
                    self.problem_of(project, &instance, &format!("its view failed: {error}"))
                });
                let reported = match problem {
                    Some(problem) => self
                        .view_problems
                        .insert(instance.clone(), problem)
                        .is_none(),
                    None => self.view_problems.remove(&instance).is_some(),
                };
                if reported {
                    self.report(project);
                }
                let drawn = match (tree, error) {
                    (_, Some(error)) => Drawn::Failed(error),
                    (Some(tree), None) => {
                        self.view_runs += 1;
                        let bun = self.bun.as_ref().map_or(0, |bun| bun.number());
                        drawn(tree, before, bun)
                    }
                    (None, None) => Drawn::None,
                };
                self.drawn.insert(instance, drawn);
                true
            }
            Event::Acted { error } => {
                self.acts = self.acts.saturating_sub(1);
                if !self.held {
                    self.finish_action(project);
                }
                self.failed_actions.extend(error);
                false
            }
            Event::Framed => {
                self.ticking = false;
                false
            }
            Event::Edit {
                instance,
                label,
                changes,
                gesture,
            } => {
                self.edit(project, &instance, &label, changes, gesture);
                false
            }
            Event::Failed { tool, message } => {
                if let Some(info) = find(&self.tools, &tool) {
                    self.problems.push(info.problem(message));
                    self.report(project);
                }
                false
            }
            Event::Ended { blame, why } => {
                self.failed(blame, &why, project);
                true
            }
            Event::Control {
                instance,
                child,
                name,
                value,
                at,
            } => {
                let at = at.map(|at| frame_of(project, at));
                let played = self.played_on(project, &instance, child.as_deref());
                if let Ok(id) = &played {
                    self.performed_on.insert((id.clone(), None));
                }
                let sent = played.and_then(|id| {
                    let info = self.info_of(project, &id).ok_or_else(|| {
                        let tool = project.tool_of(&id).unwrap_or("nothing");
                        format!("has no live controls or triggers: it is {tool}")
                    })?;
                    let update = control_update(&info, &name, value, at)?;
                    (project.send::<Signals>(&id, info.processor(), update))
                        .map_err(|error| error.to_string())
                });
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Note {
                instance,
                child,
                pitch,
                velocity,
                seconds,
                at,
            } => {
                let performed = Performed::Play {
                    pitch: Pitch::nearest(i64::from(pitch)),
                    velocity: Velocity::nearest((velocity.clamp(0.0, 1.0) * 127.0).round() as i64),
                    at: performed_frame(project, at),
                    frames: frame_of(project, f64::from(seconds)),
                };
                let sent = self.play_note(project, &instance, child.as_deref(), performed);
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Send {
                instance,
                child,
                event,
                at,
            } => {
                // The SDK sends no controller from 120 on, MIDI's channel mode messages.
                let sent = match event.event() {
                    Some(event) => {
                        let at = performed_frame(project, at);
                        let performed = Performed::Send { event, at };
                        self.play_note(project, &instance, child.as_deref(), performed)
                    }
                    None => Err(format!("sends {event:?}, which is no note event")),
                };
                self.performed(project, &instance, child.as_deref(), sent);
                false
            }
            Event::Transport { op } => {
                self.transport(project, op);
                false
            }
            Event::OpenMenu { question, .. } => {
                self.unanswered(project, question, Answer::Item(None), "a menu")
            }
            Event::Confirm { question, .. } => {
                self.unanswered(project, question, Answer::Confirmed(false), "a question")
            }
            Event::OpenFile { question, .. } | Event::Render { question, .. } => {
                self.unanswered(project, question, Answer::Asset(None), "a file")
            }
            Event::ExportNotes { question, .. } => {
                self.unanswered(project, question, Answer::Confirmed(false), "an export")
            }
            Event::Offers { question, .. } => {
                let answer = Answer::Offers {
                    offers: Vec::new(),
                    notes: Vec::new(),
                    here: None,
                };
                self.unanswered(project, question, answer, "the devices")
            }
            Event::Numbers { question, .. } => {
                self.unanswered(project, question, Answer::Numbers(Vec::new()), "numbers")
            }
            _ => false,
        }
    }

    /// A number field of `instance`, or of its child at the path `child`, as it plays at `tick`:
    /// what a lane plays into it, else its record, at its default where the record leaves it out.
    pub fn field_now(
        &self,
        project: &Project,
        instance: &InstanceId,
        child: Option<&str>,
        name: &str,
        tick: Ticks,
    ) -> f64 {
        let id = match child {
            Some(path) => instance.child(path).ok(),
            None => Some(instance.clone()),
        };
        let Some(id) = id else {
            return 0.0;
        };
        let at = crate::feeds::Moment::at(tick);
        if let Some((value, _)) = crate::feeds::lane_value(project, &id, name, at) {
            return f64::from(value);
        }
        let state = project.state_json(&id);
        let state = state.and_then(|state| serde_json::from_str::<Value>(&state).ok());
        let state = match (state, self.info_of(project, &id)) {
            (Some(state), Some(info)) => Some(info.with_defaults(state)),
            (state, _) => state,
        };
        let value = (state.as_ref())
            .and_then(|state| crate::path::at(state, name).and_then(crate::tree::number_of));
        value.map_or(0.0, f64::from)
    }

    fn info_of(&self, project: &Project, id: &InstanceId) -> Option<Rc<ToolInfo>> {
        (project.tool_of(id))
            .and_then(|tool| find(&self.tools, tool))
            .cloned()
    }

    /// The instance a performer of `instance` plays: itself, or its child at the path `child`,
    /// never one outside it.
    fn played_on(
        &self,
        project: &Project,
        instance: &InstanceId,
        child: Option<&str>,
    ) -> Result<InstanceId, String> {
        let Some(child) = child else {
            return Ok(instance.clone());
        };
        InstanceId::new(&format!("{instance}/{child}"))
            .ok()
            .filter(|id| id.is_inside(instance) && project.tool_of(id).is_some())
            .ok_or_else(|| "is not one it owns".to_string())
    }

    /// Plays a key of `instance`, or lets go of one, as the keyboard would: on its own voices,
    /// or into the `notes` of its child `child` through the feed it declares.
    fn play_note(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        child: Option<&str>,
        performed: Performed,
    ) -> Result<(), String> {
        (self.performed_on).insert((instance.clone(), child.map(str::to_string)));
        let error = |error: sound_core::ProjectError| error.to_string();
        let Some(child) = child else {
            let info = (self.info_of(project, instance)).ok_or("has no voices to play")?;
            let update = SignalsUpdate::Note(performed);
            return (project.send::<Signals>(instance, info.processor(), update)).map_err(error);
        };
        let played = self.played_on(project, instance, Some(child))?;
        if project.input_port(&played, NOTES_INPUT).is_none() {
            return Err("has no `notes` input".to_string());
        }
        if feed_child(project.assets(), instance, child) {
            project.rebind(instance).map_err(error)?;
        }
        let (feed, update) = (feed_processor(child), NoteFeedUpdate::Note(performed));
        (project.send::<NoteFeed>(instance, &feed, update)).map_err(error)
    }

    /// Tells each place the code of the Bun that ended played on that it is gone, so what it
    /// held lets go. A place deleted since has nothing left to release.
    fn performer_gone(&mut self, project: &mut Project) {
        for (id, child) in std::mem::take(&mut self.performed_on) {
            let gone = match &child {
                None => match self.info_of(project, &id) {
                    Some(info) => {
                        project.send::<Signals>(&id, info.processor(), SignalsUpdate::PerformerGone)
                    }
                    None => Ok(()),
                },
                Some(child) => project.send::<NoteFeed>(
                    &id,
                    &feed_processor(child),
                    NoteFeedUpdate::PerformerGone,
                ),
            };
            match gone {
                Ok(()) | Err(sound_core::ProjectError::MissingProcessor { .. }) => {}
                Err(error) => self.problems.push(crate::folder_problem(error.to_string())),
            }
        }
    }

    /// What code of `instance` played could not be played: a problem of its tool, once, as its
    /// types do not check it.
    fn performed(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        child: Option<&str>,
        sent: Result<(), String>,
    ) {
        let Err(error) = sent else {
            return;
        };
        let message = match child {
            Some(child) => format!("a performer plays the child {child:?}, which {error}"),
            None => format!("a performer plays it, but it {error}"),
        };
        if let Some(problem) = self.problem_of(project, instance, &message)
            && !self.problems.contains(&problem)
        {
            self.problems.push(problem);
            self.report(project);
        }
    }

    /// Code played, stopped or moved the clock.
    fn transport(&mut self, project: &mut Project, op: TransportOp) {
        let engine = project.engine();
        match op {
            TransportOp::Play => engine.play(),
            TransportOp::Pause => engine.pause(),
            TransportOp::Stop => engine.stop(),
            TransportOp::Jump { to, at: None } => engine.seek(Ticks(to)),
            TransportOp::Jump { to, at: Some(at) } => engine.jump_at(Ticks(at), Ticks(to)),
            TransportOp::Repeat { range } => {
                engine.set_loop(range.map(|[from, to]| Ticks(from)..Ticks(to)));
            }
            TransportOp::Click { .. }
            | TransportOp::Keys { .. }
            | TransportOp::Automation { .. } => {
                let problem = crate::folder_problem(
                    "code turns the click, the computer keys or the writing of automation, which this window does not have yet".to_string(),
                );
                if !self.problems.contains(&problem) {
                    self.problems.push(problem);
                    self.report(project);
                }
            }
        }
    }

    /// Code asked for `what`, which this window has no way to ask yet: it gets nothing, as when
    /// the composer says no, so its code does not wait for ever.
    fn unanswered(
        &mut self,
        project: &mut Project,
        question: u64,
        answer: Answer,
        what: &str,
    ) -> bool {
        if let Some(bun) = &self.bun {
            bun.send(&Request::Answer { question, answer });
        }
        let problem = crate::folder_problem(format!(
            "code asks for {what}, which this window does not have yet, so it got none"
        ));
        if !self.problems.contains(&problem) {
            self.problems.push(problem);
            self.report(project);
        }
        false
    }

    /// A problem of the tool of `instance`, if it is one of the project's.
    fn problem_of(
        &self,
        project: &Project,
        instance: &InstanceId,
        message: &str,
    ) -> Option<Problem> {
        let info = project
            .tool_of(instance)
            .and_then(|tool| find(&self.tools, tool))?;
        Some(info.problem(message.to_string()))
    }

    /// Code changed records, as one edit: with the others of the action on a view that runs, as
    /// one step of undo, or else into a run of its label.
    fn edit(
        &mut self,
        project: &mut Project,
        instance: &InstanceId,
        label: &str,
        edits: Vec<RecordEdit>,
        gesture: bool,
    ) {
        let info = project
            .tool_of(instance)
            .and_then(|tool| find(&self.tools, tool))
            .cloned();
        let refused = match info.map(|info| changes_of(project, &info, instance, edits)) {
            None => None,
            Some(Err(refused)) => Some(refused),
            Some(Ok((changes, touched))) => {
                self.changed_records.extend(touched);
                let published = match gesture {
                    true => {
                        let edit = self.acting.get_or_insert_with(|| project.begin(label));
                        project.publish(edit, changes)
                    }
                    false => project.publish_run(instance, label, changes),
                };
                published.err().map(|error| error.to_string())
            }
        };
        if let Some(bun) = &self.bun {
            bun.heard_edit();
            if refused.is_some() {
                bun.tell([]);
            }
        }
        let Some(refused) = refused else {
            return;
        };
        let problem =
            format!("its update {label:?} was refused, and none of it applied: {refused}");
        if gesture {
            self.failed_actions.push(problem);
        } else if let Some(problem) = self.problem_of(project, instance, &problem) {
            self.problems.push(problem);
            self.report(project);
        }
    }

    /// Bun hung or stopped: the file of the tool whose code ran then runs no more until the
    /// folder changes. With nothing to blame, no Bun runs until then.
    fn failed(&mut self, blame: Option<Runs>, why: &str, project: &mut Project) {
        let info = blame
            .as_ref()
            .and_then(|runs| find(&self.tools, &runs.tool).cloned());
        self.end_actions(project, &format!("the code of the project {why}"));
        match (info, blame) {
            (Some(info), Some(runs)) => {
                let problem = info.problem(format!(
                    "its {} {why}, so the tools run without this file: fix it and save",
                    runs.what
                ));
                self.skipped.insert(info.file.clone(), problem.message);
                self.restart(project);
            }
            _ => {
                self.bun = None;
                self.performer_gone(project);
                // A card of fields works without Bun.
                self.forget_views();
                let message = format!(
                    "the code of the project {why} with no tool to blame, so the tools do not run: fix them and save"
                );
                self.problems.push(crate::folder_problem(message));
                self.report(project);
            }
        }
    }

    /// Tells Bun the records that changed since it was last told, and what the window shows.
    fn tell(&mut self, project: &Project) {
        let Some(bun) = &self.bun else {
            return;
        };
        let ids = std::mem::take(&mut self.changed_records);
        let records: Vec<(&InstanceId, Option<(&str, String)>)> = (ids.iter())
            .map(|id| {
                let tool = project
                    .tool_of(id)
                    .filter(|_| told_to_bun(project, &self.tools, id));
                (
                    id,
                    tool.and_then(|tool| Some((tool, project.state_json(id)?))),
                )
            })
            .collect();
        let telling = records.iter().map(|(id, record)| -> Telling {
            (
                id,
                (record.as_ref())
                    .map(|(tool, state)| (*tool, record_key(tool, state), state.as_str())),
            )
        });
        if !bun.tell(telling) {
            drop(records);
            self.changed_records = ids;
            return;
        }
        if !self.told_shown {
            let instances = self.shown.iter().collect();
            self.told_shown = bun.send(&Request::Show { instances });
        }
    }
}

/// A tree as Bun `bun` sends it, with the memos the window kept from `before` that it still
/// uses. A memo's version holds the number of its Bun, whose builds count from one again: an
/// older Bun's memos are not this one's, which sends every memo it uses.
fn drawn(mut tree: Value, before: Option<Drawn>, bun: u64) -> Drawn {
    let mut memos = match before {
        Some(Drawn::View { memos, .. }) => memos,
        _ => HashMap::new(),
    };
    memos.retain(|_, ((number, _), _)| *number == bun);
    if let Some(Value::Array(dropped)) = tree.get_mut("drop").map(Value::take) {
        for key in dropped.iter().filter_map(Value::as_str) {
            memos.remove(key);
        }
    }
    if let Some(Value::Object(fresh)) = tree.get_mut("memos").map(Value::take) {
        for (key, mut memo) in fresh {
            let version = memo.get("v").and_then(Value::as_u64).unwrap_or_default();
            memos.insert(key, ((bun, version), Rc::new(memo["node"].take())));
        }
    }
    let root = Rc::new(tree.get_mut("root").map(Value::take).unwrap_or_default());
    Drawn::View { root, memos }
}

/// The frame of engine time at `seconds` of it.
fn frame_of(project: &Project, seconds: f64) -> u64 {
    let rate = f64::from(project.clock().sample_rate());
    (seconds.max(0.0) * rate).round() as u64
}

/// The frame of engine time at `at`, or now when that is gone or left out.
fn performed_frame(project: &mut Project, at: Option<f64>) -> u64 {
    let rate = f64::from(project.clock().sample_rate());
    let now = project.engine().poll().map_or(0, |status| status.frames) as f64 / rate;
    frame_of(project, at.map_or(now, |at| at.max(now)))
}

/// A field of a tool of the project as a JSON Schema says it: what a card with no view shows.
fn field_schema(field: &Field) -> Option<Value> {
    let choice = |choice: &Choice| match choice {
        Choice::Number(number) => json!(number),
        Choice::Word(word) => json!(word),
    };
    Some(match field {
        Field::Number {
            min,
            max,
            integer,
            default,
            label,
            ..
        } => {
            let kind = if *integer { "integer" } else { "number" };
            json!({ "type": kind, "minimum": min, "maximum": max, "default": default.unwrap_or(*min), "title": label })
        }
        Field::Boolean { default, label } => {
            json!({ "type": "boolean", "default": default, "title": label })
        }
        Field::Choice {
            options,
            default,
            label,
        } => {
            json!({ "enum": options.iter().map(choice).collect::<Vec<_>>(), "default": choice(default), "title": label })
        }
        Field::Text { .. } | Field::File { .. } | Field::List { .. } | Field::Object { .. } => {
            return None;
        }
    })
}

/// Watches `extensions/` and says on `changed` when code changed, waking the window.
fn watching(
    folder: &Path,
    changed: mpsc::Sender<()>,
    wake: Waker,
) -> (Option<RecommendedWatcher>, Option<Problem>) {
    let (send, heard) = smol::channel::unbounded();
    let passed = std::thread::Builder::new()
        .name("views-watch".into())
        .spawn(move || {
            while heard.recv_blocking().is_ok() {
                if changed.send(()).is_err() {
                    return;
                }
                wake();
            }
        });
    if let Err(error) = passed {
        return (None, Some(crate::folder_problem(error.to_string())));
    }
    watch(folder, send)
}
