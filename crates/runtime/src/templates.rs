//! What a new project starts as: the files of a folder of `templates/`, written into its empty
//! folder before it opens. Each folder there is one of [`TEMPLATES`], with what its
//! `template.json` says of it; `build.rs` embeds them.
//!
//! A project keeps the template's code as it wrote it, in [`KEPT`], so that a later release
//! with a newer template can [`update`] what the project never changed and leave the rest to a
//! merge.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::PROJECT_FILE;

mod written;

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

/// Writes the files of `template` into `folder`, which must be new or an empty project: one
/// that holds nothing but its `project.json` and what the runtime writes. Makes the folder when
/// it is missing.
pub fn write(folder: &Path, template: &Template) -> Result<()> {
    if folder.exists() && !is_empty_project(folder, folder)? {
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
    let kept = folder.join(KEPT);
    for (path, contents) in code(template) {
        put(&kept.join(path), Some(contents))?;
    }
    write_kept_file(folder, template)
}

/// The hidden folder where a project made from a template keeps the template's name and
/// version, in `template.json`, and its code as the project last took it.
pub const KEPT: &str = ".template";
/// Where a merge puts the template's code from before it, beside the new in [`KEPT`].
const PREVIOUS: &str = "previous";

/// The template's code: what a newer release may change. Its records and `project.json` are
/// the project's own from the start.
fn code(template: &Template) -> impl Iterator<Item = (&'static str, &'static str)> {
    (template.files.iter().copied()).filter(|(path, _)| path.starts_with("extensions/"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KeptFile {
    name: String,
    /// The release whose template the kept code is.
    version: String,
}

fn write_kept_file(folder: &Path, template: &Template) -> Result<()> {
    let file = KeptFile {
        name: template.name.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    put(
        &folder.join(KEPT).join("template.json"),
        Some(&serde_json::to_string_pretty(&file)?),
    )
}

/// A newer template's code for a project that changed the same files: they wait for a merge.
pub struct Update {
    pub template: &'static Template,
    /// Paths in the project.
    pub files: Vec<String>,
}

/// Brings the code of the template the project was made from up to this release's: each file
/// the project never changed becomes the template's, a new one is added and one the template
/// no longer has goes. A file the project changed stays as it is and is in the [`Update`] for a
/// merge. A project made before the template was kept, or from a template this release has not,
/// is left alone. Doing it again changes nothing.
pub fn update(folder: &Path) -> Result<Option<Update>> {
    let kept = folder.join(KEPT);
    let Ok(file) = std::fs::read_to_string(kept.join("template.json")) else {
        return Ok(None);
    };
    let file: KeptFile = serde_json::from_str(&file)
        .with_context(|| format!("{KEPT}/template.json is not what this release wrote"))?;
    let Some(template) = named(&file.name) else {
        return Ok(None);
    };
    let mut paths: Vec<String> = code(template).map(|(path, _)| path.to_string()).collect();
    for path in files_under(&kept, &kept.join("extensions"))? {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let mut files = Vec::new();
    for path in paths {
        let newer = code(template)
            .find(|(each, _)| *each == path)
            .map(|(_, contents)| contents);
        let before = read(&kept.join(&path))?;
        if before.as_deref() == newer {
            continue;
        }
        let ours = read(&folder.join(&path))?;
        if ours == before {
            put(&folder.join(&path), newer)?;
        } else if ours.as_deref() != newer {
            files.push(path);
            continue;
        }
        put(&kept.join(&path), newer)?;
    }
    if files.is_empty() {
        if file.version != env!("CARGO_PKG_VERSION") {
            write_kept_file(folder, template)?;
        }
        return Ok(None);
    }
    Ok(Some(Update { template, files }))
}

/// Starts the merge of `update`: the kept code becomes the new template's, the code it had
/// moves to `previous`, and what to ask the agent, which merges with the three at hand.
pub fn begin_merge(folder: &Path, update: &Update) -> Result<String> {
    let kept = folder.join(KEPT);
    let previous = kept.join(PREVIOUS);
    for path in &update.files {
        put(&previous.join(path), read(&kept.join(path))?.as_deref())?;
        let newer = code(update.template).find(|(each, _)| each == path);
        put(&kept.join(path), newer.map(|(_, contents)| contents))?;
    }
    write_kept_file(folder, update.template)?;
    let list: String = update
        .files
        .iter()
        .map(|path| format!("\n- {path}"))
        .collect();
    Ok(format!(
        "The {name} template this project was made from has a newer version, and it changes files \
         this project changed too:{list}\n\nFor each, bring the template's change into the \
         project's file and keep the project's own changes. The template's old version of a file \
         is in {KEPT}/{PREVIOUS}/ at the same path, its new one in {KEPT}/; a file missing there \
         is one that version has not. Then delete {KEPT}/{PREVIOUS}.",
        name = update.template.name,
    ))
}

/// The files under `folder`, by their paths from `root`, with `/` between parts.
fn files_under(root: &Path, folder: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Ok(files);
    };
    for entry in entries {
        let path = entry
            .with_context(|| format!("could not read {}", folder.display()))?
            .path();
        match path.is_dir() {
            true => files.extend(files_under(root, &path)?),
            false => files.push(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            ),
        }
    }
    Ok(files)
}

fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

/// Writes `contents` at `path`, making its folder, or removes the file when there are none.
fn put(path: &Path, contents: Option<&str>) -> Result<()> {
    match contents {
        Some(contents) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not make {}", parent.display()))?;
            }
            std::fs::write(path, contents)
                .with_context(|| format!("could not write {}", path.display()))
        }
        None => match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(error).with_context(|| format!("could not remove {}", path.display()))
            }
            _ => Ok(()),
        },
    }
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

/// Whether `folder`, in the project at `root`, holds no file but `project.json` and what the
/// runtime writes.
fn is_empty_project(root: &Path, folder: &Path) -> Result<bool> {
    let entries = std::fs::read_dir(folder)
        .with_context(|| format!("could not read {}", folder.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("could not read {}", folder.display()))?
            .path();
        let inside = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let empty = match path.is_dir() {
            true => written::written_by_the_runtime(&inside) || is_empty_project(root, &path)?,
            false => inside == PROJECT_FILE || written::written_by_the_runtime(&inside),
        };
        if !empty {
            return Ok(false);
        }
    }
    Ok(true)
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
    fn a_template_writes_into_a_new_folder_only_with_what_it_plays() {
        let daw = named("DAW").unwrap();
        let folder = tempfile::tempdir().unwrap();
        let new = folder.path().join("new");
        write(&new, daw).unwrap();
        assert!(new.join("extensions/song.tsx").exists());
        assert!(!new.join("template.json").exists());
        let file = std::fs::read_to_string(new.join(PROJECT_FILE)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&file).unwrap();
        // Its tracks play the synth of the `instrument` extension, which it enables.
        let enabled = file["extensions"].as_array().unwrap();
        assert!(enabled.contains(&"instrument".into()), "{enabled:?}");

        // A project, or someone's files: nothing is written, and the error says why.
        let error = write(&new, daw).unwrap_err();
        assert!(error.to_string().contains("Pick a new folder"), "{error}");
        let theirs = folder.path().join("theirs");
        std::fs::create_dir_all(theirs.join("state")).unwrap();
        std::fs::write(theirs.join("state/a.json"), "{}").unwrap();
        write(&theirs, daw).unwrap_err();
        assert!(!theirs.join(crate::PROJECT_FILE).exists());

        // An empty project the window opened takes it in place.
        let opened = folder.path().join("opened");
        std::fs::create_dir_all(opened.join("state")).unwrap();
        std::fs::create_dir_all(opened.join("extensions")).unwrap();
        std::fs::write(opened.join(PROJECT_FILE), "{}").unwrap();
        std::fs::write(opened.join("extensions/sdk.ts"), "").unwrap();
        std::fs::write(opened.join("AGENTS.md"), "").unwrap();
        write(&opened, daw).unwrap();
        assert!(opened.join("state/song/instance.json").exists());
    }

    /// A project from the DAW template whose `song.tsx` and the kept one are `before`, as if an
    /// older release had written it, and whose own is `ours`.
    fn made_earlier(before: Option<&str>, ours: Option<&str>) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), named("DAW").unwrap()).unwrap();
        put(&folder.path().join(KEPT).join(SONG), before).unwrap();
        put(&folder.path().join(SONG), ours).unwrap();
        folder
    }
    const SONG: &str = "extensions/song.tsx";

    fn song(folder: &Path) -> Option<String> {
        read(&folder.join(SONG)).unwrap()
    }
    fn template_song() -> &'static str {
        code(named("DAW").unwrap())
            .find(|(path, _)| *path == SONG)
            .unwrap()
            .1
    }

    #[test]
    fn code_the_project_never_changed_becomes_the_newer_templates() {
        let folder = made_earlier(Some("old"), Some("old"));
        assert!(update(folder.path()).unwrap().is_none());
        assert_eq!(song(folder.path()).as_deref(), Some(template_song()));
        assert_eq!(
            read(&folder.path().join(KEPT).join(SONG))
                .unwrap()
                .as_deref(),
            Some(template_song())
        );

        // A file the newer template adds.
        let folder = made_earlier(None, None);
        assert!(update(folder.path()).unwrap().is_none());
        assert_eq!(song(folder.path()).as_deref(), Some(template_song()));

        // One it no longer has goes, when the project never changed it, and stays for a merge
        // when it did.
        let folder = made_earlier(Some("old"), Some("old"));
        let gone = |folder: &Path| folder.join("extensions/gone.ts");
        put(
            &folder.path().join(KEPT).join("extensions/gone.ts"),
            Some("gone"),
        )
        .unwrap();
        put(&gone(folder.path()), Some("gone")).unwrap();
        assert!(update(folder.path()).unwrap().is_none());
        assert!(!gone(folder.path()).exists());
        put(
            &folder.path().join(KEPT).join("extensions/gone.ts"),
            Some("gone"),
        )
        .unwrap();
        put(&gone(folder.path()), Some("ours")).unwrap();
        let waiting = update(folder.path()).unwrap().unwrap();
        assert_eq!(waiting.files, ["extensions/gone.ts"]);
        assert_eq!(read(&gone(folder.path())).unwrap().as_deref(), Some("ours"));
    }

    #[test]
    fn code_the_project_changed_stays_and_waits_for_a_merge_with_all_three_at_hand() {
        let folder = made_earlier(Some("old"), Some("ours"));
        let waiting = update(folder.path()).unwrap().unwrap();
        assert_eq!(waiting.files, [SONG]);
        assert_eq!(song(folder.path()).as_deref(), Some("ours"));

        let ask = begin_merge(folder.path(), &waiting).unwrap();
        assert!(ask.contains(SONG), "{ask}");
        let kept = folder.path().join(KEPT);
        assert_eq!(
            read(&kept.join(PREVIOUS).join(SONG)).unwrap().as_deref(),
            Some("old")
        );
        assert_eq!(
            read(&kept.join(SONG)).unwrap().as_deref(),
            Some(template_song())
        );
        assert_eq!(song(folder.path()).as_deref(), Some("ours"));
        // Asked once: the agent's merge is what the project has from here.
        assert!(update(folder.path()).unwrap().is_none());
    }

    #[test]
    fn a_project_that_kept_no_template_is_left_alone() {
        let folder = made_earlier(Some("old"), Some("old"));
        std::fs::remove_dir_all(folder.path().join(KEPT)).unwrap();
        assert!(update(folder.path()).unwrap().is_none());
        assert_eq!(song(folder.path()).as_deref(), Some("old"));
    }
}
