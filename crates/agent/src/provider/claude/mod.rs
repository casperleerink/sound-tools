//! Claude Code: the `claude` CLI in stream-json mode, one process per thread.
//!
//! Every Claude flag and type is in this module. The process reads one JSON message per line
//! on stdin and writes one per line on stdout. Approvals come as `can_use_tool` requests that
//! we answer (`--permission-prompt-tool stdio`). It exits when stdin closes. The pinned
//! download and the sign-in are in [`setup`].

mod mapper;
// The fake CLI is a shell script.
#[cfg(test)]
#[cfg(unix)]
mod process_tests;
mod protocol;
mod setup;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::Path;

use serde_json::Value;
use smol::channel::Receiver;

use self::mapper::Mapper;
use self::protocol::{CliRequest, ControlResponse, Incoming, Outgoing, PermissionMode, Request};
pub(super) use self::setup::{
    SIGN_IN_CHOICES, account, ask_small_model, download, sign_in, sign_out,
};
use super::{AgentEvent, ApprovalMode, Command, ThreadOptions, jsonl};

/// The tools a composer needs. No web, no subagents, no questions: the agent asks in plain
/// text.
#[cfg(not(windows))]
const TOOLS: &str = "Bash,Read,Edit,Write,Glob,Grep";
/// On Windows, Bash needs Git for Windows. Without it PowerShell is the only shell, and with
/// it the CLI offers both.
#[cfg(windows)]
const TOOLS: &str = "Bash,PowerShell,Read,Edit,Write,Glob,Grep";

fn permission_mode(mode: ApprovalMode) -> PermissionMode {
    match mode {
        ApprovalMode::AskForEverything => PermissionMode::Default,
        ApprovalMode::AskBeforeCommands => PermissionMode::AcceptEdits,
        ApprovalMode::NeverAsk => PermissionMode::BypassPermissions,
    }
}

/// The flags, every one explicit: the docs say the defaults of `-p` will change.
fn arguments(options: &ThreadOptions, session_id: &str) -> Vec<OsString> {
    let session_flag = match options.resume {
        Some(_) => "--resume",
        None => "--session-id",
    };
    let mut arguments: Vec<OsString> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        permission_mode(options.approval_mode).as_str(),
        // Lets `set_permission_mode` switch a running process to "never ask". Without it the
        // CLI refuses that switch. The mode itself is still the one above.
        "--allow-dangerously-skip-permissions",
        session_flag,
        session_id,
        // Trimmed to what a composer needs. The composer's own MCP servers, skills, hooks and
        // plugins do not load, so every composer gets the same agent.
        "--tools",
        TOOLS,
        "--strict-mcp-config",
        // No settings file at all, not even the project's: the agent writes in the project,
        // and a permission rule or a hook there would give it more than the approval mode.
        // Without the project source the project's CLAUDE.md would not load either, so the
        // folder comes back as an added folder, whose CLAUDE.md loads (see `Events::start`).
        "--setting-sources",
        "",
        "--disable-slash-commands",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    arguments.extend(["--add-dir".into(), options.folder.clone().into()]);
    if let Some(model) = &options.model {
        arguments.extend(["--model".into(), model.into()]);
    }
    if let Some(instructions) = &options.instructions {
        arguments.extend(["--append-system-prompt-file".into(), instructions.into()]);
    }
    arguments
}

/// `claude` in the environment without what would confuse it. Every run of the CLI starts
/// here: a thread, and `auth`.
fn command(program: &Path, environment: &HashMap<OsString, OsString>) -> std::process::Command {
    let environment = environment.iter().filter(|(key, _)| {
        let key = key.to_string_lossy();
        // A nested session inherits the outer one's markers and then never saves its
        // transcript, which breaks every later resume.
        let nested = key == "CLAUDECODE" || key.starts_with("CLAUDE_CODE_");
        // Set when the app was started from Electron; the CLI would run as plain Node.
        !nested && key != "ELECTRON_RUN_AS_NODE"
    });
    let mut command = sound_core::process::background_command(program);
    command
        .env_clear()
        .envs(environment)
        // A self-update would replace the pinned version the protocol was tested with.
        .env("DISABLE_AUTOUPDATER", "1");
    command
}

/// Starts `claude` for one thread. `session_id` is the one `options.resume` names, or the new
/// one to give the session.
pub(super) fn start(
    options: ThreadOptions,
    session_id: &str,
    commands: Receiver<Command>,
) -> io::Result<Events> {
    let mut command = command(&options.installed.program, &options.installed.environment);
    command
        .args(arguments(&options, session_id))
        .current_dir(&options.folder)
        // Loads the CLAUDE.md of the `--add-dir` folder, the project's.
        .env("CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD", "1")
        // No connection to an editor the composer has open, and no extension installed
        // into it.
        .env("CLAUDE_CODE_AUTO_CONNECT_IDE", "0")
        .env("CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL", "1");
    let mut lines = Lines {
        mapper: Mapper::new(options.folder),
        requests_sent: 0,
    };
    let initialize = lines.request(Request::Initialize);
    Events::start(command, lines, [initialize], commands)
}

pub(super) type Events = jsonl::Events<Lines>;

/// Claude's side of [`jsonl::Protocol`]: the mapper, and the ids of our requests.
#[derive(Debug)]
pub(super) struct Lines {
    mapper: Mapper,
    requests_sent: u64,
}

impl Lines {
    fn request(&mut self, request: Request) -> Outgoing {
        self.requests_sent += 1;
        Outgoing::ControlRequest {
            request_id: format!("request-{}", self.requests_sent),
            request,
        }
    }
}

/// Refuses a request of the CLI, so it does not wait for an answer forever.
fn refuse(request_id: String) -> Outgoing {
    Outgoing::ControlResponse {
        response: ControlResponse::Error {
            request_id,
            error: "Not supported by Sound Tools".to_string(),
        },
    }
}

impl jsonl::Protocol for Lines {
    type Outgoing = Outgoing;

    fn command(&mut self, command: Command) -> Vec<Outgoing> {
        let message = match command {
            Command::Send(message) => Some(Outgoing::user(message)),
            Command::Interrupt => Some(self.request(Request::Interrupt)),
            // An approval the turn already ended has nobody waiting for the answer.
            Command::Answer(approval, answer) => self.mapper.answer(&approval, answer),
            Command::SetApprovalMode(mode) => Some(self.request(Request::SetPermissionMode {
                mode: permission_mode(mode),
            })),
            Command::SetModel(model) => Some(self.request(Request::SetModel { model })),
        };
        message.into_iter().collect()
    }

    fn sent(&mut self, message: &Outgoing) -> Vec<AgentEvent> {
        self.mapper.sent(message)
    }

    fn received(&mut self, line: &str) -> (Vec<Outgoing>, Vec<AgentEvent>) {
        let message = match serde_json::from_str::<Incoming>(line) {
            Ok(message) => message,
            Err(error) => {
                // What can still be read keeps the turn going: a broken `result` still ends
                // it, and a broken request still gets an answer.
                let value = serde_json::from_str::<Value>(line).unwrap_or_default();
                let kind = value.get("type").and_then(Value::as_str);
                let mut answers = Vec::new();
                if kind == Some("control_request")
                    && let Some(request_id) = value.get("request_id")
                {
                    // Echoed as it came, also when it is not a string.
                    let request_id = match request_id {
                        Value::String(request_id) => request_id.clone(),
                        other => other.to_string(),
                    };
                    answers.push(refuse(request_id));
                }
                let ends_turn = kind == Some("result");
                let events = self.mapper.unreadable(ends_turn, &error.to_string());
                return (answers, events);
            }
        };
        let mut answers = Vec::new();
        if let Incoming::ControlRequest {
            request_id,
            request: CliRequest::Other,
        } = &message
        {
            // Nothing we start asks anything else.
            answers.push(refuse(request_id.clone()));
        }
        (answers, self.mapper.received(message))
    }

    fn unwritable(&self, error: serde_json::Error) -> AgentEvent {
        AgentEvent::Error {
            message: format!("Could not write to Claude Code: {error}"),
        }
    }

    fn exited(&mut self, code: Option<i32>, said: Option<&str>) -> Vec<AgentEvent> {
        self.mapper.exited(code, said)
    }
}
