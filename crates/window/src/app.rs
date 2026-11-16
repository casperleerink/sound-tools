//! The window of one project: a title row (its name, play and stop, where it plays), and the
//! view of its main instance filling the rest. In a run without a window the engine plays
//! offline, a step per frame, and Bun answers before each frame, so a run is always the same.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context as _, Result, anyhow, bail};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use rune::{Cx, El, KeyEvent, Tx, col, el, row, text};
use serde_json::{Value, json};
use sound_core::{Engine, InstanceId, OutputDevice, OutputStream, Peaks, Project, Ticks};
use sound_typescript::next::{Drawn, Tools};

use crate::controls::{self, Step, Target};
use crate::lives::Lives;
use crate::theme::Theme;
use crate::tree;

/// How often a playing window draws: what moves, moves at 60 frames a second.
const FRAME: std::time::Duration = std::time::Duration::from_millis(16);
const TITLE_ROW: f32 = 44.0;

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
    pub(crate) act: Vec<Step>,
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
    tools: Option<Tools>,
    pub(crate) theme: Theme,
    /// The JSON Schema of each built-in tool's record, by tool name.
    schemas: HashMap<&'static str, Value>,
    pub(crate) main: Option<InstanceId>,
    pub(crate) lives: Lives,
    pub(crate) playing: bool,
    playhead: Ticks,
    peaks: RefCell<HashMap<String, Option<Peaks>>>,
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
        let copy = match (headless, act.is_empty()) {
            (true, true) => Some(copied(&folder)?),
            _ => None,
        };
        let folder = match &copy {
            Some(copy) => copy.path().join(folder.file_name().unwrap_or_default()),
            None => folder,
        };
        if !folder.join(runtime::PROJECT_FILE).exists() {
            runtime::templates::write(&folder, &TEMPLATE)?;
        }
        let name = dunce::canonicalize(&folder)?
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
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
        if !headless {
            project.watch()?;
        }
        let wake = waker.clone();
        let wake: sound_typescript::next::Waker = Arc::new(move || {
            if let Some(wake) = wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                wake();
            }
        });
        let tools =
            extensions.map(|extensions| Tools::start(extensions, &project, !headless, wake));
        let (theme, theme_problem) = Theme::of(&folder);
        let schemas = (runtime::tool_schemas()?.into_iter())
            .map(|schema| (schema.tool, schema.schema))
            .collect();
        let main = project.project_file().main.clone();
        Ok(Self {
            name,
            project,
            sound,
            plugins,
            tools,
            theme,
            schemas,
            main,
            lives: Lives::new("1.1"),
            playing: false,
            playhead: Ticks(0),
            peaks: RefCell::default(),
            notice: theme_problem,
            shown: BTreeSet::new(),
            waker,
            first: headless.then_some((act, play)),
            last: 0.0,
            _copy: copy,
        })
    }

    /// What the window shows of `id`, once Bun said.
    pub(crate) fn drawn(&self, id: &InstanceId) -> Option<&Drawn> {
        self.tools.as_ref()?.drawn(id)
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
        let inside = (self.main.as_ref())
            .and_then(|main| id.as_str().strip_prefix(main.as_str())?.strip_prefix('/'));
        match (Some(id) == self.main.as_ref(), inside) {
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
    ) {
        if let Some(tools) = &mut self.tools {
            tools.act(instance, key, action, value);
        }
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
                self.act_on_view(instance, key, action, value);
            }
            Target::Field {
                instance,
                field,
                option,
            } => {
                let now = (self.project.state_json(instance))
                    .and_then(|json| serde_json::from_str::<Value>(&json).ok())
                    .and_then(|state| state.get(field).cloned());
                let number =
                    |value: Option<&Value>| value.and_then(Value::as_f64).unwrap_or_default();
                let next = match (action, option) {
                    (_, Some(option)) => option.clone(),
                    ("press" | "toggle", None) => {
                        json!(!now.and_then(|now| now.as_bool()).unwrap_or(false))
                    }
                    ("set", None) => value.ok_or_else(|| anyhow!("set takes a value"))?,
                    ("increment", None) => json!(number(now.as_ref()) + 1.0),
                    ("decrement", None) => json!(number(now.as_ref()) - 1.0),
                    (other, None) => bail!("a field takes no {other}"),
                };
                self.set_field(instance, field, next);
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
            let (_, shown) = controls::collect(self);
            if shown == self.shown {
                return changed;
            }
            self.shown = shown.clone();
            let Some(tools) = &mut self.tools else {
                return changed;
            };
            tools.show(shown);
            if !settle {
                return true;
            }
            changed |= tools.settle(&mut self.project);
        }
    }

    /// The first frame of a run without a window: the steps of `--act`, then `--play`.
    fn first_frame(&mut self) -> Result<()> {
        let Some((steps, play)) = self.first.take() else {
            return Ok(());
        };
        for step in steps {
            step.run(self)?;
            self.settle();
        }
        if play.is_some() {
            self.project.engine().play();
        }
        Ok(())
    }

    fn settle(&mut self) {
        if let Some(tools) = &mut self.tools {
            tools.settle(&mut self.project);
        }
        self.discover(true);
    }

    /// What `--tree` adds for an agent: the controls, where the piece plays and the meters.
    fn report(&self) -> Value {
        let (controls, _) = controls::collect(self);
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
            "meters": self.lives.levels().into_iter().map(|(name, level)| (name.trim_start_matches("meter:").to_string(), json!(level))).collect::<serde_json::Map<_, _>>(),
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

    /// The loudest amplitude of the meter `name` since the last frame.
    fn amplitude(&self, name: &str) -> f32 {
        let mut peaks = self.peaks.borrow_mut();
        let peaks = peaks.entry(name.to_string()).or_insert_with(|| {
            let id = InstanceId::new(name.trim_start_matches("meter:")).ok()?;
            self.project.peaks(&id, "level")
        });
        peaks
            .as_ref()
            .map_or(0.0, |peaks| peaks.take().into_iter().fold(0.0, f32::max))
    }
}

/// A copy of the project in `folder`, for a run without a window: opening writes into a folder.
fn copied(folder: &Path) -> Result<tempfile::TempDir> {
    let scratch = tempfile::tempdir()?;
    if folder.join(runtime::PROJECT_FILE).is_file() {
        let name = folder
            .canonicalize()?
            .file_name()
            .context("the project folder has no name")?
            .to_owned();
        copy_folder(folder, &scratch.path().join(name))?;
    }
    Ok(scratch)
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
    let headless = matches!(state.sound, Sound::Offline { .. });
    let mut changed = false;
    if headless {
        if let Some(tools) = &mut state.tools {
            changed |= tools.settle(&mut state.project);
        }
        changed |= state.discover(true);
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
    changed |= state.discover(headless);
    if let Ok(status) = state.project.engine().poll() {
        changed |= status.playing != state.playing;
        (state.playing, state.playhead) = (status.playing, status.playhead_tick);
    }
    let dt = (seconds - state.last).max(0.0) as f32;
    state.last = seconds;
    let time = state.time();
    let moving = state
        .lives
        .update(state.playhead.0 as f32, time, dt, |name| {
            state.amplitude(name)
        });
    if headless {
        *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(state.report());
    } else if state.playing || moving {
        tx.wake_after(FRAME);
    }
    tx.instant();
    changed
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
    true
}

pub(crate) fn view(state: &State, _: &mut Cx<State>) -> El {
    let theme = &state.theme;
    let body = match &state.main {
        Some(main) => match state.drawn(main) {
            Some(drawn @ (Drawn::View { .. } | Drawn::Failed(_))) => {
                tree::view(state, main, "", drawn)
            }
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
        .on_press(|s: &mut State| s.play_or_stop())
        .child(
            text(if state.playing { "■" } else { "▶" })
                .font(theme.text("label"))
                .color(theme.color(if state.playing {
                    "text-inverse"
                } else {
                    "text"
                })),
        );
    let problems = state.project.problems().len()
        + state
            .tools
            .as_ref()
            .map_or(0, |tools| tools.problems().count());
    let warning = (problems > 0).then(|| {
        text(format!("{problems} not live"))
            .font(theme.text("label"))
            .color(theme.color("peach"))
    });
    row()
        .w_full()
        .h(TITLE_ROW)
        .px(theme.space(4))
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
