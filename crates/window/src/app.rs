//! The window of one project: a title row (its name, play and stop, where it plays), and the
//! view of its main instance filling the rest. In a run without a window the engine plays
//! offline, a step per frame, and Bun answers before each frame, so a run is always the same.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use rune::{Cx, El, KeyEvent, Tx, col, el, row, text};
use serde_json::{Value, json};
use sound_core::{Engine, GROUPING_WINDOW, InstanceId, OutputDevice, OutputStream, Project, Ticks};
use sound_typescript::next::{Drawn, FRAME_INTERVAL, Looped, Tools};

use crate::card::Kind;
use crate::controls::{self, Step, Target};
use crate::lives::Lives;
use crate::theme::Theme;
use crate::tree;

/// How often a playing window draws: what moves, moves at 60 frames a second.
const FRAME: Duration = Duration::from_millis(16);
/// How often an idle window looks at what has no way to wake it: plugins, and sounds that load.
const IDLE: Duration = Duration::from_millis(250);
const TITLE_ROW: f32 = 44.0;
/// The room the title row leaves at its left for the window's buttons, which macOS draws on it.
const TITLE_LEFT: f32 = if cfg!(target_os = "macos") {
    88.0
} else {
    16.0
};
/// What the composer reads when a control did nothing: why is for the agent, in `--act`.
const DID_NOTHING: &str = "That did nothing: this tool could not do it.";
/// Where macOS draws the window's buttons: in the middle of the title row.
pub(crate) const WINDOW_BUTTONS: (f32, f32) = (16.0, 15.0);

/// The template of a new project: the DAW with views of `sdk/view.ts`.
pub(crate) const TEMPLATE: runtime::Template = runtime::Template {
    name: "DAW (new window)",
    description: "Tracks of clips, the devices of a track and a mixer.",
    files: include!(concat!(env!("OUT_DIR"), "/template.rs")),
};

/// What the command line asked of the window.
pub(crate) struct Options {
    pub(crate) folder: PathBuf,
    /// Without a window: `--snap`, `--tree` or `--script`.
    pub(crate) headless: bool,
    /// Plays from the start for this many seconds before the picture.
    pub(crate) play: Option<f64>,
    /// The steps of `--act`, which runs in the folder itself: what they change is the point.
    pub(crate) act: Option<Vec<Step>>,
}

/// What a run without a window says after Rune wrote its tree, and what failed in it.
pub(crate) static REPORT: Mutex<Option<Value>> = Mutex::new(None);
pub(crate) static FAILED: Mutex<Option<String>> = Mutex::new(None);

/// The engine: a device's, or offline, stepped by the frames of a run without a window.
enum Sound {
    Device(#[allow(dead_code)] OutputStream),
    Offline { engine: Box<Engine>, played: f64 },
}

pub(crate) struct State {
    name: String,
    pub(crate) project: Project,
    sound: Sound,
    plugins: Plugins,
    pub(crate) tools: Option<Tools>,
    pub(crate) theme: Theme,
    /// The JSON Schema of each built-in tool's record, by tool name.
    schemas: HashMap<&'static str, Value>,
    pub(crate) lives: Lives,
    pub(crate) playing: bool,
    pub(crate) playhead: Ticks,
    /// Engine time in seconds, which a control loop's clock counts on.
    engine_time: f64,
    /// When the control loops last ran, in seconds of the window's clock.
    looped: f64,
    /// How many Buns loaded when the design tokens were last read.
    theme_loads: u64,
    /// What went wrong last, said in the corner.
    notice: Option<String>,
    shown: BTreeSet<InstanceId>,
    waker: Arc<Mutex<Option<rune::Waker>>>,
    /// The steps of `--act` and the seconds of `--play`, for the first frame of a run without a window.
    first: Option<(Vec<Step>, Option<f64>)>,
    last: f64,
    /// A run without a window opens a copy, so it writes nothing and works next to a window.
    _copy: Option<tempfile::TempDir>,
}

impl State {
    pub(crate) fn open(options: Options) -> Result<Self> {
        let Options {
            folder,
            headless,
            play,
            act,
        } = options;
        // Whole, so `.` and `..` have a name: a folder that is not there yet becomes a project.
        let folder = dunce::canonicalize(&folder).or_else(|_| std::path::absolute(&folder))?;
        let name = folder
            .file_name()
            .context("the project folder has no name")?
            .to_string_lossy()
            .into_owned();
        let (folder, copy) = match (headless, act.is_none()) {
            (true, true) => {
                let (copy, folder) = copied(&folder, &name)?;
                (folder, Some(copy))
            }
            _ => (folder, None),
        };
        if !folder.join(runtime::PROJECT_FILE).exists() {
            runtime::templates::write(&folder, &TEMPLATE)?;
        }
        let waker: Arc<Mutex<Option<rune::Waker>>> = Arc::default();
        let (plugins, sound, control) = match headless {
            true => {
                let plugins =
                    Plugins::listing(Vec::new(), ScanCommand::this_program()?, ScanCache::none());
                let (control, engine) = Engine::new(runtime::OFFLINE);
                (
                    plugins,
                    Sound::Offline {
                        engine: Box::new(engine),
                        played: 0.0,
                    },
                    control,
                )
            }
            false => {
                let device = OutputDevice::default_output()?;
                let config = sound_core::EngineConfig::new(device.sample_rate(), device.channels());
                let (control, engine) = Engine::new(config);
                (
                    runtime::playing_plugins(true)?,
                    Sound::Device(device.start(engine)?),
                    control,
                )
            }
        };
        let (mut project, extensions) =
            runtime::open_with_extensions(&folder, control, plugins.clone())?;
        let wake = waker.clone();
        let wake: sound_typescript::next::Waker = Arc::new(move || {
            if let Some(wake) = wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                wake();
            }
        });
        if !headless {
            project.watch()?;
        }
        let tools =
            extensions.map(|extensions| Tools::start(extensions, &project, !headless, wake));
        let (theme, theme_problem) = Theme::of(&folder);
        let theme_loads = tools.as_ref().map_or(0, Tools::loads);
        let schemas = (runtime::tool_schemas()?.into_iter())
            .map(|schema| (schema.tool, schema.schema))
            .collect();
        Ok(Self {
            name,
            project,
            sound,
            plugins,
            tools,
            theme,
            schemas,
            lives: Lives::new("1.1"),
            playing: false,
            playhead: Ticks(0),
            engine_time: 0.0,
            looped: 0.0,
            theme_loads,
            notice: theme_problem,
            shown: BTreeSet::new(),
            waker,
            first: headless.then(|| (act.unwrap_or_default(), play)),
            last: 0.0,
            _copy: copy,
        })
    }

    /// The instance the window shows, as `project.json` names it now.
    pub(crate) fn main(&self) -> Option<&InstanceId> {
        self.project.project_file().main.as_ref()
    }

    /// What the window shows of `id`, once Bun said.
    pub(crate) fn drawn(&self, id: &InstanceId) -> Option<&Drawn> {
        self.tools.as_ref()?.drawn(id)
    }

    /// Whether the view of `id` is still to come: the window shows nothing of it until then,
    /// rather than a card of its fields for a moment.
    pub(crate) fn pending(&self, id: &InstanceId) -> bool {
        (self.tools.as_ref()).is_some_and(|tools| tools.drawn(id).is_none() && tools.drawing())
    }

    fn headless(&self) -> bool {
        matches!(self.sound, Sound::Offline { .. })
    }

    /// The JSON Schema of the records of `tool`: a tool of the project's, else a built-in one's.
    pub(crate) fn schema(&self, tool: &str) -> Option<Value> {
        (self.tools.as_ref())
            .and_then(|tools| tools.schema(tool))
            .or_else(|| self.schemas.get(tool).cloned())
    }

    /// What the keys of the elements of `id` start with: nothing for the main view, its path
    /// from there and `:` for another.
    pub(crate) fn prefix_of(&self, id: &InstanceId) -> String {
        let inside = (self.main())
            .and_then(|main| id.as_str().strip_prefix(main.as_str())?.strip_prefix('/'));
        match (Some(id) == self.main(), inside) {
            (true, _) => String::new(),
            (false, Some(path)) => format!("{path}:"),
            (false, None) => format!("{id}:"),
        }
    }

    /// An action on an element of a view: its tool's code runs it.
    pub(crate) fn act_on_view(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let tools = (self.tools.as_mut()).context("the project has no code to run this")?;
        tools
            .act(instance, key, action, value)
            .map_err(|why| anyhow!(why))
    }

    /// [`Self::act_on_view`] by a pointer: what goes wrong is said in the corner.
    pub(crate) fn pointed(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) {
        if self.act_on_view(instance, key, action, value).is_err() {
            self.notice = Some(DID_NOTHING.to_string());
        }
        self.handled();
    }

    /// Input was handled. A run without a window answers it at once, so the frame it came in
    /// draws it and the report says it: Rune runs input after `poll`, and the last frame has none after.
    fn handled(&mut self) {
        if self.headless() {
            self.settle();
            *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.report());
        }
    }

    /// Why the last actions on views did nothing.
    pub(crate) fn take_failed_action(&mut self) -> Option<String> {
        let failed = self.tools.as_mut()?.take_failed_actions();
        failed.into_iter().last()
    }

    pub(crate) fn perform(
        &mut self,
        target: &Target,
        action: &str,
        value: Option<Value>,
    ) -> Result<()> {
        match target {
            Target::Play => self.play_or_stop(),
            Target::View { instance, key } => {
                let value = match action {
                    "drag" => value.or_else(|| Some(json!({ "dx": 0, "dy": 0 }))),
                    _ => value,
                };
                self.act_on_view(instance, key, action, value)?;
            }
            Target::Field {
                instance,
                field,
                option,
            } => {
                let now = field.value.as_f64().unwrap_or_default();
                let step = match field.kind {
                    Kind::Number { min, max, .. } => (max - min).abs() / 100.0,
                    _ => 1.0,
                };
                let next = match (action, option) {
                    (_, Some(option)) => option.clone(),
                    ("press" | "toggle", None) => json!(!field.value.as_bool().unwrap_or(false)),
                    ("set", None) => {
                        let value = value
                            .as_ref()
                            .and_then(Value::as_f64)
                            .ok_or_else(|| anyhow!("set takes a number"))?;
                        field.kind.fit(value)
                    }
                    // A step of an integer is one at least.
                    ("increment", None) => field.kind.fit(now + step.max(field_unit(&field.kind))),
                    ("decrement", None) => field.kind.fit(now - step.max(field_unit(&field.kind))),
                    (other, None) => bail!("a field takes no {other}"),
                };
                self.set_field(instance, &field.name, next);
            }
        }
        Ok(())
    }

    /// Changes one field of the record of `id`: one step of undo.
    pub(crate) fn set_field(&mut self, id: &InstanceId, field: &str, value: Value) {
        let label = format!("Change {}", field.replace('_', " "));
        let mut edit = self.project.begin(&label);
        let published = self
            .project
            .update_json(&mut edit, id, |state| state[field] = value);
        let finished = published.and_then(|()| self.project.finish(edit));
        if let Err(error) = finished {
            self.notice = Some(error.to_string());
        }
        self.handled();
    }

    fn play_or_stop(&mut self) {
        match self.playing {
            true => self.project.engine().stop(),
            false => self.project.engine().play(),
        }
    }

    pub(crate) fn undo(&mut self) -> Result<()> {
        self.project.undo()?;
        Ok(())
    }

    pub(crate) fn redo(&mut self) -> Result<()> {
        self.project.redo()?;
        Ok(())
    }

    /// Plays the engine offline up to `seconds`, in blocks as a device plays them.
    fn play_offline(&mut self, seconds: f64) -> Result<()> {
        let Sound::Offline { engine, played } = &mut self.sound else {
            return Ok(());
        };
        let rate = f64::from(engine.sample_rate());
        let channels = engine.channels();
        let mut frames = ((seconds - *played) * rate).round().max(0.0) as usize;
        *played += frames as f64 / rate;
        let mut buffer = vec![0.0_f32; 512 * channels];
        while frames > 0 {
            let size = frames.min(512);
            for problem in runtime::render_block(
                &mut self.project,
                engine,
                &self.plugins,
                &mut buffer[..size * channels],
            )? {
                self.notice = Some(problem.to_string());
            }
            frames -= size;
        }
        Ok(())
    }

    /// Shows what the views name, and waits for their trees in a run without a window.
    fn discover(&mut self, settle: bool) -> bool {
        let mut changed = false;
        loop {
            let shown = controls::collect(self).shown;
            if shown == self.shown {
                return changed;
            }
            self.lives.keep(&shown);
            self.shown = shown.clone();
            let Some(tools) = &mut self.tools else {
                return changed;
            };
            tools.show(shown);
            if !settle {
                return true;
            }
            changed |= self.settle_tools();
        }
    }

    /// Waits for Bun; a run without a window fails when it does not finish.
    fn settle_tools(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        tools.settle(&mut self.project).unwrap_or_else(|why| {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
            true
        })
    }

    /// The first frame of a run without a window: the steps of `--act`, then `--play`.
    fn first_frame(&mut self) -> Result<()> {
        let Some((steps, play)) = self.first.take() else {
            return Ok(());
        };
        for step in steps {
            step.run(self)?;
        }
        if play.is_some() {
            self.project.engine().play();
        }
        Ok(())
    }

    pub(crate) fn settle(&mut self) -> bool {
        self.settle_tools() | self.discover(true)
    }

    /// What `--tree` adds for an agent: the controls, where the piece plays and the meters.
    fn report(&self) -> Value {
        let controls = controls::collect(self).controls;
        let problems: Vec<String> = self
            .project
            .problems()
            .iter()
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect();
        json!({
            "controls": controls,
            "playing": self.playing,
            "playhead": self.playhead.0,
            "time": self.time(),
            "meters": self.lives.levels().into_iter().map(|(name, db)| (name, json!(db))).collect::<serde_json::Map<_, _>>(),
            "view_runs": self.tools.as_ref().map_or(0, Tools::view_runs),
            "problems": problems,
            "notice": self.notice,
        })
    }

    fn time(&self) -> String {
        let at = self
            .project
            .clock()
            .tempo_map()
            .time_signatures()
            .bar_beat_of(self.playhead);
        format!("{}.{}", at.bar, at.beat)
    }
}

/// A copy of the project in `folder`, for a run without a window: opening writes into a folder.
/// The scratch folder, and the copy in it, called `name`.
fn copied(folder: &Path, name: &str) -> Result<(tempfile::TempDir, PathBuf)> {
    let scratch = tempfile::tempdir()?;
    let copy = scratch.path().join(name);
    if folder.join(runtime::PROJECT_FILE).is_file() {
        copy_folder(folder, &copy)?;
    }
    Ok((scratch, copy))
}

/// Copies `from` to `to`, without hidden files, such as `.git` and the lock, or links.
fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("could not read {}", from.display()))?
    {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.')
            || entry.file_name() == "node_modules"
        {
            continue;
        }
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_folder(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)
                .with_context(|| format!("could not copy {}", source.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn setup(state: &mut State, waker: rune::Waker) {
    *state.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
}

/// The start of every frame: Bun, the project, the engine and what moves.
pub(crate) fn poll(state: &mut State, seconds: f64, tx: &mut Tx) -> bool {
    let (changed, next) = frame(state, seconds);
    if let Some(next) = next {
        tx.wake_after(next);
    }
    tx.instant();
    changed
}

/// One frame `seconds` after the start. Whether what the window shows changed, and, in a
/// window, when the next frame is due: Bun, the watchers and the engine wake it sooner.
pub(crate) fn frame(state: &mut State, seconds: f64) -> (bool, Option<Duration>) {
    let headless = state.headless();
    let mut changed = false;
    if headless {
        changed |= state.settle();
        if let Err(error) = state.first_frame() {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("{error:#}"));
        }
        if let Err(error) = state.play_offline(seconds) {
            state.notice = Some(error.to_string());
        }
    }
    if let Err(error) = state.project.poll() {
        state.notice = Some(error.to_string());
    }
    let (problems, errors) = runtime::tick(&mut state.project, &state.plugins);
    if let Some(problem) = problems.first() {
        state.notice = Some(problem.to_string());
    }
    if let Some(error) = errors.first() {
        state.notice = Some(error.to_string());
    }
    let events = state.project.drain_events();
    changed |= !events.is_empty();
    if let Some(tools) = &mut state.tools {
        let drawn = tools.poll(&mut state.project, &events);
        if drawn {
            state.lives.drop_drags();
        }
        changed |= drawn;
    }
    // What the views name changes only with what they show.
    if changed {
        changed |= state.discover(headless);
    }
    if let Ok(status) = state.project.engine().poll() {
        changed |= status.playing != state.playing;
        (state.playing, state.playhead) = (status.playing, status.playhead_tick);
        state.engine_time = status.frames as f64 / f64::from(state.project.clock().sample_rate());
    }
    if run_loops(state, seconds) && headless {
        changed |= state.settle();
    }
    if let Some(tools) = &mut state.tools
        && !tools.take_failed_actions().is_empty()
    {
        state.notice = Some(DID_NOTHING.to_string());
    }
    if let Some(tools) = &mut state.tools {
        if tools.loads() != state.theme_loads {
            state.theme_loads = tools.loads();
            let (theme, problem) = Theme::of(state.project.root());
            (state.theme, state.notice) = (theme, problem.or(state.notice.take()));
            changed = true;
        }
    }
    let dt = (seconds - state.last).max(0.0);
    state.last = seconds;
    let moving = state.lives.update(state, dt);
    if headless {
        *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(state.report());
        return (changed, None);
    }
    let tools = state.tools.as_ref();
    let mut next = if state.playing || moving { FRAME } else { IDLE };
    if tools.is_some_and(|tools| tools.loops(&state.project)) {
        // Overdue only while Bun runs the last step: it wakes the window when done.
        let left = FRAME_INTERVAL.saturating_sub(Duration::from_secs_f64(seconds - state.looped));
        next = next.min(if left.is_zero() { FRAME_INTERVAL } else { left });
    }
    if tools.is_some_and(Tools::waits) {
        next = next.min(GROUPING_WINDOW + FRAME);
    }
    (changed, Some(next))
}

/// Runs the control loops when their interval passed on the window's clock, the virtual one of
/// a run without a window. A step Bun is still busy with keeps its time for the next. Whether a
/// step went to Bun.
fn run_loops(state: &mut State, seconds: f64) -> bool {
    let dt = seconds - state.looped;
    let Some(tools) = &mut state.tools else {
        return false;
    };
    if dt < FRAME_INTERVAL.as_secs_f64() {
        return false;
    }
    let looped = tools.run_loops(
        &state.project,
        state.playing,
        state.playhead,
        state.engine_time,
        dt as f32,
    );
    if looped != Looped::Busy {
        state.looped = seconds;
    }
    looped == Looped::Sent
}

/// The least a step of an integer field moves.
fn field_unit(kind: &Kind) -> f64 {
    match kind {
        Kind::Number { integer: true, .. } => 1.0,
        _ => 0.0,
    }
}

pub(crate) fn on_key(state: &mut State, key: &KeyEvent, _: &mut Tx) -> bool {
    let done = if key.is_command('z') {
        match key.mods.shift {
            true => state.redo(),
            false => state.undo(),
        }
    } else if matches!(&key.key, rune::Key::Text(space) if space == " ") && !key.mods.cmd {
        state.play_or_stop();
        Ok(())
    } else {
        return false;
    };
    if let Err(error) = done {
        state.notice = Some(error.to_string());
    }
    state.handled();
    true
}

pub(crate) fn view(state: &State, _: &mut Cx<State>) -> El {
    let theme = &state.theme;
    let body = match state.main() {
        Some(main) => match state.drawn(main) {
            Some(drawn @ (Drawn::View { .. } | Drawn::Failed(_))) => {
                tree::view(state, main, "", drawn)
            }
            _ if state.pending(main) => el(),
            _ => el()
                .padding(theme.space(4))
                .child(crate::card::card(state, main)),
        },
        None => text("This project has no main tool.")
            .font(theme.text("body"))
            .color(theme.color("text-muted"))
            .padding(theme.space(4)),
    };
    let notice = (state.notice.iter()).map(|notice| {
        text(notice.clone())
            .font(theme.text("label"))
            .color(theme.color("peach"))
            .absolute(theme.space(3), 0.0)
            .pin(None, None, Some(theme.space(3)), Some(theme.space(2)))
    });
    col()
        .w_full()
        .h_full()
        .fill(theme.color("background"))
        .child(title_row(state))
        .child(el().grow(1.0).w_full().clip_soft(0.0).child(body))
        .children(notice)
}

fn title_row(state: &State) -> El {
    let theme = &state.theme;
    let label = if state.playing { "Stop" } else { "Play" };
    let play = row()
        .key("play")
        .role("button")
        .label(label)
        .focusable(true)
        .h(28.0)
        .px(theme.space(3))
        .items_center()
        .radius(theme.radius("md").unwrap_or(6.0))
        .fill(theme.color(if state.playing { "green" } else { "fill" }))
        .on_press(|s: &mut State| {
            s.play_or_stop();
            s.handled();
        })
        .child(
            text(if state.playing { "■" } else { "▶" })
                .font(theme.text("label"))
                .color(theme.color(if state.playing {
                    "text-inverse"
                } else {
                    "text"
                })),
        );
    // The tools' problems are the project's too.
    let problems = state.project.problems().len();
    let warning = (problems > 0).then(|| {
        text(format!("{problems} not live"))
            .font(theme.text("label"))
            .color(theme.color("peach"))
    });
    row()
        .w_full()
        .h(TITLE_ROW)
        .px(theme.space(4))
        .pl(TITLE_LEFT)
        .window_drag()
        .gap(theme.space(3))
        .items_center()
        .fill(theme.color("surface"))
        .border_sides([0.0, 0.0, 1.0, 0.0], theme.color("border-strong"))
        .child(
            text(state.name.clone())
                .font(theme.text("title"))
                .color(theme.color("text")),
        )
        .children(warning)
        .child(el().grow(1.0))
        .child(
            text("1.1")
                .key("time")
                .font(theme.text("number"))
                .color(theme.color("text-secondary"))
                .w(48.0)
                .live_text(&state.lives.time),
        )
        .child(play)
}

#[cfg(test)]
mod tests;
