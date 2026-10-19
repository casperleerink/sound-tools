//! What a new project starts as: files written into its empty folder before it opens, then
//! records made once it is open. The window offers each of [`TEMPLATES`].

use std::path::Path;

use anyhow::{Context as _, Result};
use sound_core::{Project, ProjectError};

use crate::{NEW_PROJECT, PROJECT_FILE};

#[derive(Clone, Copy)]
pub struct Template {
    pub name: &'static str,
    /// Its files by their path in the folder. A `project.json` that names no `extensions`
    /// enables every bundled one, as a project the core makes does.
    pub files: &'static [(&'static str, &'static str)],
    /// Its first records once the folder is open: not an edit to undo, so a new project has no
    /// history. Runs only for a folder that had no `project.json` and no records.
    pub create: fn(&mut Project) -> Result<(), ProjectError>,
}

/// Nothing in `state/`: the tools the composer and the agent write.
pub const BARE: Template = Template {
    name: "Bare",
    files: &[(
        PROJECT_FILE,
        r#"{"format": 1, "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]}, "connections": []}"#,
    )],
    create: |_| Ok(()),
};

/// A song of tracks, clips, a mixer and a note editor, written in TypeScript with the SDK as any
/// tool of a project is: one instrument track and one audio track to start with.
pub const DAW: Template = Template {
    name: "DAW",
    files: &[
        (
            "extensions/bus.ts",
            include_str!("../../../templates/daw/extensions/bus.ts"),
        ),
        (
            "extensions/chain.ts",
            include_str!("../../../templates/daw/extensions/chain.ts"),
        ),
        (
            "extensions/clip-editor.tsx",
            include_str!("../../../templates/daw/extensions/clip-editor.tsx"),
        ),
        (
            "extensions/song.tsx",
            include_str!("../../../templates/daw/extensions/song.tsx"),
        ),
        (
            "extensions/tape.ts",
            include_str!("../../../templates/daw/extensions/tape.ts"),
        ),
        (
            "extensions/track.ts",
            include_str!("../../../templates/daw/extensions/track.ts"),
        ),
        (
            "project.json",
            include_str!("../../../templates/daw/project.json"),
        ),
        (
            "state/song/instance.json",
            include_str!("../../../templates/daw/state/song/instance.json"),
        ),
        (
            "state/song/limit.json",
            include_str!("../../../templates/daw/state/song/limit.json"),
        ),
        (
            "state/song/master.json",
            include_str!("../../../templates/daw/state/song/master.json"),
        ),
        (
            "state/song/quiet.json",
            include_str!("../../../templates/daw/state/song/quiet.json"),
        ),
        (
            "state/song/t00/chain/instance.json",
            include_str!("../../../templates/daw/state/song/t00/chain/instance.json"),
        ),
        (
            "state/song/t00/fx.json",
            include_str!("../../../templates/daw/state/song/t00/fx.json"),
        ),
        (
            "state/song/t00/instance.json",
            include_str!("../../../templates/daw/state/song/t00/instance.json"),
        ),
        (
            "state/song/t00/mix.json",
            include_str!("../../../templates/daw/state/song/t00/mix.json"),
        ),
        (
            "state/song/t00/rec.json",
            include_str!("../../../templates/daw/state/song/t00/rec.json"),
        ),
        (
            "state/song/t00/synth.json",
            include_str!("../../../templates/daw/state/song/t00/synth.json"),
        ),
        (
            "state/song/voice/instance.json",
            include_str!("../../../templates/daw/state/song/voice/instance.json"),
        ),
        (
            "state/song/voice/mix.json",
            include_str!("../../../templates/daw/state/song/voice/mix.json"),
        ),
        (
            "state/song/voice/rec.json",
            include_str!("../../../templates/daw/state/song/voice/rec.json"),
        ),
    ],
    create: |_| Ok(()),
};

/// Every template a new project can start from, the default first.
pub const TEMPLATES: [Template; 3] = [NEW_PROJECT, DAW, BARE];

/// Writes the files of `template` into `folder` when it is a new project: no `project.json` and
/// nothing in it but hidden files. Makes the folder when it is missing. Any other folder is left
/// as it is.
pub fn write(folder: &Path, template: &Template) -> Result<()> {
    if !is_new(folder)? {
        return Ok(());
    }
    for (path, contents) in template.files {
        let target = folder.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        let contents = match *path {
            PROJECT_FILE => with_extensions(contents)?,
            _ => (*contents).to_string(),
        };
        std::fs::write(&target, contents)
            .with_context(|| format!("could not write {}", target.display()))?;
    }
    Ok(())
}

fn is_new(folder: &Path) -> Result<bool> {
    if !folder.exists() {
        return Ok(true);
    }
    for entry in
        std::fs::read_dir(folder).with_context(|| format!("could not read {}", folder.display()))?
    {
        let entry = entry.with_context(|| format!("could not read {}", folder.display()))?;
        if !entry.file_name().to_string_lossy().starts_with('.') {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `project`, with every bundled extension when it names none.
fn with_extensions(project: &str) -> Result<String> {
    let mut file: serde_json::Value =
        serde_json::from_str(project).context("the project.json of a template")?;
    if let Some(fields) = file.as_object_mut()
        && !fields.contains_key("extensions")
    {
        fields.insert("extensions".into(), crate::bundled_extensions()?.into());
    }
    Ok(serde_json::to_string_pretty(&file)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_writes_into_a_new_folder_only_and_a_bare_one_enables_every_extension() {
        let folder = tempfile::tempdir().unwrap();
        let new = folder.path().join("new");
        write(&new, &BARE).unwrap();
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new.join(PROJECT_FILE)).unwrap())
                .unwrap();
        let enabled = file["extensions"].as_array().unwrap();
        assert!(enabled.iter().any(|name| name == "instrument"));
        assert!(enabled.iter().any(|name| name == "plugin-host"));

        // Someone's files: nothing is written.
        let theirs = folder.path().join("theirs");
        std::fs::create_dir_all(theirs.join("state")).unwrap();
        std::fs::write(theirs.join("state/a.json"), "{}").unwrap();
        write(&theirs, &DAW).unwrap();
        assert!(!theirs.join(PROJECT_FILE).exists());
    }
}
