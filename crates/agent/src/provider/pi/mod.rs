//! Pi: `pi --mode rpc`, one process per thread, with our extension (`extension.js`) loaded.
//!
//! Every Pi flag and record is in this module. Pi reads one JSON command per line on stdin and
//! writes responses and events on stdout. Pi has no approvals of its own: the extension asks
//! before a tool runs, as an extension UI request we answer. The sign-in, the models and the
//! one-shot question are in [`setup`].

mod mapper;
mod protocol;
mod setup;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use smol::channel::Receiver;

use self::mapper::{APPROVAL_MODE, MESSAGE, MODEL, MODELS, Mapper, STOP};
use self::protocol::Outgoing;
pub(super) use self::setup::{PiSignIn, ask_small_model, models, sign_in, sign_out, status};
use super::{AgentEvent, ApprovalMode, Command, Installed, ThreadOptions, jsonl};

const EXTENSION: &str = include_str!("extension.js");

/// The tools a composer needs, Pi's own. No web: the agent works in the project.
const TOOLS: &str = "read,bash,edit,write,grep,find,ls";

/// The extension on disk, written once per run of the app into a folder of its own. `-e`
/// takes only a path.
fn extension() -> io::Result<PathBuf> {
    // Kept for as long as the app runs: every Pi it starts loads it. Written again when the
    // system clears old temporary files meanwhile.
    static WRITTEN: Mutex<Option<PathBuf>> = Mutex::new(None);
    let mut written = WRITTEN
        .lock()
        .map_err(|_| io::Error::other("a write of it panicked"))?;
    if let Some(file) = written.as_ref().filter(|file| file.is_file()) {
        return Ok(file.clone());
    }
    let write = || -> io::Result<PathBuf> {
        let folder = tempfile::Builder::new()
            .prefix("sound-tools-pi")
            .tempdir()?
            .keep();
        let file = folder.join("sound-tools.js");
        fs::write(&file, EXTENSION)?;
        Ok(file)
    };
    let file = write().map_err(|error| {
        io::Error::other(format!("the extension of Pi was not written: {error}"))
    })?;
    *written = Some(file.clone());
    Ok(file)
}

/// The value of `--sound-tools-approval` and of the extension's command.
fn approval_mode(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::AskForEverything => "ask_for_everything",
        ApprovalMode::AskBeforeCommands => "ask_before_commands",
        ApprovalMode::NeverAsk => "never_ask",
    }
}

/// `pi` with only our extension, and nothing of the composer's own Pi setup that would change
/// what the agent may do: no other extensions, skills, prompt templates or MCP servers, and
/// nothing the project folder would load. The project's AGENTS.md still loads.
fn command(installed: &Installed) -> io::Result<std::process::Command> {
    let mut command = sound_core::process::background_command(&installed.program);
    command
        .env_clear()
        .envs(&installed.environment)
        .args(["--no-extensions", "--extension"])
        .arg(extension()?)
        .args([
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-mcp",
            "--no-approve",
        ]);
    Ok(command)
}

/// [`command`] in RPC mode, in the temporary folder: a Pi of no thread works in no project.
fn rpc(installed: &Installed) -> io::Result<std::process::Command> {
    let mut command = command(installed)?;
    command
        .args(["--mode", "rpc"])
        .current_dir(std::env::temp_dir());
    Ok(command)
}

fn arguments(options: &ThreadOptions, session_id: &str) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        // The session of this folder with that id, made when it is not there yet.
        "--session-id",
        session_id,
        "--tools",
        TOOLS,
        "--sound-tools-approval",
        approval_mode(options.approval_mode),
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if let Some(model) = &options.model {
        arguments.extend(["--model".into(), model.into()]);
    }
    if let Some(instructions) = &options.instructions {
        arguments.extend(["--append-system-prompt".into(), instructions.into()]);
    }
    arguments
}

/// Starts Pi for one thread. `session_id` is the one `options.resume` names, or the new one.
pub(super) fn start(
    options: ThreadOptions,
    session_id: &str,
    commands: Receiver<Command>,
) -> io::Result<Events> {
    let mut command = rpc(&options.installed)?;
    command
        .args(arguments(&options, session_id))
        .current_dir(&options.folder);
    let mut lines = Lines {
        mapper: Mapper::new(options.folder),
        sent: 0,
    };
    // Its answer is the start: the models the composer can pick.
    let models = lines.command(MODELS, "/sound-tools-models".to_string());
    Events::start(command, lines, [models], commands)
}

pub(super) type Events = jsonl::Events<Lines>;

/// Pi's side of [`jsonl::Protocol`]: the mapper, and how many commands went, for their ids.
#[derive(Debug)]
pub(super) struct Lines {
    mapper: Mapper,
    sent: u64,
}

impl Lines {
    /// An id of the kind `kind`, which the mapper reads back from a refusal.
    fn id(&mut self, kind: &str) -> String {
        self.sent += 1;
        format!("{kind}-{}", self.sent)
    }

    fn command(&mut self, kind: &str, command: String) -> Outgoing {
        Outgoing::command(self.id(kind), command)
    }
}

impl jsonl::Protocol for Lines {
    type Outgoing = Outgoing;

    fn command(&mut self, command: Command) -> Vec<Outgoing> {
        let message = match command {
            Command::Send(message) => Some(Outgoing::message(self.id(MESSAGE), message)),
            Command::Interrupt => Some(Outgoing::Abort { id: self.id(STOP) }),
            // An approval the turn already ended has nobody waiting for the answer.
            Command::Answer(approval, answer) => self.mapper.answer(&approval, answer),
            Command::SetApprovalMode(mode) => Some(self.command(
                APPROVAL_MODE,
                format!("/sound-tools-approval {}", approval_mode(mode)),
            )),
            Command::SetModel(model) => {
                // Our models are `provider/id`; anything else is Pi's to refuse.
                let (provider, model_id) = model.split_once('/').unwrap_or(("", &model));
                Some(Outgoing::SetModel {
                    id: self.id(MODEL),
                    provider: provider.to_string(),
                    model_id: model_id.to_string(),
                })
            }
        };
        message.into_iter().collect()
    }

    fn sent(&mut self, message: &Outgoing) -> Vec<AgentEvent> {
        self.mapper.sent(message)
    }

    fn received(&mut self, line: &str) -> (Vec<Outgoing>, Vec<AgentEvent>) {
        self.mapper.received(line)
    }

    fn unwritable(&self, error: serde_json::Error) -> AgentEvent {
        AgentEvent::Error {
            message: format!("Could not write to Pi: {error}"),
        }
    }

    fn exited(&mut self, code: Option<i32>, said: Option<&str>) -> Vec<AgentEvent> {
        self.mapper.exited(code, said)
    }
}
