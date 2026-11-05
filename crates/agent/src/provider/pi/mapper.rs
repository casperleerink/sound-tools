//! From Pi's RPC records to [`AgentEvent`]s. No I/O: the driver hands in each line it wrote
//! and read, and the tests hand in lines the same way.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::protocol::{Ended, Incoming, Models, Outgoing, Responded, Streamed, UiRequest};
use crate::provider::action::{Action, relative, shorten};
use crate::provider::{
    AgentEvent, ApprovalAnswer, ApprovalId, ExitReason, Model, StepId, StepOutcome, TurnOutcome,
};

/// The id of our first command of each kind, which the mapper reads back from its response.
pub(super) const MESSAGE: &str = "message";
pub(super) const MODELS: &str = "models";
pub(super) const APPROVAL_MODE: &str = "approval-mode";
pub(super) const MODEL: &str = "model";
pub(super) const STOP: &str = "stop";

#[derive(Debug)]
pub(super) struct Mapper {
    /// Paths in titles are relative to it.
    folder: PathBuf,
    turn_open: bool,
    /// Streamed text of the block that is not done yet.
    text: String,
    /// Tool calls that started and did not end, in order.
    open_steps: Vec<String>,
    /// What each tool call of the turn does, by id, for the approval that asks about it.
    actions: HashMap<String, Action>,
    /// Open approvals: the id of the request, and the tool call it asks about.
    approvals: HashMap<String, String>,
    /// Tool calls the composer denied, so their error result reads as denied.
    denied: HashSet<String>,
    /// The error the turn's last answer ended with.
    error: Option<String>,
}

impl Mapper {
    pub(super) fn new(folder: PathBuf) -> Self {
        Mapper {
            folder,
            turn_open: false,
            text: String::new(),
            open_steps: Vec::new(),
            actions: HashMap::new(),
            approvals: HashMap::new(),
            denied: HashSet::new(),
            error: None,
        }
    }

    /// A line we wrote.
    pub(super) fn sent(&mut self, message: &Outgoing) -> Vec<AgentEvent> {
        match message {
            Outgoing::Prompt {
                starts_turn: true, ..
            } if !self.turn_open => {
                self.turn_open = true;
                self.error = None;
                vec![AgentEvent::TurnStarted]
            }
            _ => Vec::new(),
        }
    }

    /// A line Pi wrote, and what to answer it with.
    pub(super) fn received(&mut self, line: &str) -> (Vec<Outgoing>, Vec<AgentEvent>) {
        let incoming = match serde_json::from_str::<Incoming>(line) {
            Ok(incoming) => incoming,
            Err(error) => {
                let message = format!("Pi sent a message this app cannot read: {error}");
                return (Vec::new(), vec![AgentEvent::Error { message }]);
            }
        };
        match incoming {
            Incoming::Response {
                id,
                success: false,
                error,
                ..
            } => (Vec::new(), self.refused(id.as_deref(), error)),
            // A message Pi took for one of its commands: no run starts, so nothing else ends
            // the turn.
            Incoming::Response {
                id: Some(id),
                data:
                    Some(Responded {
                        disposition: Some(disposition),
                    }),
                ..
            } if disposition == "handled" && id.starts_with(MESSAGE) && self.turn_open => {
                self.turn_open = false;
                let mut events = self.close_blocks();
                events.push(AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                });
                (Vec::new(), events)
            }
            Incoming::MessageUpdate {
                event: Streamed::TextDelta { delta },
            } => {
                self.text.push_str(&delta);
                (Vec::new(), vec![AgentEvent::TextDelta { text: delta }])
            }
            Incoming::MessageUpdate {
                event: Streamed::TextEnd { content },
            } => {
                self.text.clear();
                let done = (!content.is_empty()).then_some(AgentEvent::TextDone { text: content });
                (Vec::new(), done.into_iter().collect())
            }
            Incoming::MessageEnd {
                message:
                    Ended {
                        role,
                        stop_reason,
                        error_message,
                    },
            } => {
                // The last answer says how the turn went: a retry that worked clears the error.
                if role == "assistant" {
                    self.error = error_message.filter(|_| stop_reason.as_deref() == Some("error"));
                }
                (Vec::new(), Vec::new())
            }
            Incoming::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => {
                let action = action(&tool_name, &args, &self.folder);
                let started = action.started(StepId(tool_call_id.clone()));
                self.open_steps.push(tool_call_id.clone());
                self.actions.insert(tool_call_id, action);
                (Vec::new(), vec![started])
            }
            Incoming::ToolExecutionEnd {
                tool_call_id,
                is_error,
            } => (Vec::new(), self.step_done(tool_call_id, is_error)),
            Incoming::AgentSettled { aborted } => (Vec::new(), self.settled(aborted)),
            Incoming::ExtensionUiRequest(request) => self.asked(request),
            Incoming::ExtensionError { error } => {
                let message = format!("Pi failed: {error}");
                (Vec::new(), vec![AgentEvent::Error { message }])
            }
            Incoming::Response { .. } | Incoming::MessageUpdate { .. } | Incoming::Unknown => {
                (Vec::new(), Vec::new())
            }
        }
    }

    /// The answer to send for an open approval, or `None` when it is void.
    pub(super) fn answer(&mut self, id: &ApprovalId, answer: ApprovalAnswer) -> Option<Outgoing> {
        let tool_call = self.approvals.remove(&id.0)?;
        let value = match answer {
            ApprovalAnswer::Allow => "allow",
            ApprovalAnswer::AllowForThread => "allow_for_thread",
            ApprovalAnswer::Deny => {
                self.denied.insert(tool_call);
                "deny"
            }
        };
        Some(Outgoing::ExtensionUiResponse {
            id: id.0.clone(),
            value: Some(value.to_string()),
            cancelled: false,
        })
    }

    /// The process ended with `code`, or `None` when a signal stopped it. `said` is the first
    /// line it wrote on stderr.
    pub(super) fn exited(&mut self, code: Option<i32>, said: Option<&str>) -> Vec<AgentEvent> {
        let mut events = self.close_blocks();
        if self.turn_open {
            self.turn_open = false;
            let message = said.unwrap_or("Pi stopped during the turn.").to_string();
            events.push(AgentEvent::TurnEnded {
                outcome: TurnOutcome::Failed { message },
            });
        }
        let reason = if code == Some(0) {
            ExitReason::Finished
        } else {
            let message = match (said, code) {
                (Some(said), _) => format!("Pi stopped: {said}"),
                (None, Some(code)) => format!("Pi stopped with exit code {code}."),
                (None, None) => "Pi stopped unexpectedly.".to_string(),
            };
            ExitReason::Failed { message }
        };
        events.push(AgentEvent::Exited { reason });
        events
    }

    /// A command of ours that Pi refused. The ids say which: see the constants above.
    fn refused(&mut self, id: Option<&str>, error: Option<String>) -> Vec<AgentEvent> {
        let error = error.unwrap_or_else(|| "it gave no reason".to_string());
        let kind = id.and_then(|id| id.rsplit_once('-')).map(|(kind, _)| kind);
        if kind == Some(MESSAGE) && self.turn_open {
            self.turn_open = false;
            let mut events = self.close_blocks();
            events.push(AgentEvent::TurnEnded {
                outcome: TurnOutcome::Failed { message: error },
            });
            return events;
        }
        let action = match kind {
            Some(MODELS) => "list its models",
            Some(APPROVAL_MODE) => "change the approval mode",
            Some(MODEL) => "change the model",
            Some(STOP) => "stop the turn",
            _ => "do what it was asked",
        };
        vec![AgentEvent::Error {
            message: format!("Pi could not {action}: {error}"),
        }]
    }

    fn step_done(&mut self, tool_call_id: String, is_error: bool) -> Vec<AgentEvent> {
        let Some(index) = self.open_steps.iter().position(|id| *id == tool_call_id) else {
            return Vec::new();
        };
        self.open_steps.remove(index);
        let outcome = if self.denied.remove(&tool_call_id) {
            StepOutcome::Denied
        } else if is_error {
            StepOutcome::Failed
        } else {
            StepOutcome::Done
        };
        vec![AgentEvent::StepDone {
            id: StepId(tool_call_id),
            outcome,
        }]
    }

    fn settled(&mut self, aborted: bool) -> Vec<AgentEvent> {
        let mut events = self.close_blocks();
        // A run with no turn of ours, such as one an extension started, ends nothing the
        // composer sees.
        if !self.turn_open {
            return events;
        }
        self.turn_open = false;
        let outcome = match self.error.take() {
            _ if aborted => TurnOutcome::Interrupted,
            Some(error) => TurnOutcome::Failed {
                message: plain(&error),
            },
            None => TurnOutcome::Completed,
        };
        events.push(AgentEvent::TurnEnded { outcome });
        events
    }

    fn asked(&mut self, request: UiRequest) -> (Vec<Outgoing>, Vec<AgentEvent>) {
        match request {
            UiRequest::Select {
                id,
                title: tool_call,
            } => match self.actions.get(&tool_call) {
                Some(action) => {
                    let (title, command) = question(action);
                    self.approvals.insert(id.clone(), tool_call);
                    let asked = AgentEvent::ApprovalRequested {
                        id: ApprovalId(id),
                        title,
                        command,
                    };
                    (Vec::new(), vec![asked])
                }
                // Nothing of ours asks about anything else.
                None => (vec![cancel(id)], Vec::new()),
            },
            // A sign-in question has nobody to answer it in a thread.
            UiRequest::Input { id, .. } => (vec![cancel(id)], Vec::new()),
            UiRequest::Notify { message, kind } => {
                if let Ok(models) = serde_json::from_str::<Models>(&message) {
                    let started = AgentEvent::Started {
                        account: None,
                        models: listed(models),
                    };
                    return (Vec::new(), vec![started]);
                }
                let error =
                    (kind.as_deref() == Some("error")).then_some(AgentEvent::Error { message });
                (Vec::new(), error.into_iter().collect())
            }
            UiRequest::Other => (Vec::new(), Vec::new()),
        }
    }

    /// Ends what the turn left open: the text it streamed is done, and steps that never got a
    /// result failed.
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
        self.actions.clear();
        self.approvals.clear();
        self.denied.clear();
        events
    }
}

/// What a tool call of Pi does. Its paths may be relative to the project already.
fn action(tool: &str, input: &Value, folder: &Path) -> Action {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let path = || text("path").map(|path| relative(path, folder));
    let action = match tool {
        "bash" => text("command").map(|command| Action::Run(command.trim().to_string(), None)),
        "read" => path().map(Action::Read),
        "ls" => Some(Action::Read(
            path().unwrap_or_else(|| "the project".to_string()),
        )),
        "edit" => path().map(Action::Edit),
        "write" => path().map(Action::Write),
        "grep" | "find" => text("pattern").map(|pattern| Action::Search(shorten(pattern))),
        _ => None,
    };
    action.unwrap_or_else(|| Action::Use(tool.to_string()))
}

/// What an approval asks: the request's title, and for a command the whole command apart, as
/// it will run, since that is what the composer allows.
fn question(action: &Action) -> (String, Option<String>) {
    match action.command() {
        Some(command) => ("Run".to_string(), Some(command)),
        None => (action.request_title(), None),
    }
}

fn cancel(id: String) -> Outgoing {
    Outgoing::ExtensionUiResponse {
        id,
        value: None,
        cancelled: true,
    }
}

/// The models as the composer picks them, the one a thread starts on first.
pub(super) fn listed(models: Models) -> Vec<Model> {
    let Models { models, current } = models;
    let mut models: Vec<Model> = models
        .into_iter()
        .map(|model| Model {
            short_name: model.name.clone(),
            id: model.id,
            name: model.name,
            description: String::new(),
            source: Some(model.source),
        })
        .collect();
    if let Some(index) =
        current.and_then(|current| models.iter().position(|model| model.id == current))
    {
        let current = models.remove(index);
        models.insert(0, current);
    }
    models
}

/// A failure in words for the composer: a busy service gets a sentence of ours.
fn plain(error: &str) -> String {
    if error.to_lowercase().contains("overloaded") {
        "The model's service is busy. Try again in a moment.".to_string()
    } else {
        error.to_string()
    }
}
