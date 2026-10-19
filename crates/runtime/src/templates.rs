//! What a new project starts as: the files of a folder of `templates/`, written into its empty
//! folder before it opens. Each folder there is one of [`TEMPLATES`], with what its
//! `template.json` says of it; `build.rs` embeds them.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::PROJECT_FILE;

pub struct Template {
    pub name: &'static str,
    pub description: &'static str,
    /// Its files by their path in the folder.
    pub files: &'static [(&'static str, &'static str)],
}

/// Every template, in the name order of their folders.
pub const TEMPLATES: &[Template] = include!(concat!(env!("OUT_DIR"), "/templates.rs"));

/// The template called `name`.
pub fn named(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.name == name)
}

/// Writes the files of `template` into `folder`, which must hold nothing but hidden files.
/// Makes the folder when it is missing.
pub fn write(folder: &Path, template: &Template) -> Result<()> {
    if folder.exists() && !is_empty(folder)? {
        let name = folder.file_name().unwrap_or(folder.as_os_str());
        bail!(
            "“{}” has files in it. Pick a new folder for a new project.",
            name.to_string_lossy()
        );
    }
    std::fs::create_dir_all(folder)
        .with_context(|| format!("could not make {}", folder.display()))?;
    for (path, contents) in template.files {
        let target = folder.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        let contents = match *path == PROJECT_FILE {
            true => with_extensions(contents)?,
            false => (*contents).to_string(),
        };
        std::fs::write(&target, contents)
            .with_context(|| format!("could not write {}", target.display()))?;
    }
    Ok(())
}

/// The `project.json` of a template, with every bundled extension enabled where it names none.
fn with_extensions(contents: &str) -> Result<String> {
    let mut file: serde_json::Value =
        serde_json::from_str(contents).context("the project.json of a template is no JSON")?;
    if let Some(file) = file.as_object_mut()
        && !file.contains_key("extensions")
    {
        file.insert("extensions".into(), crate::bundled_extensions()?.into());
    }
    Ok(serde_json::to_string_pretty(&file)?)
}

/// Whether `folder` holds nothing but hidden files.
pub(crate) fn is_empty(folder: &Path) -> Result<bool> {
    let entries = std::fs::read_dir(folder)
        .with_context(|| format!("could not read {}", folder.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("could not read {}", folder.display()))?;
        if !entry.file_name().to_string_lossy().starts_with('.') {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_writes_into_a_new_folder_only_with_every_extension() {
        let daw = named("DAW").unwrap();
        let folder = tempfile::tempdir().unwrap();
        let new = folder.path().join("new");
        write(&new, daw).unwrap();
        assert!(new.join("extensions/song.tsx").exists());
        assert!(!new.join("template.json").exists());
        let file = std::fs::read_to_string(new.join(PROJECT_FILE)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&file).unwrap();
        let enabled: Vec<&str> = (file["extensions"].as_array().unwrap().iter())
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(enabled, crate::bundled_extensions().unwrap());

        // A project, or someone's files: nothing is written, and the error says why.
        let error = write(&new, daw).unwrap_err();
        assert!(error.to_string().contains("Pick a new folder"), "{error}");
        let theirs = folder.path().join("theirs");
        std::fs::create_dir_all(theirs.join("state")).unwrap();
        std::fs::write(theirs.join("state/a.json"), "{}").unwrap();
        write(&theirs, daw).unwrap_err();
        assert!(!theirs.join(crate::PROJECT_FILE).exists());
    }
}
