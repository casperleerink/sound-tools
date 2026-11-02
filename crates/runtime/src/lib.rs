//! What the project runtime is made of, apart from the command line in `main.rs`: the bundled
//! extensions and their views, the agent sidebar, the default project, the project summary, offline rendering and
//! the application window. Tests of whole projects, with every bundled extension, use this
//! crate.

pub mod app;
pub mod log_file;
pub mod recorder;
pub mod report;
#[cfg(target_os = "macos")]
pub mod screenshot;
pub mod templates;
pub mod update;
pub mod window;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use gpui::{App, AppContext as _};
use plugin_host::{
    Parameter, PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand, VST_TRADEMARK,
    WeakPlugins, default_search_paths,
};
use sound_agent::{AgentSettings, Sidebar};
use sound_core::{
    AgentDoc, Changes, Engine, EngineConfig, EngineControl, InstanceId, PortReference, Project,
    ProjectError, Registry, SavedConnection, SavedDestination, SavedSource, Ticks, ToolSchema,
};
use sound_typescript::{Extensions, FirstLoad};
use sound_ui::{DeviceOffer, Devices, OfferGroup, Views};
use window::{LeftPanel, LeftPanelSlot};

pub use templates::{TEMPLATES, Template};

pub const PROJECT_FILE: &str = "project.json";

/// The release this build is, which `.github/workflows/release.yml` takes from its tag.
/// `0.0.0` for a build of no release.
pub const VERSION: &str = match option_env!("SOUND_TOOLS_VERSION") {
    Some(version) => version,
    None => "0.0.0",
};

/// Offline renders have no device to ask. `offline` is what every processor is told, and what a
/// hosted plugin passes on to the plugin in the way its format intends.
pub const OFFLINE: EngineConfig = EngineConfig {
    sample_rate: 48_000,
    channels: 2,
    ring_capacity: 64,
    event_capacity: 256,
    processor_slots: 256,
    offline: true,
};

/// The same text on every machine and for every build, so a project in git gets no diff from
/// being opened somewhere else. Hence no path of this executable in it.
const INSPECT_DOC: AgentDoc = AgentDoc {
    name: "inspect",
    group: "Project",
    when: "You can run commands: check how your work sounds, read the whole piece, or render a WAV",
    markdown: "# Inspect, measure and render from a command line

Every form works while the project is open and changes no record.

```sh
sound-tools . --inspect
sound-tools . --analyze
sound-tools . --render <wav>
```

- `--inspect` prints where each time signature starts, the tempo, the bars and ticks of each section, every instance with its record, the connections and the problems.
- `--analyze` plays the piece and its tail offline, prints what it measures of the whole and writes the rest to a report file. Use it after a change, to check that it sounds as you meant.
- `--render` writes the piece as a 48 kHz stereo 32-bit float WAV and prints its peak.

`--analyze` and `--render` take the same options:

- `--from <at>` and `--to <at>`: a part of the piece, and its tail unless `--no-tail` follows. `<at>` is ticks (`3840`), a time as the playhead shows it (`1:23.5`) or seconds (`83.5s`). When the composer says what they heard at a time, measure that time.
- `--seconds <n>`: the first `n` seconds, with no tail.
- `--only <path>`: only the sound of this instance, such as `song/bass`, before what its parents do to it.

To turn a part into audio, render it alone into `assets/audio/` under a new name and play it with a `region`: `sound-tools . --render assets/audio/bass.wav --only song/bass/chain --from 0 --to 7680`.

`sound-tools <file> --analyze` measures an audio file, such as a sample under `assets/audio/`.

## What `--analyze` prints

The first line is the whole: loudness, the loudest 400 ms, the true peak and where it is, its pitch and drift. The second is the notes: how many and in how many phrases, the tuning they share and how far they spread around it (`+20c ±3c`; a speaking voice spreads 20c or more), and how far apart they start, as the tempos that gap is a beat or a part of at.

Then where the report file is, and the line each part starts on. Read only the part you need: the pitch part has 100 lines a second.

Rows of the same size start at a tick, a bar and a time:

- `LUFS`: the loudness of the row. `max`: its loudest 400 ms, which shows short hits.
- `peak`: the true peak in dBTP. Above 0 clips.
- `sub` under 60 Hz, `bass` 60 to 250, `lowmid` 250 to 500, `mid` 500 to 2k, `highmid` 2k to 6k, `high` above 6k: the level of each band in dB.
- `width`: the share of the sound in the side. 0% is mono; above 50% the channels cancel when summed to mono.
- `pitch`: the middle pitch heard and how far off it in cents: `A4+3c`. `drift`: how far the pitch moves, in cents: a vibrato of 20 cents either way reads about `40c`, two notes in one row their interval.
- `-` is silence. A track that should play and measures `-` alone did not load or plays nothing. Under `pitch`, `-` is also sound with no single pitch: noise, drums, a chord.

A note starts at an attack, a syllable or a change of pitch:

- `phrases`: notes with no silence of a quarter second or more between them, where each starts and its `length` in seconds. Cut a recording into regions there.
- `notes`: each one's `length` in seconds, the `level` of its loudest 10 ms in dB, its middle `pitch`, and its `glide` in cents from its start to its end.
- `pitch every 10 ms`: the level and the pitch.

Silence is 40 dB under the loudest moment, or 10 dB over the noise of a recording. Which key and which bar the notes belong to is for you and the composer to decide.

To check a vibrato or the tuning, solo the track and measure one held note with `--from` and `--to`. A mono file measures 3 dB above what a meter of one channel shows, as it plays on both. Compare runs (before and after your change, a soloed track against the whole); the composer says whether it sounds good.

When `sound-tools` is not on your `PATH`, ask the composer to pick **Install command line tool** in the project menu, or skip this step: `problems.txt` tells you whether your files loaded.",
};

/// The plugin host of one session: it scans this machine, keeps the plugins a project loads
/// and saves their state. `read_only` is for `--inspect` and `--render`, which never write.
///
/// The scan runs this same executable with [`plugin_host::SCAN_ARGUMENT`], one child process
/// per bundle, so a plugin that crashes while it is looked at costs one bundle.
pub fn plugins(read_only: bool) -> Result<Plugins> {
    let loading = if read_only {
        Loading::Only
    } else {
        Loading::AndSaving
    };
    with_cache(loading, ScanCache::of_this_machine())
}

/// The host `runtime --plugins` uses: it looks at every bundle again, whatever the cache of
/// this machine remembers, and writes the result. A plugin that hung or crashed once is tried
/// again with it, as `Scan again` in a picker does.
pub fn plugins_refreshing_the_cache() -> Result<Plugins> {
    with_cache(Loading::None, ScanCache::of_this_machine().refreshing())
}

/// The host `--inspect` uses: it says which plugins this machine has and loads none of them.
/// See [`Plugins::listing`] for why.
pub fn plugins_for_inspecting() -> Result<Plugins> {
    with_cache(Loading::None, ScanCache::of_this_machine())
}

/// How much of a plugin a host is for.
enum Loading {
    /// Loads a plugin and saves its state into the project.
    AndSaving,
    /// Loads a plugin and never writes: `--render`.
    Only,
    /// Looks a plugin up and never loads one: `--inspect`, `--plugins`.
    None,
}

fn with_cache(loading: Loading, cache: ScanCache) -> Result<Plugins> {
    let scanner = ScanCommand::this_program()?;
    let paths = default_search_paths();
    Ok(match loading {
        Loading::AndSaving => Plugins::new(paths, scanner, cache),
        Loading::Only => Plugins::read_only(paths, scanner, cache),
        Loading::None => Plugins::listing(paths, scanner, cache),
    })
}

/// Every bundled extension registers here.
pub fn registry(plugins: Plugins) -> Result<Registry> {
    let mut registry = Registry::new();
    registry.keep_out_of_history(&sound_typescript::WRITTEN_BY_THE_RUNTIME);
    // First, as the map lists its groups in the order of their first doc.
    registry.runtime_agent_doc(INSPECT_DOC)?;
    // MIDI input registers no tool, so it has no extension to enable in `project.json`. Every
    // project can be recorded into, so its doc is one every project gets.
    registry.runtime_agent_doc(midi::AGENT_DOC)?;
    for doc in sound_typescript::AGENT_DOCS {
        registry.runtime_agent_doc(doc)?;
    }
    register_extensions(&mut registry, Some(plugins))?;
    Ok(registry)
}

/// Registers every bundled extension, and the plugin host when given the plugins it hosts.
fn register_extensions(registry: &mut Registry, plugins: Option<Plugins>) -> Result<()> {
    analyzer::register(registry)?;
    compressor::register(registry)?;
    delay::register(registry)?;
    drum_pad::register(registry)?;
    eq::register(registry)?;
    filter::register(registry)?;
    gate::register(registry)?;
    instrument::register(registry)?;
    limiter::register(registry)?;
    modulation::register(registry)?;
    pitch_correction::register(registry)?;
    if let Some(plugins) = plugins {
        plugin_host::register(registry, plugins)?;
    }
    reverb::register(registry)?;
    sampler::register(registry)?;
    saturator::register(registry)?;
    tone::register(registry)?;
    utility::register(registry)?;
    vocoder::register(registry)?;
    wavetable::register(registry)?;
    Ok(())
}

/// The name of every bundled extension.
pub fn bundled_extensions() -> Result<Vec<&'static str>> {
    let mut registry = Registry::new();
    register_extensions(&mut registry, None)?;
    let mut names = registry.extensions();
    names.insert(plugin_host::EXTENSION);
    Ok(names.into_iter().collect())
}

/// The built-in tools as the tools of a project see them, for their SDK.
pub fn tool_schemas() -> Result<Vec<ToolSchema>> {
    let mut registry = Registry::new();
    register_extensions(&mut registry, None)?;
    Ok(registry.tool_schemas())
}

/// Every bundled extension with a view registers it here, and what a rack calls its devices
/// and what a composer can pick. The window names no view type and no instrument: it takes
/// both registries and installs them, see `Shell::new`.
///
/// This is the one place that knows the built-in synth and the plugin host at once.
pub fn views(plugins: WeakPlugins) -> (Views, Devices) {
    let mut views = Views::new();
    let mut devices = Devices::new();
    // The order here is the order of a picker: the instruments, then the effects group by
    // group, and the plugins of this Mac after the built-in devices.
    instrument::view::register(&mut views, &mut devices);
    wavetable::view::register(&mut views, &mut devices);
    sampler::view::register(&mut views, &mut devices);
    drum_pad::view::register(&mut views, &mut devices);
    eq::view::register(&mut views, &mut devices);
    filter::view::register(&mut views, &mut devices);
    saturator::view::register(&mut views, &mut devices);
    compressor::view::register(&mut views, &mut devices);
    gate::view::register(&mut views, &mut devices);
    limiter::view::register(&mut views, &mut devices);
    modulation::view::register(&mut views, &mut devices);
    vocoder::view::register(&mut views, &mut devices);
    pitch_correction::view::register(&mut views, &mut devices);
    delay::view::register(&mut views, &mut devices);
    reverb::view::register(&mut views, &mut devices);
    utility::view::register(&mut views, &mut devices);
    analyzer::view::register(&mut views, &mut devices);
    sound_typescript::register_views(&mut views);
    plugin_host::view::register(&mut views, &mut devices, plugins.clone());
    // What the picker says under its offers: that the scan of this machine is still running,
    // and what Steinberg asks of anyone who writes "VST". Their guidelines want the VST
    // Compatible Logo next to the term and the attribution where the logo does not fit; a menu
    // row is such a place, so the attribution is what is shown. See `plugin_host::VST_TRADEMARK`.
    devices.notes({
        let plugins = plugins.clone();
        move || {
            let Some(plugins) = plugins.upgrade() else {
                return Vec::new();
            };
            let mut notes = Vec::new();
            if plugins.scan_is_running() {
                notes.push("Still looking for the plugins of this computer…".into());
            }
            let vst3 = plugins
                .instruments()
                .iter()
                .any(|found| found.format == PluginFormat::Vst3);
            if vst3 {
                notes.push(VST_TRADEMARK.into());
            }
            notes
        }
    });
    // The scan runs on a thread of its own, so what the plugin host offers grows while a
    // window is open. This is what tells a picker that its list is not the list any more.
    devices.offers_change({
        let plugins = plugins.clone();
        move || {
            plugins
                .upgrade()
                .map_or(0, |plugins| plugins.scan_generation())
        }
    });
    // One record serves both slots, so one function makes both lists. What a plugin says it is
    // decides which list it is in; a record written by hand may name any plugin in any slot.
    devices.instruments({
        let plugins = plugins.clone();
        move || plugin_offers(&plugins, plugin_host::Plugins::instruments)
    });
    devices.effects(move || plugin_offers(&plugins, plugin_host::Plugins::effects));
    (views, devices)
}

/// The agent sidebar, in the left panel of the window. The window names no agent type, so this
/// is where the two meet. In `support`, the support folder of the machine, the panel keeps
/// whether it is open (see [`LeftPanelSlot`]), and the sidebar the agent it downloads, the
/// threads of each project and the composer's approval mode and model. The settings are read
/// once here and shared by every window, so a change in one is a change in all.
pub fn agent_panel(support: Option<PathBuf>, cx: &mut App) -> LeftPanelSlot {
    let remembered = support.as_deref().map(app::left_panel_file);
    let agents = support.as_deref().map(app::agents_folder);
    let threads = support.as_deref().map(app::threads_folder);
    let file = support.as_deref().map(app::agent_settings_file);
    let settings = cx.new(|cx| AgentSettings::new(file, cx));
    // The agent checks the types of the project's tools with the Bun that runs them.
    let bun_folder =
        sound_typescript::bundled_bun().and_then(|bun| bun.parent().map(Path::to_path_buf));
    LeftPanelSlot::new(remembered, move |session, window, cx| {
        let (agents, threads, settings) = (agents.clone(), threads.clone(), settings.clone());
        let bun_folder = bun_folder.clone();
        let sidebar =
            cx.new(|cx| Sidebar::new(session, agents, threads, settings, bun_folder, window, cx));
        let (composer, asks) = (sidebar.clone(), sidebar.clone());
        LeftPanel::new(sidebar, Sidebar::is_busy, cx)
            .with_composer(move |cx| composer.read(cx).composer().clone())
            .with_ask(move |message, window, cx| {
                asks.update(cx, |sidebar, cx| {
                    sidebar.ask_from_outside(message, window, cx)
                })
            })
    })
}

/// The plugins `list` gives, as offers for a picker. Each writes the record of that plugin
/// into the slot it is picked for, with a state file of its own. After them every bundle that
/// could not be scanned, named by its file and with why, and `Scan again`, which tries those
/// again. What a failed bundle holds is unknown, so both pickers list it.
fn plugin_offers(
    weak: &WeakPlugins,
    list: fn(&plugin_host::Plugins) -> Vec<plugin_host::ScannedPlugin>,
) -> Vec<DeviceOffer> {
    let Some(plugins) = weak.upgrade() else {
        return Vec::new();
    };
    let failures = plugins.failures();
    let failed = failures.iter().map(|failure| {
        let name = failure.path.file_stem().unwrap_or(failure.path.as_os_str());
        DeviceOffer::unavailable(
            format!("failed:{}", failure.path.display()),
            name.to_string_lossy().into_owned(),
            OfferGroup::Plugins,
            format!("Could not be scanned: {}", failure.message),
        )
    });
    let scan_again = (!failures.is_empty() && !plugins.scan_is_running()).then(|| {
        let weak = weak.clone();
        let run = move || {
            if let Some(plugins) = weak.upgrade() {
                plugins.scan_again();
            }
        };
        DeviceOffer::command(SCAN_AGAIN, "Scan again", OfferGroup::Plugins, run)
            .icon("search")
            .with_detail("Try the plugins that could not be scanned")
    });
    let found = list(&plugins).into_iter().map(|found| {
        let (id, name, format) = (found.id.clone(), found.name.clone(), found.format);
        let offer =
            DeviceOffer::new(
                found.offer_key(),
                found.name.clone(),
                OfferGroup::Plugins,
                move |project, slot, changes| {
                    // A state file of its own that no plugin has ever written into, so a
                    // plugin that is picked never comes up holding the sound an older one
                    // left behind.
                    let state_asset = plugin_host::new_state_asset(project.assets(), &name)
                        .map_err(|error| ProjectError::InvalidState {
                            id: slot.clone(),
                            message: error.to_string(),
                        })?;
                    changes.create(
                        slot.clone(),
                        PluginRecord {
                            format,
                            plugin_id: id.clone(),
                            state_asset,
                            parameters: Default::default(),
                        },
                    );
                    Ok(())
                },
            )
            .needs(
                plugin_host::EXTENSION,
                "This project does not load plugins.",
            );
        offer.with_detail(found.detail())
    });
    found.chain(failed).chain(scan_again).collect()
}

/// The key of the `Scan again` row of a picker, see [`plugin_offers`].
pub const SCAN_AGAIN: &str = "plugins:scan-again";

/// Opens the project with its lock. The plugin host is given: the application passes
/// [`plugins`], which scans this machine, and a test passes one that looks in a folder of its
/// own. A host that was not told to scan on a thread scans on this one, the first time a
/// record needs a plugin.
///
/// An empty or missing folder becomes an empty project: 120 bpm, 4/4, every bundled extension.
/// A folder with other files and no `project.json` is refused, see [`app::check_project_folder`].
///
/// With no window, it waits for what the behaviours of its own tools declare, so it plays what
/// the records say from the start.
pub fn open_or_create_with(
    folder: &Path,
    control: EngineControl,
    plugins: Plugins,
) -> Result<Project> {
    let extensions = load_tools(folder, false)?;
    let mut project = open_with_loaded(folder, control, plugins, extensions)?.0;
    first(settle_behaviours(&mut project))?;
    Ok(project)
}

/// [`open_or_create_with`], and the project's own tools in `extensions/` for a window to keep
/// running: it defines them again when their code is saved. Without the window they play on
/// as long as the project lives.
pub fn open_with_extensions(
    folder: &Path,
    control: EngineControl,
    plugins: Plugins,
) -> Result<(Project, Option<Extensions>)> {
    open_with_loaded(folder, control, plugins, load_tools(folder, false)?)
}

/// The project's own tools, once the code of the template it was made from is this release's
/// where the project never changed it, see [`templates::update`]. What waits for a merge the
/// window asks again. With `make_tools_folder`, `extensions/` is made when it is missing.
///
/// The first step of every open that may write: nothing is written into a folder that is not a
/// project.
pub(crate) fn load_tools(folder: &Path, make_tools_folder: bool) -> Result<Option<FirstLoad>> {
    app::check_project_folder(folder)?;
    if make_tools_folder {
        std::fs::create_dir_all(folder.join(sound_typescript::FOLDER))?;
    }
    if let Err(error) = templates::update(folder) {
        eprintln!("error: {error:#}");
    }
    Ok(Extensions::load(folder, &tool_schemas()?, true))
}

/// [`open_with_extensions`] with the tools of the project loaded already, see
/// [`Extensions::load`].
pub(crate) fn open_with_loaded(
    folder: &Path,
    control: EngineControl,
    plugins: Plugins,
    extensions: Option<FirstLoad>,
) -> Result<(Project, Option<Extensions>)> {
    let (registry, extensions) = registry_of(plugins, extensions)?;
    let mut project = Project::open(folder, registry, control)?;
    if let Some(extensions) = &extensions {
        extensions.report(&mut project);
    }
    Ok((project, extensions))
}

/// The registry, with the tools of the project when it has its own.
fn registry_of(
    plugins: Plugins,
    extensions: Option<FirstLoad>,
) -> Result<(Registry, Option<Extensions>)> {
    let mut registry = registry(plugins)?;
    let extensions = extensions.map(|loaded| loaded.register(&mut registry));
    Ok((registry, extensions))
}

/// Makes the device play only the `audio` output of `path`, in memory, for a render of one part
/// of a read-only project: every `project.json` connection to the device output gives way to one
/// from it. What its parents do to its sound after it, such as a mixer or a send, is not heard,
/// and a tool that plays to the device by itself rather than through `project.json` still does.
pub fn listen_only(project: &mut Project, path: &str) -> Result<()> {
    let id = InstanceId::new(path)?;
    if project.tool_of(&id).is_none() {
        bail!(
            "no instance is called {path:?}: name it by its path under state/, such as song/bass"
        );
    }
    let mut changes = Changes::new();
    let to_device = (project.project_file().connections.iter())
        .filter(|connection| matches!(connection.to, SavedDestination::DeviceOutput(_)));
    for connection in to_device {
        changes.disconnect(connection.clone());
    }
    changes.connect(SavedConnection::to_device(
        PortReference::new(&id, "audio"),
        0,
    ));
    project.apply_in_memory(changes)?;
    Ok(())
}

/// Opens the project the way `--inspect` does: without its lock, and with a host that looks a
/// plugin up and loads none ([`Plugins::listing`]). Inspecting prints a project and makes no
/// sound, so no third-party code runs in this process and no plugin can end it.
pub fn open_for_inspect(folder: &Path) -> Result<Project> {
    let plugins = plugins_for_inspecting()?;
    let (project, _engine) = open_read_only_with(folder, plugins)?;
    Ok(project)
}

/// Opens the project without its lock, so it works next to a running runtime. The engine
/// renders it offline. The plugin host is given, see [`open_or_create_with`].
pub fn open_read_only_with(folder: &Path, plugins: Plugins) -> Result<(Project, Engine)> {
    let (control, engine) = Engine::new(OFFLINE);
    // A runtime that holds the project may be writing its log.
    let extensions = Extensions::load(folder, &tool_schemas()?, false);
    let (registry, extensions) = registry_of(plugins, extensions)?;
    let mut project = Project::open_read_only(folder, registry, control)?;
    if let Some(extensions) = &extensions {
        extensions.report(&mut project);
    }
    first(settle_behaviours(&mut project))?;
    Ok((project, engine))
}

/// What `--inspect` prints: the time, the sections, every instance with its record and sound
/// ports beyond `audio`, the connections and the problems.
pub fn summary(project: &Project) -> String {
    let project_file = project.project_file();
    let tempo_map = &project_file.tempo_map;
    let time_signatures = tempo_map.time_signatures();
    let mut lines = vec![format!(
        "extensions: {}",
        project_file.extensions.join(", ")
    )];
    // Where each time signature starts, so bar math across changes needs no adding up.
    for bar in time_signatures.changes() {
        let signature = bar.signature;
        lines.push(format!(
            "time signature: {signature} from bar {} (tick {}), {} ticks per bar, {} ticks per beat",
            bar.number,
            bar.start.0,
            signature.ticks_per_bar(),
            signature.ticks_per_beat()
        ));
    }
    for change in tempo_map.tempo_changes() {
        let position = time_signatures.bar_beat_of(change.tick);
        lines.push(format!(
            "tempo: {} bpm from {position} (tick {})",
            change.bpm.bpm(),
            change.tick.0
        ));
    }
    lines.extend(sections(project));

    for (id, tool) in project.instances() {
        let indent = "  ".repeat(id.as_str().matches('/').count());
        let state = project.state_json(id).unwrap_or_default();
        lines.push(format!("{indent}instance `{id}` [{tool}] {state}"));
        // Only sound ports a connection names beyond `audio`, such as a sidechain.
        let (inputs, outputs) = project.sound_ports(id);
        if inputs.iter().chain(&outputs).any(|port| *port != "audio") {
            let (inputs, outputs) = (inputs.join(", "), outputs.join(", "));
            lines.push(format!("{indent}  sound ports: in {inputs}; out {outputs}"));
        }
    }

    lines.push(format!("connections: {}", project_file.connections.len()));
    for connection in &project_file.connections {
        let from = match &connection.from {
            SavedSource::DeviceInput(channel) => format!("device input {channel}"),
            SavedSource::App(app) => format!("the sound of {app}"),
            SavedSource::Output(output) => format!("{}:{}", output.instance, output.port),
        };
        let to = match &connection.to {
            SavedDestination::DeviceOutput(channel) => format!("device output {channel}"),
            SavedDestination::Input(input) => format!("{}:{}", input.instance, input.port),
        };
        lines.push(format!("  {from} -> {to}"));
    }
    lines.push(problems(project));
    lines.join("\n")
}

/// One line per section with its bars, and its ticks up to where it ends. The last section ends
/// on the bar line at or after the end of the project.
fn sections(project: &Project) -> Vec<String> {
    let time_signatures = project.project_file().tempo_map.time_signatures();
    let end = project.end();
    let sections = project.project_file().sections.spans();
    sections
        .map(|(section, next)| {
            let name = section.name.as_str();
            let Some(start) = time_signatures.bar(section.bar.get()) else {
                return format!("section {name:?}: from bar {}", section.bar);
            };
            let last = match next {
                Some(next) => time_signatures.bar(next.get() - 1),
                None => end
                    .filter(|end| *end > start.start)
                    .map(|end| time_signatures.bar_at(end.saturating_sub(Ticks(1)))),
            };
            match last {
                Some(last) => format!(
                    "section {name:?}: bars {} to {}, ticks {} to {}",
                    start.number,
                    last.number,
                    start.start.0,
                    last.end().0
                ),
                None => format!(
                    "section {name:?}: from bar {} (tick {}) to the end",
                    start.number, start.start.0
                ),
            }
        })
        .collect()
}

/// What `--plugin-params` prints: every parameter of one plugin of this machine that a host may
/// set, one line each, with its id, name, range and default, and its steps and their names
/// when it has them. The numbers are the format's own: CLAP's plain values, VST 3's from 0 to 1.
///
/// It loads the plugin in this process, which is why it is a command of its own.
pub fn plugin_parameters(
    plugins: &Plugins,
    format: PluginFormat,
    plugin_id: &str,
) -> Result<String> {
    plugins.wait_for_scan();
    let Some(found) = plugins.installed(format, plugin_id) else {
        bail!(
            "this machine has no {} plugin with the id {plugin_id:?}. `sound-tools --plugins` lists the ones it has",
            format.name()
        );
    };
    let parameters = plugin_host::read_parameters(&found)?;
    if parameters.is_empty() {
        return Ok(format!("{} has no parameters a host may set", found.name));
    }
    let rows: Vec<[String; 5]> = parameters
        .iter()
        .map(|parameter| {
            [
                parameter.id.to_string(),
                parameter.name.clone(),
                format!(
                    "{} to {}",
                    readable(parameter.minimum),
                    readable(parameter.maximum)
                ),
                format!("default {}", readable(parameter.default)),
                parameter_details(parameter),
            ]
        })
        .collect();
    // Every column but the last as wide as its widest cell, so the lines read as a table.
    let mut widths = [0; 4];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let [id_width, name_width, range_width, default_width] = widths;
    let lines: Vec<String> = rows
        .iter()
        .map(|[id, name, range, default, details]| {
            let line = format!(
                "{id:<id_width$}  {name:<name_width$}  {range:<range_width$}  {default:<default_width$}  {details}"
            );
            line.trim_end().to_string()
        })
        .collect();
    Ok(lines.join("\n"))
}

/// The steps of a parameter with their names, and whether it cannot be automated, which is the
/// rare case.
fn parameter_details(parameter: &Parameter) -> String {
    let mut details = Vec::new();
    if let Some(steps) = &parameter.steps {
        let names: Vec<String> = steps
            .names
            .iter()
            .map(|step| format!("{} = {}", readable(step.value), step.name))
            .collect();
        details.push(match names.is_empty() {
            true => format!("{} steps", steps.count),
            false => format!("{} steps: {}", steps.count, names.join(", ")),
        });
    }
    if !parameter.automatable {
        details.push("not automatable".to_string());
    }
    details.join("  ")
}

/// A number of a plugin as `--plugin-params` prints it: six significant digits and no trailing
/// zeros, so `8192 / 16383` reads `0.500031`. A whole part longer than that keeps every digit.
/// The value itself keeps its full precision; this is only for reading.
fn readable(number: f64) -> String {
    // Zero has no magnitude, and is never `-0`.
    if number == 0.0 {
        return "0".to_string();
    }
    if !number.is_finite() {
        return number.to_string();
    }
    let magnitude = number.abs().log10().floor() as i32;
    let decimals = (5 - magnitude).max(0) as usize;
    let text = format!("{number:.decimals$}");
    match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => text,
    }
}

pub fn problems(project: &Project) -> String {
    let problems = project.problems();
    let mut lines = vec![format!("problems: {}", problems.len())];
    for problem in problems {
        lines.push(format!("  {}: {}", problem.path, problem.message));
    }
    lines.join("\n")
}

/// One buffer of an offline render: the engine, and then the main-thread work of the plugin
/// host, exactly as the loop of a live session does.
///
/// A render that leaves the host out is a render in which a plugin's main-thread requests are
/// never answered: a CLAP plugin that asked for a callback waits for ever, and what a VST 3
/// plugin changed by itself never reaches its controller. A plugin may be silent until it is
/// answered, so this is not a nicety. Both callers of a render loop go through here.
pub fn render_block(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    output: &mut [f32],
) -> Result<Vec<plugin_host::PluginProblem>> {
    // A render plays what the records say from its first block: the sounds of the Drum pads
    // that were asked for are waited for and put in their kits before it, and not after it
    // with the rest of the tick, and every edit that waits for room in the rings is taken.
    drum_pad::wait_for_sounds();
    first(take_drum_sounds(project))?;
    first(settle_behaviours(project))?;
    while project.engine().pending_edits() > 0 {
        engine.apply_edits();
        project.engine().poll()?;
    }
    engine.process_block(output);
    project.engine().poll()?;
    let (problems, errors) = tick(project, plugins);
    first(errors)?;
    Ok(problems)
}

/// A render ends at the first behaviour that could not run again.
fn first(errors: Vec<ProjectError>) -> Result<()> {
    errors
        .into_iter()
        .next()
        .map_or(Ok(()), |error| Err(error.into()))
}

/// One round of the background work of the extensions, in the one order that is right. Every
/// loop of a session calls it as often as it polls the project: the window, headless, a render
/// and the test harnesses. A new background service is added here and nowhere else.
///
/// What a plugin or a behaviour could not do comes back and stops nothing: one bad record must
/// not end the session.
pub fn tick(
    project: &mut Project,
    plugins: &Plugins,
) -> (Vec<plugin_host::PluginProblem>, Vec<ProjectError>) {
    let mut errors = take_drum_sounds(project);
    // A Sampler whose instrument was downloaded or loaded on a thread of its own. A render, an
    // inspect and the tests load at once, so there is nothing here for them.
    let loaded = sampler::take_ready(project.assets());
    errors.extend(rebind(project, &loaded));
    // A tool of the project that owns children, whose behaviour Bun answered.
    let declared = sound_typescript::take_declared(project.assets());
    errors.extend(rebind(project, &declared));
    sound_typescript::forget_gone(project);
    // The main-thread callbacks the plugins ask for, and the state they say changed.
    let mut problems = plugins.poll(project);
    // A plugin that is started again is handed to the engine. After the poll, which is what
    // notes that it asked.
    problems.extend(plugins.send_restarts(project));
    // The pins of the plugin records, both ways: what a record changed goes to its plugin, and
    // what a plugin changed itself goes to its record. After the poll, which is where a plugin
    // that changed which parameters it has says so.
    errors.extend(plugins.follow_pins(project));
    // Records that were waiting for a plugin the scan had not reached, plugins that asked to be
    // loaded again, and plugins whose parameters changed. After the poll too.
    errors.extend(rebind(project, &plugins.take_retries()));
    (problems, errors)
}

/// Runs the behaviour of each instance again, which is what makes it play what a service
/// outside the project has ready now and takes its problem away. It is not an edit and is
/// never undone.
fn rebind(project: &mut Project, instances: &[InstanceId]) -> Vec<ProjectError> {
    let failed = instances.iter().filter_map(|id| project.rebind(id).err());
    failed.collect()
}

/// Waits for Bun to answer every behaviour it was asked, and runs each of them again, which
/// plays the answer, until none asks again: what a render plays from its first block, and what
/// `--inspect` prints. An answer for a record that changed since asks again.
fn settle_behaviours(project: &mut Project) -> Vec<ProjectError> {
    let mut errors = Vec::new();
    loop {
        sound_typescript::wait_for_behaviours();
        let declared = sound_typescript::take_declared(project.assets());
        if declared.is_empty() {
            return errors;
        }
        errors.extend(rebind(project, &declared));
    }
}

/// Runs the behaviour of every Drum pad whose sounds were made since the last call, which puts
/// them in its kit.
fn take_drum_sounds(project: &mut Project) -> Vec<ProjectError> {
    let ready = drum_pad::take_ready(project.assets());
    let instances: Vec<InstanceId> = ready.iter().map(|(instance, _)| instance.clone()).collect();
    let errors = rebind(project, &instances);
    // The sounds are let go of here, now that the kits hold them.
    drop(ready);
    errors
}

/// Puts the library of sampled instruments in the support folder of this machine, which every
/// project shares.
pub fn use_library() {
    if let Ok(support) = app::support_folder() {
        sampler::library::set_folder(support.join("library"));
    }
}

/// Renders `frames` frames in device buffers of 512 frames, interleaved by channel. See
/// [`render_into`].
pub fn render(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    frames: usize,
) -> Result<Vec<f32>> {
    let mut output = Vec::with_capacity(frames * engine.channels());
    render_into(project, engine, plugins, frames, |samples| {
        output.extend_from_slice(samples);
        Ok(())
    })?;
    Ok(output)
}

/// Renders `frames` frames in device buffers of 512 frames and gives them to `write`,
/// interleaved by channel, with whatever the plugin host reported on the way.
///
/// What the engine plays while it waits after a play or a seek, for the tracks with latency to
/// reach the device, is left out and does not count: a render from the start has tick 0 on its
/// first frame and ends where it would without latency. A project without latency never waits,
/// so it renders exactly what it did before latency was compensated.
pub fn render_into(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    frames: usize,
    mut write: impl FnMut(&[f32]) -> Result<()>,
) -> Result<Vec<plugin_host::PluginProblem>> {
    let channels = engine.channels();
    let mut buffer = vec![0.0_f32; 512 * channels];
    let mut problems = Vec::new();
    let mut waited = engine.preroll_frames();
    let mut left = frames;
    while left > 0 {
        let size = left.min(512);
        let output = &mut buffer[..size * channels];
        problems.extend(render_block(project, engine, plugins, output)?);
        // The engine plays a wait before anything else of a block, so it is the front of this
        // buffer.
        let now = engine.preroll_frames();
        let skip = usize::try_from(now - waited).unwrap_or(size).min(size);
        waited = now;
        write(&output[skip * channels..])?;
        left -= size - skip;
    }
    Ok(problems)
}

/// Below this every sample of a tail is silence: -90 dB.
const SILENT: f32 = 3.2e-5;

/// Plays `from..to` and then, with `tail`, what reverbs and releases still sound after the
/// transport stops at `to`. The tail ends after half a second of silence, or after ten seconds
/// for a sound that never ends, such as the noise of a plugin. See [`render_into`].
pub fn render_range(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    from: Ticks,
    to: Ticks,
    tail: bool,
    mut write: impl FnMut(&[f32]) -> Result<()>,
) -> Result<Vec<plugin_host::PluginProblem>> {
    let clock = project.clock();
    let frames = clock.frame_of(to).0.saturating_sub(clock.frame_of(from).0);
    project.engine().seek(from);
    project.engine().play();
    let mut problems = render_into(project, engine, plugins, frames as usize, &mut write)?;
    if !tail {
        return Ok(problems);
    }
    // Stopping releases every held note, which is where the tail starts.
    project.engine().pause();
    let channels = engine.channels();
    let rate = engine.sample_rate() as usize;
    let (mut quiet, mut played) = (0, 0);
    while quiet < rate / 2 && played < rate * 10 {
        problems.extend(render_into(project, engine, plugins, 512, |samples| {
            let frames = samples.len() / channels;
            quiet = if samples.iter().all(|sample| sample.abs() < SILENT) {
                quiet + frames
            } else {
                0
            };
            played += frames;
            write(samples)
        })?);
    }
    Ok(problems)
}
