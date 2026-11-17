//! The window of one project: a title row (its name, play and stop, where it plays), and the
//! view of its main instance filling the rest. In a run without a window the engine plays
//! offline, a step per frame, and Bun answers before each frame, so a run is always the same.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use rune::{Cx, El, KeyEvent, Tx, col, el, row, text};
use serde::Deserialize as _;
use serde_json::{Value, json};
use sound_core::{Engine, GROUPING_WINDOW, InstanceId, OutputDevice, OutputStream, Project, Ticks};
use sound_typescript::next::{Drawn, FRAME_INTERVAL, Looped, Tools};

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
    /// The instance shown in place of the project's main one.
    pub(crate) main: Option<InstanceId>,
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
    pub(crate) shown: BTreeSet<InstanceId>,
    /// The view a held press paints, and the keys of the elements it entered.
    painting: Option<(InstanceId, BTreeSet<String>)>,
    /// The press that may become a drag now.
    pub(crate) gesture: Option<tree::Gesture>,
    /// The element a drag ended on in this frame: the release that ends a drag is no press.
    pub(crate) dragged: Option<String>,
    /// The cards made for the generic card, by instance, with the tool and the load of Bun they
    /// were made for.
    cards: HashMap<InstanceId, (String, u64, Value)>,
    /// The overlays open now, each inside the one before, by the key of what opened it. How the
    /// composer works, so never saved.
    pub(crate) open: Vec<String>,
    /// What `--main` shows in place of the project's main instance.
    main: Option<InstanceId>,
    /// What the text fields say, by key, and the value each was made from.
    fields: RefCell<HashMap<String, (rune::TextEdit, String)>>,
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
            main,
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
            painting: None,
            gesture: None,
            dragged: None,
            open: Vec::new(),
            cards: HashMap::new(),
            main,
            fields: RefCell::default(),
            waker,
            first: headless.then(|| (act.unwrap_or_default(), play)),
            last: 0.0,
            _copy: copy,
        })
    }

    /// The instance the window shows, as `project.json` names it now.
    pub(crate) fn main(&self) -> Option<&InstanceId> {
        (self.main.as_ref()).or(self.project.project_file().main.as_ref())
    }

    /// What the window shows of `id`, once Bun said.
    pub(crate) fn drawn(&self, id: &InstanceId) -> Option<&Drawn> {
        self.tools.as_ref()?.drawn(id)
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
            Target::View(press) => {
                let value = match action {
                    "drag" => value.or_else(|| Some(json!({ "dx": 0, "dy": 0 }))),
                    _ => value,
                };
                self.try_press(press, action, value)?;
            }
        }
        Ok(())
    }

    /// A press on the element `key` of the view of `instance`, which has `onEnter`, paints it
    /// and what it enters until let go: one step of undo.
    pub(crate) fn paint(&mut self, instance: &InstanceId, key: &str) {
        self.painting = Some((instance.clone(), BTreeSet::from([key.to_string()])));
        if let Some(tools) = &mut self.tools {
            tools.hold(&mut self.project, true);
        }
        self.pointed(instance, key, "enter", None);
    }

    /// The pointer of a paint is over the element at `path`, Rune's path of keys: the first
    /// time it is over one that has `onEnter`, or is inside one, that one is painted.
    pub(crate) fn paint_over(&mut self, path: &str) {
        let Some((instance, entered)) = &self.painting else {
            return;
        };
        let Some(Drawn::View { root, memos }) = self.drawn(instance) else {
            return;
        };
        let Ok(root) = tree::Node::deserialize(&**root) else {
            return;
        };
        let mut keys = Vec::new();
        tree::enterable(&root, memos, &mut keys);
        let (prefix, path) = (self.prefix_of(instance), format!("/{path}/"));
        let found = (keys.into_iter())
            .filter(|key| !entered.contains(key))
            .find(|key| path.contains(&format!("/{prefix}{key}/")));
        let Some(key) = found else {
            return;
        };
        let instance = instance.clone();
        if let Some((_, entered)) = &mut self.painting {
            entered.insert(key.clone());
        }
        self.pointed(&instance, &key, "enter", None);
    }

    /// The press of a paint is let go.
    pub(crate) fn painted(&mut self) {
        self.painting = None;
        if let Some(tools) = &mut self.tools {
            tools.hold(&mut self.project, false);
        }
        self.handled();
    }

    /// Opens the overlay that the element `key` opens, inside the one it is in, or closes it
    /// and those inside it.
    pub(crate) fn toggle_overlay(&mut self, key: &str, within: Option<&str>) {
        match self.open.iter().position(|open| open == key) {
            Some(at) => self.open.truncate(at),
            None => {
                let kept =
                    within.and_then(|within| self.open.iter().position(|open| open == within));
                self.open.truncate(kept.map_or(0, |at| at + 1));
                self.open.push(key.to_string());
            }
        }
        self.handled();
    }

    /// Closes the overlay the element `key` opened, and those inside it.
    pub(crate) fn close_overlay(&mut self, key: &str) {
        if let Some(at) = self.open.iter().position(|open| open == key) {
            self.open.truncate(at);
        }
        self.handled();
    }

    /// `action` on an element of a view, as a pointer, a key or an agent does it: `open` opens
    /// or closes what it opens, a pick from a menu closes the menu, and a typed value closes the
    /// overlay it was typed in.
    pub(crate) fn press(&mut self, press: &tree::Press, action: &str, value: Option<Value>) {
        if self.try_press(press, action, value).is_err() {
            self.notice = Some(DID_NOTHING.to_string());
        }
        self.handled();
    }

    fn try_press(&mut self, press: &tree::Press, action: &str, value: Option<Value>) -> Result<()> {
        let key = format!("{}{}", self.prefix_of(&press.instance), press.element);
        let within = press.within.as_ref();
        if press.opens && (action == "open" || (action == "press" && !press.pressed)) {
            self.toggle_overlay(&key, within.map(|within| within.opener.as_str()));
            return Ok(());
        }
        let closes = match action {
            "press" => within.filter(|within| within.picks),
            "commit" => within,
            _ => None,
        };
        if let Some(within) = closes {
            let opener = within.opener.clone();
            self.close_overlay(&opener);
        }
        if press.opens && action == "press" {
            self.toggle_overlay(&key, within.map(|within| within.opener.as_str()));
        }
        self.act_on_view(&press.instance, &press.element, action, value)
    }

    /// What the field `key` says now: `value` while nobody types in it, all of it chosen, so
    /// what is typed takes its place.
    pub(crate) fn typed(&self, key: &str, value: &str) -> rune::TextEdit {
        let mut fields = self.fields.borrow_mut();
        let (edit, from) = (fields.entry(key.to_string()))
            .or_insert_with(|| (rune::TextEdit::new(value), value.to_string()));
        if !edit.focused {
            if from != value || edit.text != value {
                (*edit, *from) = (rune::TextEdit::new(value), value.to_string());
            }
            (edit.anchor, edit.focus) = (0, edit.text.len());
        }
        edit.clone()
    }

    /// The field `key`, which Rune edits.
    pub(crate) fn typing(&mut self, key: &str, value: &str) -> &mut rune::TextEdit {
        let fields = self.fields.get_mut();
        let entry = (fields.entry(key.to_string()))
            .or_insert_with(|| (rune::TextEdit::new(value), value.to_string()));
        &mut entry.0
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

    /// Shows what the views name, and waits for their trees in a run without a window. Only
    /// what the trees bind now stays bound.
    fn discover(&mut self, settle: bool) -> bool {
        let changed = self.show_named(settle);
        self.lives.keep(&tree::bound(self));
        changed
    }

    fn show_named(&mut self, settle: bool) -> bool {
        let mut changed = false;
        loop {
            let shown = controls::collect(self).shown;
            let cards = (shown.iter())
                .filter_map(|id| Some((id.clone(), self.card(id)?)))
                .collect();
            self.cards.retain(|id, _| shown.contains(id));
            let more = shown != self.shown;
            self.shown = shown.clone();
            let Some(tools) = &mut self.tools else {
                return changed;
            };
            if !tools.show(shown, cards) {
                return changed;
            }
            if !settle {
                return true;
            }
            changed |= self.settle_tools();
            if !more {
                return changed;
            }
        }
    }

    /// The card of `id` for the generic card, unless its tool draws a view of its own. Its
    /// fields follow its instance's tool and that tool's code: made again only when either
    /// changed. Which a lane moves follows the lanes.
    fn card(&mut self, id: &InstanceId) -> Option<Value> {
        let tool = self.project.tool_of(id)?.to_string();
        let tools = self.tools.as_ref();
        if tools.is_some_and(|tools| tools.has_view(&tool)) {
            return None;
        }
        let loads = tools.map_or(0, Tools::loads);
        let fresh = !matches!(self.cards.get(id), Some((made_for, made_at, _)) if *made_for == tool && *made_at == loads);
        if fresh {
            let card = crate::fields::card_of(self, id)?;
            self.cards.insert(id.clone(), (tool, loads, card));
        }
        let mut card = self.cards.get(id)?.2.clone();
        for field in card["fields"].as_array_mut()?.iter_mut() {
            let name = field["name"].as_str().unwrap_or_default().to_string();
            let automated = Tools::automated(&self.project, id, &name);
            field["automated"] = json!(automated);
        }
        Some(card)
    }

    /// Waits for Bun; a run without a window fails when it does not finish.
    fn settle_tools(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        let changed = tools.settle(&mut self.project).unwrap_or_else(|why| {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
            true
        });
        if changed {
            self.trees_came();
        }
        changed
    }

    /// New trees came: what drags moved is where the records say now, but a drag that goes on.
    fn trees_came(&self) {
        let held = self.gesture.as_ref().and_then(tree::Gesture::dragging);
        self.lives.drop_drags(held);
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
    // A press that a drag's release brings comes before the next frame.
    state.dragged = None;
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
            state.trees_came();
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
    changed |= state.lives.take_wider();
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

pub(crate) fn view(state: &State, cx: &mut Cx<State>) -> El {
    let theme = &state.theme;
    let body = match state.main() {
        Some(main) => match state.drawn(main) {
            Some(drawn) => tree::view(state, main, "", drawn),
            None => el(),
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
    cx.focus_ring(2.0, 2.0, theme.color("focus"));
    col()
        .w_full()
        .h_full()
        .fill(theme.color("background"))
        .child(title_row(state))
        .child(el().grow(1.0).w_full().clip_soft(0.0).child(body))
        .children(notice)
        .children(tree::overlays(state))
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
