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
    thread: &SavedThread,
    conversation: &mut Conversation,
    message: &str,
    sent: u64,
    events: Vec<AgentEvent>,
) {
    conversation.send(message, at(sent));
    let mut lines = vec![Line::Sent {
        at: at(sent),
        message: message.to_string(),
    }];
    for (event, second) in events.into_iter().zip(sent + 1..) {
        lines.extend(Line::of(&event, at(second)));
        conversation.apply(event, at(second));
    }
    store.write(&Write::Current(Some(thread.clone()))).unwrap();
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

/// What a replay shows, one word per entry.
fn shown(conversation: &Conversation) -> Vec<&str> {
    conversation
        .entries()
        .iter()
        .map(|entry| match entry {
            Entry::Message(text) | Entry::Notice(text) => text.as_str(),
            Entry::Turn(turn) => turn.blocks.first().map_or("", String::as_str),
        })
        .collect()
}

#[test]
fn a_thread_reads_back_as_the_conversation_it_was() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let thread = SavedThread {
        session_id: Some("session".to_string()),
        ..SavedThread::fresh()
    };
    let mut conversation = Conversation::default();
    let step = StepId("write".to_string());
    keep(
        &store,
        &thread,
        &mut conversation,
        "Add a bass line",
        0,
        vec![
            AgentEvent::TurnStarted,
            AgentEvent::Started {
                account: Account::default(),
                models: Vec::new(),
            },
            AgentEvent::StepStarted {
                id: step.clone(),
                title: "Wrote state/arrangement/bass/clip.json".to_string(),
                request_title: "Write state/arrangement/bass/clip.json".to_string(),
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
        &thread,
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

    let (saved, replayed) = store.current().unwrap().unwrap();
    assert_eq!(replayed, conversation);
    assert_eq!(saved, thread);
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
    let thread = SavedThread::fresh();
    let mut conversation = Conversation::default();
    keep(
        &store,
        &thread,
        &mut conversation,
        "First",
        0,
        answered("One."),
    );
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(store.log(&thread.id))
        .unwrap();
    file.write_all(b"{\"sent\": {\"at\": 5\n").unwrap();
    keep(
        &store,
        &thread,
        &mut conversation,
        "Second",
        10,
        answered("Two."),
    );

    let (_, replayed) = store.current().unwrap().unwrap();
    assert_eq!(
        shown(&replayed),
        [
            "First",
            "One.",
            "Part of this thread could not be read.",
            "Second",
            "Two."
        ]
    );
}

/// A write a crash cut off has no end of line. The next write ends it first, so only the cut
/// line is lost, not the good one after it.
#[test]
fn a_line_cut_off_does_not_take_the_next_one_with_it() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    let thread = SavedThread::fresh();
    let mut conversation = Conversation::default();
    keep(
        &store,
        &thread,
        &mut conversation,
        "First",
        0,
        answered("One."),
    );
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(store.log(&thread.id))
        .unwrap();
    file.write_all(b"{\"event\": {\"at\": {\"secs_since")
        .unwrap();
    keep(
        &store,
        &thread,
        &mut conversation,
        "Second",
        10,
        answered("Two."),
    );

    let (_, replayed) = store.current().unwrap().unwrap();
    assert_eq!(
        shown(&replayed),
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
    let thread = SavedThread::fresh();
    let mut conversation = Conversation::default();
    let events = vec![
        AgentEvent::TurnStarted,
        AgentEvent::TextDone {
            text: "Half".to_string(),
        },
    ];
    keep(&store, &thread, &mut conversation, "Hello", 0, events);

    let (_, replayed) = store.current().unwrap().unwrap();
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
    let thread = SavedThread::fresh();
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
    keep(&store, &thread, &mut conversation, "Hello", 0, events);
    let (_, replayed) = store.current().unwrap().unwrap();
    assert!(!replayed.can_continue());
}

#[test]
fn two_projects_never_share_a_thread() {
    let machine = tempfile::tempdir().unwrap();
    let (night, night_store) = project(machine.path(), "night");
    let (_, day_store) = project(machine.path(), "a-b");
    let (_, other_store) = project(&machine.path().join("a"), "b");
    assert!(other_store.current().unwrap().is_none());
    for (store, message) in [
        (&night_store, "Night"),
        (&day_store, "Day"),
        (&other_store, "Other"),
    ] {
        let mut conversation = Conversation::default();
        let thread = SavedThread::fresh();
        keep(
            store,
            &thread,
            &mut conversation,
            message,
            0,
            answered("Ok."),
        );
    }
    let first = |store: &ThreadStore| {
        let (_, conversation) = store.current().unwrap().unwrap();
        shown(&conversation).first().map(|text| text.to_string())
    };
    assert_eq!(first(&night_store).as_deref(), Some("Night"));
    assert_eq!(first(&day_store).as_deref(), Some("Day"));
    assert_eq!(first(&other_store).as_deref(), Some("Other"));

    // The same folder through a link or `..` is the same project. A link on Windows needs
    // admin rights or developer mode.
    let threads = machine.path().join("agent/threads");
    #[cfg(unix)]
    {
        let link = machine.path().join("link");
        std::os::unix::fs::symlink(&night, &link).unwrap();
        let through_link = ThreadStore::new(&threads, &link);
        assert_eq!(first(&through_link).as_deref(), Some("Night"));
    }
    let roundabout = ThreadStore::new(&threads, &night.join("..").join("night"));
    assert_eq!(first(&roundabout).as_deref(), Some("Night"));
}

#[test]
fn the_index_keeps_every_thread_and_names_the_current_one() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    assert!(store.current().unwrap().is_none());
    let first = SavedThread::fresh();
    let second = SavedThread::fresh();
    let mut conversation = Conversation::default();
    keep(
        &store,
        &first,
        &mut conversation,
        "First",
        0,
        answered("One."),
    );
    let mut conversation = Conversation::default();
    keep(
        &store,
        &second,
        &mut conversation,
        "Second",
        10,
        answered("Two."),
    );
    assert_eq!(store.current().unwrap().unwrap().0, second);

    let recent = |store: &ThreadStore| -> Vec<String> {
        let recent = store.recent(10).unwrap();
        recent.into_iter().map(|thread| thread.title).collect()
    };
    assert_eq!(recent(&store), ["First"]);

    // **+**: no thread is current, and both stay, the one last shown first.
    store.write(&Write::Current(None)).unwrap();
    assert!(store.current().unwrap().is_none());
    assert_eq!(recent(&store), ["Second", "First"]);
    // Back to the first: it moves to the front once it is left again.
    store.write(&Write::Current(Some(first.clone()))).unwrap();
    assert_eq!(store.current().unwrap().unwrap().0, first);
    assert_eq!(recent(&store), ["Second"]);
    store.write(&Write::Current(None)).unwrap();
    assert_eq!(recent(&store), ["First", "Second"]);
    let (opened, conversation) = store.thread(&second.id).unwrap().unwrap();
    assert_eq!(opened, second);
    assert_eq!(shown(&conversation), ["Second", "Two."]);
}

/// An index that does not read is put aside, never written over, and the store goes on.
#[test]
fn an_index_that_does_not_read_is_kept_aside() {
    let machine = tempfile::tempdir().unwrap();
    let (_, store) = project(machine.path(), "night");
    fs::create_dir_all(&store.folder).unwrap();
    fs::write(store.folder.join(INDEX), "[1, 2").unwrap();
    let error = store.current().unwrap_err();
    assert!(error.contains("index.json.bad"), "{error}");
    assert_eq!(
        fs::read_to_string(store.folder.join(BAD_INDEX)).unwrap(),
        "[1, 2"
    );

    let thread = SavedThread::fresh();
    let mut conversation = Conversation::default();
    keep(
        &store,
        &thread,
        &mut conversation,
        "Hello",
        0,
        answered("Hi."),
    );
    assert_eq!(store.current().unwrap().unwrap().0, thread);
    assert_eq!(
        fs::read_to_string(store.folder.join(BAD_INDEX)).unwrap(),
        "[1, 2"
    );
}

/// One saved line of each event. The log is a file format: a line that changes here is a
/// thread saved before that no longer reads.
#[test]
fn every_event_saves_as_the_same_line() {
    let step = || StepId("toolu_1".to_string());
    let text = |text: &str| text.to_string();
    let events = [
        AgentEvent::TurnStarted,
        AgentEvent::TextDone {
            text: text("Done."),
        },
        AgentEvent::StepStarted {
            id: step(),
            title: text("Ran cargo build"),
            request_title: text("Run cargo build"),
            running_title: text("Running cargo build"),
        },
        AgentEvent::StepDone {
            id: step(),
            outcome: StepOutcome::Denied,
        },
        AgentEvent::ApprovalRequested {
            id: ApprovalId(text("request-1")),
            title: text("Run `cargo build`"),
        },
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: text("Killed"),
            },
        },
        AgentEvent::Error {
            message: text("Could not stop"),
        },
        AgentEvent::Exited {
            reason: ExitReason::SessionNotFound,
        },
    ];
    let mut lines = vec![
        serde_json::to_string(&Line::Sent {
            at: at(0),
            message: text("Hello"),
        })
        .unwrap(),
    ];
    for event in events {
        // A new event fails to compile here until it has a line below, or is not saved.
        match &event {
            AgentEvent::TurnStarted
            | AgentEvent::TextDone { .. }
            | AgentEvent::StepStarted { .. }
            | AgentEvent::StepDone { .. }
            | AgentEvent::ApprovalRequested { .. }
            | AgentEvent::TurnEnded { .. }
            | AgentEvent::Error { .. }
            | AgentEvent::Exited { .. } => {}
            AgentEvent::Started { .. } | AgentEvent::TextDelta { .. } => {
                unreachable!("not saved")
            }
        }
        let line = Line::of(&event, at(0)).unwrap();
        let saved = serde_json::to_string(&line).unwrap();
        assert_eq!(serde_json::from_str::<Line>(&saved).unwrap(), line);
        lines.push(saved);
    }
    let at = r#""at":{"secs_since_epoch":1800000000,"nanos_since_epoch":0}"#;
    let expected = [
        format!(r#"{{"sent":{{{at},"message":"Hello"}}}}"#),
        format!(r#"{{"event":{{{at},"event":"turn_started"}}}}"#),
        format!(r#"{{"event":{{{at},"event":{{"text_done":{{"text":"Done."}}}}}}}}"#),
        format!(
            r#"{{"event":{{{at},"event":{{"step_started":{{"id":"toolu_1","title":"Ran cargo build","running_title":"Running cargo build","request_title":"Run cargo build"}}}}}}}}"#
        ),
        format!(
            r#"{{"event":{{{at},"event":{{"step_done":{{"id":"toolu_1","outcome":"denied"}}}}}}}}"#
        ),
        format!(
            r#"{{"event":{{{at},"event":{{"approval_requested":{{"id":"request-1","title":"Run `cargo build`"}}}}}}}}"#
        ),
        format!(
            r#"{{"event":{{{at},"event":{{"turn_ended":{{"outcome":{{"failed":{{"message":"Killed"}}}}}}}}}}}}"#
        ),
        format!(r#"{{"event":{{{at},"event":{{"error":{{"message":"Could not stop"}}}}}}}}"#),
        format!(r#"{{"event":{{{at},"event":{{"exited":{{"reason":"session_not_found"}}}}}}}}"#),
    ];
    assert_eq!(lines, expected);
}
