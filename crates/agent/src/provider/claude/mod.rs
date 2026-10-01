//! Claude Code: the `claude` CLI in stream-json mode, one process per thread.
//!
//! Every Claude flag and type is in this module. The process reads one JSON message per line
//! on stdin and writes one per line on stdout. Approvals come as `can_use_tool` requests that
//! we answer (`--permission-prompt-tool stdio`). It exits when stdin closes. The pinned
//! download and the sign-in are in [`setup`].

mod mapper;
#[cfg(test)]
mod process_tests;
mod protocol;
mod setup;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;

use serde_json::Value;
use smol::channel::Receiver;
use smol::future;
use smol::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use smol::process::{Child, ChildStderr, ChildStdin, ChildStdout};

use self::mapper::Mapper;
use self::protocol::{CliRequest, ControlResponse, Incoming, Outgoing, PermissionMode, Request};
pub use self::setup::{SIGN_IN_CHOICES, account, download, sign_in, sign_out};
use super::{AgentEvent, ApprovalMode, Command, Session, ThreadOptions};

/// The tools a composer needs. No web, no subagents, no questions: the agent asks in plain
/// text.
const TOOLS: &str = "Bash,Read,Edit,Write,Glob,Grep";

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
    let mut command = std::process::Command::new(program);
    command
        .env_clear()
        .envs(environment)
        // A self-update would replace the pinned version the protocol was tested with.
        .env("DISABLE_AUTOUPDATER", "1");
    command
}

/// The process of one thread and what it said so far. Polling it also writes what the
/// [`super::Thread`] sends, so the process needs no task of its own.
#[derive(Debug)]
pub struct Events {
    child: Child,
    /// `None` once every [`super::Thread`] is gone or the pipe broke.
    stdin: Option<ChildStdin>,
    /// The line being written and how much of it is written. It stays here between calls
    /// of [`Events::next`], so a call dropped halfway loses nothing.
    writing: Option<Writing>,
    stdout: BufReader<ChildStdout>,
    stdout_line: Vec<u8>,
    /// `None` once it closed.
    stderr: Option<BufReader<ChildStderr>>,
    stderr_line: Vec<u8>,
    /// The first line on stderr, which names what went wrong.
    said: Option<String>,
    /// `None` once every [`super::Thread`] is gone.
    commands: Option<Receiver<Command>>,
    mapper: Mapper,
    outbox: VecDeque<Outgoing>,
    events: VecDeque<AgentEvent>,
    requests_sent: u64,
    exited: bool,
}

#[derive(Debug)]
struct Writing {
    message: Outgoing,
    line: Vec<u8>,
    written: usize,
}

impl Events {
    /// `session_id` is the one `options.session` names, or the new one to give the session.
    pub fn start(
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
        // Its own process group: a ctrl-c in the terminal that started the app does not stop
        // the agent halfway through a write, and dropping the events ends the agent's own
        // children too.
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
            writing: None,
            stdout: BufReader::new(stdout),
            stdout_line: Vec::new(),
            stderr: Some(BufReader::new(stderr)),
            stderr_line: Vec::new(),
            said: None,
            commands: Some(commands),
            mapper: Mapper::new(options.folder),
            outbox: VecDeque::new(),
            events: VecDeque::new(),
            requests_sent: 0,
            exited: false,
        };
        events.request(Request::Initialize);
        Ok(events)
    }

    /// Cancel-safe: dropping the future before it is ready loses no event and no part of a
    /// message on its way to the CLI, so it can race a timer.
    pub async fn next(&mut self) -> Option<AgentEvent> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Some(event);
            }
            if self.exited {
                return None;
            }
            if self.writing.is_none()
                && let Some(message) = self.outbox.pop_front()
            {
                self.prepare(message);
                continue;
            }
            match self.input().await {
                Input::Command(Some(command)) => self.command(command),
                Input::Command(None) => {
                    // Every handle is gone. Closing stdin ends the CLI, and its output ends.
                    self.commands = None;
                    self.close_stdin();
                }
                Input::Written(Ok(written)) => self.written(written),
                // The CLI is gone. The end of its output says why.
                Input::Written(Err(_)) => self.close_stdin(),
                Input::Line(Some(line)) => self.line(&line),
                Input::Line(None) => self.exit().await,
                Input::Stderr(Some(line)) => self.keep_stderr(&line),
                Input::Stderr(None) => self.stderr = None,
            }
        }
    }

    /// Whichever comes first. Every source keeps its progress in `self` and makes it only
    /// when it is ready, so the ones that lose the race lose nothing. Writing races with
    /// reading because the CLI may wait for us to read before it reads more.
    async fn input(&mut self) -> Input {
        let Events {
            commands,
            stdin,
            writing,
            stdout,
            stdout_line,
            stderr,
            stderr_line,
            ..
        } = self;
        let command = async {
            match commands {
                Some(commands) => Input::Command(commands.recv().await.ok()),
                None => future::pending().await,
            }
        };
        let write = async {
            match (stdin, writing) {
                (Some(stdin), Some(writing)) => {
                    let rest = writing.line.get(writing.written..).unwrap_or_default();
                    let result =
                        future::poll_fn(|context| Pin::new(&mut *stdin).poll_write(context, rest))
                            .await;
                    Input::Written(result)
                }
                _ => future::pending().await,
            }
        };
        let line = async { Input::Line(read_line(stdout, stdout_line).await) };
        let error = async {
            match stderr {
                Some(stderr) => Input::Stderr(read_line(stderr, stderr_line).await),
                None => future::pending().await,
            }
        };
        future::or(command, future::or(write, future::or(line, error))).await
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
            Command::SetModel(model) => self.request(Request::SetModel { model }),
        }
    }

    fn request(&mut self, request: Request) {
        self.requests_sent += 1;
        self.outbox.push_back(Outgoing::ControlRequest {
            request_id: format!("request-{}", self.requests_sent),
            request,
        });
    }

    /// Refuses a request of the CLI, so it does not wait for an answer forever.
    fn refuse(&mut self, request_id: String) {
        self.outbox.push_back(Outgoing::ControlResponse {
            response: ControlResponse::Error {
                request_id,
                error: "Not supported by Sound Tools".to_string(),
            },
        });
    }

    fn prepare(&mut self, message: Outgoing) {
        if self.stdin.is_none() {
            return;
        }
        match serde_json::to_vec(&message) {
            Ok(mut line) => {
                line.push(b'\n');
                self.writing = Some(Writing {
                    message,
                    line,
                    written: 0,
                });
            }
            Err(error) => self.events.push_back(AgentEvent::Error {
                message: format!("Could not write to Claude Code: {error}"),
            }),
        }
    }

    /// A pipe has no buffer of ours, so a line written whole has reached the CLI.
    fn written(&mut self, written: usize) {
        let Some(writing) = &mut self.writing else {
            return;
        };
        if written == 0 {
            self.close_stdin();
            return;
        }
        writing.written += written;
        if writing.written >= writing.line.len()
            && let Some(writing) = self.writing.take()
        {
            self.events.extend(self.mapper.sent(&writing.message));
        }
    }

    fn close_stdin(&mut self) {
        self.stdin = None;
        self.writing = None;
    }

    fn line(&mut self, line: &str) {
        let message = match serde_json::from_str::<Incoming>(line) {
            Ok(message) => message,
            Err(error) => {
                // What can still be read keeps the turn going: a broken `result` still ends
                // it, and a broken request still gets an answer.
                let value = serde_json::from_str::<Value>(line).unwrap_or_default();
                let kind = value.get("type").and_then(Value::as_str);
                if kind == Some("control_request")
                    && let Some(request_id) = value.get("request_id")
                {
                    // Echoed as it came, also when it is not a string.
                    let request_id = match request_id {
                        Value::String(request_id) => request_id.clone(),
                        other => other.to_string(),
                    };
                    self.refuse(request_id);
                }
                let ends_turn = kind == Some("result");
                let events = self.mapper.unreadable(ends_turn, &error.to_string());
                self.events.extend(events);
                return;
            }
        };
        if let Incoming::ControlRequest {
            request_id,
            request: CliRequest::Other,
        } = &message
        {
            // Nothing we start asks anything else.
            self.refuse(request_id.clone());
        }
        self.events.extend(self.mapper.received(message));
    }

    fn keep_stderr(&mut self, line: &str) {
        let line = line.trim();
        if self.said.is_none() && !line.is_empty() {
            self.said = Some(line.to_string());
        }
    }

    async fn exit(&mut self) {
        // The rest of stderr, which may say why it stopped.
        while let Some(stderr) = &mut self.stderr {
            match read_line(stderr, &mut self.stderr_line).await {
                Some(line) => self.keep_stderr(&line),
                None => self.stderr = None,
            }
        }
        self.close_stdin();
        // Before the wait: until the CLI is reaped its pid cannot name another group.
        self.end_group();
        let code = match self.child.status().await {
            Ok(status) => status.code(),
            Err(error) => {
                self.said.get_or_insert(error.to_string());
                None
            }
        };
        // Reaped, so from here the pid may belong to someone else.
        self.exited = true;
        let events = self.mapper.exited(code, self.said.as_deref());
        self.events.extend(events);
    }

    /// Ends the agent's own children, such as a `cargo build` it started. `kill_on_drop`
    /// ends only the CLI. Only while the CLI is not reaped yet.
    fn end_group(&self) {
        #[cfg(unix)]
        if let Ok(group) = libc::pid_t::try_from(self.child.id()) {
            // SAFETY: `killpg` only sends a signal; it touches no memory of ours. The group
            // is the CLI's own (`process_group(0)` at the start), and the CLI is not reaped,
            // so the id is still its. It fails only when the group is gone already, which
            // is what this wants.
            unsafe { libc::killpg(group, libc::SIGTERM) };
        }
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        if !self.exited {
            self.end_group();
        }
    }
}

/// The next line, or `None` at the end. The bytes read so far stay in `buffer`, so a call
/// dropped halfway loses nothing. Not UTF-8 is replaced, never an error.
async fn read_line(
    reader: &mut BufReader<impl AsyncRead + Unpin>,
    buffer: &mut Vec<u8>,
) -> Option<String> {
    // A pipe that fails to read is as good as closed: the process is going.
    let more = matches!(reader.read_until(b'\n', buffer).await, Ok(read) if read > 0);
    if !more && buffer.is_empty() {
        return None;
    }
    let line = String::from_utf8_lossy(buffer)
        .trim_end_matches(['\n', '\r'])
        .to_string();
    buffer.clear();
    Some(line)
}

enum Input {
    Command(Option<Command>),
    Written(io::Result<usize>),
    Line(Option<String>),
    Stderr(Option<String>),
}
