//! What the project runtime is made of, apart from the command line in `main.rs`: the bundled
//! extensions and their views, the agent sidebar, the default project, the project summary, offline rendering and
//! the application window. Tests of whole projects, with every bundled extension, use this
//! crate.

pub mod app;
pub mod recorder;
pub mod update;
pub mod window;

use std::path::{Path, PathBuf};

use anyhow::Result;
use arrangement::{ArrangementState, Colour, TrackKind};
use compressor::CompressorState;
use delay::DelayState;
use drum_pad::DrumPadState;
use eq::EqState;
use filter::FilterState;
use gpui::{App, AppContext as _};
use instrument::SynthState;
use limiter::LimiterState;
use modulation::ModulationState;
use plugin_host::{
    PluginFormat, PluginRecord, Plugins, ScanCache, ScanCommand, VST_TRADEMARK, WeakPlugins,
    default_search_paths,
};
use reverb::ReverbState;
use sampler::SamplerState;
use saturator::SaturatorState;
use sound_agent::{AgentSettings, Sidebar};
use sound_core::{
    AgentDoc, Changes, Engine, EngineConfig, EngineControl, Instance, InstanceId, Project,
    ProjectError, Registry, SavedDestination, State, Ticks,
};
use sound_ui::{DeviceOffer, Devices, OfferGroup, Views};
use utility::UtilityState;
use wavetable::WavetableState;
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
    when: "You can run commands and want the whole piece in one read",
    markdown: "# Inspect from a command line

When you can run commands, the Sound Tools runtime prints where each time signature starts, the tempo, every track in order, every clip with its bar range, note count and pitch range, and the problems. It works while the project is open and changes nothing.

```sh
sound-tools . --inspect
```

`sound-tools` is the command line tool of the Sound Tools app. When it is not on your `PATH`, ask the composer to pick **Install command line tool** in the project menu, or skip this step: `problems.txt` tells you whether your files loaded.",
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
    instrument::view::register(&mut views, &mut devices);
    drum_pad::view::register(&mut views, &mut devices);
    filter::view::register(&mut views, &mut devices);
    compressor::view::register(&mut views, &mut devices);
    limiter::view::register(&mut views, &mut devices);
    eq::view::register(&mut views, &mut devices);
    delay::view::register(&mut views, &mut devices);
    reverb::view::register(&mut views, &mut devices);
    saturator::view::register(&mut views, &mut devices);
    utility::view::register(&mut views, &mut devices);
    modulation::view::register(&mut views, &mut devices);
    sampler::view::register(&mut views, &mut devices);
    wavetable::view::register(&mut views, &mut devices);
    plugin_host::view::register(&mut views, &mut devices, plugins.clone());
    devices.instruments(|| {
        vec![
            built_in::<SynthState>(
                instrument::view::NAME,
                OfferGroup::BuiltIn,
                "device-synth",
                instrument::EXTENSION,
                "This project does not load the synth.",
            ),
            built_in::<WavetableState>(
                wavetable::view::NAME,
                OfferGroup::BuiltIn,
                "device-wavetable",
                wavetable::EXTENSION,
                "This project does not load the Wavetable.",
            ),
            built_in::<SamplerState>(
                sampler::view::NAME,
                OfferGroup::BuiltIn,
                "device-sampler",
                sampler::EXTENSION,
                "This project does not load the sampler.",
            ),
            built_in::<DrumPadState>(
                drum_pad::view::NAME,
                OfferGroup::BuiltIn,
                "device-drum-pad",
                drum_pad::EXTENSION,
                "This project does not load the Drum pad.",
            ),
        ]
    });
    // The built-in effects come before the plugins of this Mac in the list, in the order a
    // picker shows them.
    devices.effects(|| {
        vec![
            built_in::<EqState>(
                eq::view::NAME,
                OfferGroup::Tone,
                "device-eq",
                eq::EXTENSION,
                "This project does not load the EQ.",
            ),
            built_in::<FilterState>(
                filter::view::NAME,
                OfferGroup::Tone,
                "device-filter",
                filter::EXTENSION,
                "This project does not load the filter.",
            ),
            built_in::<SaturatorState>(
                saturator::view::NAME,
                OfferGroup::Tone,
                "device-saturator",
                saturator::EXTENSION,
                "This project does not load the saturator.",
            ),
            built_in::<CompressorState>(
                compressor::view::NAME,
                OfferGroup::Dynamics,
                "device-compressor",
                compressor::EXTENSION,
                "This project does not load the compressor.",
            ),
            built_in::<LimiterState>(
                limiter::view::NAME,
                OfferGroup::Dynamics,
                "device-limiter",
                limiter::EXTENSION,
                "This project does not load the limiter.",
            ),
            built_in::<ModulationState>(
                modulation::view::NAME,
                OfferGroup::Space,
                "device-modulation",
                modulation::EXTENSION,
                "This project does not load the modulation.",
            ),
            built_in::<DelayState>(
                delay::view::NAME,
                OfferGroup::Space,
                "device-delay",
                delay::EXTENSION,
                "This project does not load the delay.",
            ),
            built_in::<ReverbState>(
                reverb::view::NAME,
                OfferGroup::Space,
                "device-reverb",
                reverb::EXTENSION,
                "This project does not load the reverb.",
            ),
            built_in::<UtilityState>(
                utility::view::NAME,
                OfferGroup::Mix,
                "device-utility",
                utility::EXTENSION,
                "This project does not load the utility.",
            ),
        ]
    });
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
    LeftPanelSlot::new(remembered, move |session, _, cx| {
        let (agents, threads, settings) = (agents.clone(), threads.clone(), settings.clone());
        let sidebar = cx.new(|cx| Sidebar::new(session, agents, threads, settings, cx));
        LeftPanel::new(sidebar, Sidebar::is_busy, cx)
    })
}

/// A built-in device at its defaults, which a project that does not enable `extension` shows
/// and does not take.
fn built_in<S: State + Default>(
    name: &'static str,
    group: OfferGroup,
    icon: &'static str,
    extension: &'static str,
    reason: &'static str,
) -> DeviceOffer {
    DeviceOffer::new(S::TOOL, name, group, |_, slot, changes| {
        changes.create(slot.clone(), S::default());
        Ok(())
    })
    .icon(icon)
    .needs(extension, reason)
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

/// The name and colour of the next track: `Track <n>`, in the next colour of the palette, so
/// that tracks are easy to tell apart.
fn next_track(project: &Project, arrangement: &Instance<ArrangementState>) -> (String, Colour) {
    let count = arrangement::tracks(project, arrangement.id()).len();
    let colour = Colour::ALL[count % Colour::ALL.len()];
    (format!("Track {}", count + 1), colour)
}

/// Adds `Track <n>` with the default synth, as one undo step, in the next colour of the
/// palette. This and [`open_or_create`] are the two places that know the default instrument.
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

/// Opens the project the way `--inspect` does: without its lock, and with a host that looks a
/// plugin up and loads none ([`Plugins::listing`]). Inspecting prints a project and makes no
/// sound, so no third-party code runs in this process and no plugin can end it.
pub fn open_for_inspect(folder: &Path) -> Result<Project> {
    let plugins = plugins_for_inspecting()?;
    let (project, _engine) = open_read_only_with(folder, plugins)?;
    Ok(project)
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
    // that were asked for are waited for and put in their kits first.
    drum_pad::wait_for_sounds();
    take_drum_sounds(project)?;
    engine.process_block(output);
    project.engine().poll()?;
    let mut problems = plugins.poll(project);
    problems.extend(plugins.send_restarts(project));
    // A plugin that asked to be loaded again gets what a record that changed gets.
    for instance in plugins.take_retries() {
        project.rebind(&instance)?;
    }
    Ok(problems)
}

/// Runs the behaviour of every Drum pad whose sounds were made since the last call, which puts
/// them in its kit. What every loop of a session calls, as it polls the plugin host.
pub fn take_drum_sounds(project: &mut Project) -> Result<(), ProjectError> {
    // Held until the behaviour has put them in the kit, see `drum_pad::take_ready`.
    for (instance, _sounds) in drum_pad::take_ready(project.assets()) {
        project.rebind(&instance)?;
    }
    Ok(())
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
