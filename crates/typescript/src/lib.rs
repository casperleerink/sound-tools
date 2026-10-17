//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun. An experiment: how far a project can make Sound Tools its own with no build.
//!
//! A project whose `extensions/` folder has a tool file starts Bun on `host.ts` when it opens.
//! Bun loads every `.ts` and `.tsx` file there once and says which tools they define: the
//! fields of a record, a doc for agents, and the sound of every combination of its choices, a
//! graph of signals that the SDK sends as JSON. The runtime registers each as a [`JsonTool`]
//! before the records load, so a record of one loads like any other, and plays with no Bun.
//! Only an owner asks Bun while it plays: how its children connect and what its note players
//! loop (`behaviours`).
//!
//! In the window, any change in `extensions/`, a crash or a hang of Bun starts a new Bun, which
//! loads every file again and defines the tools again: one path. Nothing of it runs on the
//! audio thread, and the window never waits for Bun.

mod behaviours;
mod bun;
mod card;
mod samples;
mod tools;
mod tree;
mod window;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use sound_core::{AgentDoc, JsonTool, Problem, Project, Registry};

pub use behaviours::{take_declared, wait_for_behaviours};
pub use window::{hear_midi, page_shows_transport, start_window};

use crate::bun::{Bun, LoadFailure, Loaded, LoadedTool, Skipped};
use crate::tools::{Sounds, ToolInfo};

/// The folder of a project that holds its own tools and cards.
pub const FOLDER: &str = "extensions";

/// The key context of a card or a page whose tool hears the computer keyboard while it has the
/// focus. The plain keys of the window, such as space, leave it alone.
pub const KEY_CONTEXT: &str = "ToolKeys";

/// A message of the MIDI keyboard as a tool hears it, see `Midi` in `sdk.ts`: a velocity or a
/// value from 0 to 1, the bend from -1 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Midi {
    NoteOn { pitch: u8, velocity: f32 },
    NoteOff { pitch: u8 },
    Cc { controller: u8, value: f32 },
    Bend { value: f32 },
}

const SDK: &str = include_str!("sdk.ts");
const HOST: &str = include_str!("host.ts");
/// What the runtime writes in `extensions/` and owns there.
const OWNED: [&str; 2] = ["sdk.ts", "tsconfig.json"];
/// The sounds of a tool that do not build that its problem names; it counts the others.
const SHOWN_FAILURES: usize = 3;
/// JSX makes the nodes of `sdk.ts`. Also what `tsc -p extensions` checks with.
const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "esnext",
    "module": "esnext",
    "moduleResolution": "bundler",
    "jsx": "react",
    "jsxFactory": "h",
    "noEmit": true
  }
}
"#;

/// How an agent writes a tool of the project.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "extensions",
    when: "No tool does what you need: write a new effect, instrument or experiment for this project",
    markdown: include_str!("agent-doc.md"),
};

/// The tools of a project, from their first load.
pub struct Extensions {
    /// The `extensions/` folder of the project.
    folder: PathBuf,
    /// Bun after its first load, for the window. `None` when the folder has no tool file, or
    /// Bun did not start.
    bun: Option<Arc<Bun>>,
    /// The opening of the project every Bun of it runs for.
    opening: u64,
    /// The tools of the first load that the registry took.
    tools: Vec<ToolInfo>,
    skipped: Skipped,
    /// What is wrong with `extensions/`, for the problems of the project.
    problems: Vec<Problem>,
}

/// The first load of the tools of a project, before a registry takes them.
pub struct FirstLoad {
    folder: PathBuf,
    opening: u64,
    launched: Result<Option<Launched>, String>,
}

impl Extensions {
    /// Loads the tools of the project in `folder`, for [`FirstLoad::register`]. `None` when it
    /// has no `extensions/` folder, which is every project that has not asked for its own
    /// tools. Bun starts only when the folder has a tool file; the SDK is written either way,
    /// for the agent that writes the first one. Waits for Bun, up to a timeout for each file:
    /// not on the thread that draws.
    pub fn load(folder: &Path) -> Option<FirstLoad> {
        let folder = folder.join(FOLDER);
        if !folder.is_dir() {
            return None;
        }
        let opening = bun::new_opening();
        let launched = write_sdk(&folder)
            .map_err(|error| error.to_string())
            .and_then(|()| launch((&folder, opening), Skipped::new()));
        Some(FirstLoad {
            folder,
            opening,
            launched,
        })
    }

    /// Lists what is wrong with `extensions/` among the problems of the project.
    pub fn report(&self, project: &mut Project) {
        project.set_problems_in(&format!("{FOLDER}/"), self.problems.clone());
    }
}

impl FirstLoad {
    /// Registers the tools in `registry`. Call it before the project opens, so their records
    /// load with it.
    pub fn register(self, registry: &mut Registry) -> Extensions {
        let Self {
            folder,
            opening,
            launched,
        } = self;
        let none = |problems| (None, Vec::new(), Skipped::new(), problems);
        let (bun, tools, skipped, problems) = match launched {
            Ok(Some(Launched {
                bun,
                sounds,
                skipped,
                mut problems,
            })) => {
                let tools = define(sounds, &mut problems, |tools| {
                    (tools.into_iter())
                        .filter_map(|tool| {
                            let name = tool.name.clone();
                            let refused = registry.json_tool(tool).err()?;
                            Some((name, refused.to_string()))
                        })
                        .collect()
                });
                (Some(bun), tools, skipped, problems)
            }
            Ok(None) => none(Vec::new()),
            Err(message) => none(vec![folder_problem(message)]),
        };
        Extensions {
            folder,
            bun,
            opening,
            tools,
            skipped,
            problems,
        }
    }
}

/// The tools of `loaded` by `bun` with their sounds compiled, and what is wrong: the errors of
/// the files, the files Bun runs without, and every tool whose fields, sounds, card or page
/// fail. Takes as long as compiling every sound: not on the thread that draws.
fn checked(loaded: Loaded, skipped: &Skipped, bun: &Arc<Bun>) -> (Vec<Sounds>, Vec<Problem>) {
    let file_problem = |file: &str, message: &str| Problem {
        path: format!("{FOLDER}/{file}"),
        message: message.to_string(),
    };
    let mut problems: Vec<Problem> = (loaded.errors.iter())
        .map(|error| file_problem(&error.file, &error.message))
        .chain(skipped.iter().map(|(file, why)| file_problem(file, why)))
        .collect();
    let mut tools = Vec::new();
    for tool in loaded.tools {
        let LoadedTool {
            info,
            sounds,
            card,
            page,
        } = tool;
        // Before anything is made from the fields, such as a pattern at its defaults.
        let checked = (info
            .declarations()
            .check()
            .map_err(|error| error.to_string()))
        .and_then(|()| info.fields.check_defaults("state."));
        if let Err(error) = checked {
            problems.push(info.problem(error));
            continue;
        }
        let sounds = Sounds::new(&info, sounds, bun);
        // One problem a file: a list keeps one.
        let mut sounds_failed: Vec<&String> = sounds.failures().collect();
        sounds_failed.sort();
        let shown = sounds_failed.len().min(SHOWN_FAILURES);
        let more = (sounds_failed.len() > shown)
            .then(|| format!("and {} more sounds", sounds_failed.len() - shown));
        let failed: Vec<String> = (sounds_failed.into_iter().take(shown).cloned())
            .chain(more)
            .chain(
                drawn(card)
                    .err()
                    .map(|error| format!("its card does not draw: {error}")),
            )
            .chain(
                page.and_then(|page| drawn(page).err())
                    .map(|error| format!("its page does not draw: {error}")),
            )
            .collect();
        if !failed.is_empty() {
            problems.push(info.problem(failed.join("; ")));
        }
        tools.push(sounds);
    }
    (tools, problems)
}

/// Defines the tools of `sounds` with `define`, which answers the ones it refused and why, and
/// gives the tools it kept. Each refused one is a problem.
fn define(
    sounds: Vec<Sounds>,
    problems: &mut Vec<Problem>,
    define: impl FnOnce(Vec<JsonTool>) -> Vec<(String, String)>,
) -> Vec<ToolInfo> {
    let mut infos: Vec<ToolInfo> = sounds.iter().map(|sounds| sounds.info().clone()).collect();
    let refused = define(sounds.into_iter().map(Sounds::json_tool).collect());
    infos.retain(|info| {
        let Some((_, message)) = refused.iter().find(|(name, _)| *name == info.name) else {
            return true;
        };
        problems.push(info.problem(message));
        false
    });
    infos
}

/// Whether a card or a page drew a tree the window can show.
fn drawn(tree: Result<serde_json::Value, String>) -> Result<(), String> {
    serde_json::from_value::<tree::Node>(tree?)
        .map(drop)
        .map_err(|error| error.to_string())
}

/// The files of tools in `extensions`, by name, in order: those that are not the runtime's.
/// A file they import may be anywhere under it.
fn tool_files(extensions: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(extensions)
        .into_iter()
        .flatten()
        .flatten();
    let mut files: Vec<String> = entries
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            (name.ends_with(".ts") || name.ends_with(".tsx"))
                && !OWNED.contains(&name.as_str())
                && !name.ends_with(".d.ts")
        })
        .collect();
    files.sort();
    files
}

/// A Bun that loaded.
struct Launched {
    bun: Arc<Bun>,
    /// The tools it loaded, see [`checked`].
    sounds: Vec<Sounds>,
    /// The files it runs without: those given, and each whose code hung or stopped Bun while it
    /// loaded.
    skipped: Skipped,
    problems: Vec<Problem>,
}

/// Starts Bun on the tools of `extensions` for the project's `opening` without the `skipped`
/// files, and waits for its load. A file whose code hangs or stops Bun while it loads is
/// skipped too, and Bun starts again without it. `None` when the folder has no tool file.
/// Blocks for as long as the load takes.
fn launch(
    (extensions, opening): (&Path, u64),
    mut skipped: Skipped,
) -> Result<Option<Launched>, String> {
    let files = tool_files(extensions);
    if files.is_empty() {
        return Ok(None);
    }
    let program = bun_program().ok_or_else(|| {
        "Bun is not installed, so the tools of this project do not load: install it from https://bun.sh and open the project again".to_string()
    })?;
    // The host script is no file of the project.
    let host = host_script().map_err(|error| error.to_string())?;
    // Ends: a file is blamed at most once, as Bun never loads a skipped one.
    loop {
        let loading: Vec<&str> = (files.iter())
            .filter(|file| !skipped.contains_key(*file))
            .map(String::as_str)
            .collect();
        match Bun::start(&program, &host, (extensions, opening), &loading) {
            Ok((bun, loaded)) => {
                // The tools that own children ask it what they declare while their records
                // play, also outside the window.
                let bun = Arc::new(bun);
                let (sounds, problems) = checked(loaded, &skipped, &bun);
                return Ok(Some(Launched {
                    bun,
                    sounds,
                    skipped,
                    problems,
                }));
            }
            Err(LoadFailure {
                file: Some(file),
                why,
            }) => {
                let message =
                    format!("its code {why}, so the tools run without it: fix it and save");
                skipped.insert(file, message);
            }
            Err(LoadFailure { file: None, why }) => {
                return Err(format!("the TypeScript host {why}"));
            }
        }
    }
}

/// `host.ts` in a new temporary file only this user can change, as `/tmp` may be shared. Gone
/// when dropped: Bun reads it only as it starts.
fn host_script() -> std::io::Result<tempfile::TempPath> {
    let mut file = tempfile::Builder::new()
        .prefix("sound-tools-host-")
        .suffix(".ts")
        .tempfile()?;
    file.write_all(HOST.as_bytes())?;
    Ok(file.into_temp_path())
}

/// A problem of the whole `extensions/` folder.
fn folder_problem(message: String) -> Problem {
    Problem {
        path: format!("{FOLDER}/"),
        message,
    }
}

/// Writes what the runtime owns in `extensions/`, only where the text changed.
fn write_sdk(extensions: &Path) -> std::io::Result<()> {
    for (name, text) in OWNED.into_iter().zip([SDK, TSCONFIG]) {
        let path = extensions.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text)?;
        }
    }
    Ok(())
}

/// Whether this machine has a Bun to run the tools with. Tests that need one pass without it.
pub fn has_bun() -> bool {
    bun_program().is_some()
}

/// The Bun the app ships, else Bun on the `PATH`, else where its installer puts it. An app
/// opened from the Finder has a short `PATH`.
fn bun_program() -> Option<PathBuf> {
    let bun = format!("bun{}", std::env::consts::EXE_SUFFIX);
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join(&bun));
    let installed =
        std::env::var_os("HOME").map(|home| Path::new(&home).join(".bun/bin").join(&bun));
    (bundled_bun().into_iter())
        .chain(on_path)
        .chain(installed)
        .find(|path| path.is_file())
}

/// The Bun the app ships next to its program: in `Sound Tools.app/Contents/MacOS`, or in the
/// folder the Linux or Windows install puts the program in. `None` for `cargo run`.
pub fn bundled_bun() -> Option<PathBuf> {
    // The program may run through a link, such as the command line tool.
    let program = dunce::canonicalize(std::env::current_exe().ok()?).ok()?;
    let bun = program.with_file_name(format!("bun{}", std::env::consts::EXE_SUFFIX));
    bun.is_file().then_some(bun)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_with_no_tool_starts_no_bun_but_has_the_sdk() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(FOLDER)).unwrap();
        let extensions = Extensions::load(project.path())
            .unwrap()
            .register(&mut Registry::new());
        assert!(extensions.bun.is_none());
        assert!(extensions.problems.is_empty(), "{:?}", extensions.problems);
        assert!(project.path().join(FOLDER).join("sdk.ts").is_file());
        // The runtime's own files are no tool.
        assert!(tool_files(&project.path().join(FOLDER)).is_empty());
    }

    #[test]
    fn the_types_of_the_sdk_refuse_what_an_agent_gets_wrong() {
        let Some(bun) = bun_program() else {
            eprintln!("skipped: Bun is not installed");
            return;
        };
        let project = tempfile::tempdir().unwrap();
        let extensions = project.path().join(FOLDER);
        std::fs::create_dir(&extensions).unwrap();
        write_sdk(&extensions).unwrap();
        let types = include_str!("../tests/sdk-types.ts");
        std::fs::write(extensions.join("types.ts"), types).unwrap();
        // As the doc for agents says to check a tool.
        let checked = sound_core::process::background_command(bun)
            .args(["x", "tsc", "-p", FOLDER])
            .current_dir(project.path())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&checked.stdout);
        assert!(checked.status.success(), "{said}");
    }
}
