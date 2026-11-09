//! Recorded runs of `pi --mode rpc` with our extension, replayed through the mapper.
//!
//! The fixtures are in `tests/fixtures/pi/`, one run each, and `record-pi.py` next to them
//! records them again. Each line is what we wrote, what Pi wrote, or how it exited.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::arguments;
use super::mapper::Mapper;
use super::protocol::Outgoing;
use crate::provider::{
    AgentEvent, ApprovalAnswer, ApprovalId, ApprovalMode, Installed, Message, Provider,
    ThreadOptions,
};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Entry {
    Sent(Value),
    Received(Value),
    Exited { code: Option<i32>, stderr: String },
}

fn entries(name: &str) -> Vec<Entry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi")
        .join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// What we wrote, as the driver writes it. An answer goes through the mapper, which must
/// write the line that was recorded.
fn sent(mapper: &mut Mapper, line: &Value) -> Outgoing {
    let text = |key: &str| line[key].as_str().unwrap_or_default().to_string();
    match line["type"].as_str() {
        Some("prompt") if text("id").starts_with("message") => {
            Outgoing::message(text("id"), Message::from(text("message")))
        }
        Some("prompt") => Outgoing::command(text("id"), text("message")),
        Some("abort") => Outgoing::Abort { id: text("id") },
        Some("extension_ui_response") => {
            let answer = match line["value"].as_str() {
                Some("allow") => ApprovalAnswer::Allow,
                Some("allow_for_thread") => ApprovalAnswer::AllowForThread,
                _ => ApprovalAnswer::Deny,
            };
            let outgoing = mapper.answer(&ApprovalId(text("id")), answer).unwrap();
            assert_eq!(&serde_json::to_value(&outgoing).unwrap(), line);
            outgoing
        }
        other => panic!("not a line we write: {other:?}"),
    }
}

/// The events of a recorded run, each run of text deltas joined into one, so the snapshot
/// reads as the text.
fn replay(name: &str) -> Vec<AgentEvent> {
    let mut mapper = Mapper::new(PathBuf::from("/tmp/project"));
    let mut events: Vec<AgentEvent> = Vec::new();
    for entry in entries(name) {
        let new = match entry {
            Entry::Sent(line) => {
                let outgoing = sent(&mut mapper, &line);
                mapper.sent(&outgoing)
            }
            Entry::Received(line) => {
                let (answers, events) = mapper.received(&line.to_string());
                assert!(answers.is_empty(), "answered {line}");
                events
            }
            Entry::Exited { code, stderr } => {
                let said = stderr.lines().map(str::trim).find(|line| !line.is_empty());
                mapper.exited(code, said)
            }
        };
        for event in new {
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
fn approval_allowed() {
    insta::assert_debug_snapshot!(replay("approval_allowed"));
}

#[test]
fn approval_denied() {
    insta::assert_debug_snapshot!(replay("approval_denied"));
}

/// A stop while it asks: the step fails and the turn ends stopped.
#[test]
fn interrupt_while_asking() {
    insta::assert_debug_snapshot!(replay("interrupt"));
}

/// A refused message ends its turn with Pi's reason; a refused command of ours says what
/// failed, and the thread goes on.
#[test]
fn a_refusal_ends_only_a_turn() {
    let mut mapper = Mapper::new(PathBuf::from("/tmp/project"));
    let message = Outgoing::message("message-1".to_string(), "Hi".into());
    assert_eq!(mapper.sent(&message), [AgentEvent::TurnStarted]);
    let refused = r#"{"id":"message-1","type":"response","command":"prompt","success":false,"error":"No model selected"}"#;
    let (_, events) = mapper.received(refused);
    assert_eq!(
        events,
        [AgentEvent::TurnEnded {
            outcome: crate::TurnOutcome::Failed {
                message: "No model selected".to_string()
            }
        }]
    );
    let refused = r#"{"id":"model-2","type":"response","command":"set_model","success":false,"error":"Model not found: x/y"}"#;
    let (_, events) = mapper.received(refused);
    assert_eq!(
        events,
        [AgentEvent::Error {
            message: "Pi could not change the model: Model not found: x/y".to_string()
        }]
    );
}

/// A retry that answers ends the turn completed, though the try before it failed.
#[test]
fn a_retry_that_works_ends_the_turn_completed() {
    let mut mapper = Mapper::new(PathBuf::from("/tmp/project"));
    mapper.sent(&Outgoing::message("message-1".to_string(), "Hi".into()));
    for line in [
        r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"529 overloaded"}}"#,
        r#"{"type":"message_end","message":{"role":"assistant","stopReason":"stop"}}"#,
    ] {
        mapper.received(line);
    }
    let (_, events) = mapper.received(r#"{"type":"agent_settled","aborted":false}"#);
    let completed = AgentEvent::TurnEnded {
        outcome: crate::TurnOutcome::Completed,
    };
    assert_eq!(events, [completed]);
}

/// A message Pi takes for a command of an extension starts no run: its answer ends the turn.
#[test]
fn a_message_pi_took_for_a_command_ends_its_turn() {
    let mut mapper = Mapper::new(PathBuf::from("/tmp/project"));
    mapper.sent(&Outgoing::message(
        "message-1".to_string(),
        "/sound-tools-models".into(),
    ));
    let handled = r#"{"id":"message-1","type":"response","command":"prompt","success":true,"data":{"disposition":"handled"}}"#;
    let (_, events) = mapper.received(handled);
    let completed = AgentEvent::TurnEnded {
        outcome: crate::TurnOutcome::Completed,
    };
    assert_eq!(events, [completed]);
}

/// A sign-in question in a thread, or a select about no step of ours, has nobody to answer
/// it: it is cancelled, so Pi does not wait.
#[test]
fn a_question_nobody_answers_is_cancelled() {
    let mut mapper = Mapper::new(PathBuf::from("/tmp/project"));
    for request in [
        r#"{"type":"extension_ui_request","id":"q","method":"input","title":"{}"}"#,
        r#"{"type":"extension_ui_request","id":"q","method":"select","title":"unknown","options":[]}"#,
    ] {
        let (answers, events) = mapper.received(request);
        assert!(events.is_empty());
        let answers: Vec<Value> = (answers.iter())
            .map(|answer| serde_json::to_value(answer).unwrap())
            .collect();
        assert_eq!(
            answers,
            [serde_json::json!({ "type": "extension_ui_response", "id": "q", "cancelled": true })]
        );
    }
}

#[test]
fn starts_pi_with_the_session_mode_and_model() {
    let options = ThreadOptions {
        provider: Provider::Pi,
        installed: Installed {
            program: PathBuf::from("pi"),
            environment: Default::default(),
        },
        folder: PathBuf::from("/tmp/project"),
        model: Some("openrouter/google/gemini-2.5-flash-lite".to_string()),
        approval_mode: ApprovalMode::NeverAsk,
        resume: None,
        instructions: Some(PathBuf::from("/tmp/instructions.md")),
    };
    let arguments: Vec<String> = arguments(&options, "id")
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    insta::assert_debug_snapshot!(arguments);
}
