//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun. An experiment: how far a project can make Sound Tools its own with no build.
//!
//! A project whose `extensions/` folder has a tool file starts Bun on `host.ts` when it opens.
//! Bun loads every `.ts` and `.tsx` file there once and says which tools they define: the
//! fields of a record, a doc for agents, and the sound of every combination of its choices, a
//! graph of signals that the SDK sends as JSON. The runtime registers each as a [`JsonTool`]
//! before the records load, so a record of one loads like any other, and plays with no Bun.
//! Only an owner asks Bun while it plays: how its children connect (`behaviours`). The
//! built-in children a parent plays with, such as the `timeline`, are tools of this crate,
//! defined with them.
//!
//! In the window, any change in `extensions/`, a crash or a hang of Bun starts a new Bun, which
//! loads every file again and defines the tools again: one path. Nothing of it runs on the
//! audio thread, and the window never waits for Bun.

mod behaviours;
#[cfg(test)]
mod bindings;
mod bun;
mod card;
mod fader;
mod feeds;
mod lane;
mod live;
mod meter;
#[cfg(test)]
mod performance;
mod recorder;
mod records;
mod region;
mod samples;
mod timeline;
mod tools;
mod tree;
mod window;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{AgentDoc, JsonTool, Problem, Project, Registry, ToolSchema};
use sound_notes::{Amount, Bend, Controller, NoteEvent, Pitch, Velocity};

pub use behaviours::{asked_behaviours, take_declared, wait_for_behaviours};
pub use bun::{heard_batches, heard_bytes, last_batch, sent_bytes, sent_updates_and_events};
pub use card::embedded_view;
#[cfg(feature = "test-support")]
pub use feeds::shown_by_live_values;
pub use recorder::{ARMED, HEARD, Heard, RECORDER, RecorderState};
pub use window::start_window;

use crate::bun::{Bun, LoadFailure, Loaded, LoadedTool, Skipped};
use crate::tools::{Sounds, ToolInfo, Tools};

/// The folder of a project that holds its own tools and cards.
pub const FOLDER: &str = "extensions";

/// The key context of a card or a page whose tool hears the computer keyboard while it has the
/// focus. The plain keys of the window, such as space, leave it alone.
pub const KEY_CONTEXT: &str = "ToolKeys";

/// A note event as the code of a tool sends and hears it, see `NoteEvent` in `sdk.ts`: a
/// velocity or a value from 0 to 1, the bend from -1 to 1. The pedal is controller 64 and the
/// mod wheel controller 1, as in MIDI.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum NoteMessage {
    NoteOn { pitch: u8, velocity: f32 },
    NoteOff { pitch: u8 },
    Cc { controller: u8, value: f32 },
    Bend { value: f32 },
    Pressure { value: f32 },
    AllOff,
}

impl From<NoteEvent> for NoteMessage {
    fn from(event: NoteEvent) -> Self {
        let fraction = |value: u8| f32::from(value) / 127.0;
        match event {
            NoteEvent::On { pitch, velocity } => Self::NoteOn {
                pitch: pitch.number(),
                velocity: fraction(velocity.value()),
            },
            NoteEvent::Off { pitch } => Self::NoteOff {
                pitch: pitch.number(),
            },
            NoteEvent::Pedal(pedal) => Self::Cc {
                controller: Controller::SUSTAIN_PEDAL,
                value: fraction(pedal.value()),
            },
            NoteEvent::ModWheel(amount) => Self::Cc {
                controller: Controller::MOD_WHEEL,
                value: amount.fraction(),
            },
            NoteEvent::Control { controller, value } => Self::Cc {
                controller: controller.number(),
                value: value.fraction(),
            },
            NoteEvent::Bend(bend) => Self::Bend {
                value: bend.fraction(),
            },
            NoteEvent::Pressure(amount) => Self::Pressure {
                value: amount.fraction(),
            },
            NoteEvent::AllOff => Self::AllOff,
        }
    }
}

impl NoteMessage {
    /// The event of the note contract, at the nearest step of each value. `None` for a
    /// controller MIDI has no control for: 120 and on.
    fn event(self) -> Option<NoteEvent> {
        // `as` saturates, and a NaN is 0.
        let steps = |value: f32| (value * 127.0).round() as i64;
        let amount = |value| Amount::nearest(steps(value));
        let pitch = |pitch| Pitch::nearest(i64::from(pitch));
        Some(match self {
            Self::NoteOn {
                pitch: key,
                velocity,
            } => NoteEvent::On {
                pitch: pitch(key),
                velocity: Velocity::nearest(steps(velocity)),
            },
            Self::NoteOff { pitch: key } => NoteEvent::Off { pitch: pitch(key) },
            Self::Cc { controller, value } => NoteEvent::controller(controller, amount(value))?,
            Self::Bend { value } => {
                let value = value.clamp(-1.0, 1.0);
                let most = if value < 0.0 { 8192.0 } else { 8191.0 };
                NoteEvent::Bend(Bend::nearest((value * most).round() as i64))
            }
            Self::Pressure { value } => NoteEvent::Pressure(amount(value)),
            Self::AllOff => NoteEvent::AllOff,
        })
    }
}

const SDK: &str = include_str!("sdk.ts");
pub const HOST: &str = include_str!("host.ts");
/// What [`HOST`] uses of Node and Bun, for a check of its types.
pub const HOST_TYPES: &str = include_str!("node.d.ts");
const BUN_TYPES: &str = include_str!("bun.d.ts");
/// What the runtime writes in `extensions/` and owns there.
const OWNED: [&str; 3] = ["sdk.ts", "tsconfig.json", "bun.d.ts"];
/// The sounds of a tool that do not build that its problem names; it counts the others.
const SHOWN_FAILURES: usize = 3;
/// JSX makes the nodes of `sdk.ts`. Also what `bun check -p extensions` checks with.
const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "esnext",
    "module": "esnext",
    "moduleResolution": "bundler",
    "jsx": "react",
    "jsxFactory": "h",
    "jsxFragmentFactory": "Fragment",
    "noEmit": true
  }
}
"#;

/// How an agent writes a tool of the project.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "extensions",
    when: "No tool does what you need: write a device for a track, or make the whole project one tool",
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
    /// for the agent that writes the first one, with the records of the built-in tools of
    /// `schemas`. Waits for Bun, up to a timeout for each file: not on the thread that draws.
    pub fn load(folder: &Path, schemas: &[ToolSchema]) -> Option<FirstLoad> {
        let folder = folder.join(FOLDER);
        if !folder.is_dir() {
            return None;
        }
        let opening = bun::new_opening();
        let launched = write_sdk(&folder, schemas)
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
/// the files, the files Bun runs without, and every tool whose fields, sounds or view fail.
/// Takes as long as compiling every sound: not on the thread that draws.
fn checked(loaded: Loaded, skipped: &Skipped, bun: &Arc<Bun>) -> (Vec<Sounds>, Vec<Problem>) {
    let file_problem = |file: &str, message: &str| Problem {
        path: format!("{FOLDER}/{file}"),
        message: message.to_string(),
    };
    let mut problems: Vec<Problem> = (loaded.errors.iter())
        .map(|error| file_problem(&error.file, &error.message))
        .chain(skipped.iter().map(|(file, why)| file_problem(file, why)))
        .collect();
    let mut kept = Vec::new();
    for tool in loaded.tools {
        // Before anything is made from the fields, such as a list at its defaults. Each sound
        // checks the ranges of what it reads as it builds.
        let checked = tool.info.fields.check_defaults("state.");
        match checked {
            Ok(()) => kept.push(tool),
            Err(error) => problems.push(tool.info.problem(error)),
        }
    }
    let infos = kept
        .iter()
        .map(|tool| (tool.info.name.clone(), tool.info.clone()));
    let infos: Arc<Tools> = Arc::new(infos.collect());
    let mut tools = Vec::new();
    for tool in kept {
        let LoadedTool { info, sounds, view } = tool;
        let sounds = Sounds::new(&info, sounds, (bun, &infos));
        // One problem a file: a list keeps one.
        let mut sounds_failed: Vec<&String> = sounds.failures().collect();
        sounds_failed.sort();
        let shown = sounds_failed.len().min(SHOWN_FAILURES);
        let more = (sounds_failed.len() > shown)
            .then(|| format!("and {} more sounds", sounds_failed.len() - shown));
        let failed: Vec<String> = (sounds_failed.into_iter().take(shown).cloned())
            .chain(more)
            .chain(
                drawn(view)
                    .err()
                    .map(|error| format!("its view does not draw: {error}")),
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
    // First, so a tool of the project of its name is the one refused.
    let tools = built_in().into_iter();
    let refused = define(
        tools
            .chain(sounds.into_iter().map(Sounds::json_tool))
            .collect(),
    );
    infos.retain(|info| {
        let Some((_, message)) = refused.iter().find(|(name, _)| *name == info.name) else {
            return true;
        };
        problems.push(info.problem(message));
        false
    });
    infos
}

/// The built-in children a parent of the project plays with.
fn built_in() -> [JsonTool; 6] {
    [
        timeline::json_tool(),
        meter::json_tool(),
        fader::json_tool(),
        lane::json_tool(),
        region::json_tool(),
        recorder::json_tool(),
    ]
}

/// The records of the built-in children, for `sdk.ts`.
pub(crate) fn built_in_schemas() -> [ToolSchema; 6] {
    [
        timeline::schema(),
        meter::schema(),
        fader::schema(),
        lane::schema(),
        region::schema(),
        recorder::schema(),
    ]
}

/// Whether a card or a page made nodes the window can show.
fn drawn(ops: Result<serde_json::Value, String>) -> Result<(), String> {
    let ops: Vec<tree::Op> = serde_json::from_value(ops?).map_err(|error| error.to_string())?;
    tree::NodeTable::default().apply(ops)
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

/// Writes what the runtime owns in `extensions/`, only where the text changed: the SDK with
/// the records of the built-in tools of `schemas`.
fn write_sdk(extensions: &Path, schemas: &[ToolSchema]) -> std::io::Result<()> {
    // Bun's own types, once installed, declare `Bun` as well.
    let bun_types = extensions.join("node_modules/bun-types").is_dir();
    let sdk = format!("{SDK}{}", records::sdk_records(schemas));
    for (name, text) in OWNED.into_iter().zip([&*sdk, TSCONFIG, BUN_TYPES]) {
        let path = extensions.join(name);
        if name == "bun.d.ts" && bun_types {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                _ => continue,
            }
        }
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
pub fn bun_program() -> Option<PathBuf> {
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

    /// So a recording made from `onNotes` and sent again plays what was heard, the pedal and
    /// the mod wheel included, which a tool sends as their MIDI controllers.
    #[test]
    fn every_event_a_tool_hears_is_the_same_event_when_it_sends_it_again() {
        let pitch = Pitch::new(60).unwrap();
        let controller = |number| Controller::new(number).unwrap();
        let events = (1..=127).flat_map(|value| {
            let amount = Amount::new(value).unwrap();
            [
                NoteEvent::On {
                    pitch,
                    velocity: Velocity::new(value).unwrap(),
                },
                NoteEvent::Pedal(sound_notes::Pedal::new(value).unwrap()),
                NoteEvent::ModWheel(amount),
                NoteEvent::Pressure(amount),
                NoteEvent::Control {
                    controller: controller(value % 60 + 2),
                    value: amount,
                },
            ]
        });
        let bends = (-8192..=8191).map(|bend| NoteEvent::Bend(Bend::new(bend).unwrap()));
        let others = [NoteEvent::Off { pitch }, NoteEvent::AllOff];
        for event in events.chain(bends).chain(others) {
            assert_eq!(NoteMessage::from(event).event(), Some(event));
        }
        let channel_mode = NoteMessage::Cc {
            controller: 120,
            value: 0.0,
        };
        assert_eq!(channel_mode.event(), None);
    }

    /// Bun's own types declare `Bun` too, and two declarations fail `bun check`.
    #[test]
    fn the_types_of_bun_are_left_out_once_bun_types_are_installed() {
        let extensions = tempfile::tempdir().unwrap();
        write_sdk(extensions.path(), &[]).unwrap();
        assert!(extensions.path().join("bun.d.ts").is_file());
        std::fs::create_dir_all(extensions.path().join("node_modules/bun-types")).unwrap();
        write_sdk(extensions.path(), &[]).unwrap();
        assert!(!extensions.path().join("bun.d.ts").exists());
    }

    #[test]
    fn a_folder_with_no_tool_starts_no_bun_but_has_the_sdk() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(FOLDER)).unwrap();
        let extensions = Extensions::load(project.path(), &[])
            .unwrap()
            .register(&mut Registry::new());
        assert!(extensions.bun.is_none());
        assert!(extensions.problems.is_empty(), "{:?}", extensions.problems);
        assert!(project.path().join(FOLDER).join("sdk.ts").is_file());
        // The runtime's own files are no tool.
        assert!(tool_files(&project.path().join(FOLDER)).is_empty());
    }
}
