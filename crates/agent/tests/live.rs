#![allow(clippy::unwrap_used)]
//! The real `claude`, in a temporary project. Ignored by default: it needs a signed-in
//! `claude` on the login shell's `PATH`, and it costs a cent of haiku.
//!
//! ```sh
//! cargo test -p sound-agent --test live -- --ignored
//! ```

use std::fs;
use std::path::Path;

use sound_agent::{
    AgentEvent, ApprovalMode, Events, ExitReason, Provider, Session, Thread, ThreadOptions,
    TurnOutcome, login_shell_environment, program_on_path,
};

fn start(folder: &Path, session: Session) -> (Thread, Events) {
    let environment = smol::block_on(login_shell_environment());
    let program = program_on_path("claude", &environment).expect("claude is not on PATH");
    Thread::start(ThreadOptions {
        provider: Provider::Claude,
        program,
        folder: folder.to_path_buf(),
        model: Some("haiku".to_string()),
        approval_mode: ApprovalMode::default(),
        session,
        environment,
    })
    .unwrap()
}

/// Sends `message` and gives the session id and the text of the answer.
fn ask(thread: &Thread, events: &mut Events, message: &str) -> (Option<String>, String) {
    thread.send(message).unwrap();
    let mut session = None;
    let mut answer = String::new();
    smol::block_on(async {
        while let Some(event) = events.next().await {
            match event {
                AgentEvent::Started { session_id, .. } => session = Some(session_id),
                AgentEvent::TextDone { text } => answer.push_str(&text),
                AgentEvent::TurnEnded { outcome } => {
                    assert_eq!(outcome, TurnOutcome::Completed, "{answer}");
                    return;
                }
                AgentEvent::Exited { reason } => panic!("exited: {reason:?}"),
                _ => {}
            }
        }
    });
    (session, answer)
}

/// Drops the thread and gives how the process ended.
fn close(thread: Thread, mut events: Events) -> ExitReason {
    drop(thread);
    smol::block_on(async {
        while let Some(event) = events.next().await {
            if let AgentEvent::Exited { reason } = event {
                return reason;
            }
        }
        panic!("no exit");
    })
}

/// The trimmed flags still load the project's `CLAUDE.md`, which imports `AGENTS.md` as the
/// app writes them, and a thread resumes where it left off.
#[test]
#[ignore]
fn reads_the_project_map_and_resumes() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(folder.path().join("CLAUDE.md"), "@AGENTS.md\n").unwrap();
    fs::write(
        folder.path().join("AGENTS.md"),
        "# Project\n\nThe code word of this project is tangerine-viola.\n",
    )
    .unwrap();

    let (thread, mut events) = start(folder.path(), Session::New);
    let (session, answer) = ask(
        &thread,
        &mut events,
        "What is the code word of this project? Answer with the word only. Use no tools.",
    );
    assert!(answer.contains("tangerine-viola"), "{answer}");
    assert_eq!(close(thread, events), ExitReason::Finished);

    let (thread, mut events) = start(folder.path(), Session::Resume(session.unwrap()));
    let (_, answer) = ask(
        &thread,
        &mut events,
        "Repeat the word you answered with before. Answer with the word only. Use no tools.",
    );
    assert!(answer.contains("tangerine-viola"), "{answer}");
    assert_eq!(close(thread, events), ExitReason::Finished);
}
