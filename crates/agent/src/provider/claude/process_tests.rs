//! The I/O half of the driver, the thread and the sign-in, against
//! `tests/fixtures/fake-claude.sh` in place of `claude`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use smol::future;

use super::protocol::Outgoing;
use crate::provider::{
    Account, AgentEvent, ApprovalMode, Driver, Events, ExitReason, Installed, Provider, Thread,
    ThreadOptions, TurnOutcome,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The fake in the scenario `scenario`, with `FAKE_OUTPUT` in `output`.
fn start(scenario: &str, output: &Path) -> (Thread, Events) {
    let mut environment: std::collections::HashMap<_, _> = std::env::vars_os().collect();
    environment.insert("FAKE_CLAUDE".into(), scenario.into());
    environment.insert("FAKE_OUTPUT".into(), output.into());
    environment.insert(
        "FAKE_FIXTURE".into(),
        fixtures().join("claude/plain.jsonl").into(),
    );
    Thread::start(ThreadOptions {
        provider: Provider::Claude,
        installed: Installed {
            program: fixtures().join("fake-claude.sh"),
            environment,
        },
        folder: std::env::temp_dir(),
        model: None,
        approval_mode: ApprovalMode::default(),
        resume: None,
        instructions: None,
    })
    .unwrap()
}

/// How long any wait of these tests may take, so a hang fails fast and names what it waited
/// for.
const DEADLINE: Duration = Duration::from_secs(10);

/// Runs `future` to the end, or fails the test after [`DEADLINE`].
fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    // Not a gpui test, and the timer only bounds a hang.
    #[allow(clippy::disallowed_methods)]
    let deadline = async {
        smol::Timer::after(DEADLINE).await;
        panic!("{what} took over {DEADLINE:?}");
    };
    smol::block_on(future::or(future, deadline))
}

fn rest(events: &mut Events) -> Vec<AgentEvent> {
    within("the end of the events", async {
        let mut rest = Vec::new();
        while let Some(event) = events.next().await {
            rest.push(event);
        }
        rest
    })
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < DEADLINE, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dropping_the_thread_ends_the_agent() {
    let folder = tempfile::tempdir().unwrap();
    let (thread, mut events) = start("replay", &folder.path().join("output"));
    thread.send("Say hello.").unwrap();
    let turn = within("the end of the turn", async {
        loop {
            if let Some(AgentEvent::TurnEnded { outcome }) = events.next().await {
                return outcome;
            }
        }
    });
    assert_eq!(turn, TurnOutcome::Completed);
    drop(thread);
    let rest = rest(&mut events);
    assert_eq!(
        rest.last(),
        Some(&AgentEvent::Exited {
            reason: ExitReason::Finished
        })
    );
}

#[test]
fn dropping_the_events_ends_what_the_agent_started() {
    let folder = tempfile::tempdir().unwrap();
    let output = folder.path().join("output");
    let (_thread, events) = start("children", &output);
    wait_for("the fake did not start its child", || {
        fs::read_to_string(&output).is_ok_and(|pid| !pid.trim().is_empty())
    });
    let child = fs::read_to_string(&output).unwrap().trim().to_string();
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", &child])
            .status()
            .unwrap()
            .success()
    };
    assert!(alive());
    drop(events);
    wait_for("the agent's child still runs", || !alive());
}

#[test]
fn a_line_it_cannot_read_ends_the_turn_and_answers_the_request() {
    let folder = tempfile::tempdir().unwrap();
    let output = folder.path().join("output");
    let (thread, mut events) = start("malformed", &output);
    thread.send("Say hello.").unwrap();
    let mut seen = Vec::new();
    within("the end of the turn", async {
        while let Some(event) = events.next().await {
            let ended = matches!(event, AgentEvent::TurnEnded { .. });
            seen.push(event);
            if ended {
                break;
            }
        }
    });
    let errors = seen
        .iter()
        .filter(|event| matches!(event, AgentEvent::Error { .. }))
        .count();
    assert_eq!(errors, 2, "{seen:?}");
    assert!(
        matches!(
            seen.last(),
            Some(AgentEvent::TurnEnded {
                outcome: TurnOutcome::Failed { .. }
            })
        ),
        "{seen:?}"
    );
    let answer: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(answer["response"]["subtype"], "error");
    assert_eq!(answer["response"]["request_id"], "broken");
}

/// The fake reads nothing until the test lets it, so a long message fills the pipe and its
/// write waits. `next` is dropped halfway through the write many times, and the CLI still
/// gets the whole line once.
#[test]
fn cancelling_next_loses_nothing_of_a_message() {
    let folder = tempfile::tempdir().unwrap();
    let go = folder.path().join("go");
    let (thread, mut events) = start("slow_reader", &go);
    let text = "a".repeat(300_000);
    let line = serde_json::to_vec(&Outgoing::user(text.clone())).unwrap();
    thread.send(text).unwrap();
    for _ in 0..10 {
        if let Some(Some(event)) = smol::block_on(future::poll_once(events.next())) {
            panic!("an event before the fake read anything: {event:?}");
        }
    }
    let Driver::Claude(driver) = &events.driver;
    let written = driver.writing.as_ref().map_or(0, |writing| writing.written);
    assert!(
        written > 0 && written < line.len(),
        "not halfway through the message: {written} of {} bytes",
        line.len()
    );

    fs::write(&go, "").unwrap();
    let mut seen = Vec::new();
    let answer = within("the answer to the long message", async {
        loop {
            match events.next().await {
                Some(AgentEvent::TextDone { text }) => return text,
                Some(event) => seen.push(event),
                None => panic!("ended early: {seen:?}"),
            }
        }
    });
    assert_eq!(answer, line.len().to_string());
    assert_eq!(seen, vec![AgentEvent::TurnStarted]);
}

/// The fake as the program of the sign-in, in the scenario `scenario`. The file `account`
/// says signed in, and `output` is `FAKE_OUTPUT`.
fn installed(scenario: &str, account: &Path, output: &Path) -> Installed {
    let mut environment: std::collections::HashMap<_, _> = std::env::vars_os().collect();
    environment.insert("FAKE_CLAUDE".into(), scenario.into());
    environment.insert("FAKE_ACCOUNT".into(), account.into());
    environment.insert("FAKE_OUTPUT".into(), output.into());
    Installed {
        program: fixtures().join("fake-claude.sh"),
        environment,
    }
}

#[test]
fn the_models_come_with_no_message() {
    let folder = tempfile::tempdir().unwrap();
    let account = folder.path().join("account");
    let mut installed = installed("models", &account, &folder.path().join("output"));
    let fixture = fixtures().join("claude/plain.jsonl");
    installed
        .environment
        .insert("FAKE_FIXTURE".into(), fixture.into());
    let models = within(
        "the models",
        Provider::Claude.models(installed, std::env::temp_dir()),
    );
    let models = models.unwrap();
    assert_eq!(
        models.first().map(|model| model.id.as_str()),
        Some("default")
    );
}

#[test]
fn signs_in_with_the_choice_then_out() {
    let folder = tempfile::tempdir().unwrap();
    let account = folder.path().join("account");
    let installed = installed("login", &account, &folder.path().join("output"));
    let provider = Provider::Claude;
    assert_eq!(smol::block_on(provider.account(&installed)).unwrap(), None);

    let choices = provider.sign_in_choices();
    let labels: Vec<_> = choices.iter().map(|choice| choice.label()).collect();
    assert_eq!(
        labels,
        [
            "Sign in with your Claude plan",
            "Use an Anthropic Console account (API)"
        ]
    );
    smol::block_on(choices[0].run(&installed)).unwrap();
    let signed_in = smol::block_on(provider.account(&installed)).unwrap();
    assert_eq!(
        signed_in,
        Some(Account {
            email: Some("composer@example.com".to_string()),
            plan: Some("Claude Pro".to_string()),
        })
    );

    smol::block_on(provider.sign_out(&installed)).unwrap();
    assert_eq!(smol::block_on(provider.account(&installed)).unwrap(), None);
}

#[test]
fn a_failed_sign_in_and_a_broken_status_say_why() {
    let folder = tempfile::tempdir().unwrap();
    let account = folder.path().join("account");
    let output = folder.path().join("output");
    let choice = Provider::Claude.sign_in_choices()[1];
    let failed = smol::block_on(choice.run(&installed("login_fails", &account, &output)));
    assert_eq!(
        failed.unwrap_err().to_string(),
        "OAuth login failed: access denied"
    );
    let broken = smol::block_on(Provider::Claude.account(&installed("broken", &account, &output)));
    assert_eq!(
        broken.unwrap_err().to_string(),
        "Error: the config is damaged"
    );
    let missing = Installed {
        program: folder.path().join("missing"),
        environment: Default::default(),
    };
    assert!(smol::block_on(Provider::Claude.account(&missing)).is_err());
}

#[test]
fn cancelling_the_sign_in_ends_its_process() {
    let folder = tempfile::tempdir().unwrap();
    let output = folder.path().join("output");
    let installed = installed("login_waits", &folder.path().join("account"), &output);
    let choice = Provider::Claude.sign_in_choices()[0];
    let mut signing_in = Box::pin(choice.run(&installed));
    assert!(smol::block_on(future::poll_once(&mut signing_in)).is_none());
    wait_for("the sign-in did not start", || {
        fs::read_to_string(&output).is_ok_and(|pid| !pid.trim().is_empty())
    });
    let pid = fs::read_to_string(&output).unwrap().trim().to_string();
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", &pid])
            .status()
            .unwrap()
            .success()
    };
    assert!(alive());
    drop(signing_in);
    wait_for("the sign-in still runs", || !alive());
}
