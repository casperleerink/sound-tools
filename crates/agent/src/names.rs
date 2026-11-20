//! The words a composer knows a file of the project by, for the steps of the agent and the
//! problems it left: "Bass · Clip 1", "the code of Song tracks", "your instructions", not a
//! path.

use sound_core::{InstanceId, Project};

pub fn file_name(project: &Project, path: &str) -> String {
    match path {
        "project.json" => return "the project's settings".to_string(),
        "instructions.md" => return "your instructions".to_string(),
        "problems.txt" => return "what is not live".to_string(),
        "AGENTS.md" | "CLAUDE.md" => return "the docs".to_string(),
        _ if path.starts_with("agent-docs/") => return "the docs".to_string(),
        _ => {}
    }
    if let Some(tool) =
        (path.strip_prefix("extensions/")).and_then(|file| stem(file, &[".tsx", ".ts"]))
    {
        return format!("the code of {}", words(tool));
    }
    let id = (path.strip_prefix("state/"))
        .and_then(|file| stem(file, &[".json"]))
        .map(|id| id.strip_suffix("/instance").unwrap_or(id))
        .and_then(|id| InstanceId::new(id).ok());
    match id {
        Some(id) => part_name(project, &id),
        None => path.to_string(),
    }
}

/// `path` without the first of `ends` it ends in.
fn stem<'a>(path: &'a str, ends: &[&str]) -> Option<&'a str> {
    ends.iter().find_map(|end| path.strip_suffix(end))
}

/// A part by the name of each part it is in, the page that holds them all left out: `Bass ·
/// Clip 1`. Each by its record's `title`, else by its name, as the song names its tracks.
fn part_name(project: &Project, id: &InstanceId) -> String {
    let mut path: Vec<InstanceId> = id.ancestors().collect();
    path.reverse();
    path.push(id.clone());
    let shown = path
        .get(1..)
        .filter(|inner| !inner.is_empty())
        .unwrap_or(&path);
    let name = |id: &InstanceId| {
        (project.state_json(id))
            .and_then(|state| serde_json::from_str::<serde_json::Value>(&state).ok())
            .and_then(|state| state.get("title")?.as_str().map(str::to_string))
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| words(id.name()))
    };
    shown.iter().map(name).collect::<Vec<_>>().join(" · ")
}

/// A name in words: `clip-001` is Clip 001, `song_tracks` Song tracks.
fn words(name: &str) -> String {
    let spaced = name.replace(['-', '_', '.'], " ");
    let mut letters = spaced.trim().chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}
