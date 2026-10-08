//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun. An experiment: how far a project can make Sound Tools its own with no build.
//!
//! A project with an `extensions/` folder starts Bun on `host.ts` when it opens. Bun loads every
//! `.ts` and `.tsx` file there and says which tools they define: the fields of a record, a doc
//! for agents, and a function that makes the tool's sound in Hum. The runtime registers each
//! as a [`JsonTool`] before the records load, so a record of one loads like any other. Its
//! check runs in Rust; only a new combination of choices asks Bun for Hum. In the window, a
//! save of a file defines the tools again and draws the cards again.
//!
//! Nothing of it runs on the audio thread, and a knob never waits for Bun: it moves a value of
//! the Hum that plays, as the knob of a built-in effect moves its processor.

mod bun;
mod card;
mod tools;
mod tree;
mod window;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sound_core::{AgentDoc, JsonTool, Problem, Project, Registry};

pub use window::start_window;

use crate::bun::{Bun, Loaded};

/// The folder of a project that holds its own tools and cards.
pub const FOLDER: &str = "extensions";

const SDK: &str = include_str!("sdk.ts");
const HOST: &str = include_str!("host.ts");
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
    when: "No tool does what you need: write a new effect for this project",
    markdown: include_str!("agent-doc.md"),
};

/// The running tools of a project, from its first load.
pub struct Extensions {
    /// Bun and the tools it loaded first. `None` when it did not start or load.
    running: Option<(Arc<Bun>, Loaded)>,
    /// What is wrong with `extensions/`, for the problems of the project.
    problems: Vec<Problem>,
}

impl Extensions {
    /// Starts the tools of the project in `folder`. `None` when it has no `extensions/` folder,
    /// which is every project that has not asked for its own tools.
    pub fn start(folder: &Path) -> Option<Self> {
        let extensions = folder.join(FOLDER);
        if !extensions.is_dir() {
            return None;
        }
        let started = bun_program()
            .ok_or_else(|| {
                "Bun is not installed, so the tools of this project do not load: install it from https://bun.sh and open the project again".to_string()
            })
            .and_then(|program| {
                let host = write_files(&extensions).map_err(|error| error.to_string())?;
                Bun::start(&program, &host, &extensions)
            });
        Some(match started {
            Ok((bun, loaded)) => Self {
                running: Some((Arc::new(bun), loaded)),
                problems: Vec::new(),
            },
            Err(message) => Self {
                running: None,
                problems: vec![Problem {
                    path: format!("{FOLDER}/"),
                    message,
                }],
            },
        })
    }

    /// Registers every tool of the first load. Call it before the project opens, so their
    /// records load with it.
    pub fn register(&mut self, registry: &mut Registry) {
        let Some((bun, loaded)) = &mut self.running else {
            return;
        };
        self.problems = define(bun, loaded, |tool| {
            registry.json_tool(tool).map_err(|error| error.to_string())
        });
    }

    /// Lists what is wrong with `extensions/` among the problems of the project.
    pub fn report(&self, project: &mut Project) {
        project.set_problems_in(&format!("{FOLDER}/"), self.problems.clone());
    }
}

/// Defines each tool of `loaded` with `define` and keeps the ones it takes. Says what is wrong
/// with the load: the errors of its files, the tools `define` refused, and every tool whose
/// sound, card or page fails at its defaults.
fn define(
    bun: &Arc<Bun>,
    loaded: &mut Loaded,
    mut define: impl FnMut(JsonTool) -> Result<(), String>,
) -> Vec<Problem> {
    let of_file = |file: &str, message: String| Problem {
        path: format!("{FOLDER}/{file}"),
        message,
    };
    let mut problems: Vec<Problem> = (loaded.errors.iter())
        .map(|error| of_file(&error.file, error.message.clone()))
        .collect();
    loaded
        .tools
        .retain(|info| match define(info.json_tool(bun)) {
            Ok(()) => true,
            Err(message) => {
                problems.push(of_file(
                    &info.file,
                    format!("tool {}: {message}", info.name),
                ));
                false
            }
        });
    for info in &loaded.tools {
        let sound = tools::Sounds::new(info, bun).code(&serde_json::json!({}));
        let card = drawn(bun, &info.name, false)
            .map_err(|error| format!("its card does not draw: {error}"));
        let page = match info.page {
            true => drawn(bun, &info.name, true)
                .map_err(|error| format!("its page does not draw: {error}")),
            false => Ok(()),
        };
        for message in [sound.map(|_| ()), card, page]
            .into_iter()
            .filter_map(Result::err)
        {
            problems.push(of_file(
                &info.file,
                format!("tool {}: {message}", info.name),
            ));
        }
    }
    problems
}

/// Whether the card or the page of `tool` draws a tree the window can show.
fn drawn(bun: &Bun, tool: &str, page: bool) -> Result<(), String> {
    let tree = bun.draw(tool, page)?;
    serde_json::from_value::<tree::Node>(tree)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Writes what the runtime owns in `extensions/`, only where the text changed, and the host
/// script, which is no file of the project.
fn write_files(extensions: &Path) -> std::io::Result<PathBuf> {
    for (name, text) in [("sdk.ts", SDK), ("tsconfig.json", TSCONFIG)] {
        let path = extensions.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text)?;
        }
    }
    let host = std::env::temp_dir().join(format!("sound-tools-host-{}.ts", std::process::id()));
    std::fs::write(&host, HOST)?;
    Ok(host)
}

/// Bun on the `PATH`, or where its installer puts it. An app opened from the Finder has a
/// short `PATH`.
fn bun_program() -> Option<PathBuf> {
    let bun = format!("bun{}", std::env::consts::EXE_SUFFIX);
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join(&bun));
    let installed =
        std::env::var_os("HOME").map(|home| Path::new(&home).join(".bun/bin").join(&bun));
    on_path.chain(installed).find(|path| path.is_file())
}
