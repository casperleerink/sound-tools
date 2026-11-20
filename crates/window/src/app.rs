//! The window of one project: a title row (its name, the side panel, the transport), and the
//! view of its main instance filling the rest. In a run without a window the engine plays
//! offline, a step per frame, and Bun answers before each frame, so a run is always the same.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context as _, Result};
use plugin_host::{Plugins, ScanCache, ScanCommand};
use runtime::app::ChosenDevices;
use runtime::session::Session;
use serde_json::Value;
use sound_core::{Engine, InstanceId, OutputDevice, OutputStream, Project, Ticks};
use sound_typescript::window::{Drawn, Tools};

use crate::controls::Step;
use crate::feeds::Feeds;
use crate::lives::Lives;
use crate::plugin_windows::{self, Events, PluginWindows, RuneDesktop};
use crate::theme::Theme;
use crate::tree;

mod actions;
mod chrome;
mod dialog;
mod drops;
mod export;
mod frame;
mod headless;
mod history;
mod host;
mod input;
mod keys;
mod menu;
mod notices;
mod panel;
pub(crate) mod plays;
mod problems;
mod projects;
mod render;
mod settings;
mod transport;

use headless::copied;

pub(crate) use chrome::{Act, MIN_SIZE, WINDOW_BUTTONS, controls as chrome_controls, view};
pub(crate) use dialog::controls as dialog_controls;
pub(crate) use frame::{closing, poll, quitting, setup};
#[cfg(test)]
pub(crate) use keys::MAIN_MODIFIER;
pub(crate) use keys::{on_focus, on_key, on_modifiers};
pub(crate) use panel::{PanelTab, Tab};

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
    Device(OutputStream),
    Offline { engine: Box<Engine>, played: f64 },
}

/// What the output device went through, for the log, as the app ends.
impl Drop for Sound {
    fn drop(&mut self) {
        if let Sound::Device(output) = self {
            let status = output.status();
            println!("xruns: {}", status.xruns);
            println!("late callbacks: {}", status.late_callbacks);
            println!("slowest callback: {:?}", status.slowest_callback);
            println!("device output latency: {:?}", status.output_latency);
        }
    }
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
    pub(crate) walked: tree::Walked,
    pub(crate) planes: tree::Planes,
    pub(crate) feeds: Feeds,
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
    /// The facts Bun was last told, see `State::facts_now`.
    facts: BTreeMap<InstanceId, sound_core::facts::Facts>,
    /// When facts were last read, in seconds of the window's clock.
    facts_read: f64,
    /// Events of the project a run without a window took while it waited for Bun, which the
    /// next poll gives the rest of the window.
    heard: Vec<sound_core::ProjectEvent>,
    /// New trees came since the last frame, see [`frame::keep_focus_in_view`].
    trees_moved: bool,
    /// The offers of the plugins of this computer, and the scan they were made for.
    plugin_offers: (u64, Vec<sound_typescript::devices::Offer>),
    /// The overlays open now, each inside the one before, by the key of what opened it. How the
    /// composer works, so never saved.
    pub(crate) open: Vec<String>,
    /// The menu a right press opened, or will once its press is answered.
    pub(crate) menu: Option<actions::Menu>,
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
    pub(crate) plays: plays::Playing,
    pub(crate) drops: drops::Drops,
    pub(crate) shell: crate::shell::Projects,
    pub(crate) incoming: input::Incoming,
    /// Files of a newer template that the project changed too: they wait for a merge.
    pub(crate) template_update: Option<runtime::templates::Update>,
    pub(crate) export: export::Export,
    renders: render::Renders,
    pub(crate) project_menu: menu::Menu,
    pub(crate) settings: settings::Settings,
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
                // The copy's SDK goes into the project too, as `--inspect` writes it.
                runtime::write_sdk(&folder)?;
                let (copy, folder) = copied(&folder, &name)?;
                (folder, Some(copy))
            }
            _ => (folder, None),
        };
        if !folder.join(runtime::PROJECT_FILE).exists() {
            let daw =
                runtime::templates::named("DAW").context("this release has no DAW template")?;
            runtime::templates::write(&folder, daw)?;
        }
        // The tools wait for Bun while the output device opens.
        let tools = std::thread::spawn({
            let folder = folder.clone();
            move || runtime::load_tools(&folder, false)
        });
        let waker: Arc<Mutex<Option<rune::Waker>>> = Arc::default();
        let mut playing = String::new();
        let mut chosen_input = None;
        let opened = match headless {
            true => {
                let plugins =
                    Plugins::listing(Vec::new(), ScanCommand::this_program()?, ScanCache::none());
                let (control, engine) = Engine::new(runtime::OFFLINE);
                let engine = Box::new(engine);
                anyhow::Ok((
                    plugins,
                    Sound::Offline {
                        engine,
                        played: 0.0,
                    },
                    control,
                ))
            }
            false => {
                let chosen = runtime::app::support_folder()
                    .map(|support| ChosenDevices::read(&runtime::app::audio_devices_file(&support)))
                    .unwrap_or_default();
                chosen_input = chosen.input;
                (|| {
                    let device = OutputDevice::open(chosen.output.as_deref())?;
                    playing = device.name()?;
                    println!("device: {playing}, {} Hz", device.sample_rate());
                    let config =
                        sound_core::EngineConfig::new(device.sample_rate(), device.channels());
                    let (control, engine) = Engine::new(config);
                    anyhow::Ok((
                        runtime::playing_plugins(true)?,
                        Sound::Device(device.start(engine)?),
                        control,
                    ))
                })()
            }
        };
        // What is wrong with the folder is what to say, before the device.
        let tools = tools
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
        let (plugins, sound, control) = opened?;
        let (mut project, extensions) =
            runtime::open_with_loaded(&folder, control, plugins.clone(), tools)?;
        let wake = waker.clone();
        let wake: sound_typescript::window::Waker = Arc::new(move || {
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
            tools.read_trees(tree::read);
            tools
        });
        let events = Rc::new(Events::waking(waker.clone()));
        let desktop = RuneDesktop::new(plugins.clone(), events.clone());
        let plugin_windows = PluginWindows::new(desktop, events, cfg!(target_os = "windows"));
        let output = project.engine().output_peaks().clone();
        let output_stream = match &sound {
            Sound::Device(stream) => Some(stream),
            Sound::Offline { .. } => None,
        };
        let incoming =
            input::Incoming::open(&mut project, output_stream, chosen_input, wake.clone());
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
        // Asked again after the open brought the rest of its code up to date.
        let template_update = match runtime::templates::update(&folder) {
            Ok(update) => update,
            Err(error) => {
                session.report(format!("{error:#}"));
                None
            }
        };
        let schemas = (runtime::tool_schemas()?.into_iter())
            .map(|schema| (schema.tool, schema.schema))
            .collect();
        let agent = crate::sidebar::open(session.project(), headless, &waker);
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
            walked: tree::Walked::default(),
            planes: tree::Planes::default(),
            feeds: Feeds::new(),
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
            menu: None,
            facts: BTreeMap::new(),
            facts_read: 0.0,
            heard: Vec::new(),
            trees_moved: false,
            plugin_offers: (0, Vec::new()),
            main,
            waiting: host::Waiting::default(),
            keys,
            fields: RefCell::default(),
            waker,
            first: headless.then(|| (act.unwrap_or_default(), play)),
            panel: panel::LeftPanel::new(
                agent,
                (!headless)
                    .then(runtime::app::support_folder)
                    .and_then(Result::ok)
                    .map(|support| runtime::app::left_panel_file(&support)),
            ),
            problems_open: false,
            transport: transport::Transport::new(output),
            plays: plays::Playing::default(),
            project_menu: menu::Menu::default(),
            settings: match headless {
                true => settings::Settings::new(settings::Settings::none(), None, playing),
                false => settings::Settings::new(
                    settings::Settings::machine(),
                    (runtime::app::support_folder().ok())
                        .map(|support| runtime::app::audio_devices_file(&support)),
                    playing,
                ),
            },
            drops: drops::Drops::default(),
            shell: crate::shell::Projects::new(
                crate::shell::Projects::machine(),
                headless,
                Some(folder),
            ),
            incoming,
            template_update,
            export: export::Export::new(
                (!headless)
                    .then(runtime::app::support_folder)
                    .and_then(Result::ok)
                    .map(|support| runtime::app::export_settings_file(&support)),
            ),
            renders: render::Renders::new()?,
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
