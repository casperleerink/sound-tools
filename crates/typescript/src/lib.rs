//! The project's own tools and cards, written in TypeScript in its `extensions/` folder and run
//! by Bun, with no build. The DAW template is made of them.
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
mod bun;
pub mod devices;
mod fader;
mod feeds;
mod lane;
pub mod live;
mod meter;
pub mod path;
mod project;
mod readout;
mod recorder;
mod records;
mod region;
mod samples;
pub mod shortcuts;
mod timeline;
mod tools;
pub mod touch;
pub mod window;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{AgentDoc, JsonTool, Problem, Project, Registry, ToolSchema};
use sound_notes::{Amount, Bend, Controller, NoteEvent, Pitch, Velocity};

pub use behaviours::{take_declared, wait_for_behaviours};
pub use recorder::{ARMED, HEARD, Heard, RECORDER, RecorderState};
pub use timeline::timeline_state;
pub use tools::forget_gone;

/// Registers the feeds and facts of the tools this crate defines, and the keys down on the
/// sound of every instance. Call it before the project opens.
pub fn register_feeds(registry: &mut Registry) {
    region::register_feeds(registry);
    recorder::register_feeds(registry);
    registry.every_instance_feed(
        "sounding",
        "1 for each of the 128 keys down on its sound now, else 0: `sounding.60` is middle C.",
        window::sounding,
    );
}

use crate::bun::{Bun, LoadFailure, Loaded, LoadedTool, Skipped};
use crate::tools::{Sounds, ToolInfo, Tools};

/// The folder of a project that holds its own tools and cards.
pub const FOLDER: &str = "extensions";

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

/// The SDK of every project, by its path in `extensions/`. The runtime adds `sdk/records.ts`,
/// `sdk/feeds.ts` and `sdk/icons.ts`.
const SDK: [(&str, &str); 9] = [
    ("sdk.ts", include_str!("sdk.ts")),
    ("sdk/fields.ts", include_str!("sdk/fields.ts")),
    ("sdk/notes.ts", include_str!("sdk/notes.ts")),
    ("sdk/sound.ts", include_str!("sdk/sound.ts")),
    ("sdk/tools.ts", include_str!("sdk/tools.ts")),
    ("sdk/clock.ts", include_str!("sdk/clock.ts")),
    ("sdk/values.ts", include_str!("sdk/values.ts")),
    ("sdk/view.ts", include_str!("sdk/view.ts")),
    ("sdk/test.d.ts", include_str!("sdk/test.d.ts")),
];
/// What the SDK of a project of the GPUI app had in `sdk/` that this one has not.
const GONE: [&str; 8] = [
    "sdk/live.ts",
    "sdk/reactivity.ts",
    "sdk/nodes.ts",
    "sdk/drawing.ts",
    "sdk/components.ts",
    "sdk/page.ts",
    "sdk/card.ts",
    "sdk/host.ts",
];
pub const HOST: &str = include_str!("host.ts");
/// What [`HOST`] uses of Node and Bun, for a check of its types.
pub const HOST_TYPES: &str = include_str!("node.d.ts");
const BUN_TYPES: &str = include_str!("bun.d.ts");
/// What the runtime writes in `extensions/` and owns there: files, and the folder `sdk`.
const OWNED: [&str; 4] = ["sdk.ts", "sdk", "tsconfig.json", "bun.d.ts"];
/// What the runtime writes in `extensions/`, which the project history leaves out.
pub const WRITTEN_BY_THE_RUNTIME: [&str; 5] =
    [OWNED[0], OWNED[1], OWNED[2], OWNED[3], bun::LOG_FILE];
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
pub const AGENT_DOCS: [AgentDoc; 1] = [AgentDoc {
    name: "tools",
    group: WRITING,
    when: "No tool does what you need: write a device for a track, change the song page, or make the whole project one tool",
    markdown: include_str!("tools.md"),
}];

const WRITING: &str = "Writing tools";
/// The group of the docs of the built-in children.
const BLOCKS: &str = "Blocks a parent plays";
/// The group of the docs of the tools a project writes.
const PROJECT_TOOLS: &str = "Tools of this project";

/// The tools of a project, from their first load.
pub struct Extensions {
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
    /// `schemas`. Bun `logs` for a runtime that holds the project, see [`Bun::start`]. Waits
    /// for Bun, up to a timeout for each file: not on the thread that draws.
    pub fn load(folder: &Path, built_in: &BuiltIn, logs: bool) -> Option<FirstLoad> {
        let folder = folder.join(FOLDER);
        if !folder.is_dir() {
            return None;
        }
        let opening = bun::new_opening();
        let launched = write_sdk(&folder, built_in)
            .map_err(|error| error.to_string())
            .and_then(|()| launch((&folder, opening), Skipped::new(), logs));
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
                view.err()
                    .map(|error| format!("its view does not draw: {error}")),
            )
            .collect();
        if !failed.is_empty() {
            problems.push(info.problem(failed.join("; ")));
        }
        if info.old_views {
            problems.push(info.problem(window::OLD_VIEWS));
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
                // What `bun test` runs.
                && !name.contains(".test.")
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
    logs: bool,
) -> Result<Option<Launched>, String> {
    let mut files = tool_files(extensions);
    write_icon_names(extensions).map_err(|error| error.to_string())?;
    // Bun runs for the generic card too.
    let card = extensions.join(window::GENERIC_CARD).is_file();
    if files.is_empty() && !card {
        return Ok(None);
    }
    if card {
        files.push(window::GENERIC_CARD.to_string());
        files.extend(window::card_files(extensions));
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
        match Bun::start(&program, &host, (extensions, opening), &loading, logs) {
            Ok((bun, loaded)) => {
                // The tools that own children ask it what they declare while their records
                // play, also outside the window.
                let bun = Arc::new(bun);
                let (sounds, mut problems) = checked(loaded, &skipped, &bun);
                // A project with the kit of the templates draws its devices with it.
                if !card && extensions.join(window::THEME).is_file() {
                    problems.push(folder_problem(format!(
                        "{} is missing, so a device whose tool has no view shows no card: write one that exports `card`, a `Card`",
                        window::GENERIC_CARD
                    )));
                }
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
                return Err(format!("the code of the project {why}"));
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

/// What the code of a project reads of the built-in tools, for the types of its SDK: their
/// records, the feeds of each, and the JSON Schema of the facts of each that adds some.
#[derive(Default)]
pub struct BuiltIn {
    pub schemas: Vec<ToolSchema>,
    pub feeds: Vec<sound_core::feeds::FeedName>,
    pub facts: Vec<(&'static str, serde_json::Value)>,
    /// The bang inputs of each tool that has some.
    pub commands: Vec<(&'static str, Vec<&'static str>)>,
}

/// Writes what the runtime owns in `extensions/`, only where the text changed: the SDK with
/// the records, feeds and facts of the built-in tools.
pub fn write_sdk(extensions: &Path, built_in: &BuiltIn) -> std::io::Result<()> {
    // Bun's own types, once installed, declare `Bun` as well.
    let bun_types = extensions.join("node_modules/bun-types").is_dir();
    let records = records::sdk_records(&built_in.schemas, &built_in.commands);
    let feeds = records::sdk_feeds(&built_in.feeds, &built_in.facts, &built_in.schemas);
    std::fs::create_dir_all(extensions.join("sdk"))?;
    let files = [
        ("sdk/records.ts", records.as_str()),
        ("sdk/feeds.ts", feeds.as_str()),
        ("tsconfig.json", TSCONFIG),
        ("bun.d.ts", BUN_TYPES),
    ];
    // Keeps the log out of git. One the project has already is its own.
    let ignore = extensions.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, format!("{}\n", bun::LOG_FILE))?;
    }
    // Other files in `sdk/` are the project's.
    for name in GONE {
        match std::fs::remove_file(extensions.join(name)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    for (name, text) in files.into_iter().chain(SDK) {
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

/// `sdk/icons.ts`: the names of the icons in `ui/icons/`, so a view that draws one without a
/// file does not type-check. Written as the code loads, so an icon added meanwhile is there.
fn write_icon_names(extensions: &Path) -> std::io::Result<()> {
    let mut names: Vec<String> = std::fs::read_dir(extensions.join("ui/icons"))
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            Some(format!("{:?}", name.strip_suffix(".svg")?))
        })
        .collect();
    names.sort();
    let union = match names.is_empty() {
        true => "never".to_string(),
        false => names.join(" | "),
    };
    let text = format!(
        "// The icons of `ui/icons/`, by file name. Written as the code loads.\nexport type IconName = {union};\n"
    );
    let path = extensions.join("sdk/icons.ts");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        std::fs::create_dir_all(extensions.join("sdk"))?;
        std::fs::write(path, text)?;
    }
    Ok(())
}

/// The instances whose sample, region file or periods were read since the last call, to run
/// their behaviours again.
pub fn take_read(assets: &sound_core::Assets) -> Vec<sound_core::InstanceId> {
    let mut read = samples::take_read(assets);
    read.extend(region::take_read(assets));
    read
}

/// Whether this machine has a Bun to run the tools with.
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
        write_sdk(extensions.path(), &BuiltIn::default()).unwrap();
        assert!(extensions.path().join("bun.d.ts").is_file());
        std::fs::create_dir_all(extensions.path().join("node_modules/bun-types")).unwrap();
        write_sdk(extensions.path(), &BuiltIn::default()).unwrap();
        assert!(!extensions.path().join("bun.d.ts").exists());
    }

    #[test]
    fn the_sdk_of_the_gpui_app_goes_and_the_projects_own_files_stay() {
        let extensions = tempfile::tempdir().unwrap();
        let folder = extensions.path();
        std::fs::create_dir_all(folder.join("sdk")).unwrap();
        std::fs::write(folder.join("sdk/my-helper.ts"), "export {}").unwrap();
        std::fs::write(folder.join("sdk/live.ts"), "export {}").unwrap();
        write_sdk(folder, &BuiltIn::default()).unwrap();
        assert!(folder.join("sdk/view.ts").is_file());
        assert!(!folder.join("sdk/live.ts").exists());
        assert!(folder.join("sdk/my-helper.ts").is_file());
    }

    /// Code of an older Sound Tools as people write imports: comments, line breaks, single
    /// quotes and renamed parts, of the SDK and of what it no longer has.
    const OLD_PAGE: &str = r#"import {
  h, number, // the pitch
  Knob /* gone */,
  Page as Screen,
  sine as wave,
  tool // makes it
} from './sdk';
export const old = tool({
  name: "old", title: "Old", when: "Never", doc: "A tone.",
  state: { hz: number({ min: 20, max: 2000, default: 220 }) },
  sound: ({ hz }) => wave(hz).times(0.2),
  view: () => <Screen><Knob field="hz" /></Screen>,
});
"#;

    #[test]
    fn a_file_of_an_older_sound_tools_loads_whatever_shape_its_imports_have() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(FOLDER)).unwrap();
        std::fs::write(project.path().join(FOLDER).join("old.tsx"), OLD_PAGE).unwrap();
        let extensions = Extensions::load(project.path(), &BuiltIn::default(), false)
            .unwrap()
            .register(&mut Registry::new());
        let messages: Vec<&str> = (extensions.problems.iter())
            .map(|problem| problem.message.as_str())
            .collect();
        assert!(
            matches!(&messages[..], [message] if message.ends_with(window::OLD_VIEWS)),
            "{messages:?}"
        );
        assert!(
            extensions
                .tools
                .iter()
                .any(|tool| tool.name == "old" && tool.old_views)
        );
    }

    #[test]
    fn a_folder_with_no_tool_starts_no_bun_but_has_the_sdk() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(FOLDER)).unwrap();
        let extensions = Extensions::load(project.path(), &BuiltIn::default(), true)
            .unwrap()
            .register(&mut Registry::new());
        assert!(extensions.bun.is_none());
        assert!(extensions.problems.is_empty(), "{:?}", extensions.problems);
        assert!(project.path().join(FOLDER).join("sdk.ts").is_file());
        assert!(project.path().join(FOLDER).join("sdk/records.ts").is_file());
        // The runtime's own files are no tool.
        assert!(tool_files(&project.path().join(FOLDER)).is_empty());
    }
}
