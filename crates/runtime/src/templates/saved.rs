//! Templates the composer saved from a project, each a folder in `templates` of the support
//! folder laid out as a folder of the repository's `templates/`: its `template.json`, and the
//! files a new project starts with. One layout for both, so a saved template can later be
//! shared as it is.

use std::path::{Path, PathBuf};

use std::collections::BTreeSet;

use anyhow::{Context as _, Result, bail};
use sound_core::{InstanceId, Project};

use super::about::About;
use super::written::written_by_the_runtime;
use super::{ABOUT_FILE, TEMPLATES, files_under, make_room};

/// What a saved template keeps of the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The project's tools and, by their ids, the records at the top and the children their
    /// tools require; no asset. A song keeps its master and no track.
    Tools(BTreeSet<String>),
    /// The whole project as it is, assets too.
    Everything,
}

impl Keep {
    /// [`Keep::Tools`] of `project`.
    pub fn tools_of(project: &Project) -> Self {
        let required = |id: &InstanceId| -> bool {
            let mut id = id.clone();
            while let Some(parent) = id.parent() {
                let tool = project.tool_of(&parent).unwrap_or_default();
                if !(project.required_children(tool).iter()).any(|name| name == id.name()) {
                    return false;
                }
                id = parent;
            }
            true
        };
        let kept = (project.instances())
            .filter(|(id, _)| required(id))
            .map(|(id, _)| id.to_string())
            .collect();
        Self::Tools(kept)
    }
}

#[derive(Clone, Debug)]
pub struct Saved {
    pub name: String,
    pub description: String,
    folder: PathBuf,
}

/// Where the saved templates of this machine are.
pub fn saved_folder() -> Result<PathBuf> {
    Ok(crate::app::support_folder()?.join("templates"))
}

/// The templates saved in `root`, by name. A folder that is no template is left out, and logged.
pub fn saved_in(root: &Path) -> Result<Vec<Saved>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", root.display()));
        }
    };
    let mut saved = Vec::new();
    for entry in entries {
        let folder = entry
            .with_context(|| format!("could not read {}", root.display()))?
            .path();
        let hidden =
            (folder.file_name()).is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if hidden || !folder.is_dir() {
            continue;
        }
        match read_about(&folder) {
            Ok(About { name, description }) => saved.push(Saved {
                name,
                description,
                folder,
            }),
            Err(error) => eprintln!("error: {error:#}"),
        }
    }
    saved.sort_by_key(|template| template.name.to_lowercase());
    Ok(saved)
}

fn read_about(folder: &Path) -> Result<About> {
    let file = folder.join(ABOUT_FILE);
    let about = std::fs::read_to_string(&file)
        .with_context(|| format!("could not read {}", file.display()))?;
    serde_json::from_str(&about).with_context(|| format!("{} is no template", file.display()))
}

/// The folder in `root` of the template called `name`. Two names that differ only in what a
/// file name cannot hold are one template.
fn folder_of(root: &Path, name: &str) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Give the template a name.");
    }
    if (TEMPLATES.iter()).any(|template| template.name.eq_ignore_ascii_case(name)) {
        bail!("“{name}” is a template of the app already. Pick another name.");
    }
    let file_name: String = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect();
    // Windows drops dots and spaces at the end, and a leading dot would hide the folder.
    let file_name = file_name
        .trim_end_matches(['.', ' '])
        .trim_start_matches('.');
    match file_name.is_empty() {
        true => bail!("“{name}” cannot be the name of a template. Pick another name."),
        false => Ok(root.join(file_name)),
    }
}

/// Whether saving `name` in `root` would replace a template.
pub fn exists_in(root: &Path, name: &str) -> Result<bool> {
    Ok(folder_of(root, name)?.exists())
}

/// Saves the project in `project` as the template `name` in `root`, in place of one of that
/// name. A save that fails leaves the one before.
pub fn save_in(root: &Path, project: &Path, name: &str, keep: &Keep) -> Result<Saved> {
    let target = folder_of(root, name)?;
    let name = name.trim().to_string();
    let file_name = target.file_name().unwrap_or_default().to_string_lossy();
    let saving = root.join(format!(".saving-{file_name}"));
    remove_folder(&saving)?;
    for path in files_under(project, project)? {
        if !kept(&path, keep) {
            continue;
        }
        let to = saving.join(&path);
        make_room(&to)?;
        std::fs::copy(project.join(&path), &to)
            .with_context(|| format!("could not copy {path} into the template"))?;
    }
    let project_name = project.file_name().unwrap_or(project.as_os_str());
    let project_name = project_name.to_string_lossy();
    let description = match keep {
        Keep::Tools(_) => format!("From “{project_name}”: its tools, with nothing in them."),
        Keep::Everything => format!("From “{project_name}”: its tools and everything in them."),
    };
    let about = About { name, description };
    let file = saving.join(ABOUT_FILE);
    make_room(&file)?;
    std::fs::write(&file, serde_json::to_string_pretty(&about)?)
        .with_context(|| format!("could not write {}", file.display()))?;
    remove_folder(&target)?;
    std::fs::rename(&saving, &target)
        .with_context(|| format!("could not move the template into {}", target.display()))?;
    Ok(Saved {
        name: about.name,
        description: about.description,
        folder: target,
    })
}

/// Whether `path`, in a project, goes into a template that keeps `keep`: the project's own
/// files, not what the runtime writes nor what else sits in its folder.
fn kept(path: &str, keep: &Keep) -> bool {
    if written_by_the_runtime(path) {
        return false;
    }
    let parts: Vec<&str> = path.split('/').collect();
    match (parts.as_slice(), keep) {
        ([crate::PROJECT_FILE | "instructions.md"] | ["extensions", ..], _) => true,
        (["state" | "assets", ..], Keep::Everything) => true,
        (["state", ..], Keep::Tools(ids)) => {
            let record = path.trim_start_matches("state/");
            let id = (record.strip_suffix("/instance.json")).or(record.strip_suffix(".json"));
            id.is_some_and(|id| ids.contains(id))
        }
        _ => false,
    }
}

impl Saved {
    /// Writes its files into `folder`, as [`super::write`] does a template of the app's. A
    /// project made from it keeps no template code: a saved template has no newer version.
    pub fn write(&self, folder: &Path) -> Result<()> {
        super::check_new(folder)?;
        for path in files_under(&self.folder, &self.folder)? {
            if path == ABOUT_FILE || written_by_the_runtime(&path) {
                continue;
            }
            let to = folder.join(&path);
            make_room(&to)?;
            std::fs::copy(self.folder.join(&path), &to)
                .with_context(|| format!("could not write {}", to.display()))?;
        }
        Ok(())
    }

    pub fn remove(&self) -> Result<()> {
        remove_folder(&self.folder)
    }
}

fn remove_folder(folder: &Path) -> Result<()> {
    match std::fs::remove_dir_all(folder) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error).with_context(|| format!("could not remove {}", folder.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{named, write};

    /// A DAW project with a sound file, an export dropped in its folder and what the runtime
    /// writes.
    fn project() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), named("DAW").unwrap()).unwrap();
        for (path, contents) in [
            ("assets/audio/take.wav", "RIFF"),
            ("export.wav", "RIFF"),
            ("instructions.md", "Keep it slow."),
            ("AGENTS.md", ""),
            ("extensions/sdk.ts", ""),
        ] {
            make_room(&folder.path().join(path)).unwrap();
            std::fs::write(folder.path().join(path), contents).unwrap();
        }
        folder
    }

    fn files(folder: &Path) -> Vec<String> {
        let mut files = files_under(folder, folder).unwrap();
        files.retain(|path| !path.starts_with("extensions/"));
        files.sort();
        files
    }

    #[test]
    fn tools_only_keep_the_records_it_names_and_everything_keeps_the_whole_project() {
        let project = project();
        let root = tempfile::tempdir().unwrap();
        let tools = Keep::Tools(["song", "song/master"].map(String::from).into());
        save_in(root.path(), project.path(), "Spectral", &tools).unwrap();
        let everything = save_in(root.path(), project.path(), "Full", &Keep::Everything).unwrap();

        let [full, tools] = <[Saved; 2]>::try_from(saved_in(root.path()).unwrap()).unwrap();
        assert_eq!(
            (full.name.as_str(), tools.name.as_str()),
            ("Full", "Spectral")
        );
        let new = tempfile::tempdir().unwrap();
        tools.write(new.path()).unwrap();
        assert_eq!(
            files(new.path()),
            [
                "instructions.md",
                "project.json",
                "state/song/instance.json",
                "state/song/master.json"
            ]
        );
        assert!(new.path().join("extensions/song.tsx").exists());
        assert!(!new.path().join("extensions/sdk.ts").exists());
        assert!(!new.path().join(crate::templates::KEPT).exists());

        let new = tempfile::tempdir().unwrap();
        everything.write(new.path()).unwrap();
        let mut expected = files(project.path());
        expected.retain(|path| !["export.wav", "AGENTS.md"].contains(&path.as_str()));
        expected.retain(|path| !path.starts_with('.'));
        assert_eq!(files(new.path()), expected);
        assert!(expected.contains(&"assets/audio/take.wav".to_string()));
    }

    #[test]
    fn a_name_saved_again_replaces_its_template_and_one_of_the_app_is_refused() {
        let project = project();
        let root = tempfile::tempdir().unwrap();
        assert!(!exists_in(root.path(), "Mine").unwrap());
        save_in(root.path(), project.path(), "Mine", &Keep::Everything).unwrap();
        assert!(exists_in(root.path(), " Mine ").unwrap());
        let tools = Keep::Tools(BTreeSet::new());
        save_in(root.path(), project.path(), "Mine", &tools).unwrap();
        let [mine] = <[Saved; 1]>::try_from(saved_in(root.path()).unwrap()).unwrap();
        assert!(!mine.folder.join("assets").exists());

        for refused in ["daw", "  ", "..."] {
            save_in(root.path(), project.path(), refused, &tools).unwrap_err();
        }
        // A name a file cannot hold still saves.
        let odd = save_in(root.path(), project.path(), "A/B: take 2", &tools).unwrap();
        assert_eq!(odd.name, "A/B: take 2");

        mine.remove().unwrap();
        odd.remove().unwrap();
        assert!(saved_in(root.path()).unwrap().is_empty());
    }
}
