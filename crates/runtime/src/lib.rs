//! What the project runtime is made of, apart from the command line in `main.rs`: the bundled
//! extensions and their views, the default project, the project summary, offline rendering and
//! the application window. Tests of whole projects, with every bundled extension, use this
//! crate.

pub mod window;

use std::path::Path;

use anyhow::Result;
use arrangement::{ArrangementState, Colour};
use instrument::SynthState;
use sound_core::{
    AgentDoc, Changes, Engine, EngineConfig, EngineControl, Instance, InstanceId, Project,
    ProjectError, Registry, SavedDestination, State,
};
use sound_ui::Views;

const PROJECT_FILE: &str = "project.json";

/// Offline renders have no device to ask.
pub const OFFLINE: EngineConfig = EngineConfig {
    sample_rate: 48_000,
    channels: 2,
    ring_capacity: 64,
    event_capacity: 256,
    processor_slots: 256,
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

/// Every bundled extension registers here.
pub fn registry() -> Result<Registry> {
    let mut registry = Registry::new();
    arrangement::register(&mut registry)?;
    instrument::register(&mut registry)?;
    tone::register(&mut registry)?;
    registry.runtime_agent_doc(INSPECT_DOC)?;
    // MIDI input registers no tool, so it has no extension to enable in `project.json`. Every
    // project can be recorded into, so its doc is one every project gets.
    registry.runtime_agent_doc(midi::AGENT_DOC)?;
    Ok(registry)
}

/// Every bundled extension with a view registers it here. The window names no view type.
/// `Shell::new` takes the result and installs it.
pub fn views() -> Views {
    let mut views = Views::new();
    arrangement::view::register(&mut views);
    instrument::view::register(&mut views);
    views
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

/// Opens the project with its lock. A folder without a `project.json` becomes the default
/// project: 120 bpm, 4/4, one arrangement with one track and its synth, no clips. Other
/// files in it, such as `.git` or `.DS_Store`, do not make it an existing project.
///
/// Making the default content is not something to undo, so a new project has no history.
pub fn open_or_create(folder: &Path, control: EngineControl) -> Result<Project> {
    let is_new = !folder.join(PROJECT_FILE).exists();
    let mut project = Project::open(folder, registry()?, control)?;
    // A `state/` folder with content but no project file is someone's work, not a new project.
    if is_new && project.instances().next().is_none() && project.problems().is_empty() {
        arrangement::create_default_project(&mut project, SynthState::default())?;
        project.clear_history();
    }
    Ok(project)
}

/// Opens the project without its lock, so it works next to a running runtime. The engine
/// renders it offline.
pub fn open_read_only(folder: &Path) -> Result<(Project, Engine)> {
    let (control, engine) = Engine::new(OFFLINE);
    let project = Project::open_read_only(folder, registry()?, control)?;
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

/// Renders `frames` frames in device buffers of 512 frames, interleaved by channel.
pub fn render(project: &mut Project, engine: &mut Engine, frames: usize) -> Result<Vec<f32>> {
    let channels = engine.channels();
    let mut output = vec![0.0_f32; frames * channels];
    for buffer in output.chunks_mut(512 * channels) {
        engine.process_block(buffer);
        project.engine().poll()?;
    }
    Ok(output)
}
