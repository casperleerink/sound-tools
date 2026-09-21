//! What the project runtime is made of, apart from the command line in `main.rs`: the bundled
//! extensions and their views, the default project, the project summary, offline rendering and
//! the application window. Tests of whole projects, with every bundled extension, use this
//! crate.

pub mod window;

use std::path::Path;

use anyhow::Result;
use arrangement::{ArrangementState, Colour};
use instrument::SynthState;
use plugin_host::{
    PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand, VST_TRADEMARK, WeakPlugins,
    default_search_paths,
};
use sound_core::{
    AgentDoc, Changes, Engine, EngineConfig, EngineControl, Instance, InstanceId, Project,
    ProjectError, Registry, SavedDestination, State,
};
use sound_ui::{DeviceOffer, Devices, Views};

const PROJECT_FILE: &str = "project.json";

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
    when: "You can run commands and want the whole piece in one read",
    markdown: "# Inspect from a command line

When you can run commands, the Sound Tools runtime prints the tempo, every track in order, every clip with its bar range, note count and pitch range, and the problems. It works while the project is open and changes nothing.

```sh
runtime . --inspect
```

`runtime` is the program that has this project open. When it is not on your `PATH`, ask the composer where it is, or skip this step: `problems.txt` tells you whether your files loaded.",
};

/// The plugin host of one session: it scans this machine, keeps the plugins a project loads
/// and saves their state. `read_only` is for `--inspect` and `--render`, which never write.
///
/// The scan runs this same executable with [`plugin_host::SCAN_ARGUMENT`], one child process
/// per bundle, so a plugin that crashes while it is looked at costs one bundle.
pub fn plugins(read_only: bool) -> Result<Plugins> {
    with_cache(read_only, ScanCache::of_this_machine())
}

/// The host `runtime --plugins` uses: it looks at every bundle again, whatever the cache of
/// this machine remembers, and writes the result. That is how a plugin that hung or crashed
/// once is tried again.
pub fn plugins_refreshing_the_cache() -> Result<Plugins> {
    with_cache(true, ScanCache::of_this_machine().refreshing())
}

fn with_cache(read_only: bool, cache: ScanCache) -> Result<Plugins> {
    let scanner = ScanCommand::this_program()?;
    let paths = default_search_paths();
    Ok(if read_only {
        Plugins::read_only(paths, scanner, cache)
    } else {
        Plugins::new(paths, scanner, cache)
    })
}

/// Every bundled extension registers here.
pub fn registry(plugins: Plugins) -> Result<Registry> {
    let mut registry = Registry::new();
    arrangement::register(&mut registry)?;
    fit_tempo::register(&mut registry)?;
    instrument::register(&mut registry)?;
    plugin_host::register(&mut registry, plugins)?;
    tone::register(&mut registry)?;
    registry.runtime_agent_doc(INSPECT_DOC)?;
    // MIDI input registers no tool, so it has no extension to enable in `project.json`. Every
    // project can be recorded into, so its doc is one every project gets.
    registry.runtime_agent_doc(midi::AGENT_DOC)?;
    Ok(registry)
}

/// Every bundled extension with a view registers it here, and what a rack calls its devices
/// and what a composer can pick. The window names no view type and no instrument: it takes
/// both registries and installs them, see `Shell::new`.
///
/// This is the one place that knows the built-in synth and the plugin host at once. The
/// arrangement, which owns the track panel, depends on neither.
pub fn views(plugins: WeakPlugins) -> (Views, Devices) {
    let mut views = Views::new();
    let mut devices = Devices::new();
    arrangement::view::register(&mut views);
    instrument::view::register(&mut views, &mut devices);
    plugin_host::view::register(&mut views, &mut devices, plugins.clone());
    devices.instruments(|| {
        vec![
            DeviceOffer::new(
                SynthState::TOOL,
                instrument::view::NAME,
                |_, slot, changes| {
                    changes.create(slot.clone(), SynthState::default());
                    Ok(())
                },
            )
            .needs(instrument::EXTENSION),
        ]
    });
    // What the picker says under its offers: that the scan of this machine is still running,
    // and what Steinberg asks of anyone who writes "VST". Their guidelines want the VST
    // Compatible Logo next to the term and the attribution where the logo does not fit; a menu
    // row is such a place, so the attribution is what is shown. See the plugin host's README.
    devices.notes({
        let plugins = plugins.clone();
        move || {
            let Some(plugins) = plugins.upgrade() else {
                return Vec::new();
            };
            let mut notes = Vec::new();
            if plugins.scan_is_running() {
                notes.push("Still looking for the plugins of this Mac…".into());
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

/// The plugins `list` gives, as offers for a picker. Each writes the record of that plugin
/// into the slot it is picked for, with a state file of its own.
fn plugin_offers(
    plugins: &WeakPlugins,
    list: fn(&plugin_host::Plugins) -> Vec<plugin_host::ScannedPlugin>,
) -> Vec<DeviceOffer> {
    let Some(plugins) = plugins.upgrade() else {
        return Vec::new();
    };
    list(&plugins)
        .into_iter()
        .map(|found| {
            let (id, name, format) = (found.id.clone(), found.name.clone(), found.format);
            let offer = DeviceOffer::new(
                found.offer_key(),
                found.name.clone(),
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
                        },
                    );
                    Ok(())
                },
            )
            .needs(plugin_host::EXTENSION);
            match found.vendor.is_empty() {
                true => offer.with_detail(format.name()),
                false => offer.with_detail(format!("{} · {}", format.name(), found.vendor)),
            }
        })
        .collect()
}

/// The arrangement that "Add track" adds to: the first one at the top of the project.
pub fn main_arrangement(project: &Project) -> Option<Instance<ArrangementState>> {
    let mut instances = project.instances();
    let (id, _) =
        instances.find(|(id, tool)| id.parent().is_none() && *tool == ArrangementState::TOOL)?;
    project.resolve(id)
}

/// Adds `Track <n>` with the default synth, as one undo step. The next colour of the palette,
/// so that tracks are easy to tell apart. This and [`open_or_create`] are the two places that
/// know the default instrument.
pub fn add_track(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
) -> Result<(), ProjectError> {
    let count = arrangement::tracks(project, arrangement.id()).len();
    let colour = Colour::ALL[count % Colour::ALL.len()];
    let name = format!("Track {}", count + 1);
    let mut changes = Changes::new();
    let instrument = SynthState::default();
    arrangement::add_track(
        project,
        &mut changes,
        arrangement.id(),
        &name,
        colour,
        instrument,
    )?;
    project.commit("Add track", changes)
}

/// Opens the project with its lock, scanning for plugins on this thread the first time a
/// record needs one. `--headless` may block; the window uses [`open_or_create_with`] with a
/// host of its own that scans on a thread.
///
/// A folder without a `project.json` becomes the default
/// project: 120 bpm, 4/4, one arrangement with one track and its synth, no clips. Other
/// files in it, such as `.git` or `.DS_Store`, do not make it an existing project.
///
/// Making the default content is not something to undo, so a new project has no history.
pub fn open_or_create(folder: &Path, control: EngineControl) -> Result<(Project, Plugins)> {
    let plugins = plugins(false)?;
    let project = open_or_create_with(folder, control, plugins.clone())?;
    Ok((project, plugins))
}

/// [`open_or_create`] with a plugin host given, for tests that look for plugins in a folder of
/// their own instead of on this machine.
pub fn open_or_create_with(
    folder: &Path,
    control: EngineControl,
    plugins: Plugins,
) -> Result<Project> {
    let is_new = !folder.join(PROJECT_FILE).exists();
    let mut project = Project::open(folder, registry(plugins)?, control)?;
    // A `state/` folder with content but no project file is someone's work, not a new project.
    if is_new && project.instances().next().is_none() && project.problems().is_empty() {
        arrangement::create_default_project(&mut project, SynthState::default())?;
        project.clear_history();
    }
    Ok(project)
}

/// Opens the project without its lock, so it works next to a running runtime. The engine
/// renders it offline.
pub fn open_read_only(folder: &Path) -> Result<(Project, Engine, Plugins)> {
    let plugins = plugins(true)?;
    let (project, engine) = open_read_only_with(folder, plugins.clone())?;
    Ok((project, engine, plugins))
}

/// [`open_read_only`] with a plugin host given, for tests. See [`open_or_create_with`].
pub fn open_read_only_with(folder: &Path, plugins: Plugins) -> Result<(Project, Engine)> {
    let (control, engine) = Engine::new(OFFLINE);
    let project = Project::open_read_only(folder, registry(plugins)?, control)?;
    Ok((project, engine))
}

/// What `--inspect` prints. A tool with a summary of its own, such as the arrangement, tells
/// what it owns. Every other instance is one line with its record.
pub fn summary(project: &Project) -> String {
    let project_file = project.project_file();
    let tempo_map = &project_file.tempo_map;
    let time_signature = tempo_map.time_signature();
    let mut lines = vec![
        format!("extensions: {}", project_file.extensions.join(", ")),
        format!(
            "time signature: {time_signature}, {} ticks per bar, {} ticks per beat",
            time_signature.ticks_per_bar(),
            time_signature.ticks_per_beat()
        ),
    ];
    for change in tempo_map.tempo_changes() {
        let position = time_signature.bar_beat_of(change.tick);
        lines.push(format!(
            "tempo: {} bpm from {position} (tick {})",
            change.bpm.bpm(),
            change.tick.0
        ));
    }

    let mut summarised: Vec<&InstanceId> = Vec::new();
    for (id, tool) in project.instances() {
        // Ids come parents first, so what a summary covers follows it directly.
        if summarised.last().is_some_and(|owner| id.is_inside(owner)) {
            continue;
        }
        if let Some(summary) = project.summary(id) {
            lines.push(summary);
            summarised.push(id);
        } else {
            let indent = "  ".repeat(id.as_str().matches('/').count());
            let state = project.state_json(id).unwrap_or_default();
            lines.push(format!("{indent}instance `{id}` [{tool}] {state}"));
        }
    }

    lines.push(format!("connections: {}", project_file.connections.len()));
    for connection in &project_file.connections {
        let from = &connection.from;
        let to = match &connection.to {
            SavedDestination::DeviceOutput(channel) => format!("device output {channel}"),
            SavedDestination::Input(input) => format!("{}:{}", input.instance, input.port),
        };
        lines.push(format!("  {}:{} -> {to}", from.instance, from.port));
    }
    lines.push(problems(project));
    lines.join("\n")
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
    engine.process_block(output);
    project.engine().poll()?;
    Ok(plugins.poll(project))
}

/// Renders `frames` frames in device buffers of 512 frames, interleaved by channel.
pub fn render(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    frames: usize,
) -> Result<Vec<f32>> {
    let channels = engine.channels();
    let mut output = vec![0.0_f32; frames * channels];
    for buffer in output.chunks_mut(512 * channels) {
        render_block(project, engine, plugins, buffer)?;
    }
    Ok(output)
}
