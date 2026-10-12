//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun. An experiment: how far a project can make Sound Tools its own with no build.
//!
//! A project whose `extensions/` folder has a tool file starts Bun on `host.ts` when it opens.
//! Bun loads every `.ts` and `.tsx` file there once and says which tools they define: the
//! fields of a record, a doc for agents, and the sound of every combination of its choices, a
//! graph of signals that the SDK sends as JSON. The runtime registers each as a [`JsonTool`]
//! before the records load, so a record of one loads like any other, and plays with no Bun.
//!
//! In the window, any change in `extensions/`, a crash or a hang of Bun starts a new Bun, which
//! loads every file again and defines the tools again: one path. Nothing of it runs on the
//! audio thread, and the window never waits for Bun.

mod bun;
mod card;
mod samples;
mod tools;
mod tree;
mod window;

use std::path::{Path, PathBuf};

use serde::Serialize;
use sound_core::{AgentDoc, JsonTool, Problem, Project, Registry};

pub use window::{hear_midi, start_window};

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
    bun: Option<Bun>,
    /// The tools of the first load that the registry took.
    tools: Vec<ToolInfo>,
    skipped: Skipped,
    /// What is wrong with `extensions/`, for the problems of the project.
    problems: Vec<Problem>,
}

impl Extensions {
    /// Starts the tools of the project in `folder` and registers them in `registry`. Call it
    /// before the project opens, so their records load with it. `None` when it has no
    /// `extensions/` folder, which is every project that has not asked for its own tools. Bun
    /// starts only when the folder has a tool file; the SDK is written either way, for the
    /// agent that writes the first one.
    pub fn start(folder: &Path, registry: &mut Registry) -> Option<Self> {
        let folder = folder.join(FOLDER);
        if !folder.is_dir() {
            return None;
        }
        let mut extensions = Self {
            folder,
            bun: None,
            tools: Vec::new(),
            skipped: Skipped::new(),
            problems: Vec::new(),
        };
        let launched = write_sdk(&extensions.folder)
            .map_err(|error| error.to_string())
            .and_then(|()| launch(&extensions.folder, Skipped::new()));
        match launched {
            Ok(Some(Launched {
                bun,
                loaded,
                skipped,
            })) => {
                (extensions.tools, extensions.problems) = define(loaded, &skipped, |tools| {
                    (tools.into_iter())
                        .filter_map(|tool| {
                            let name = tool.name.clone();
                            let refused = registry.json_tool(tool).err()?;
                            Some((name, refused.to_string()))
                        })
                        .collect()
                });
                extensions.bun = Some(bun);
                extensions.skipped = skipped;
            }
            Ok(None) => {}
            Err(message) => extensions.problems = vec![folder_problem(message)],
        }
        Some(extensions)
    }

    /// Lists what is wrong with `extensions/` among the problems of the project.
    pub fn report(&self, project: &mut Project) {
        project.set_problems_in(&format!("{FOLDER}/"), self.problems.clone());
    }
}

/// Defines the tools of `loaded` with `define`, which answers the ones it refused and why.
/// Gives the tools it kept, and what is wrong: the errors of the files, the files Bun runs
/// without, the tools `define` refused, and every tool whose sound, card or page fails at its
/// defaults.
fn define(
    loaded: Loaded,
    skipped: &Skipped,
    define: impl FnOnce(Vec<JsonTool>) -> Vec<(String, String)>,
) -> (Vec<ToolInfo>, Vec<Problem>) {
    let file_problem = |file: &str, message: &str| Problem {
        path: format!("{FOLDER}/{file}"),
        message: message.to_string(),
    };
    let mut problems: Vec<Problem> = (loaded.errors.iter())
        .map(|error| file_problem(&error.file, &error.message))
        .chain(skipped.iter().map(|(file, why)| file_problem(file, why)))
        .collect();
    let mut infos = Vec::new();
    let mut tools = Vec::new();
    for tool in loaded.tools {
        let LoadedTool {
            info,
            sounds,
            card,
            page,
        } = tool;
        let sounds = Sounds::new(&info, sounds);
        let failed = [
            sounds.code(&serde_json::json!({})).map(drop),
            drawn(card).map_err(|error| format!("its card does not draw: {error}")),
            (page.map_or(Ok(()), drawn))
                .map_err(|error| format!("its page does not draw: {error}")),
        ];
        problems.extend(
            failed
                .into_iter()
                .filter_map(Result::err)
                .map(|message| info.problem(message)),
        );
        tools.push(sounds.json_tool());
        infos.push(info);
    }
    let refused = define(tools);
    infos.retain(|info| {
        let Some((_, message)) = refused.iter().find(|(name, _)| *name == info.name) else {
            return true;
        };
        problems.push(info.problem(message));
        false
    });
    (infos, problems)
}

/// Whether a card or a page drew a tree the window can show.
fn drawn(tree: Result<serde_json::Value, String>) -> Result<(), String> {
    serde_json::from_value::<tree::Node>(tree?)
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Whether `extensions` has a file of a tool: one that is not the runtime's.
fn has_tools(extensions: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(extensions) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        (name.ends_with(".ts") || name.ends_with(".tsx"))
            && !OWNED.contains(&name.as_ref())
            && !name.ends_with(".d.ts")
    })
}

/// A Bun that loaded.
struct Launched {
    bun: Bun,
    loaded: Loaded,
    /// The files it runs without: those given, and each whose code hung or stopped Bun while it
    /// loaded.
    skipped: Skipped,
}

/// Starts Bun on the tools of `extensions` without the `skipped` files, and waits for its load.
/// A file whose code hangs or stops Bun while it loads is skipped too, and Bun starts again
/// without it. `None` when the folder has no tool file. Blocks for as long as the load takes.
fn launch(extensions: &Path, mut skipped: Skipped) -> Result<Option<Launched>, String> {
    if !has_tools(extensions) {
        return Ok(None);
    }
    let program = bun_program().ok_or_else(|| {
        "Bun is not installed, so the tools of this project do not load: install it from https://bun.sh and open the project again".to_string()
    })?;
    // The host script is no file of the project.
    let host = std::env::temp_dir().join(format!("sound-tools-host-{}.ts", std::process::id()));
    std::fs::write(&host, HOST).map_err(|error| error.to_string())?;
    // Ends: a file is blamed at most once, as Bun never loads a skipped one.
    loop {
        match Bun::start(&program, &host, extensions, skipped.keys()) {
            Ok((bun, loaded)) => {
                return Ok(Some(Launched {
                    bun,
                    loaded,
                    skipped,
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

/// Whether this machine has a Bun to run the tools with. Tests that need one pass without it,
/// as in CI.
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
        let extensions = Extensions::start(project.path(), &mut Registry::new()).unwrap();
        assert!(extensions.bun.is_none());
        assert!(extensions.problems.is_empty(), "{:?}", extensions.problems);
        assert!(project.path().join(FOLDER).join("sdk.ts").is_file());
        // The runtime's own files are no tool.
        assert!(!has_tools(&project.path().join(FOLDER)));
    }
}
