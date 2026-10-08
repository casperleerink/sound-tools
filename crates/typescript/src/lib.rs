//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun. An experiment: how far a project can make Sound Tools its own with no build.
//!
//! A project with an `extensions/` folder starts Bun on `host.ts` when it opens. Bun loads every
//! `.ts` and `.tsx` file there and says which tools they define: the fields of a record, a doc
//! for agents, and a function that makes the tool's sound in Hum. The runtime registers each
//! as a [`JsonTool`](sound_core::JsonTool) before the records load, so a record of one loads
//! like any other. Its check runs in Rust; only a new combination of choices asks Bun for Hum.
//! In the window, a save of a file defines the tools again and draws the cards again.
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
use std::time::{Duration, Instant};

use sound_core::{AgentDoc, Problem, Project, Registry};

pub use window::start_window;

use crate::bun::{Bun, Loaded};

/// The folder of a project that holds its own tools and cards.
pub const FOLDER: &str = "extensions";

/// How long the first load may take. Bun starts in a few tens of milliseconds.
const FIRST_LOAD_TIMEOUT: Duration = Duration::from_secs(10);

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
    bun: Option<Arc<Bun>>,
    loaded: Option<Loaded>,
    /// What went wrong before the project could hear of it, such as Bun missing.
    problems: Vec<Problem>,
    /// How long Bun took from start to its first load.
    pub started_in: Duration,
}

impl Extensions {
    /// Starts the tools of the project in `folder`. `None` when it has no `extensions/` folder,
    /// which is every project that has not asked for its own tools.
    pub fn start(folder: &Path) -> Option<Self> {
        let extensions = folder.join(FOLDER);
        if !extensions.is_dir() {
            return None;
        }
        let started = Instant::now();
        let mut this = Self {
            bun: None,
            loaded: None,
            problems: Vec::new(),
            started_in: Duration::ZERO,
        };
        let problem = |message: String| Problem {
            path: format!("{FOLDER}/"),
            message,
        };
        let Some(bun_program) = bun_program() else {
            let message = "Bun is not installed, so the tools of this project do not load: install it from https://bun.sh and open the project again";
            this.problems.push(problem(message.to_string()));
            return Some(this);
        };
        let bun = write_files(&extensions)
            .and_then(|host| Bun::start(&bun_program, &host, &extensions))
            .map_err(|error| error.to_string());
        let loaded = bun.and_then(|bun| {
            let loaded = bun.first_load(FIRST_LOAD_TIMEOUT)?;
            Ok((Arc::new(bun), loaded))
        });
        match loaded {
            Ok((bun, loaded)) => {
                this.bun = Some(bun);
                this.loaded = Some(loaded);
            }
            Err(error) => this.problems.push(problem(error)),
        }
        this.started_in = started.elapsed();
        Some(this)
    }

    /// Registers every tool of the first load. Call it before the project opens, so their
    /// records load with it. A tool that cannot be registered becomes a problem of its file.
    pub fn register(&mut self, registry: &mut Registry) {
        let (Some(bun), Some(loaded)) = (&self.bun, &self.loaded) else {
            return;
        };
        for info in &loaded.tools {
            if let Err(error) = registry.json_tool(info.json_tool(bun)) {
                self.problems.push(Problem {
                    path: format!("{FOLDER}/{}", info.file),
                    message: error.to_string(),
                });
            }
        }
    }

    /// Lists what is wrong with `extensions/` among the problems of the project: files that
    /// failed to load, and tools whose sound at their defaults is no Hum that compiles.
    pub fn report(&self, project: &mut Project) {
        let mut problems = self.problems.clone();
        if let (Some(bun), Some(loaded)) = (&self.bun, &self.loaded) {
            problems.extend(problems_of(bun, loaded));
        }
        project.set_problems_in(&format!("{FOLDER}/"), problems);
    }
}

/// What is wrong with a load: its errors, and every tool whose Hum at its defaults fails.
fn problems_of(bun: &Arc<Bun>, loaded: &Loaded) -> Vec<Problem> {
    let mut problems: Vec<Problem> = (loaded.errors.iter())
        .map(|error| Problem {
            path: format!("{FOLDER}/{}", error.file),
            message: error.message.clone(),
        })
        .collect();
    for info in &loaded.tools {
        if let Err(message) = tools::Sounds::new(info, bun).code(&serde_json::json!({})) {
            problems.push(Problem {
                path: format!("{FOLDER}/{}", info.file),
                message: format!("tool {}: {message}", info.name),
            });
        }
    }
    problems
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
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|folder| folder.join("bun"));
    let installed = std::env::var_os("HOME").map(|home| Path::new(&home).join(".bun/bin/bun"));
    on_path.chain(installed).find(|path| path.is_file())
}
