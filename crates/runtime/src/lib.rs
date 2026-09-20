//! What the project runtime is made of, apart from the live loop in `main.rs`: the bundled
//! extensions, the default project, the project summary and offline rendering. Tests of whole
//! projects, with every bundled extension, use this crate.

use std::path::Path;

use anyhow::Result;
use instrument::SynthState;
use sound_core::{
    Engine, EngineConfig, EngineControl, InstanceId, Project, Registry, SavedDestination,
};

/// Offline renders have no device to ask.
pub const OFFLINE: EngineConfig = EngineConfig {
    sample_rate: 48_000,
    channels: 2,
    ring_capacity: 64,
    event_capacity: 256,
    processor_slots: 256,
};

/// Every bundled extension registers here.
pub fn registry() -> Result<Registry> {
    let mut registry = Registry::new();
    arrangement::register(&mut registry)?;
    instrument::register(&mut registry)?;
    tone::register(&mut registry)?;
    if let Ok(executable) = std::env::current_exe() {
        registry.runtime_agent_doc_section(format!(
            "## Inspect from a command line\n\nWhen you can run commands, this prints the tempo, every track in order, every clip with its bar range, note count and pitch range, and the problems. It works while the project is open and changes nothing.\n\n```sh\n{} . --inspect\n```",
            executable.display()
        ));
    }
    Ok(registry)
}

/// Opens the project with its lock. An empty or missing folder becomes the default project:
/// 120 bpm, 4/4, one arrangement with one track and its synth, no clips.
pub fn open_or_create(folder: &Path, control: EngineControl) -> Result<Project> {
    let is_empty = match std::fs::read_dir(folder) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true,
    };
    let mut project = Project::open(folder, registry()?, control)?;
    if is_empty {
        arrangement::create_default_project(&mut project, SynthState::default())?;
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
