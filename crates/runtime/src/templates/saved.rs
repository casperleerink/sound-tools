//! Templates the composer saved from a project, each a folder in `templates` of the support
//! folder laid out as a folder of the repository's `templates/`: its `template.json`, and the
//! files a new project starts with. One layout for both, so a saved template can later be
//! shared as it is.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
    /// [`Keep::Tools`] of `project`. Refused while a tool, or a record it would keep, did not
    /// load: what they require is then unknown.
    pub fn tools_of(project: &Project) -> Result<Self> {
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
        for problem in project.problems() {
            if problem.path.starts_with("extensions/") {
                bail!(
                    "A tool of this project does not load, so what it requires is unknown. Fix it first, or save the tools and content."
                );
            }
            let id = record_id(&problem.path).map(|id| InstanceId::try_from(id.to_string()));
            if let Some(Ok(id)) = id
                && project.tool_of(&id).is_none()
                && required(&id)
            {
                bail!(
                    "Part of this project did not load ({id}), so a template of its tools would miss it. Fix it first, or save the tools and content."
                );
            }
        }
        let kept = (project.instances())
            .filter(|(id, _)| required(id))
            .map(|(id, _)| id.to_string())
            .collect();
        Ok(Self::Tools(kept))
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
/// name. A save that fails leaves the one before, and no copy.
pub fn save_in(root: &Path, project: &Path, name: &str, keep: &Keep) -> Result<Saved> {
    let target = folder_of(root, name)?;
    // `.` has no name of its own to describe the template with.
    let project = &std::fs::canonicalize(project)
        .with_context(|| format!("could not find {}", project.display()))?;
    if !project.join(crate::PROJECT_FILE).is_file() {
        bail!("{} is not a project.", project.display());
    }
    let file_name = target.file_name().unwrap_or_default().to_string_lossy();
    let saving = root.join(format!(".saving-{file_name}"));
    let replaced = root.join(format!(".replaced-{file_name}"));
    remove_folder(&saving)?;
    remove_folder(&replaced)?;
    let about = match copy_project(&saving, project, name.trim(), keep) {
        Ok(about) => about,
        Err(error) => return Err(left_out(&saving, error)),
    };
    let moved_aside = target.exists();
    if moved_aside && let Err(error) = std::fs::rename(&target, &replaced) {
        let error = anyhow::Error::from(error)
            .context(format!("could not move {} aside", target.display()));
        return Err(left_out(&saving, error));
    }
    if let Err(error) = std::fs::rename(&saving, &target) {
        if moved_aside {
            std::fs::rename(&replaced, &target)
                .with_context(|| format!("could not put {} back", target.display()))?;
        }
        let error = anyhow::Error::from(error).context(format!(
            "could not move the template into {}",
            target.display()
        ));
        return Err(left_out(&saving, error));
    }
    // Saved already: what is left of the one before goes before the next save of the name.
    if let Err(error) = remove_folder(&replaced) {
        eprintln!("error: {error:#}");
    }
    Ok(Saved {
        name: about.name,
        description: about.description,
        folder: target,
    })
}

/// `error`, once the copy in `saving` is gone: a failed save of every sound file must not keep
/// the disk full.
fn left_out(saving: &Path, error: anyhow::Error) -> anyhow::Error {
    if let Err(removed) = remove_folder(saving) {
        eprintln!("error: {removed:#}");
    }
    error
}

/// Copies what `keep` keeps of `project` into `saving`, with the template's `template.json`.
fn copy_project(saving: &Path, project: &Path, name: &str, keep: &Keep) -> Result<About> {
    for path in files_under(project, project)? {
        if !kept(&path, keep) {
            continue;
        }
        let to = saving.join(&path);
        make_room(&to)?;
        match (path == crate::PROJECT_FILE, keep) {
            (true, Keep::Tools(ids)) => {
                let file = std::fs::read_to_string(project.join(&path))
                    .with_context(|| format!("could not read {path}"))?;
                std::fs::write(&to, project_file_of_tools(&file, ids)?)
            }
            _ => std::fs::copy(project.join(&path), &to).map(drop),
        }
        .with_context(|| format!("could not copy {path} into the template"))?;
    }
    let project_name = project.file_name().unwrap_or(project.as_os_str());
    let project_name = project_name.to_string_lossy();
    let description = match keep {
        Keep::Tools(_) => format!("From “{project_name}”: its tools, with nothing in them."),
        Keep::Everything => format!("From “{project_name}”: its tools and everything in them."),
    };
    let about = About {
        name: name.to_string(),
        description,
    };
    let file = saving.join(ABOUT_FILE);
    make_room(&file)?;
    std::fs::write(&file, serde_json::to_string_pretty(&about)?)
        .with_context(|| format!("could not write {}", file.display()))?;
    Ok(about)
}

/// The `project.json` in `file` with its tools only: no connection that names an instance not
/// in `ids`, which would be a problem in every new project, no section, and the first tempo and
/// time signature alone.
fn project_file_of_tools(file: &str, ids: &BTreeSet<String>) -> Result<String> {
    use serde_json::Value;
    let mut project: Value = serde_json::from_str(file).context("project.json is no JSON")?;
    let names_another = |connection: &Value| {
        ["from", "to"].iter().any(|end| {
            let instance = connection[end]["instance"].as_str();
            instance.is_some_and(|instance| !ids.contains(instance))
        })
    };
    if let Some(connections) = project["connections"].as_array_mut() {
        connections.retain(|connection| !names_another(connection));
    }
    if let Some(project) = project.as_object_mut() {
        project.remove("sections");
    }
    for list in ["time_signatures", "tempo_changes"] {
        if let Some(list) = project["tempo_map"][list].as_array_mut() {
            list.truncate(1);
        }
    }
    Ok(serde_json::to_string_pretty(&project)?)
}

/// The id of the instance whose record is at `path` in a project, such as `song/master` of
/// `state/song/master.json`.
fn record_id(path: &str) -> Option<&str> {
    let record = path.strip_prefix("state/")?;
    (record.strip_suffix("/instance.json")).or(record.strip_suffix(".json"))
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
        (["state", ..], Keep::Tools(ids)) => record_id(path).is_some_and(|id| ids.contains(id)),
        _ => false,
    }
}

impl Saved {
    /// Writes its files into `folder`, as [`super::write`] does a template of the app's. A
    /// project made from it keeps no template code: a saved template has no newer version.
    pub fn write(&self, folder: &Path) -> Result<()> {
        if !self.folder.is_dir() {
            bail!("The template “{}” is not there anymore.", self.name);
        }
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

    /// A DAW project with a sound file, an export dropped in its folder, what the runtime
    /// writes, and the device input into its first track.
    fn project() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), named("DAW").unwrap()).unwrap();
        let file = folder.path().join(crate::PROJECT_FILE);
        let mut project: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        project["connections"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "from": { "device_input": 0 },
                "to": { "instance": "song/t00/rec", "port": "audio" }
            }));
        std::fs::write(&file, project.to_string()).unwrap();
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
        // The input into the track it left out is left out too; the song still plays out. The
        // sections and the later tempo were the piece's.
        let project_file = |folder: &Path| -> serde_json::Value {
            let file = std::fs::read_to_string(folder.join(crate::PROJECT_FILE)).unwrap();
            serde_json::from_str(&file).unwrap()
        };
        let connections = |folder: &Path| {
            project_file(folder)["connections"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(connections(new.path()), 1);
        let tools_only = project_file(new.path());
        assert!(tools_only.get("sections").is_none());
        assert_eq!(
            tools_only["tempo_map"]["tempo_changes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // An instance may be called `state`.
        let named_state = Keep::Tools(["state".to_string()].into());
        assert!(kept("state/state/instance.json", &named_state));
        assert!(!new.path().join(crate::templates::KEPT).exists());

        let new = tempfile::tempdir().unwrap();
        everything.write(new.path()).unwrap();
        let mut expected = files(project.path());
        expected.retain(|path| !["export.wav", "AGENTS.md"].contains(&path.as_str()));
        expected.retain(|path| !path.starts_with('.'));
        assert_eq!(files(new.path()), expected);
        assert!(expected.contains(&"assets/audio/take.wav".to_string()));
        assert_eq!(connections(new.path()), 2);
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
