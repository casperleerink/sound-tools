use std::io::Write as _;
use std::time::Duration;

use super::*;
use crate::conversation::Entry;
use crate::{Account, ApprovalId, ExitReason, StepId, StepOutcome};

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
}

/// A project folder in `machine`, and the store of its threads there.
fn project(machine: &Path, name: &str) -> (PathBuf, ThreadStore) {
    let folder = machine.join(name);
    fs::create_dir_all(&folder).unwrap();
    let store = ThreadStore::new(&machine.join("agent/threads"), &folder);
    (folder, store)
}

/// Sends `message`, applies `events` a second apart, and keeps it all as the sidebar does.
fn keep(
    store: &ThreadStore,
    thread: &mut SavedThread,
    conversation: &mut Conversation,
    message: &str,
    sent: u64,
    events: Vec<AgentEvent>,
) {
    conversation.send(message, at(sent));
    thread.updated = at(sent);
    let mut lines = vec![Line::Sent {
        at: at(sent),
        message: message.to_string(),
    }];
    for (event, second) in events.into_iter().zip(sent + 1..) {
        if let AgentEvent::Started { session_id, .. } = &event {
            thread.session_id = Some(session_id.clone());
        }
        lines.extend(Line::of(&event, at(second)));
        conversation.apply(event, at(second));
    }
    store.write(&Write::Thread(thread.clone())).unwrap();
    let thread = thread.id.clone();
    store.write(&Write::Lines { thread, lines }).unwrap();
}

fn answered(text: &str) -> Vec<AgentEvent> {
    vec![
        AgentEvent::TurnStarted,
        AgentEvent::TextDone {
            text: text.to_string(),
        },
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]
}

#[test]
fn a_thread_reads_back_as_the_conversation_it_was() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let mut thread = SavedThread::new(Provider::Claude, "Add a bass line", at(0));
    let mut conversation = Conversation::default();
    let step = StepId("write".to_string());
    keep(
        &store,
        &mut thread,
        &mut conversation,
        "Add a bass line",
        0,
        vec![
            AgentEvent::TurnStarted,
            AgentEvent::Started {
                session_id: "session".to_string(),
                account: Account::default(),
                models: Vec::new(),
            },
            AgentEvent::StepStarted {
                id: step.clone(),
                title: "Wrote state/arrangement/bass/clip.json".to_string(),
                running_title: "Writing state/arrangement/bass/clip.json".to_string(),
            },
            AgentEvent::StepDone {
                id: step,
                outcome: StepOutcome::Done,
            },
            AgentEvent::TextDelta {
                text: "Added".to_string(),
            },
            AgentEvent::TextDone {
                text: "Added a bass line.".to_string(),
            },
            AgentEvent::TurnEnded {
                outcome: TurnOutcome::Completed,
            },
        ],
    );
    keep(
        &store,
        &mut thread,
        &mut conversation,
        "Build it",
        20,
        vec![
            AgentEvent::TurnStarted,
            AgentEvent::ApprovalRequested {
                id: ApprovalId("build".to_string()),
                title: "Run `cargo build`".to_string(),
            },
            AgentEvent::TurnEnded {
                outcome: TurnOutcome::Interrupted,
            },
            AgentEvent::Error {
                message: "Could not stop".to_string(),
            },
        ],
    );

    let (saved, replayed) = store.last().unwrap().unwrap();
    assert_eq!(replayed, conversation);
    assert_eq!(saved, thread);
    assert_eq!(saved.title, "Add a bass line");
    assert_eq!(saved.session(), Session::Resume("session".to_string()));
    // "Worked for" survives: the first turn worked from the message to its end.
    let Some(Entry::Turn(turn)) = replayed.entries().get(1) else {
        panic!("{:?}", replayed.entries());
    };
    let worked = turn.end.as_ref().map(|end| end.worked);
    assert_eq!(worked, Some(Duration::from_secs(7)));
}

#[test]
fn a_damaged_line_is_left_out_with_a_notice() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let mut thread = SavedThread::new(Provider::Claude, "First", at(0));
    let mut conversation = Conversation::default();
    keep(
        &store,
        &mut thread,
        &mut conversation,
        "First",
        0,
        answered("One."),
    );
    let log = store.log(&thread.id);
    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    file.write_all(b"{\"sent\": {\"at\": 5\n").unwrap();
    keep(
        &store,
        &mut thread,
        &mut conversation,
        "Second",
        10,
        answered("Two."),
    );

    let (_, replayed) = store.last().unwrap().unwrap();
    let kinds: Vec<_> = replayed
        .entries()
        .iter()
        .map(|entry| match entry {
            Entry::Message(text) => text.as_str(),
            Entry::Turn(turn) => turn.blocks.first().map_or("", String::as_str),
            Entry::Notice(text) => text.as_str(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "First",
            "One.",
            "Part of this thread could not be read.",
            "Second",
            "Two."
        ]
    );
}

/// The app quit while the agent worked: the turn shows as stopped, not working forever.
#[test]
fn a_turn_the_app_quit_during_ends_stopped() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let mut thread = SavedThread::new(Provider::Claude, "Hello", at(0));
    let mut conversation = Conversation::default();
    let events = vec![
        AgentEvent::TurnStarted,
        AgentEvent::TextDone {
            text: "Half".to_string(),
        },
    ];
    keep(&store, &mut thread, &mut conversation, "Hello", 0, events);

    let (_, replayed) = store.last().unwrap().unwrap();
    assert!(!replayed.is_working());
    let Some(Entry::Turn(turn)) = replayed.entries().get(1) else {
        panic!("{:?}", replayed.entries());
    };
    let end = turn.end.as_ref().unwrap();
    assert_eq!(end.outcome, TurnOutcome::Interrupted);
    assert_eq!(end.worked, Duration::from_secs(2));
}

/// A lost session is in the log too, so the thread stays ended when it opens again.
#[test]
fn a_lost_session_stays_lost() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let mut thread = SavedThread::new(Provider::Claude, "Hello", at(0));
    let mut conversation = Conversation::default();
    let events = vec![
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "No conversation found".to_string(),
            },
        },
        AgentEvent::Exited {
            reason: ExitReason::SessionNotFound,
        },
    ];
    keep(&store, &mut thread, &mut conversation, "Hello", 0, events);
    let (_, replayed) = store.last().unwrap().unwrap();
    assert!(!replayed.can_continue());
}

#[test]
fn two_projects_never_share_a_thread() {
    let machine = tempfile::tempdir().unwrap();
    let (night, night_store) = project(machine.path(), "night");
    // Names that a plain `/` to `-` would give the same key.
    let (_, day_store) = project(machine.path(), "a-b");
    let (_, other_store) = project(&machine.path().join("a"), "b");
    assert!(other_store.last().unwrap().is_none());
    for (store, message) in [
        (&night_store, "Night"),
        (&day_store, "Day"),
        (&other_store, "Other"),
    ] {
        let mut thread = SavedThread::new(Provider::Claude, message, at(0));
        let mut conversation = Conversation::default();
        keep(
            store,
            &mut thread,
            &mut conversation,
            message,
            0,
            answered("Ok."),
        );
    }
    let title = |store: &ThreadStore| store.last().unwrap().unwrap().0.title;
    assert_eq!(title(&night_store), "Night");
    assert_eq!(title(&day_store), "Day");
    assert_eq!(title(&other_store), "Other");

    // The same folder through a link or `..` is the same project.
    let link = machine.path().join("link");
    std::os::unix::fs::symlink(&night, &link).unwrap();
    let threads = machine.path().join("agent/threads");
    assert_eq!(title(&ThreadStore::new(&threads, &link)), "Night");
    let roundabout = night.join("..").join("night");
    assert_eq!(title(&ThreadStore::new(&threads, &roundabout)), "Night");
}

#[test]
fn the_index_keeps_every_thread_and_the_last_one_used_last() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    assert!(store.last().unwrap().is_none());
    let mut first = SavedThread::new(Provider::Claude, "First", at(0));
    let mut second = SavedThread::new(Provider::Claude, "Second", at(10));
    let mut conversation = Conversation::default();
    keep(
        &store,
        &mut first,
        &mut conversation,
        "First",
        0,
        answered("One."),
    );
    let mut conversation = Conversation::default();
    keep(
        &store,
        &mut second,
        &mut conversation,
        "Second",
        10,
        answered("Two."),
    );
    assert_eq!(store.last().unwrap().unwrap().0, second);

    // Back to the first: it is the last one used, and the second stays in the index.
    store.write(&Write::Thread(first.clone())).unwrap();
    assert_eq!(store.last().unwrap().unwrap().0, first);
    let ids: Vec<_> = store
        .index()
        .unwrap()
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    assert_eq!(ids, [second.id, first.id]);
}
