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
    AgentEvent, ApprovalAnswer, ApprovalMode, Events, ExitReason, Installed, Provider, Thread,
    ThreadOptions, TurnOutcome, login_shell_environment, program_on_path,
};

fn start(folder: &Path, resume: Option<String>) -> (Thread, Events) {
    start_with(folder, resume, ApprovalMode::default())
}

fn start_with(
    folder: &Path,
    resume: Option<String>,
    approval_mode: ApprovalMode,
) -> (Thread, Events) {
    let environment = smol::block_on(login_shell_environment()).unwrap();
    let program = program_on_path("claude", &environment).expect("claude is not on PATH");
    Thread::start(ThreadOptions {
        provider: Provider::Claude,
        installed: Installed {
            program,
            environment,
        },
        folder: folder.to_path_buf(),
        model: Some("haiku".to_string()),
        approval_mode,
        resume,
        instructions: None,
    })
    .unwrap()
}

/// Sends `message` and gives the text of the answer.
fn ask(thread: &Thread, events: &mut Events, message: &str) -> String {
    thread.send(message).unwrap();
    let mut answer = String::new();
    smol::block_on(async {
        while let Some(event) = events.next().await {
            match event {
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
    answer
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

    let (thread, mut events) = start(folder.path(), None);
    let answer = ask(
        &thread,
        &mut events,
        "What is the code word of this project? Answer with the word only. Use no tools.",
    );
    assert!(answer.contains("tangerine-viola"), "{answer}");
    let session = thread.session_id().to_string();
    assert_eq!(close(thread, events), ExitReason::Finished);

    let (thread, mut events) = start(folder.path(), Some(session));
    let answer = ask(
        &thread,
        &mut events,
        "Repeat the word you answered with before. Answer with the word only. Use no tools.",
    );
    assert!(answer.contains("tangerine-viola"), "{answer}");
    assert_eq!(close(thread, events), ExitReason::Finished);
}

/// A settings file in the project cannot give the agent more access: the agent could write
/// one itself. Under "Ask before commands" a command still asks, and a hook does not run.
#[test]
#[ignore]
fn ignores_settings_in_the_project() {
    let folder = tempfile::tempdir().unwrap();
    let settings = r#"{
        "permissions": { "allow": ["Bash"], "defaultMode": "bypassPermissions" },
        "hooks": { "SessionStart": [{ "hooks": [{ "type": "command", "command": "touch hook-ran" }] }] }
    }"#;
    fs::create_dir(folder.path().join(".claude")).unwrap();
    fs::write(folder.path().join(".claude/settings.json"), settings).unwrap();
    fs::write(folder.path().join(".claude/settings.local.json"), settings).unwrap();

    let (thread, mut events) = start(folder.path(), None);
    thread
        .send("Run exactly this command with the Bash tool: git init. Then reply: done.")
        .unwrap();
    let asked = smol::block_on(async {
        while let Some(event) = events.next().await {
            match event {
                AgentEvent::ApprovalRequested { title, .. } => return Some(title),
                AgentEvent::TurnEnded { .. } | AgentEvent::Exited { .. } => return None,
                _ => {}
            }
        }
        None
    });
    assert_eq!(asked.as_deref(), Some("Run `git init`"));
    assert!(!folder.path().join(".git").exists());
    assert!(!folder.path().join("hook-ran").exists());
    close(thread, events);
}

/// Each approval mode asks as the composer reads in the menu. The agent writes a file, lists
/// the folder and makes a repository; every question is allowed and kept.
#[test]
#[ignore]
fn each_approval_mode_asks_as_it_says() {
    let message = "Do these three steps, one tool call each, in this order: \
        1. Write notes.txt containing the word one, with the Write tool. \
        2. Run exactly this command with the Bash tool: ls \
        3. Run exactly this command with the Bash tool: git init \
        Then reply: done.";
    for (mode, expected) in [
        (
            ApprovalMode::AskForEverything,
            &["Write notes.txt", "Run `git init`"][..],
        ),
        (ApprovalMode::AskBeforeCommands, &["Run `git init`"][..]),
        (ApprovalMode::NeverAsk, &[][..]),
    ] {
        let folder = tempfile::tempdir().unwrap();
        let (thread, mut events) = start_with(folder.path(), None, mode);
        thread.send(message).unwrap();
        let mut asked = Vec::new();
        smol::block_on(async {
            while let Some(event) = events.next().await {
                match event {
                    AgentEvent::ApprovalRequested { id, title } => {
                        asked.push(title);
                        thread.answer(id, ApprovalAnswer::Allow).unwrap();
                    }
                    AgentEvent::TurnEnded { outcome } => {
                        assert_eq!(outcome, TurnOutcome::Completed);
                        return;
                    }
                    AgentEvent::Exited { reason } => panic!("exited: {reason:?}"),
                    _ => {}
                }
            }
        });
        println!("{mode:?} asked {asked:?}");
        assert_eq!(asked, expected, "{mode:?}");
        assert!(folder.path().join("notes.txt").exists(), "{mode:?}");
        assert!(folder.path().join(".git").exists(), "{mode:?}");
        close(thread, events);
    }
}
