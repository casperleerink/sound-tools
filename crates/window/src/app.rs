//! The window of one project: a title row (its name, the side panel, the transport), and the
//! view of its main instance filling the rest. In a run without a window the engine plays
//! offline, a step per frame, and Bun answers before each frame, so a run is always the same.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context as _, Result};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use runtime::session::Session;
use serde_json::Value;
use sound_core::{Engine, InstanceId, OutputDevice, OutputStream, Project, Ticks};
use sound_typescript::next::{Drawn, Tools};

use crate::controls::Step;
use crate::lives::Lives;
use crate::plugin_windows::{self, Events, PluginWindows, RuneDesktop};
use crate::theme::Theme;
use crate::tree;

mod actions;
mod chrome;
mod dialog;
mod frame;
mod headless;
mod host;
mod keys;
mod notices;
mod panel;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the drag calls it once it plays numbers")
)]
mod plays;
mod problems;
mod transport;

use headless::copied;

pub(crate) use chrome::{Act, MIN_SIZE, WINDOW_BUTTONS, controls as chrome_controls, view};
pub(crate) use dialog::controls as dialog_controls;
pub(crate) use frame::{closing, poll, quitting, setup};
pub(crate) use keys::{on_focus, on_key};

/// The template of a new project: the DAW with views of `sdk/view.ts`.
pub(crate) const TEMPLATE: runtime::Template = runtime::Template {
    name: "DAW (new window)",
    description: "Tracks of clips, the devices of a track and a mixer.",
    files: include!(concat!(env!("OUT_DIR"), "/template.rs")),
};

/// What the composer reads when a control did nothing: why is for the agent, in `--act`.
const DID_NOTHING: &str = "That did nothing: this tool could not do it.";

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
    pub(crate) name: String,
    /// The project, and every rule of working on it: undo, drags, notices, the transport.
    pub(crate) session: Session,
    sound: Sound,
    /// The session's, for the engine a run without a window plays.
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
    pub(crate) shown: BTreeSet<InstanceId>,
    /// The view a held press paints, and the keys of the elements it entered.
    painting: Option<(InstanceId, BTreeSet<String>)>,
    /// The press that may become a drag now.
    pub(crate) gesture: Option<tree::Gesture>,
    /// The element a drag ended on in this frame: the release that ends a drag is no press.
    pub(crate) dragged: Option<String>,
    /// The facts of the instances shown and their children, with the tool and the load of Bun
    /// they were made for.
    facts: HashMap<InstanceId, (String, u64, Value)>,
    /// The offers of the plugins of this computer, and the scan they were made for.
    plugin_offers: (u64, Vec<sound_typescript::devices::Offer>),
    /// The overlays open now, each inside the one before, by the key of what opened it. How the
    /// composer works, so never saved.
    pub(crate) open: Vec<String>,
    /// What `--main` shows in place of the project's main instance.
    main: Option<InstanceId>,
    /// What waits for the composer or a thread: questions of code, file dialogs, imports.
    waiting: host::Waiting,
    keys: keys::Keys,
    /// What the text fields say, by key, and the value each was made from.
    pub(crate) fields: RefCell<tree::Fields>,
    waker: Arc<Mutex<Option<rune::Waker>>>,
    /// The steps of `--act` and the seconds of `--play`, for the first frame of a run without a window.
    first: Option<(Vec<Step>, Option<f64>)>,
    last: f64,
    /// A run without a window opens a copy, so it writes nothing and works next to a window.
    _copy: Option<tempfile::TempDir>,
    /// The project's icons, each read once per load of the code.
    pub(crate) icons: tree::Icons,
    /// Where the controls of groups are drawn, which their arrows go by.
    pub(crate) reach: tree::Reach,
    // The areas of the window's chrome follow, a field each. An area is a module of `app` that
    // owns its state: `open` makes it, `chrome::view` draws it, `keys::on_key` gives it keys.
    pub(crate) panel: panel::LeftPanel,
    /// The list of the files that are not live is open.
    problems_open: bool,
    pub(crate) plugin_windows: PluginWindows<RuneDesktop>,
    pub(crate) transport: transport::Transport,
    /// The drag that plays numbers into the session's gesture now.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the drag calls it once it plays numbers")
    )]
    pub(crate) plays: Option<plays::Playing>,
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
        let tools = extensions.map(|extensions| {
            let mut tools = Tools::start(extensions, &project, !headless, wake.clone());
            tools.open_windows_with(plugin_windows::opener(plugins.clone()));
            tools
        });
        let events = Rc::new(Events::waking(waker.clone()));
        let desktop = RuneDesktop::new(plugins.clone(), events.clone());
        let plugin_windows = PluginWindows::new(desktop, events, cfg!(target_os = "windows"));
        let output = project.engine().output_peaks().clone();
        let mut session = Session::new(project, plugins.clone(), wake);
        let (theme, theme_problem) = Theme::of(&folder);
        let theme_loads = tools.as_ref().map_or(0, Tools::loads);
        // A run without a window keeps the composer's keys to itself.
        let keymap_file = match headless {
            true => None,
            false => Some(runtime::app::shortcuts_file(
                &runtime::app::support_folder()?
            )),
        };
        let (keys, unread) = keys::Keys::open(keymap_file);
        if let Some(problem) = theme_problem.or(unread) {
            session.report(problem);
        }
        let schemas = (runtime::tool_schemas()?.into_iter())
            .map(|schema| (schema.tool, schema.schema))
            .collect();
        Ok(Self {
            name,
            session,
            sound,
            plugins,
            plugin_windows,
            tools,
            theme,
            schemas,
            lives: Lives::new(),
            playing: false,
            playhead: Ticks(0),
            engine_time: 0.0,
            looped: 0.0,
            theme_loads,
            shown: BTreeSet::new(),
            painting: None,
            gesture: None,
            dragged: None,
            open: Vec::new(),
            facts: HashMap::new(),
            plugin_offers: (0, Vec::new()),
            main,
            waiting: host::Waiting::default(),
            keys,
            fields: RefCell::default(),
            waker,
            first: headless.then(|| (act.unwrap_or_default(), play)),
            panel: panel::LeftPanel::new(
                Some(panel::LeftPanel::waiting_agent()),
                (!headless)
                    .then(runtime::app::support_folder)
                    .and_then(Result::ok)
                    .map(|support| runtime::app::left_panel_file(&support)),
            ),
            problems_open: false,
            transport: transport::Transport::new(output),
            plays: None,
            last: 0.0,
            _copy: copy,
            icons: tree::Icons::default(),
            reach: tree::Reach::default(),
        })
    }

    pub(crate) fn project(&self) -> &Project {
        self.session.project()
    }

    /// Whether the tool of `id` is one of the project's whose code did not load: no built-in
    /// tool, none that loaded, or no record, and the project's code has problems.
    pub(crate) fn code_failed(&self, id: &InstanceId) -> bool {
        let Some(tools) = &self.tools else {
            return false;
        };
        let problems = self.project().problems();
        let code = problems
            .iter()
            .any(|problem| problem.path.starts_with(sound_typescript::FOLDER));
        // A record whose tool did not load may not open at all.
        let unknown = (self.project().tool_of(id))
            .is_none_or(|tool| !tools.knows(tool) && !self.schemas.contains_key(tool));
        code && unknown
    }

    /// The instance the window shows, as `project.json` names it now.
    pub(crate) fn main(&self) -> Option<&InstanceId> {
        (self.main.as_ref()).or(self.project().project_file().main.as_ref())
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
}

#[cfg(test)]
mod tests;
