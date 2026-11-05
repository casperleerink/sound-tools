//! The process of one thread, for a program that reads one JSON message per line on stdin
//! and writes one per line on stdout. Each provider's [`Protocol`] says what the lines mean;
//! this writes and reads them, and ends the process and everything it started.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::process::Stdio;

use serde::Serialize;
use smol::channel::Receiver;
use smol::future;
use smol::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use smol::process::{Child, ChildStderr, ChildStdin, ChildStdout};

use super::{AgentEvent, Command};
use crate::process::ProcessTree;

/// A provider's lines, with no I/O.
pub(super) trait Protocol {
    type Outgoing: Serialize;

    /// What the [`super::Thread`] asks for, as lines to write.
    fn command(&mut self, command: Command) -> Vec<Self::Outgoing>;

    /// A line we wrote whole.
    fn sent(&mut self, message: &Self::Outgoing) -> Vec<AgentEvent>;

    /// A line the program wrote: the lines to answer it with, and its events.
    fn received(&mut self, line: &str) -> (Vec<Self::Outgoing>, Vec<AgentEvent>);

    /// A message of ours that does not serialize.
    fn unwritable(&self, error: serde_json::Error) -> AgentEvent;

    /// The process ended with `code`, or `None` when a signal stopped it. `said` is the first
    /// line it wrote on stderr.
    fn exited(&mut self, code: Option<i32>, said: Option<&str>) -> Vec<AgentEvent>;
}

/// The process of one thread and what it said so far. Polling it also writes what the
/// [`super::Thread`] sends, so the process needs no task of its own.
#[derive(Debug)]
pub(super) struct Events<P: Protocol> {
    child: Child,
    /// The program and every program it started, such as a `cargo build`: `kill_on_drop` ends
    /// only the program. Ended only while the program is not reaped yet.
    tree: ProcessTree,
    /// `None` once every [`super::Thread`] is gone or the pipe broke.
    stdin: Option<ChildStdin>,
    /// The line being written and how much of it is written. It stays here between calls
    /// of [`Events::next`], so a call dropped halfway loses nothing.
    pub(super) writing: Option<Writing<P::Outgoing>>,
    stdout: BufReader<ChildStdout>,
    stdout_line: Vec<u8>,
    /// `None` once it closed.
    stderr: Option<BufReader<ChildStderr>>,
    stderr_line: Vec<u8>,
    /// The first line on stderr, which names what went wrong.
    said: Option<String>,
    /// `None` once every [`super::Thread`] is gone.
    commands: Option<Receiver<Command>>,
    protocol: P,
    outbox: VecDeque<P::Outgoing>,
    events: VecDeque<AgentEvent>,
    exited: bool,
}

#[derive(Debug)]
pub(super) struct Writing<Outgoing> {
    message: Outgoing,
    line: Vec<u8>,
    pub(super) written: usize,
}

impl<P: Protocol> Events<P> {
    /// Starts `command` with piped stdio, and writes `first` before any command.
    pub(super) fn start(
        mut command: std::process::Command,
        protocol: P,
        first: impl IntoIterator<Item = P::Outgoing>,
        commands: Receiver<Command>,
    ) -> io::Result<Self> {
        let program = command.get_program().to_string_lossy().into_owned();
        // Dropping the events ends the program's own children too.
        ProcessTree::prepare(&mut command);
        let mut child = smol::process::Command::from(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let tree = ProcessTree::of(&child)?;
        let pipe = |name| io::Error::other(format!("the {name} of {program} is not a pipe"));
        let stdin = child.stdin.take().ok_or_else(|| pipe("stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| pipe("stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| pipe("stderr"))?;
        Ok(Events {
            child,
            tree,
            stdin: Some(stdin),
            writing: None,
            stdout: BufReader::new(stdout),
            stdout_line: Vec::new(),
            stderr: Some(BufReader::new(stderr)),
            stderr_line: Vec::new(),
            said: None,
            commands: Some(commands),
            protocol,
            outbox: first.into_iter().collect(),
            events: VecDeque::new(),
            exited: false,
        })
    }

    /// Cancel-safe: dropping the future before it is ready loses no event and no part of a
    /// message on its way to the program, so it can race a timer.
    pub(super) async fn next(&mut self) -> Option<AgentEvent> {
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
                Input::Command(Some(command)) => {
                    let messages = self.protocol.command(command);
                    self.outbox.extend(messages);
                }
                Input::Command(None) => {
                    // Every handle is gone. Closing stdin ends the program, and its output ends.
                    self.commands = None;
                    self.close_stdin();
                }
                Input::Written(Ok(written)) => self.written(written),
                // The program is gone. The end of its output says why.
                Input::Written(Err(_)) => self.close_stdin(),
                Input::Line(Some(line)) => {
                    let (answers, events) = self.protocol.received(&line);
                    self.outbox.extend(answers);
                    self.events.extend(events);
                }
                Input::Line(None) => self.exit().await,
                Input::Stderr(Some(line)) => self.keep_stderr(&line),
                Input::Stderr(None) => self.stderr = None,
            }
        }
    }

    /// Whichever comes first. Every source keeps its progress in `self` and makes it only
    /// when it is ready, so the ones that lose the race lose nothing. Writing races with
    /// reading because the program may wait for us to read before it reads more.
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

    fn prepare(&mut self, message: P::Outgoing) {
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
            Err(error) => self.events.push_back(self.protocol.unwritable(error)),
        }
    }

    /// A pipe has no buffer of ours, so a line written whole has reached the program.
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
            self.events.extend(self.protocol.sent(&writing.message));
        }
    }

    fn close_stdin(&mut self) {
        self.stdin = None;
        self.writing = None;
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
        // Before the wait: until the program is reaped its pid cannot name another group.
        self.tree.end();
        let code = match self.child.status().await {
            Ok(status) => status.code(),
            Err(error) => {
                self.said.get_or_insert(error.to_string());
                None
            }
        };
        // Reaped, so from here the pid may belong to someone else.
        self.exited = true;
        let events = self.protocol.exited(code, self.said.as_deref());
        self.events.extend(events);
    }
}

impl<P: Protocol> Drop for Events<P> {
    fn drop(&mut self) {
        if !self.exited {
            self.tree.end();
        }
    }
}

/// The next line, or `None` at the end. The bytes read so far stay in `buffer`, so a call
/// dropped halfway loses nothing. Not UTF-8 is replaced, never an error. Lines end at LF only:
/// a JSON string may hold other line separators.
pub(super) async fn read_line(
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
