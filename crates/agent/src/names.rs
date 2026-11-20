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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use sound_core::{Engine, EngineConfig, Registry};

    use super::*;

    /// A part with a title, which may own others.
    #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Part {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        title: String,
    }

    impl sound_core::State for Part {
        const TOOL: &'static str = "test.part";
        const OWNS_CHILDREN: bool = true;
    }

    /// A composer reads "Bass · Verse", not a path: each part by its title, else by its name,
    /// the page that holds them all left out.
    #[test]
    fn a_file_of_the_song_is_named_by_its_parts() {
        let folder = tempfile::tempdir().unwrap();
        let files = [
            ("song/instance.json", ""),
            ("song/t00/instance.json", "Bass"),
            ("song/t00/c00/instance.json", "Verse"),
            ("song/t01/instance.json", ""),
        ];
        for (path, title) in files {
            let path = folder.path().join("state").join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let record = serde_json::json!({ "tool": "test.part", "state": { "title": title } });
            std::fs::write(path, record.to_string()).unwrap();
        }
        let mut registry = Registry::new();
        registry.tool::<Part>("test").unwrap();
        let (control, _engine) = Engine::new(EngineConfig::new(48_000, 1));
        let project = Project::open(folder.path(), registry, control).unwrap();
        assert_eq!(project.problems(), []);
        let name = |path: &str| file_name(&project, path);
        assert_eq!(name("state/song/t00/c00/instance.json"), "Bass · Verse");
        assert_eq!(name("state/song/t01/instance.json"), "T01");
        assert_eq!(name("project.json"), "the project's settings");
        assert_eq!(name("extensions/song-tracks.tsx"), "the code of Song tracks");
    }
}
