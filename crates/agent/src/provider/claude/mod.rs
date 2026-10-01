//! Claude Code: the `claude` CLI in stream-json mode, one process per thread.
//!
//! Every Claude flag and type is in this module. The process reads one JSON message per line
//! on stdin and writes one per line on stdout. Approvals come as `can_use_tool` requests that
//! we answer (`--permission-prompt-tool stdio`). It exits when stdin closes.

mod mapper;
mod protocol;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::process::Stdio;

use smol::channel::Receiver;
use smol::future;
use smol::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use smol::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use smol::stream::StreamExt;

use self::mapper::Mapper;
use self::protocol::{CliRequest, ControlResponse, Incoming, Outgoing, PermissionMode, Request};
use super::{AgentEvent, ApprovalMode, Command, Session, ThreadOptions};

/// The tools a composer needs. No web, no subagents, no questions: the agent asks in plain
/// text.
const TOOLS: &str = "Bash,Read,Edit,Write,Glob,Grep";

/// The lines of stderr kept for the message of a failure. Its first line names the problem.
const STDERR_LINES: usize = 20;

fn permission_mode(mode: ApprovalMode) -> PermissionMode {
    match mode {
        ApprovalMode::AskForEverything => PermissionMode::Default,
        ApprovalMode::AskBeforeCommands => PermissionMode::AcceptEdits,
        ApprovalMode::NeverAsk => PermissionMode::BypassPermissions,
    }
}

/// The flags, every one explicit: the docs say the defaults of `-p` will change.
fn arguments(options: &ThreadOptions, session_id: &str) -> Vec<OsString> {
    let session_flag = match options.session {
        Session::New => "--session-id",
        Session::Resume(_) => "--resume",
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
        // plugins do not load, so every composer gets the same agent. The project's
        // CLAUDE.md still loads.
        "--tools",
        TOOLS,
        "--strict-mcp-config",
        "--setting-sources",
        "project,local",
        "--disable-slash-commands",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if let Some(model) = &options.model {
        arguments.extend(["--model".into(), model.into()]);
    }
    arguments
}

/// The environment without what would confuse the CLI.
fn environment(options: &ThreadOptions) -> impl Iterator<Item = (&OsString, &OsString)> {
    options.environment.iter().filter(|(key, _)| {
        let key = key.to_string_lossy();
        // A nested session inherits the outer one's markers and then never saves its
        // transcript, which breaks every later resume.
        let nested = key == "CLAUDECODE" || key.starts_with("CLAUDE_CODE_");
        // Set when the app was started from Electron; the CLI would run as plain Node.
        !nested && key != "ELECTRON_RUN_AS_NODE"
    })
}

/// The process of one thread and what it said so far. Polling it also writes what the
/// [`super::Thread`] sends, so the process needs no task of its own.
#[derive(Debug)]
pub struct Events {
    child: Child,
    /// `None` once every [`super::Thread`] is gone or the pipe broke.
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    /// `None` once it closed.
    stderr: Option<Lines<BufReader<ChildStderr>>>,
    stderr_lines: Vec<String>,
    /// `None` once every [`super::Thread`] is gone.
    commands: Option<Receiver<Command>>,
    mapper: Mapper,
    outbox: VecDeque<Outgoing>,
    events: VecDeque<AgentEvent>,
    requests_sent: u64,
    exited: bool,
}

impl Events {
    pub fn start(options: ThreadOptions, commands: Receiver<Command>) -> io::Result<Events> {
        let session_id = match &options.session {
            Session::New => uuid::Uuid::new_v4().to_string(),
            Session::Resume(session_id) => session_id.clone(),
        };
        let mut command = std::process::Command::new(&options.program);
        command
            .args(arguments(&options, &session_id))
            .current_dir(&options.folder)
            .env_clear()
            .envs(environment(&options))
            // A self-update would replace the pinned version the protocol was tested with.
            .env("DISABLE_AUTOUPDATER", "1");
        // Its own process group, so a ctrl-c in the terminal that started the app does not
        // stop the agent halfway through a write. Closing stdin ends it.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = smol::process::Command::from(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let pipe = |name| io::Error::other(format!("the {name} of claude is not a pipe"));
        let stdin = child.stdin.take().ok_or_else(|| pipe("stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| pipe("stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| pipe("stderr"))?;
        let mut events = Events {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout).lines(),
            stderr: Some(BufReader::new(stderr).lines()),
            stderr_lines: Vec::new(),
            commands: Some(commands),
            mapper: Mapper::new(session_id, options.folder),
            outbox: VecDeque::new(),
            events: VecDeque::new(),
            requests_sent: 0,
            exited: false,
        };
        events.request(Request::Initialize);
        Ok(events)
    }

    pub async fn next(&mut self) -> Option<AgentEvent> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Some(event);
            }
            if self.exited {
                return None;
            }
            if let Some(message) = self.outbox.pop_front() {
                self.write(message).await;
                continue;
            }
            match self.input().await {
                Input::Command(Some(command)) => self.command(command),
                Input::Command(None) => {
                    // Every handle is gone. Closing stdin ends the CLI, and its output ends.
                    self.commands = None;
                    self.stdin = None;
                }
                Input::Line(Some(line)) => self.line(&line),
                Input::Line(None) => self.exit().await,
                Input::Stderr(Some(line)) => self.keep_stderr(line),
                Input::Stderr(None) => self.stderr = None,
            }
        }
    }

    /// Whichever comes first. Each source keeps its state between calls, so the ones that
    /// lose the race lose nothing.
    async fn input(&mut self) -> Input {
        let Events {
            commands,
            stdout,
            stderr,
            ..
        } = self;
        let command = async {
            match commands {
                Some(commands) => Input::Command(commands.recv().await.ok()),
                None => future::pending().await,
            }
        };
        // A pipe that fails to read is as good as closed: the process is going.
        let line = async { Input::Line(stdout.next().await.and_then(Result::ok)) };
        let error = async {
            match stderr {
                Some(stderr) => Input::Stderr(stderr.next().await.and_then(Result::ok)),
                None => future::pending().await,
            }
        };
        future::or(command, future::or(line, error)).await
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Send(text) => self.outbox.push_back(Outgoing::user(text)),
            Command::Interrupt => self.request(Request::Interrupt),
            Command::Answer(approval, answer) => {
                // An approval the turn already ended has nobody waiting for the answer.
                if let Some(message) = self.mapper.answer(&approval, answer) {
                    self.outbox.push_back(message);
                }
            }
            Command::SetApprovalMode(mode) => self.request(Request::SetPermissionMode {
                mode: permission_mode(mode),
            }),
        }
    }

    fn request(&mut self, request: Request) {
        self.requests_sent += 1;
        self.outbox.push_back(Outgoing::ControlRequest {
            request_id: format!("request-{}", self.requests_sent),
            request,
        });
    }

    async fn write(&mut self, message: Outgoing) {
        let Some(stdin) = &mut self.stdin else {
            return;
        };
        let mut line = match serde_json::to_string(&message) {
            Ok(line) => line,
            Err(error) => {
                self.events.push_back(AgentEvent::Error {
                    message: format!("Could not write to Claude Code: {error}"),
                });
                return;
            }
        };
        line.push('\n');
        let written = match stdin.write_all(line.as_bytes()).await {
            Ok(()) => stdin.flush().await,
            Err(error) => Err(error),
        };
        match written {
            Ok(()) => self.events.extend(self.mapper.sent(&message)),
            // The CLI is gone. The end of its output says why.
            Err(_) => self.stdin = None,
        }
    }

    fn line(&mut self, line: &str) {
        let message = match serde_json::from_str::<Incoming>(line) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("agent: skipped a line from Claude Code: {error}");
                return;
            }
        };
        if let Incoming::ControlRequest {
            request_id,
            request: CliRequest::Other,
        } = &message
        {
            // Nothing we start asks anything else. An answer keeps the CLI from waiting for
            // one forever.
            self.outbox.push_back(Outgoing::ControlResponse {
                response: ControlResponse::Error {
                    request_id: request_id.clone(),
                    error: "Not supported by Sound Tools".to_string(),
                },
            });
        }
        self.events.extend(self.mapper.received(message));
    }

    fn keep_stderr(&mut self, line: String) {
        if self.stderr_lines.len() < STDERR_LINES {
            self.stderr_lines.push(line);
        }
    }

    async fn exit(&mut self) {
        // The rest of stderr, which says why it stopped.
        if let Some(mut stderr) = self.stderr.take() {
            while let Some(Ok(line)) = stderr.next().await {
                self.keep_stderr(line);
            }
        }
        self.stdin = None;
        let code = match self.child.status().await {
            Ok(status) => status.code(),
            Err(error) => {
                self.stderr_lines.insert(0, error.to_string());
                None
            }
        };
        let stderr = self.stderr_lines.join("\n");
        self.events.extend(self.mapper.exited(code, &stderr));
        self.exited = true;
    }
}

enum Input {
    Command(Option<Command>),
    Line(Option<String>),
    Stderr(Option<String>),
}
