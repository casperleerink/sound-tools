//! The app on this machine, apart from its window: the last project it had open, a start of the
//! app again on another project, and the command line tool for agents.
//!
//! Only the window uses these. `--inspect`, `--render`, `--headless` and the tests never
//! write the last project, so a test cannot change what the app opens next.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

/// The name of the command line tool, and of the program inside `Sound Tools.app`.
pub const TOOL_NAME: &str = "sound-tools";

/// One line: the folder of the last project the window had open.
const LAST_PROJECT_FILE: &str = "last-project";

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set, so the app has nowhere to keep the last project")
}

/// Where the app keeps what it remembers between two launches:
/// `~/Library/Application Support/Sound Tools` on macOS, and on Linux `sound-tools` in
/// `XDG_CONFIG_HOME`, which is `~/.config` when it is not set.
fn support_folder() -> Result<PathBuf> {
    let home = home()?;
    if cfg!(target_os = "macos") {
        return Ok(home.join("Library/Application Support/Sound Tools"));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|config| config.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    Ok(config.join("sound-tools"))
}

/// The project the window had open last, when its folder is still there.
pub fn last_project() -> Option<PathBuf> {
    last_project_in(&support_folder().ok()?)
}

fn last_project_in(support: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(support.join(LAST_PROJECT_FILE)).ok()?;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let folder = PathBuf::from(OsStr::from_bytes(bytes));
    folder.is_dir().then_some(folder)
}

/// Remembers `folder` as the project to open when the app starts with no folder, which is
/// what a double click in the Finder does.
pub fn remember_project(folder: &Path) -> Result<()> {
    remember_project_in(&support_folder()?, folder)
}

fn remember_project_in(support: &Path, folder: &Path) -> Result<()> {
    let folder = folder
        .canonicalize()
        .with_context(|| format!("{} is not there", folder.display()))?;
    let file = support.join(LAST_PROJECT_FILE);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not make {}", parent.display()))?;
    }
    // The bytes of the path, so that any folder name comes back as it was.
    let line = [folder.as_os_str().as_bytes(), b"\n"].concat();
    std::fs::write(&file, line).with_context(|| format!("could not write {}", file.display()))
}

/// Refuses a folder that holds files but no project: opening it would put a new project's
/// files among somebody's other files. An empty folder, a new one and a project are fine.
/// Hidden files such as `.DS_Store` or `.git` do not count.
pub fn check_project_folder(folder: &Path) -> Result<()> {
    if folder.join(crate::PROJECT_FILE).exists() || !folder.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(folder)
        .with_context(|| format!("could not read {}", folder.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("could not read {}", folder.display()))?;
        if !entry.file_name().to_string_lossy().starts_with('.') {
            let name = folder.file_name().unwrap_or(folder.as_os_str());
            bail!(
                "“{}” has files in it and is not a Sound Tools project. Pick a project folder, or make a new empty folder for a new project.",
                name.to_string_lossy()
            );
        }
    }
    Ok(())
}

/// Starts this program again with no folder, so that it opens the last project. The window
/// calls it at the very end of a quit, once its own project is closed: the project's lock is
/// free by then, and the plugins have saved their state.
pub fn start_again() -> Result<()> {
    let program = std::env::current_exe().context("could not find this program")?;
    // The new process lives on after this one ends, so there is nothing to wait for.
    #[allow(clippy::disallowed_methods)]
    let child = Command::new(&program)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("could not start {}", program.display()))?;
    drop(child);
    Ok(())
}

/// Where the command line tool went, for the message the window shows.
pub struct Installed {
    pub link: PathBuf,
    /// Whether a terminal finds the link without a change to its `PATH`.
    pub on_default_path: bool,
}

/// The first of these that works gets the link. On macOS `/usr/local/bin` is on the `PATH` of
/// every Mac but needs an administrator on many; `~/.local/bin` never does. On Linux the link
/// goes to `~/.local/bin` only, which the common distributions put on the `PATH`.
fn link_candidates(home: &Path) -> Vec<PathBuf> {
    let local = home.join(".local/bin").join(TOOL_NAME);
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/usr/local/bin").join(TOOL_NAME), local]
    } else {
        vec![local]
    }
}

/// Links `sound-tools` to this program, so an agent runs `sound-tools . --inspect` in a
/// project folder. A link, not a copy: it follows the app when the app is updated in place.
pub fn install_command_line_tool() -> Result<Installed> {
    let program = std::env::current_exe().context("could not find this program")?;
    let program = program.canonicalize().unwrap_or(program);
    let candidates = link_candidates(&home()?);
    let link = install_link(&program, &candidates)?;
    // An app started from the Finder has the short `PATH` of macOS, so the first choice is
    // the one known to be on it. On Linux the `PATH` of the app is the one of the session.
    let on_default_path = if cfg!(target_os = "macos") {
        candidates.first() == Some(&link)
    } else {
        let folder = link.parent();
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|on| Some(on.as_path()) == folder))
    };
    Ok(Installed {
        link,
        on_default_path,
    })
}

/// Makes the first link of `candidates` that can be made, and says which. A link already
/// there is replaced; a file that is not a link is somebody else's and is left alone.
fn install_link(program: &Path, candidates: &[PathBuf]) -> Result<PathBuf> {
    let mut failures = Vec::new();
    for link in candidates {
        match make_link(program, link) {
            Ok(()) => return Ok(link.clone()),
            Err(error) => failures.push(format!("{}: {error:#}", link.display())),
        }
    }
    bail!(
        "the command line tool was not installed. {}",
        failures.join("; ")
    )
}

fn make_link(program: &Path, link: &Path) -> Result<()> {
    if let Some(folder) = link.parent() {
        std::fs::create_dir_all(folder)?;
    }
    if let Ok(metadata) = link.symlink_metadata() {
        if !metadata.file_type().is_symlink() {
            bail!("a file that is not a link is there already");
        }
        std::fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(program, link)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_project_is_remembered_and_forgotten_when_its_folder_goes() {
        let support = tempfile::tempdir().unwrap();
        let projects = tempfile::tempdir().unwrap();
        let piece = projects.path().join("my piece");
        std::fs::create_dir(&piece).unwrap();

        assert_eq!(last_project_in(support.path()), None);
        remember_project_in(support.path(), &piece).unwrap();
        assert_eq!(
            last_project_in(support.path()),
            Some(piece.canonicalize().unwrap())
        );
        assert!(support.path().join("last-project").is_file());

        std::fs::remove_dir(&piece).unwrap();
        assert_eq!(last_project_in(support.path()), None);
    }

    #[test]
    fn a_folder_with_other_files_is_not_taken_for_a_new_project() {
        let folder = tempfile::tempdir().unwrap();
        // Empty, or with hidden files only: a new project.
        check_project_folder(folder.path()).unwrap();
        std::fs::write(folder.path().join(".DS_Store"), "").unwrap();
        check_project_folder(folder.path()).unwrap();
        // Missing: the app makes it.
        check_project_folder(&folder.path().join("new")).unwrap();

        std::fs::write(folder.path().join("letter.txt"), "").unwrap();
        let refused = check_project_folder(folder.path()).unwrap_err();
        assert!(refused.to_string().contains("is not a Sound Tools project"));

        // A project may have any other files next to its own.
        std::fs::write(folder.path().join("project.json"), "{}").unwrap();
        check_project_folder(folder.path()).unwrap();
    }

    #[test]
    fn the_tool_goes_to_the_first_folder_that_takes_it() {
        let program = Path::new("/Applications/Sound Tools.app/Contents/MacOS/sound-tools");
        let scratch = tempfile::tempdir().unwrap();
        // A folder nobody may write to, as `/usr/local/bin` is on many Macs.
        let locked = scratch.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o555);
        std::fs::set_permissions(&locked, permissions).unwrap();
        let fallback = scratch.path().join("home/.local/bin/sound-tools");
        let candidates = [locked.join(TOOL_NAME), fallback.clone()];

        assert_eq!(install_link(program, &candidates).unwrap(), fallback);
        assert_eq!(std::fs::read_link(&fallback).unwrap(), program);

        // Installing again replaces the link, so it follows a moved app.
        let moved = Path::new("/Users/someone/Applications/Sound Tools.app/Contents/MacOS/x");
        install_link(moved, &candidates).unwrap();
        assert_eq!(std::fs::read_link(&fallback).unwrap(), moved);

        // A file that is not a link is never overwritten.
        std::fs::remove_file(&fallback).unwrap();
        std::fs::write(&fallback, "someone's script").unwrap();
        let refused = install_link(program, &candidates).unwrap_err();
        assert!(refused.to_string().contains("not a link"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&fallback).unwrap(),
            "someone's script"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_first_choice_is_on_the_path_of_every_mac() {
        assert_eq!(
            link_candidates(Path::new("/Users/someone")),
            [
                Path::new("/usr/local/bin/sound-tools"),
                Path::new("/Users/someone/.local/bin/sound-tools")
            ]
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_linux_the_tool_goes_to_the_local_bin_of_the_home_folder() {
        assert_eq!(
            link_candidates(Path::new("/home/someone")),
            [Path::new("/home/someone/.local/bin/sound-tools")]
        );
    }
}
