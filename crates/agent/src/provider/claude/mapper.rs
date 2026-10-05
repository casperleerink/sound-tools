//! From Claude's messages to [`AgentEvent`]s. No I/O: the driver hands in each line it wrote
//! and read, and the tests hand in recorded runs the same way.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::protocol::{
    Block, CliRequest, Content, ControlResponse, Delta, Incoming, Initialized, InitializedModel,
    Outgoing, PermissionResult, Request, ResultKind, StreamEvent, System, TerminalReason,
    TurnResult,
};
use crate::provider::{
    Account, AgentEvent, ApprovalAnswer, ApprovalId, ExitReason, Model, StepId, StepOutcome,
    TurnOutcome,
};

/// What the CLI says when `--resume` names a session it does not have.
const SESSION_NOT_FOUND: &str = "no conversation found with session id";

/// The model the CLI picks itself, in its list of models.
const DEFAULT_MODEL: &str = "default";

/// The longest command or pattern a title shows, in characters.
const TITLE_DETAIL_CHARACTERS: usize = 60;

#[derive(Debug)]
pub(super) struct Mapper {
    /// Paths in titles are relative to it.
    folder: PathBuf,
    turn_open: bool,
    /// An acknowledged interrupt: however the turn then ends, it was stopped.
    interrupted: bool,
    session_not_found: bool,
    /// Streamed text of the block that is not done yet.
    text: String,
    open_steps: Vec<String>,
    /// Open approvals by request id.
    approvals: HashMap<String, Approval>,
    /// Tool uses the composer denied, so their error result reads as denied.
    denied: HashSet<String>,
    /// Our requests that wait for an answer, by request id.
    requests: HashMap<String, Request>,
}

#[derive(Debug)]
struct Approval {
    tool_name: String,
    input: Value,
    suggestions: Vec<Value>,
    tool_use_id: Option<String>,
}

impl Mapper {
    pub(super) fn new(folder: PathBuf) -> Self {
        Mapper {
            folder,
            turn_open: false,
            interrupted: false,
            session_not_found: false,
            text: String::new(),
            open_steps: Vec::new(),
            approvals: HashMap::new(),
            denied: HashSet::new(),
            requests: HashMap::new(),
        }
    }

    /// A line we wrote.
    pub(super) fn sent(&mut self, message: &Outgoing) -> Vec<AgentEvent> {
        match message {
            // The CLI announces no turn of its own. It queues a message sent while a turn runs
            // as a turn of its own, which this does not follow: send only between turns.
            Outgoing::User { .. } if self.turn_open => Vec::new(),
            Outgoing::User { .. } => {
                self.turn_open = true;
                self.interrupted = false;
                vec![AgentEvent::TurnStarted]
            }
            Outgoing::ControlRequest {
                request_id,
                request,
            } => {
                self.requests.insert(request_id.clone(), request.clone());
                Vec::new()
            }
            Outgoing::ControlResponse { response } => {
                let ControlResponse::Success {
                    request_id,
                    response,
                } = response
                else {
                    return Vec::new();
                };
                let approval = self.approvals.remove(request_id);
                if let Some(PermissionResult::Deny { .. }) = response
                    && let Some(tool_use_id) = approval.and_then(|approval| approval.tool_use_id)
                {
                    self.denied.insert(tool_use_id);
                }
                Vec::new()
            }
        }
    }

    /// A line the CLI wrote.
    pub(super) fn received(&mut self, message: Incoming) -> Vec<AgentEvent> {
        match message {
            Incoming::System(System::Init { cwd }) => {
                // The folder as the CLI sees it, with symlinks resolved, as in its paths.
                self.folder = cwd;
                Vec::new()
            }
            Incoming::StreamEvent {
                event:
                    StreamEvent::ContentBlockDelta {
                        delta: Delta::TextDelta { text },
                    },
            } => {
                self.text.push_str(&text);
                vec![AgentEvent::TextDelta { text }]
            }
            Incoming::Assistant { message } => blocks(message.content)
                .into_iter()
                .filter_map(|block| self.assistant_block(block))
                .collect(),
            Incoming::User { message } => blocks(message.content)
                .into_iter()
                .filter_map(|block| self.tool_result(block))
                .collect(),
            Incoming::Result(result) => self.result(&result),
            Incoming::ControlRequest {
                request_id,
                request:
                    CliRequest::CanUseTool {
                        tool_name,
                        input,
                        permission_suggestions,
                        tool_use_id,
                    },
            } => {
                let title = Action::of(&tool_name, &input, &self.folder).request_title();
                self.approvals.insert(
                    request_id.clone(),
                    Approval {
                        tool_name,
                        input,
                        suggestions: permission_suggestions,
                        tool_use_id,
                    },
                );
                vec![AgentEvent::ApprovalRequested {
                    id: ApprovalId(request_id),
                    title,
                }]
            }
            Incoming::ControlResponse { response } => self.response(response),
            Incoming::System(System::Other)
            | Incoming::StreamEvent { .. }
            | Incoming::ControlRequest { .. }
            | Incoming::Unknown => Vec::new(),
        }
    }

    /// The process ended: it closed its output and exited with `code`, or `None` when a
    /// signal stopped it. `said` is the first line it wrote on stderr.
    pub(super) fn exited(&mut self, code: Option<i32>, said: Option<&str>) -> Vec<AgentEvent> {
        let mut events = self.close_blocks();
        if self.turn_open {
            self.turn_open = false;
            let message = said.unwrap_or("Claude Code stopped during the turn.");
            events.push(AgentEvent::TurnEnded {
                outcome: TurnOutcome::Failed {
                    message: message.to_string(),
                },
            });
        }
        let reason = if self.session_not_found {
            ExitReason::SessionNotFound
        } else if code == Some(0) {
            ExitReason::Finished
        } else {
            let message = match (said, code) {
                (Some(said), _) => format!("Claude Code stopped: {said}"),
                (None, Some(code)) => format!("Claude Code stopped with exit code {code}."),
                (None, None) => "Claude Code stopped unexpectedly.".to_string(),
            };
            ExitReason::Failed { message }
        };
        events.push(AgentEvent::Exited { reason });
        events
    }

    /// A line the types cannot read. `ends_turn` when it is a `result`: the turn will get no
    /// other end.
    pub(super) fn unreadable(&mut self, ends_turn: bool, error: &str) -> Vec<AgentEvent> {
        let mut events = vec![AgentEvent::Error {
            message: format!("Claude Code sent a message this app cannot read: {error}"),
        }];
        if ends_turn && self.turn_open {
            self.turn_open = false;
            events.extend(self.close_blocks());
            events.push(AgentEvent::TurnEnded {
                outcome: TurnOutcome::Failed {
                    message: "Claude Code ended the turn with a message this app cannot read."
                        .to_string(),
                },
            });
        }
        events
    }

    /// The answer to send for an open approval, or `None` when it is void.
    pub(super) fn answer(&self, id: &ApprovalId, answer: ApprovalAnswer) -> Option<Outgoing> {
        let approval = self.approvals.get(&id.0)?;
        let result = match answer {
            ApprovalAnswer::Allow => PermissionResult::Allow {
                updated_input: approval.input.clone(),
                updated_permissions: None,
            },
            ApprovalAnswer::AllowForThread => PermissionResult::Allow {
                updated_input: approval.input.clone(),
                updated_permissions: Some(approval.thread_permissions()),
            },
            ApprovalAnswer::Deny => PermissionResult::Deny {
                message: "The composer denied this.".to_string(),
            },
        };
        Some(Outgoing::ControlResponse {
            response: ControlResponse::Success {
                request_id: id.0.clone(),
                response: Some(result),
            },
        })
    }

    fn assistant_block(&mut self, block: Block) -> Option<AgentEvent> {
        match block {
            Block::Text { text } => {
                // The whole block, also when it never streamed, as for an error the CLI
                // writes itself.
                self.text.clear();
                (!text.is_empty()).then_some(AgentEvent::TextDone { text })
            }
            Block::ToolUse { id, name, input } => {
                let action = Action::of(&name, &input, &self.folder);
                self.open_steps.push(id.clone());
                Some(AgentEvent::StepStarted {
                    id: StepId(id),
                    title: action.done_title(),
                    running_title: action.running_title(),
                    request_title: action.request_title(),
                })
            }
            Block::ToolResult { .. } | Block::Other => None,
        }
    }

    fn tool_result(&mut self, block: Block) -> Option<AgentEvent> {
        let Block::ToolResult {
            tool_use_id,
            is_error,
        } = block
        else {
            return None;
        };
        let index = self.open_steps.iter().position(|id| *id == tool_use_id)?;
        self.open_steps.remove(index);
        let outcome = if self.denied.remove(&tool_use_id) {
            StepOutcome::Denied
        } else if is_error {
            StepOutcome::Failed
        } else {
            StepOutcome::Done
        };
        Some(AgentEvent::StepDone {
            id: StepId(tool_use_id),
            outcome,
        })
    }

    fn result(&mut self, result: &TurnResult) -> Vec<AgentEvent> {
        if result
            .errors
            .iter()
            .any(|error| error.to_lowercase().contains(SESSION_NOT_FOUND))
        {
            self.session_not_found = true;
        }
        let mut events = self.close_blocks();
        // A result with no turn of ours, such as the second of two queued messages, ends
        // nothing the composer sees.
        if !self.turn_open {
            return events;
        }
        self.turn_open = false;
        let aborted = matches!(
            result.terminal_reason,
            Some(TerminalReason::AbortedStreaming | TerminalReason::AbortedTools)
        );
        let outcome = if self.interrupted || aborted {
            TurnOutcome::Interrupted
        } else if result.is_error || result.subtype != ResultKind::Success {
            TurnOutcome::Failed {
                message: failure(result),
            }
        } else {
            TurnOutcome::Completed
        };
        events.push(AgentEvent::TurnEnded { outcome });
        events
    }

    /// Ends what the turn left open. An interrupted turn never closes its blocks, so the text
    /// it streamed is done here, and steps that never got a result failed.
    fn close_blocks(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let text = std::mem::take(&mut self.text);
        if !text.is_empty() {
            events.push(AgentEvent::TextDone { text });
        }
        for id in self.open_steps.drain(..) {
            events.push(AgentEvent::StepDone {
                id: StepId(id),
                outcome: StepOutcome::Failed,
            });
        }
        self.approvals.clear();
        self.denied.clear();
        events
    }

    fn response(&mut self, response: ControlResponse<Value>) -> Vec<AgentEvent> {
        let (request_id, answer) = match response {
            ControlResponse::Success {
                request_id,
                response,
            } => (request_id, Ok(response)),
            ControlResponse::Error { request_id, error } => (request_id, Err(error)),
        };
        let Some(request) = self.requests.remove(&request_id) else {
            return Vec::new();
        };
        match (request, answer) {
            (Request::Initialize, Ok(response)) => vec![self.started(response)],
            (Request::Interrupt, Ok(_)) => {
                // Only an acknowledged stop marks the turn: a stop that failed must not
                // relabel a turn the CLI then finishes.
                self.interrupted = self.turn_open;
                Vec::new()
            }
            (Request::SetPermissionMode { .. } | Request::SetModel { .. }, Ok(_)) => Vec::new(),
            (request, Err(error)) => {
                let action = match request {
                    Request::Initialize => "start",
                    Request::Interrupt => "stop the turn",
                    Request::SetPermissionMode { .. } => "change the approval mode",
                    Request::SetModel { .. } => "change the model",
                };
                vec![AgentEvent::Error {
                    message: format!("Claude Code could not {action}: {error}"),
                }]
            }
        }
    }

    fn started(&self, response: Option<Value>) -> AgentEvent {
        let initialized = serde_json::from_value::<Initialized>(response.unwrap_or(Value::Null));
        match initialized {
            Ok(initialized) => AgentEvent::Started {
                account: initialized
                    .account
                    .map(|account| Account {
                        email: account.email,
                        plan: account.subscription_type,
                    })
                    .unwrap_or_default(),
                models: models(initialized.models),
            },
            Err(error) => AgentEvent::Error {
                message: format!("Claude Code started, but its account is unreadable: {error}"),
            },
        }
    }
}

impl Approval {
    /// The CLI's suggestions, kept to this session so nothing is written to the project's
    /// settings. Only the rules when there are any: allowing a command for the thread allows
    /// that command, and does not also switch the mode, which the CLI suggests as well.
    fn thread_permissions(&self) -> Vec<Value> {
        let is_rule = |suggestion: &&Value| {
            suggestion.get("type").and_then(Value::as_str) == Some("addRules")
        };
        let rules: Vec<&Value> = self.suggestions.iter().filter(is_rule).collect();
        let chosen = if rules.is_empty() {
            self.suggestions.iter().collect()
        } else {
            rules
        };
        let mut permissions: Vec<Value> = chosen
            .into_iter()
            .cloned()
            .map(|mut suggestion| {
                if let Some(fields) = suggestion.as_object_mut() {
                    fields.insert("destination".to_string(), json!("session"));
                }
                suggestion
            })
            .collect();
        if permissions.is_empty() {
            permissions.push(json!({
                "type": "addRules",
                "rules": [{ "toolName": self.tool_name }],
                "behavior": "allow",
                "destination": "session",
            }));
        }
        permissions
    }
}

/// The models as the composer picks them. The CLI's `default` runs a model that has an entry
/// of its own in the list, whose name is the short name of both.
fn models(models: Vec<InitializedModel>) -> Vec<Model> {
    let named: Vec<(String, String)> = models
        .iter()
        .filter(|model| model.value != DEFAULT_MODEL)
        .filter_map(|model| Some((model.resolved_model.clone()?, model.display_name.clone())))
        .collect();
    models
        .into_iter()
        .map(|model| {
            let short_name = named
                .iter()
                .find(|(resolved, _)| Some(resolved) == model.resolved_model.as_ref())
                .map_or_else(|| model.display_name.clone(), |(_, name)| name.clone());
            Model {
                id: model.value,
                name: model.display_name,
                description: model.description,
                short_name,
            }
        })
        .collect()
}

fn blocks(content: Content) -> Vec<Block> {
    match content {
        Content::Blocks(blocks) => blocks,
        Content::Text(_) => Vec::new(),
    }
}

/// The first error the CLI reported. Lines that start with `[ede_diagnostic]` are its own notes.
fn failure(result: &TurnResult) -> String {
    result
        .errors
        .iter()
        .find(|error| !error.starts_with("[ede_diagnostic]"))
        .or(result.result.as_ref())
        .cloned()
        .unwrap_or_else(|| "Claude Code ended the turn with an error.".to_string())
}

/// What a tool use does, for its title.
enum Action {
    Run(String),
    Read(String),
    Edit(String),
    Write(String),
    Search(String),
    Use(String),
}

impl Action {
    fn of(tool: &str, input: &Value, folder: &Path) -> Action {
        let text = |key: &str| input.get(key).and_then(Value::as_str);
        let path = || text("file_path").map(|path| relative(path, folder));
        let action = match tool {
            "Bash" | "PowerShell" => text("command").map(|command| Action::Run(shorten(command))),
            "Read" => path().map(Action::Read),
            "Edit" => path().map(Action::Edit),
            "Write" => path().map(Action::Write),
            "Glob" | "Grep" => text("pattern").map(|pattern| Action::Search(shorten(pattern))),
            _ => None,
        };
        action.unwrap_or_else(|| Action::Use(tool.to_string()))
    }

    fn done_title(&self) -> String {
        match self {
            Action::Run(command) => format!("Ran {}", code(command)),
            Action::Read(path) => format!("Read {path}"),
            Action::Edit(path) => format!("Edited {path}"),
            Action::Write(path) => format!("Wrote {path}"),
            Action::Search(pattern) => format!("Searched for {}", code(pattern)),
            Action::Use(tool) => format!("Used {tool}"),
        }
    }

    fn running_title(&self) -> String {
        match self {
            Action::Run(command) => format!("Running {}", code(command)),
            Action::Read(path) => format!("Reading {path}"),
            Action::Edit(path) => format!("Editing {path}"),
            Action::Write(path) => format!("Writing {path}"),
            Action::Search(pattern) => format!("Searching for {}", code(pattern)),
            Action::Use(tool) => format!("Using {tool}"),
        }
    }

    fn request_title(&self) -> String {
        match self {
            Action::Run(command) => format!("Run {}", code(command)),
            Action::Read(path) => format!("Read {path}"),
            Action::Edit(path) => format!("Edit {path}"),
            Action::Write(path) => format!("Write {path}"),
            Action::Search(pattern) => format!("Search for {}", code(pattern)),
            Action::Use(tool) => format!("Use {tool}"),
        }
    }
}

/// A path inside the project as the composer knows it, from the project folder.
fn relative(path: &str, folder: &Path) -> String {
    Path::new(path)
        .strip_prefix(folder)
        .map(|inside| inside.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// The first line, cut to fit one line of the sidebar.
fn shorten(text: &str) -> String {
    // Trimmed first, so a command that starts with a blank line is not shown empty.
    let mut lines = text.trim().lines();
    let line = lines.next().unwrap_or_default().trim();
    let more = lines.next().is_some();
    if line.chars().count() <= TITLE_DETAIL_CHARACTERS && !more {
        return line.to_string();
    }
    let cut: String = line.chars().take(TITLE_DETAIL_CHARACTERS).collect();
    format!("{}…", cut.trim_end())
}

/// A command or a pattern in backticks, which the sidebar shows as code. One that has a
/// backtick of its own is left bare: its backtick would close ours early, and the line would
/// show the wrong part as code.
fn code(text: &str) -> String {
    if text.contains('`') {
        text.to_string()
    } else {
        format!("`{text}`")
    }
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn a_command_that_starts_with_a_blank_line_shows_its_first_line() {
        assert_eq!(shorten("\n  cargo build\n"), "cargo build");
        assert_eq!(shorten("\ncargo build\ncargo test"), "cargo build…");
    }
}
