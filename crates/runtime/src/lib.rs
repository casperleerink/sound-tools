//! What the project runtime is made of, apart from the command line in `main.rs`: the bundled
//! extensions and their views, the agent sidebar, the default project, the project summary, offline rendering and
//! the application window. Tests of whole projects, with every bundled extension, use this
//! crate.

pub mod analysis;
pub mod app;
pub mod recorder;
pub mod update;
pub mod window;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use arrangement::{ArrangementState, Colour, TrackKind, TrackState};
use gpui::{App, AppContext as _};
use instrument::SynthState;
use plugin_host::{
    Parameter, PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand, VST_TRADEMARK,
    WeakPlugins, default_search_paths,
};
use sound_agent::{AgentSettings, Sidebar};
use sound_core::{
    AgentDoc, Changes, Engine, EngineConfig, EngineControl, Instance, InstanceId, Project,
    ProjectError, Registry, SavedDestination, State, Ticks,
};
use sound_ui::{DeviceOffer, Devices, OfferGroup, Views};
use window::{LeftPanel, LeftPanelSlot};

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
    when: "You can run commands and want to check how your work sounds, read the whole piece in one go, or write it as a WAV",
    markdown: "# Inspect, measure and render from a command line

When you can run commands, the Sound Tools runtime reads the project for you. Every form works while the project is open and changes nothing.

```sh
sound-tools . --inspect
sound-tools . --analyze
sound-tools . --render <wav>
```

- `--inspect` prints where each time signature starts, the tempo, every track in order, every clip with its bar range, note count and pitch range, and the problems.
- `--analyze` plays the piece and its tail offline and prints what it measures, of the whole and in up to 32 rows. Use it after a change, to check that it sounds as you meant.
- `--render` writes the piece as a 48 kHz stereo 32-bit float WAV and prints its peak.

`--analyze` and `--render` take the same options:

- `--from <at>` and `--to <at>`, together or alone: a part of the piece, and its tail. `<at>` is ticks (`3840`), or a time as the composer reads it off the playhead (`1:23.5`) or in seconds (`83.5s`). When the composer says what they heard at a time, measure that time.
- `--seconds <n>`: the first `n` seconds, with no tail.
- `--solo <track>`, once per track: only these tracks play, by name or id. No file changes.

`sound-tools <file> --analyze` measures an audio file, such as a sample under `assets/audio/`, with `--from` and `--to` as times.

## What `--analyze` prints

The first line is the whole: its loudness (integrated, gated as streaming services measure it), the loudest 400 ms, the true peak and where it is, and its pitch and drift. Each row then starts at a tick, a bar and a time:

- `LUFS`: the loudness of the row, weighted like hearing. `max`: its loudest 400 ms, which shows short hits that the loudness of the row hides.
- `peak`: the true peak in dBTP, between the samples too. Above 0 clips.
- `sub` under 60 Hz, `bass` 60 to 250, `lowmid` 250 to 500, `mid` 500 to 2k, `highmid` 2k to 6k, `high` above 6k: the level of each band in dB.
- `width`: the share of the sound in the side. 0% is mono, 50% as wide as two unrelated channels; above 50% the channels cancel when summed to mono.
- `pitch`: the middle pitch heard, as the nearest note and how far off it in cents, 100 to a semitone: `A4+3c` is 3 cents sharp. `drift`: how far the pitch moves, low to high, in cents. A steady note in tune reads `+0c` and `0c`; a vibrato of 20 cents either way about `40c`; two notes in one row their interval, `200c` for a whole tone.
- `-` is silence. A track that should play and measures `-` alone did not load or plays nothing. Under `pitch`, `-` is also sound with no single pitch: noise, drums, a chord, a mix, or a note under E1 or above B7.

To check a vibrato, a wobble or the tuning, solo the track and measure one held note with `--from` and `--to`: the first line gives the drift of the whole note. A row shorter than one cycle of a slow wobble shows only part of it.

A mono file is measured as it plays in a project, on both channels: 3 dB above what a meter of one channel shows.

Compare rows and runs, not rules of thumb: the same part before and after your change, a soloed track against the whole, a verse against a chorus. The numbers do not say whether it sounds good; the composer does.

`sound-tools` is the command line tool of the Sound Tools app. When it is not on your `PATH`, ask the composer to pick **Install command line tool** in the project menu (on Windows the installer puts it there), or skip this step: `problems.txt` tells you whether your files loaded.",
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
/// this machine remembers, and writes the result. That is how a plugin that hung or crashed
/// once is tried again.
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
    arrangement::register(&mut registry)?;
    compressor::register(&mut registry)?;
    delay::register(&mut registry)?;
    drum_pad::register(&mut registry)?;
    eq::register(&mut registry)?;
    filter::register(&mut registry)?;
    fit_tempo::register(&mut registry)?;
    instrument::register(&mut registry)?;
    limiter::register(&mut registry)?;
    modulation::register(&mut registry)?;
    plugin_host::register(&mut registry, plugins)?;
    reverb::register(&mut registry)?;
    sampler::register(&mut registry)?;
    saturator::register(&mut registry)?;
    script::register(&mut registry)?;
    tone::register(&mut registry)?;
    utility::register(&mut registry)?;
    wavetable::register(&mut registry)?;
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
    arrangement::view::register(&mut views, add_track_of_kind);
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
    limiter::view::register(&mut views, &mut devices);
    modulation::view::register(&mut views, &mut devices);
    delay::view::register(&mut views, &mut devices);
    reverb::view::register(&mut views, &mut devices);
    utility::view::register(&mut views, &mut devices);
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
    LeftPanelSlot::new(remembered, move |session, window, cx| {
        let (agents, threads, settings) = (agents.clone(), threads.clone(), settings.clone());
        let sidebar = cx.new(|cx| Sidebar::new(session, agents, threads, settings, window, cx));
        LeftPanel::new(sidebar, Sidebar::is_busy, cx)
    })
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
        })
        .collect()
}

/// The arrangement of the piece: the first one at the top of the project.
pub fn main_arrangement(project: &Project) -> Option<Instance<ArrangementState>> {
    let mut instances = project.instances();
    let (id, _) =
        instances.find(|(id, tool)| id.parent().is_none() && *tool == ArrangementState::TOOL)?;
    project.resolve(id)
}

/// Solos the tracks of the main arrangement that `names` names, by name or by id, and unmutes
/// them: every other track goes silent. Only in memory, so a render of a read-only project can
/// play one part alone and leave every file as it is. Call it before the render plays. Gives
/// what the plugin host reported meanwhile.
pub fn solo(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    names: &[String],
) -> Result<Vec<plugin_host::PluginProblem>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let Some(arrangement) = main_arrangement(project) else {
        bail!("the project has no arrangement, so it has no tracks to solo");
    };
    let tracks = arrangement::tracks(project, arrangement.id());
    let called = |(track, state): &(Instance<TrackState>, &TrackState), name: &String| {
        state.name == *name || track.id().as_str() == name
    };
    if let Some(missing) = names
        .iter()
        .find(|name| !tracks.iter().any(|track| called(track, name)))
    {
        let all: Vec<String> = tracks
            .iter()
            .map(|(_, state)| format!("{:?}", state.name))
            .collect();
        bail!(
            "no track is called {missing:?}. The tracks: {}",
            all.join(", ")
        );
    }
    let mut changes = Changes::new();
    for track in &tracks {
        let mut state = track.1.clone();
        state.solo = names.iter().any(|name| called(track, name));
        state.mute &= !state.solo;
        changes.set(&track.0, state);
    }
    project.apply_in_memory(changes)?;
    // A track that changes its mute fades, which would fade in the start of the render. The
    // engine runs stopped until the fades are over.
    let fade = arrangement::RAMP_SECONDS * engine.sample_rate() as f32;
    render_into(project, engine, plugins, 2 * fade.ceil() as usize, |_| {
        Ok(())
    })
}

/// The name and colour of the next track: `Track <n>`, in the next colour of the palette, so
/// that tracks are easy to tell apart.
fn next_track(project: &Project, arrangement: &Instance<ArrangementState>) -> (String, Colour) {
    let count = arrangement::tracks(project, arrangement.id()).len();
    let colour = Colour::ALL[count % Colour::ALL.len()];
    (format!("Track {}", count + 1), colour)
}

/// Adds `Track <n>` with the default synth, as one undo step, in the next colour of the
/// palette. This is the one place that knows the default instrument.
pub fn add_track(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
) -> Result<(), ProjectError> {
    let (name, colour) = next_track(project, arrangement);
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

/// Adds an audio track `Track <n>`, empty, as one undo step, in the next colour of the palette.
pub fn add_audio_track(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
) -> Result<(), ProjectError> {
    let (name, colour) = next_track(project, arrangement);
    let mut changes = Changes::new();
    arrangement::add_audio_track(project, &mut changes, arrangement.id(), &name, colour)?;
    project.commit("Add audio track", changes)
}

/// [`add_track`] or [`add_audio_track`]: what the add track button of the arrangement calls.
/// The arrangement knows no instrument, so the window hands it this.
pub fn add_track_of_kind(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
    kind: TrackKind,
) -> Result<(), ProjectError> {
    match kind {
        TrackKind::Instrument => add_track(project, arrangement),
        TrackKind::Audio => add_audio_track(project, arrangement),
    }
}

/// Opens the project with its lock. The plugin host is given: the application passes
/// [`plugins`], which scans this machine, and a test passes one that looks in a folder of its
/// own. A host that was not told to scan on a thread scans on this one, the first time a
/// record needs a plugin.
///
/// A folder without a `project.json` becomes the default
/// project: 120 bpm, 4/4, one arrangement with no tracks. Other
/// files in it, such as `.git` or `.DS_Store`, do not make it an existing project.
///
/// Making the default content is not something to undo, so a new project has no history.
pub fn open_or_create_with(
    folder: &Path,
    control: EngineControl,
    plugins: Plugins,
) -> Result<Project> {
    let is_new = !folder.join(PROJECT_FILE).exists();
    let mut project = Project::open(folder, registry(plugins)?, control)?;
    // A `state/` folder with content but no project file is someone's work, not a new project.
    if is_new && project.instances().next().is_none() && project.problems().is_empty() {
        arrangement::create_default_project(&mut project)?;
        project.clear_history();
    }
    Ok(project)
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
    let project = Project::open_read_only(folder, registry(plugins)?, control)?;
    Ok((project, engine))
}

/// What `--inspect` prints. A tool with a summary of its own, such as the arrangement, tells
/// what it owns. Every other instance is one line with its record.
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

    let mut summarised: Vec<&InstanceId> = Vec::new();
    for (id, tool) in project.instances() {
        // Ids sort as text, so `arrangement-2` comes between `arrangement` and what is inside
        // it: every summarised owner is checked, not only the last.
        if summarised.iter().any(|owner| id.is_inside(owner)) {
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
    // with the rest of the tick.
    drum_pad::wait_for_sounds();
    first(take_drum_sounds(project))?;
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

/// Plays `from..to` and then the tail: what reverbs and releases still sound after the
/// transport stops at `to`. The tail ends after half a second of silence, or after ten seconds
/// for a sound that never ends, such as the noise of a plugin. See [`render_into`].
pub fn render_range(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    from: Ticks,
    to: Ticks,
    mut write: impl FnMut(&[f32]) -> Result<()>,
) -> Result<Vec<plugin_host::PluginProblem>> {
    let clock = project.clock();
    let frames = clock.frame_of(to).0.saturating_sub(clock.frame_of(from).0);
    project.engine().seek(from);
    project.engine().play();
    let mut problems = render_into(project, engine, plugins, frames as usize, &mut write)?;
    // Stopping releases every held note, which is where the tail starts.
    project.engine().pause();
    let channels = engine.channels();
    let rate = engine.sample_rate() as usize;
    let (mut quiet, mut tail) = (0, 0);
    while quiet < rate / 2 && tail < rate * 10 {
        problems.extend(render_into(project, engine, plugins, 512, |samples| {
            let frames = samples.len() / channels;
            quiet = if samples.iter().all(|sample| sample.abs() < SILENT) {
                quiet + frames
            } else {
                0
            };
            tail += frames;
            write(samples)
        })?);
    }
    Ok(problems)
}

/// The time `clips` cover together, from the first start to the last end, which is what the
/// window exports as the selection. `None` when none of them is a clip.
pub fn clips_span(project: &Project, clips: &[InstanceId]) -> Option<(Ticks, Ticks)> {
    let spans = clips.iter().filter_map(|id| {
        if let Some(clip) = project.resolve::<sound_notes::Clip>(id) {
            let clip = project.state(&clip)?;
            return Some((clip.start, clip.end()));
        }
        let clip = project.resolve::<arrangement::AudioClip>(id)?;
        let clip = project.state(&clip)?;
        Some((clip.start, arrangement::audio_clip_end(project, clip)))
    });
    spans.reduce(|(start, end), (from, to)| (start.min(from), end.max(to)))
}

/// Where a render of the whole project ends: the end of the last clip of the main
/// arrangement, before the tail. `None` when it has no clips.
pub fn project_end(project: &Project) -> Option<Ticks> {
    arrangement::end(project, main_arrangement(project)?.id())
}
