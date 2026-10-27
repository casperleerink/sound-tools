//! A file of the project as a composer knows it: a part by the names its parts have, the
//! project's settings, the code of a tool, the docs. Where the agent read or changed something,
//! or a file is not live, the sidebar says it so, not as a path.

use gpui::{App, Entity};
use sound_core::InstanceId;

use crate::session::Session;

/// What the file at `path`, relative to the project folder, is to a composer: `Bass · Clip 1`,
/// the project's settings, the code of Song tracks. A path it knows nothing of stays a path.
pub fn file_name(session: &Entity<Session>, path: &str, cx: &App) -> String {
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
        Some(id) => part_name(session, &id, cx),
        None => path.to_string(),
    }
}

/// `path` without the first of `ends` it ends in.
fn stem<'a>(path: &'a str, ends: &[&str]) -> Option<&'a str> {
    ends.iter().find_map(|end| path.strip_suffix(end))
}

/// A part by the name of each part it is in, the page that holds them all left out: `Bass ·
/// Clip 1`. Each by its record's `title`, else by its name, as the song names its tracks.
fn part_name(session: &Entity<Session>, id: &InstanceId, cx: &App) -> String {
    let mut path: Vec<InstanceId> = id.ancestors().collect();
    path.reverse();
    path.push(id.clone());
    let shown = if path.len() > 1 {
        &path[1..]
    } else {
        &path[..]
    };
    let project = session.read(cx).project();
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
