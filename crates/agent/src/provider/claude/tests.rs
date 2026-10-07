//! Recorded runs of the pinned `claude`, replayed through the parser and the mapper.
//!
//! The fixtures are in `tests/fixtures/claude/`, one run each, and `record.py` next to them
//! records them again. Each line is what we wrote, what the CLI wrote, or how it exited.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::mapper::Mapper;
use super::protocol::{ControlResponse, Outgoing, PermissionResult};
use super::{ThreadOptions, arguments};
use crate::provider::{AgentEvent, ApprovalAnswer, ApprovalId, ApprovalMode, Installed, Provider};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Entry {
    Sent(Value),
    /// Parsed as a whole, so a recorded line the types cannot read fails the test.
    Received(super::protocol::Incoming),
    Exited {
        code: Option<i32>,
        stderr: String,
    },
}

fn entries(name: &str) -> Vec<Entry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn mapper() -> Mapper {
    // The recorder writes the folder as /tmp/project.
    Mapper::new(PathBuf::from("/tmp/project"))
}

/// The events of a recorded run. Each run of text deltas is joined into one, so the snapshot
/// reads as the text and not one line per token, and only the first model is kept, since the
/// list changes with every release.
fn replay(name: &str) -> Vec<AgentEvent> {
    let mut mapper = mapper();
    let mut events: Vec<AgentEvent> = Vec::new();
    for entry in entries(name) {
        let new = match entry {
            Entry::Sent(message) => mapper.sent(&serde_json::from_value(message).unwrap()),
            Entry::Received(message) => mapper.received(message),
            Entry::Exited { code, stderr } => {
                let said = stderr.lines().map(str::trim).find(|line| !line.is_empty());
                mapper.exited(code, said)
            }
        };
        for mut event in new {
            if let AgentEvent::Started { models, .. } = &mut event {
                models.truncate(1);
            }
            if let (Some(AgentEvent::TextDelta { text }), AgentEvent::TextDelta { text: more }) =
                (events.last_mut(), &event)
            {
                text.push_str(more);
            } else {
                events.push(event);
            }
        }
    }
    events
}

#[test]
fn plain_answer() {
    insta::assert_debug_snapshot!(replay("plain"));
}

#[test]
fn file_edit() {
    insta::assert_debug_snapshot!(replay("edit"));
}

#[test]
fn approval_allowed() {
    insta::assert_debug_snapshot!(replay("approval_allowed"));
}

#[test]
fn approval_denied() {
    insta::assert_debug_snapshot!(replay("approval_denied"));
}

#[test]
fn interrupt() {
    insta::assert_debug_snapshot!(replay("interrupt"));
}

/// The question is void once the turn ends.
#[test]
fn interrupt_while_asking() {
    insta::assert_debug_snapshot!(replay("interrupt_approval"));
}

#[test]
fn permission_mode_change() {
    insta::assert_debug_snapshot!(replay("permission_mode"));
}

/// To "never ask" on a running process: the command runs with no question.
#[test]
fn permission_mode_to_never_ask() {
    insta::assert_debug_snapshot!(replay("permission_bypass"));
}

#[test]
fn stale_resume() {
    insta::assert_debug_snapshot!(replay("stale_resume"));
}

#[test]
fn error_turn() {
    insta::assert_debug_snapshot!(replay("error_turn"));
}

#[test]
fn process_killed() {
    insta::assert_debug_snapshot!(replay("crash"));
}

/// An unknown model is a quiet error, and the thread goes on.
#[test]
fn model_change() {
    insta::assert_debug_snapshot!(replay("set_model"));
}

/// Under "Ask before commands" a command that only reads runs with no question, also in a
/// pipe: the CLI checks that itself. A loop over `$(…)` still asks.
#[test]
fn read_only_commands() {
    let events = replay("read_only");
    let asked: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ApprovalRequested { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [r#"Run `for f in $(ls); do cat "$f"; done`"#]);
    insta::assert_debug_snapshot!(events);
}

const FIXTURES: [&str; 13] = [
    "plain",
    "edit",
    "approval_allowed",
    "approval_denied",
    "interrupt",
    "interrupt_approval",
    "permission_mode",
    "permission_bypass",
    "set_model",
    "read_only",
    "stale_resume",
    "error_turn",
    "crash",
];

/// The CLI accepted every line the recorder sent, so our types must write the same JSON.
#[test]
fn writes_what_the_cli_accepted() {
    for name in FIXTURES {
        for entry in entries(name) {
            let Entry::Sent(recorded) = entry else {
                continue;
            };
            let message: Outgoing = serde_json::from_value(recorded.clone()).unwrap();
            assert_eq!(serde_json::to_value(&message).unwrap(), recorded, "{name}");
            if let Outgoing::User { message, .. } = message {
                let super::protocol::UserBlock::Text { text } = &message.content[0];
                let ours = serde_json::to_value(Outgoing::user(text.clone())).unwrap();
                assert_eq!(ours, recorded, "{name}");
            }
        }
    }
}

/// The answers [`Mapper::answer`] gives are the ones the CLI took in the recordings.
#[test]
fn answers_approvals_as_recorded() {
    for (name, answer) in [
        ("approval_allowed", ApprovalAnswer::Allow),
        ("approval_denied", ApprovalAnswer::Deny),
    ] {
        let mut mapper = mapper();
        let mut answered = false;
        for entry in entries(name) {
            match entry {
                Entry::Sent(recorded) => {
                    let message: Outgoing = serde_json::from_value(recorded.clone()).unwrap();
                    if let Outgoing::ControlResponse {
                        response: ControlResponse::Success { request_id, .. },
                    } = &message
                    {
                        let ours = mapper.answer(&ApprovalId(request_id.clone()), answer);
                        assert_eq!(serde_json::to_value(ours).unwrap(), recorded, "{name}");
                        answered = true;
                    }
                    mapper.sent(&message);
                }
                Entry::Received(message) => {
                    mapper.received(message);
                }
                Entry::Exited { .. } => {}
            }
        }
        assert!(answered, "{name} has no approval");
    }
}

/// "Allow for this thread" on a command gives the CLI's rule for it, kept to the session,
/// and not the mode switch it also suggests.
#[test]
fn allows_a_command_for_the_thread() {
    let mut mapper = mapper();
    // Up to the question: the end of the turn voids it.
    let approval = entries("approval_allowed")
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Received(message) => Some(mapper.received(message)),
            Entry::Sent(_) | Entry::Exited { .. } => None,
        })
        .flatten()
        .find_map(|event| match event {
            AgentEvent::ApprovalRequested { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();
    let answer = mapper.answer(&approval, ApprovalAnswer::AllowForThread);
    let Some(Outgoing::ControlResponse {
        response:
            ControlResponse::Success {
                response:
                    Some(PermissionResult::Allow {
                        updated_permissions,
                        ..
                    }),
                ..
            },
    }) = answer
    else {
        panic!("not an allow: {answer:?}");
    };
    insta::assert_snapshot!(serde_json::to_string_pretty(&updated_permissions).unwrap());
}

#[test]
fn starts_claude_with_the_trimmed_flags() {
    let options = |approval_mode, resume| ThreadOptions {
        provider: Provider::Claude,
        installed: Installed {
            program: PathBuf::from("claude"),
            environment: Default::default(),
        },
        folder: PathBuf::from("/tmp/project"),
        model: Some("haiku".to_string()),
        approval_mode,
        resume,
        instructions: Some(PathBuf::from("/support/agent/instructions.md")),
    };
    let flags = |approval_mode, resume| {
        arguments(&options(approval_mode, resume), "id")
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    // Windows adds its own shell to the tools. The rest is the same everywhere.
    let first = flags(ApprovalMode::default(), None).replace(",PowerShell", "");
    insta::assert_snapshot!(first);
    let resume = flags(ApprovalMode::default(), Some("id".to_string()));
    assert!(resume.contains("--resume id"));
    for (mode, flag) in [
        (ApprovalMode::AskForEverything, "default"),
        (ApprovalMode::AskBeforeCommands, "acceptEdits"),
        (ApprovalMode::NeverAsk, "bypassPermissions"),
    ] {
        assert!(flags(mode, None).contains(&format!("--permission-mode {flag} ")));
    }
}
