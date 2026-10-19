//! What a new project starts as: the files of a folder of `templates/`, written into its empty
//! folder before it opens. Each folder there is one of [`TEMPLATES`], with what its
//! `template.json` says of it; `build.rs` embeds them.

use std::path::Path;

use anyhow::{Context as _, Result};

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

/// Writes the files of `template` into `folder` when it is a new project: nothing in it but
/// hidden files. Makes the folder when it is missing. Any other folder is left as it is.
pub fn write(folder: &Path, template: &Template) -> Result<()> {
    if !is_new(folder)? {
        return Ok(());
    }
    std::fs::create_dir_all(folder)
        .with_context(|| format!("could not make {}", folder.display()))?;
    for (path, contents) in template.files {
        let target = folder.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_writes_into_a_new_folder_only() {
        let daw = named("DAW").unwrap();
        let folder = tempfile::tempdir().unwrap();
        let new = folder.path().join("new");
        write(&new, daw).unwrap();
        assert!(new.join("extensions/song.tsx").exists());
        assert!(!new.join("template.json").exists());

        // Someone's files: nothing is written.
        let theirs = folder.path().join("theirs");
        std::fs::create_dir_all(theirs.join("state")).unwrap();
        std::fs::write(theirs.join("state/a.json"), "{}").unwrap();
        write(&theirs, daw).unwrap();
        assert!(!theirs.join(crate::PROJECT_FILE).exists());
    }
}
