//! The rules of the sidebar, on a project in a temporary folder with no window: a turn fed as
//! events with no process, the request it makes one undo step, approvals, the thread saved and
//! opened again, attachments, instructions, problems, going back and naming. A test waits for
//! what the background hands in, never for a fixed time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use smol::channel::Receiver;
use sound_core::{Engine, EngineConfig, GROUPING_WINDOW, HISTORY_FOLDER, Project, Registry};
use tempfile::TempDir;

use super::*;
use crate::jobs::testing;
use crate::{
    Account, AgentSettings, ApprovalAnswer, ApprovalId, ApprovalMode, Attachment, Command, Entry,
    ExitReason, Image, ImageFormat, Message, Model, SetupAction, StepId, StepKind, StepOutcome,
    TurnOutcome,
};

const MESSAGE: &str = "Add a bass line in bars 5 to 8 that follows the piano";
const LABEL: &str = "Add a bass line in bars 5 to 8 that…";
/// A record that loads, of the one tool of the test project.
const RECORD: &str = r#"{"tool": "test.note", "state": {}}"#;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Note {}

impl sound_core::State for Note {
    const TOOL: &'static str = "test.note";
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.tool::<Note>("test").unwrap();
    registry
}

fn nonexistent(name: &str) -> Installed {
    Installed {
        program: PathBuf::from(format!("/nonexistent/{name}")),
        environment: HashMap::new(),
    }
}

/// A chat on a project, with the machine's support folder. Claude Code is signed in with a
/// program that cannot start, so no test runs the real one.
struct Harness {
    chat: Chat,
    project: Project,
    _engine: Engine,
    woken: Receiver<()>,
    programs: Vec<(Provider, Installed)>,
    /// Last, so the folders outlive the project that holds their lock.
    folder: TempDir,
    machine: TempDir,
}

impl Harness {
    fn new() -> Self {
        Self::with(vec![(Provider::Claude, nonexistent("claude"))])
    }

    fn with(programs: Vec<(Provider, Installed)>) -> Self {
        let (machine, folder) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        Self::open(programs, machine, folder)
    }

    fn open(programs: Vec<(Provider, Installed)>, machine: TempDir, folder: TempDir) -> Self {
        let (control, engine) = Engine::new(EngineConfig::new(48_000, 1));
        let project = Project::open(folder.path(), registry(), control).unwrap();
        let (waker, woken) = testing::waker();
        let settings = AgentSettings::new(None, waker.clone());
        let agents = Agents::with_programs(programs.clone(), settings, waker.clone());
        let threads = machine.path().join("agent/threads");
        let chat = Chat::new(agents, &project, Some(threads), waker);
        let mut harness = Self {
            chat,
            project,
            _engine: engine,
            woken,
            programs,
            folder,
            machine,
        };
        harness.wait_until(|harness| harness.chat.loading.is_none());
        harness
    }

    /// The same project on the same machine, as quitting and starting the app gives, once
    /// every write is on disk.
    fn reopen(mut self) -> Self {
        self.wait_until(|harness| !harness.chat.is_saving());
        let Self {
            chat,
            project,
            programs,
            folder,
            machine,
            ..
        } = self;
        drop((chat, project));
        Self::open(programs, machine, folder)
    }

    fn poll(&mut self) {
        self.chat.poll(&mut self.project);
    }

    /// Takes what came until `done`.
    fn wait_until(&mut self, done: impl Fn(&Self) -> bool) {
        loop {
            self.poll();
            if done(self) {
                return;
            }
            testing::wait(&self.woken);
        }
    }

    fn connect(&mut self) -> (String, Receiver<Command>) {
        let (thread, commands) = Thread::without_agent(None);
        let session = thread.session_id().to_string();
        self.chat.connect(thread);
        (session, commands)
    }

    fn begin(&mut self, message: &str) {
        (self.chat).begin_at(message, Vec::new(), SystemTime::now(), &mut self.project);
    }

    fn receive(&mut self, events: impl IntoIterator<Item = AgentEvent>) {
        self.chat.receive(events, &mut self.project);
    }

    fn send(&mut self, text: &str) -> bool {
        self.chat.send(text, &mut self.project)
    }

    /// Writes a file as an agent does, and applies it as the watcher would, `after` the last
    /// end of a request. The chat hears what the project says of it.
    fn write_outside(&mut self, relative: &str, contents: &str, after: Duration) {
        let path = self.project.root().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        let at = Instant::now() + after;
        (self.project)
            .apply_outside_changes_at(std::slice::from_ref(&path), at)
            .unwrap();
        for event in self.project.drain_events() {
            self.chat.heard(&event, &self.project);
        }
    }

    fn entries(&self) -> String {
        format!("{:?}", self.chat.conversation().entries())
    }

    fn turn(&self, index: usize) -> TurnShown<'_> {
        (self.chat)
            .turn(index, &self.project, SystemTime::now())
            .expect("a turn")
    }
}

fn ended(outcome: TurnOutcome) -> AgentEvent {
    AgentEvent::TurnEnded { outcome }
}

fn completed() -> AgentEvent {
    ended(TurnOutcome::Completed)
}

fn failed(message: &str) -> AgentEvent {
    ended(TurnOutcome::Failed {
        message: message.to_string(),
    })
}

fn write_step(id: &str, path: &str) -> AgentEvent {
    AgentEvent::StepStarted {
        id: StepId(id.to_string()),
        title: format!("Wrote {path}"),
        request_title: format!("Write {path}"),
        command: None,
        running_title: format!("Writing {path}"),
        kind: StepKind::Change {
            path: path.to_string(),
        },
    }
}

fn sent(commands: &Receiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| commands.try_recv().ok()).collect()
}

fn sent_message(commands: &Receiver<Command>) -> Message {
    match commands.try_recv() {
        Ok(Command::Send(message)) => message,
        other => panic!("no message went: {other:?}"),
    }
}

#[test]
fn a_request_of_the_agent_is_one_undo_step_named_after_the_message() {
    let mut harness = Harness::new();
    let before = harness.project.undo_label();
    harness.begin(MESSAGE);
    harness.receive([
        AgentEvent::TurnStarted,
        write_step("write", "state/bass.json"),
    ]);
    // The writes of one request, seconds apart as an agent makes them.
    harness.write_outside("state/bass.json", "{}", Duration::ZERO);
    harness.receive([
        AgentEvent::StepDone {
            id: StepId("write".to_string()),
            outcome: StepOutcome::Done,
        },
        write_step("edit", "state/lead.json"),
    ]);
    harness.write_outside("state/lead.json", RECORD, Duration::from_secs(20));
    harness.receive([
        AgentEvent::TextDone {
            text: "Added a bass line.".to_string(),
        },
        completed(),
    ]);

    let [Entry::Message { text, .. }, Entry::Turn(turn)] = harness.chat.conversation().entries()
    else {
        panic!("{}", harness.entries());
    };
    assert_eq!(text, MESSAGE);
    assert_eq!(turn.blocks, ["Added a bass line."]);
    assert_eq!(harness.project.undo_label().as_deref(), Some(LABEL));
    harness.project.undo().unwrap();
    assert_eq!(harness.project.undo_label(), before);
    assert!(!harness.project.root().join("state/lead.json").exists());
}

#[test]
fn the_approval_row_answers_and_goes_when_the_turn_ends() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    harness.begin("Build the extension");
    let question = AgentEvent::ApprovalRequested {
        id: ApprovalId("question".to_string()),
        title: "Run `cargo build`".to_string(),
        command: Some("cargo build\n--release".to_string()),
    };
    harness.receive([AgentEvent::TurnStarted, question.clone()]);
    let shown = harness.turn(1);
    // While a question waits, the question is what the agent does.
    assert_eq!(shown.working, None);
    let Some(Below::Approval { words, command }) = shown.below else {
        panic!("no question shows");
    };
    assert_eq!(words.len(), 2);
    assert_eq!(command, Some("cargo build\n--release"));

    harness.chat.answer(ApprovalAnswer::Allow);
    let answered = ApprovalId("question".to_string());
    assert_eq!(
        sent(&commands),
        [Command::Answer(answered, ApprovalAnswer::Allow)]
    );
    assert_eq!(harness.turn(1).below, None);

    // A question still open when the turn ends is void.
    harness.receive([question]);
    harness.receive([ended(TurnOutcome::Interrupted)]);
    assert_eq!(harness.chat.conversation().approval(), None);
    assert_eq!(harness.turn(1).below, None);
}

#[test]
fn try_again_sends_the_message_of_a_turn_that_failed_and_keeps_what_is_attached() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    assert!(harness.send("Make the drums swing"));
    sent_message(&commands);
    assert_eq!(harness.turn(1).below, None);
    harness.receive([
        AgentEvent::TurnStarted,
        failed("Claude is busy. Try again in a moment."),
    ]);
    let meta = harness.turn(1).meta.unwrap();
    assert!(meta.failed);
    assert_eq!(meta.line, "Claude is busy. Try again in a moment.");
    assert_eq!(harness.turn(1).below, Some(Below::TryAgain));

    // A picture for the next message is in the composer.
    harness.chat.attach_picture(PNG.to_vec());
    assert!(harness.chat.try_again(&mut harness.project));
    let message = sent_message(&commands);
    assert_eq!(
        (message.text.as_str(), message.attachments.len()),
        ("Make the drums swing", 0)
    );
    assert_eq!(harness.chat.attachments().len(), 1);
    // The new turn works, so the failed one is no longer the last.
    assert_eq!(harness.turn(1).below, None);
}

#[test]
fn a_claude_that_does_not_start_is_a_quiet_line_and_no_request() {
    let mut harness = Harness::new();
    let undo_label = harness.project.undo_label();
    assert!(!harness.send("Hello"));
    let [Entry::Notice(line)] = harness.chat.conversation().entries() else {
        panic!("{}", harness.entries());
    };
    assert!(line.starts_with("Claude Code did not start"), "{line}");
    assert_eq!(harness.project.undo_label(), undo_label);
}

#[test]
fn with_no_agent_the_sidebar_says_why_and_has_no_composer() {
    let harness = Harness::with(Vec::new());
    let set_up = Shows::NoAgent("Set up an agent to start.".to_string());
    assert_eq!(harness.chat.shows(), set_up);
    assert!(!harness.chat.has_composer());
    assert_eq!(harness.chat.model_label(), NO_AGENT);
}

/// Sign out, in Settings, ends the agent. The turn it worked on ends as stopped, and its
/// request with it, so the sidebar is not left working.
#[test]
fn signing_out_mid_turn_stops_the_turn_and_its_request() {
    let mut harness = Harness::new();
    harness.begin(MESSAGE);
    harness.receive([AgentEvent::TurnStarted]);
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    let sign_out = SetupAction::SignOut(None);
    harness.chat.agents_mut().act(Provider::Claude, sign_out);
    harness.poll();

    assert!(!harness.chat.is_working());
    let outcome = harness.turn(1).meta.map(|meta| meta.line);
    assert!(outcome.unwrap().starts_with("Stopped after"));
    assert_eq!(harness.project.undo_label().as_deref(), Some(LABEL));
    assert!(!harness.chat.has_composer());
}

/// The process dies in the middle of a turn: the turn says so, the request ends there, and a
/// write after it is a step of its own.
#[test]
fn an_exit_mid_turn_ends_the_turn_and_its_request() {
    let mut harness = Harness::new();
    harness.begin(MESSAGE);
    harness.receive([AgentEvent::TurnStarted]);
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    let message = "Claude Code stopped: Killed".to_string();
    harness.receive([AgentEvent::Exited {
        reason: ExitReason::Failed {
            message: message.clone(),
        },
    }]);
    assert!(!harness.chat.is_working());
    assert_eq!(harness.turn(1).meta.map(|meta| meta.line), Some(message));
    assert_eq!(harness.project.undo_label().as_deref(), Some(LABEL));

    harness.write_outside("state/later.json", RECORD, GROUPING_WINDOW * 2);
    assert_eq!(harness.project.undo_label().as_deref(), Some("File change"));
}

/// **New** in the middle of a turn ends its request with the thread.
#[test]
fn a_new_thread_mid_turn_ends_the_request() {
    let mut harness = Harness::new();
    harness.begin(MESSAGE);
    harness.receive([AgentEvent::TurnStarted]);
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    harness.chat.new_thread(&mut harness.project);
    assert_eq!(harness.entries(), "[]");
    assert!(!harness.chat.is_working());
    harness.write_outside("state/later.json", RECORD, GROUPING_WINDOW * 2);
    assert_eq!(harness.project.undo_label().as_deref(), Some("File change"));
    harness.project.undo().unwrap();
    assert_eq!(harness.project.undo_label().as_deref(), Some(LABEL));
}

#[test]
fn stop_sends_the_interrupt_only_while_the_agent_works() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    harness.chat.stop();
    assert!(harness.send("Hello"));
    assert!(harness.chat.is_working());
    harness.chat.stop();
    assert_eq!(
        sent(&commands),
        [Command::Send("Hello".into()), Command::Interrupt]
    );
}

/// The window closes and the project opens again on the same machine: the thread shows as it
/// was, and the next message resumes the agent's session.
#[test]
fn a_thread_opens_again_as_it_was_and_resumes_its_session() {
    let mut harness = Harness::new();
    assert_eq!(harness.chat.resume(), None);
    // The agent's session is known as it starts, before it says anything.
    let (session, _commands) = harness.connect();
    harness.begin(MESSAGE);
    harness.receive([
        AgentEvent::TurnStarted,
        write_step("write", "state/bass.json"),
        AgentEvent::TextDone {
            text: "Added a bass line.".to_string(),
        },
        completed(),
    ]);
    let shown = harness.entries();
    assert_eq!(harness.chat.resume().as_ref(), Some(&session));

    let mut harness = harness.reopen();
    assert_eq!(harness.entries(), shown);
    assert_eq!(harness.chat.resume().as_ref(), Some(&session));
    // A thread keeps its provider, also opened again.
    assert_eq!(harness.chat.thread_provider(), Some(Provider::Claude));

    let (thread, commands) = Thread::without_agent(harness.chat.resume());
    harness.chat.connect(thread);
    assert!(harness.send("Which file was it?"));
    assert_eq!(sent_message(&commands).text, "Which file was it?");
    let [
        Entry::Message { .. },
        Entry::Turn(_),
        Entry::Message { text, .. },
        Entry::Turn(_),
    ] = harness.chat.conversation().entries()
    else {
        panic!("{}", harness.entries());
    };
    assert_eq!(text, "Which file was it?");
}

/// One red pixel.
const PNG: [u8; 69] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// A picture pasted and files dropped on the composer go with the next message: a picture as
/// itself, any other file as its path. One taken out stays behind. The message shows their
/// names, also when the thread opens again.
#[test]
fn pasted_and_dropped_files_go_with_the_next_message() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    let outside = tempfile::tempdir().unwrap();
    let [score, take, notes] = ["Score.PNG", "Take 3.wav", "notes.pdf"].map(|name| {
        let path = outside.path().join(name);
        std::fs::write(&path, PNG).unwrap();
        path
    });
    harness.chat.attach_picture(PNG.to_vec());
    harness.chat.attach_files(vec![score, take.clone(), notes]);
    harness.wait_until(|harness| !harness.chat.is_reading_attachments());
    let chips: Vec<_> = (harness.chat.attachments().iter())
        .map(|attached| (attached.name.as_str(), attached.picture().is_some()))
        .collect();
    assert_eq!(
        chips,
        [
            ("Pasted image", true),
            ("Score.PNG", true),
            ("Take 3.wav", false),
            ("notes.pdf", false)
        ]
    );
    harness.chat.remove_attachment(3);
    assert!(harness.send("Make the bass follow this"));

    let message = sent_message(&commands);
    assert_eq!(message.text, "Make the bass follow this");
    let picture = Attachment::Image(Image {
        format: ImageFormat::Png,
        bytes: PNG.to_vec(),
    });
    assert_eq!(
        message.attachments,
        [picture.clone(), picture, Attachment::File(take)]
    );
    assert!(harness.chat.attachments().is_empty());
    let [Entry::Message { attachments, .. }, Entry::Turn(_)] =
        harness.chat.conversation().entries()
    else {
        panic!("{}", harness.entries());
    };
    assert_eq!(attachments, &["Pasted image", "Score.PNG", "Take 3.wav"]);
    harness.receive([AgentEvent::TurnStarted, completed()]);
    let shown = harness.entries();
    assert_eq!(harness.reopen().entries(), shown);
}

/// A message sent while a dropped file is still read waits for it, so the file never goes
/// with the next message instead.
#[test]
fn a_message_waits_for_a_file_still_being_read() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    let outside = tempfile::tempdir().unwrap();
    let take = outside.path().join("take.wav");
    std::fs::write(&take, "").unwrap();
    harness.chat.attach_files(vec![take.clone()]);
    let text = "Tune the bass to this";
    assert!(!harness.chat.can_send(text));
    assert!(!harness.send(text), "it went before the file was in");
    harness.wait_until(|harness| harness.chat.can_send(text));
    assert!(harness.send(text));
    let message = Message {
        text: text.to_string(),
        attachments: vec![Attachment::File(take)],
        note: None,
    };
    assert_eq!(sent(&commands), [Command::Send(message)]);
}

/// The thread the chat shows, by its id in the index of the project's threads.
fn current_thread(machine: &Path) -> String {
    let projects = std::fs::read_dir(machine.join("agent/threads")).unwrap();
    let [project] = projects.collect::<Vec<_>>().try_into().unwrap();
    let index = std::fs::read_to_string(project.unwrap().path().join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    index["current"].as_str().unwrap().to_string()
}

/// A thread left with **New** is in **Open recent**, and opens again from there.
#[test]
fn a_thread_left_with_new_opens_again_from_open_recent() {
    let mut harness = Harness::new();
    harness.begin(MESSAGE);
    let answer = AgentEvent::TextDone {
        text: "Added a bass line.".to_string(),
    };
    harness.receive([AgentEvent::TurnStarted, answer, completed()]);
    let shown = harness.entries();
    harness.wait_until(|harness| !harness.chat.is_saving());
    let thread = current_thread(harness.machine.path());
    harness.chat.new_thread(&mut harness.project);
    assert_eq!(harness.entries(), "[]");

    let recent = harness.chat.recent_threads();
    assert_eq!(
        (recent[0].id.as_str(), recent[0].title.as_str()),
        (thread.as_str(), LABEL)
    );
    harness.chat.open_thread(&thread, &mut harness.project);
    harness.wait_until(|harness| harness.chat.opening.is_none() && !harness.chat.is_saving());
    assert_eq!(harness.entries(), shown);
    assert_eq!(current_thread(harness.machine.path()), thread);
    assert!(harness.chat.recent_threads().is_empty());
}

/// The project's instructions are typed in the sidebar and saved in the project folder, where
/// every agent reads them. An empty text removes the file.
#[test]
fn instructions_typed_in_the_sidebar_are_saved_in_the_project() {
    let mut harness = Harness::new();
    let file = harness.project.root().join("instructions.md");
    let open = |harness: &mut Harness| {
        harness.chat.open_instructions(Scope::Project);
        assert_eq!(harness.chat.shows(), Shows::Instructions);
        harness.wait_until(|harness| {
            (harness.chat.instructions()).is_some_and(|instructions| instructions.text().is_some())
        });
    };

    open(&mut harness);
    // Nothing to save until the composer types.
    assert!(!harness.chat.is_saving());
    harness
        .chat
        .edit_instructions("Never automate the volume directly.");
    harness
        .chat
        .edit_instructions("Never automate the volume directly.\nUse a utility.\n");
    harness.chat.close_instructions();
    assert!(harness.chat.has_composer());
    harness.wait_until(|harness| !harness.chat.is_saving());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Never automate the volume directly.\nUse a utility.\n"
    );

    open(&mut harness);
    let text = harness.chat.instructions().and_then(Instructions::text);
    assert_eq!(
        text,
        Some("Never automate the volume directly.\nUse a utility.")
    );
    harness.chat.edit_instructions("");
    harness.chat.close_instructions();
    harness.wait_until(|harness| !harness.chat.is_saving());
    assert!(!file.exists());
    // With no support folder, there are no instructions for every project.
    assert!(!harness.chat.can_open_instructions(Scope::AllProjects));
}

/// The agent no longer has the session: the thread stays to read, also when the project opens
/// again, and only **New** goes on.
#[test]
fn a_lost_session_ends_the_thread_until_new() {
    let mut harness = Harness::new();
    harness.begin("Hello");
    harness.receive([
        AgentEvent::TurnStarted,
        failed("No conversation found with session ID: gone"),
        AgentEvent::Exited {
            reason: ExitReason::SessionNotFound,
        },
    ]);
    // The turn says it already.
    let lost = Shows::Thread(Footer::CannotContinue { say: None });
    assert_eq!(harness.chat.shows(), lost);
    assert!(!harness.chat.has_composer());
    // No "Try again" for a thread that can't go on.
    assert_eq!(harness.turn(1).below, None);

    let mut harness = harness.reopen();
    assert_eq!(harness.chat.shows(), lost);
    harness.chat.new_thread(&mut harness.project);
    assert!(harness.chat.has_composer());
    assert_eq!(harness.entries(), "[]");
    assert_eq!(harness.chat.resume(), None);

    // Opened again before a message: the new, empty thread, not the old one.
    let harness = harness.reopen();
    assert!(harness.chat.has_composer());
    assert_eq!(harness.entries(), "[]");
}

/// A turn that leaves a file not live says so under its answer, and the line opens to why. A
/// turn that fixes it says nothing, and a problem that was there before is no news.
#[test]
fn a_turn_shows_the_problems_it_left() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    harness.begin("Add a bass line");
    harness.receive([AgentEvent::TurnStarted]);
    harness.write_outside("state/bass.json", "{", Duration::ZERO);
    harness.receive([completed()]);
    let Some(Below::Problems { line, lines, fix }) = harness.turn(1).below else {
        panic!("no problems show");
    };
    assert_eq!(
        (line.as_str(), lines, fix),
        ("1 file is not live", None, true)
    );
    harness.chat.toggle_problems(1);
    let Some(Below::Problems { lines, .. }) = harness.turn(1).below else {
        panic!("no problems show");
    };
    let lines = lines.unwrap();
    assert!(lines[0].starts_with("Bass: "), "{lines:?}");

    // One click asks the agent to fix them, as if typed; the turn that works on it is the
    // last now, so the way to ask goes.
    assert!(harness.chat.ask(FIX_NOT_LIVE, &mut harness.project));
    assert_eq!(sent_message(&commands).text, FIX_NOT_LIVE);
    assert!(matches!(
        harness.turn(1).below,
        Some(Below::Problems { fix: false, .. })
    ));
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    harness.receive([completed()]);
    assert_eq!(harness.turn(3).below, None);
    // The first turn still says what it left.
    assert!(matches!(
        harness.turn(1).below,
        Some(Below::Problems { .. })
    ));
}

/// The window's list of files that are not live asks the agent too: the thread shows, over
/// the instructions.
#[test]
fn asking_from_the_window_shows_the_thread_over_the_instructions() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    harness.chat.open_instructions(Scope::Project);
    assert!(harness.chat.ask(FIX_NOT_LIVE, &mut harness.project));
    assert_eq!(sent_message(&commands).text, FIX_NOT_LIVE);
    assert!(harness.chat.instructions().is_none());
}

/// The watcher applies a write after its grouping window, so the last write of a turn can
/// come just after the turn ended. Its problem is the turn's; one heard later is not.
#[test]
fn a_write_heard_just_after_the_turn_is_the_turns() {
    let mut harness = Harness::new();
    harness.begin("Add a bass line");
    harness.receive([AgentEvent::TurnStarted, completed()]);
    assert_eq!(harness.turn(1).below, None);
    harness.write_outside("state/bass.json", "{", Duration::ZERO);
    harness.chat.toggle_problems(1);
    let lines = |harness: &Harness| match harness.turn(1).below {
        Some(Below::Problems { lines, .. }) => lines.map_or(0, |lines| lines.len()),
        _ => 0,
    };
    assert_eq!(lines(&harness), 1);

    // As heard later than the grouping window.
    let (index, ended) = harness.chat.just_ended.unwrap();
    let long_ago = ended.checked_sub(GROUPING_WINDOW * 2).unwrap();
    harness.chat.just_ended = Some((index, long_ago));
    harness.write_outside("state/drums.json", "{", Duration::ZERO);
    assert_eq!(lines(&harness), 1);
}

/// Up in an empty composer recalls the earlier messages of the thread, newest first; down
/// goes forward again and back to empty. A thread opened again recalls its messages too.
#[test]
fn up_in_the_composer_recalls_earlier_messages() {
    let mut harness = Harness::new();
    for message in ["Add a bass line", "Make it louder"] {
        harness.begin(message);
        harness.receive([completed()]);
    }
    let recall = |harness: &mut Harness, text: &str, arrow| harness.chat.recall(text, arrow);
    assert_eq!(
        recall(&mut harness, "", Arrow::Up).as_deref(),
        Some("Make it louder")
    );
    let up = recall(&mut harness, "Make it louder", Arrow::Up);
    assert_eq!(up.as_deref(), Some("Add a bass line"));
    let down = recall(&mut harness, "Add a bass line", Arrow::Down);
    assert_eq!(down.as_deref(), Some("Make it louder"));
    assert_eq!(
        recall(&mut harness, "Make it louder", Arrow::Down).as_deref(),
        Some("")
    );

    let mut harness = harness.reopen();
    assert_eq!(
        recall(&mut harness, "", Arrow::Up).as_deref(),
        Some("Make it louder")
    );
}

fn model(id: &str) -> Model {
    Model {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        short_name: id.to_string(),
        source: None,
    }
}

/// The approval mode and the model picked go to the running agent at once, also while a turn
/// runs.
#[test]
fn the_menu_settings_go_to_the_agent_at_once() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    harness.receive([AgentEvent::Started {
        account: Some(Account::default()),
        models: vec![model("default"), model("haiku")],
    }]);
    assert_eq!(harness.chat.model_label(), "default");

    harness.chat.set_approval_mode(ApprovalMode::NeverAsk);
    harness.chat.pick_model(Provider::Claude, "haiku");
    assert_eq!(
        sent(&commands),
        [
            Command::SetApprovalMode(ApprovalMode::NeverAsk),
            Command::SetModel("haiku".to_string()),
        ]
    );
    assert_eq!(harness.chat.model_label(), "haiku");
    let checked: Vec<_> = (harness.chat.access_choices().iter())
        .filter(|choice| choice.checked)
        .map(|choice| choice.mode.label())
        .collect();
    assert_eq!(checked, ["Full access"]);

    assert!(harness.send("Hello"));
    assert!(harness.chat.is_working());
    harness
        .chat
        .set_approval_mode(ApprovalMode::AskForEverything);
    assert_eq!(
        sent(&commands),
        [
            Command::Send("Hello".into()),
            Command::SetApprovalMode(ApprovalMode::AskForEverything)
        ]
    );
}

/// The picker lists every provider signed in. A thread keeps its provider: once its agent
/// started, the others are off, and say why.
#[test]
fn a_thread_keeps_its_provider_in_the_model_picker() {
    let pi = (Provider::Pi, nonexistent("pi"));
    let mut harness = Harness::with(vec![(Provider::Claude, nonexistent("claude")), pi]);
    let settings = harness.chat.agents_mut().settings_mut();
    settings.set_models(Provider::Claude, vec![model("haiku")]);
    settings.set_models(
        Provider::Pi,
        vec![model("openrouter/kimi"), model("gemini")],
    );
    let groups = harness.chat.model_groups("kimi");
    let listed: Vec<_> = (groups.iter())
        .map(|group| (group.provider, group.models.len(), group.empty))
        .collect();
    assert_eq!(
        listed,
        [
            (Provider::Claude, 0, Some("No model matches.")),
            (Provider::Pi, 1, None)
        ]
    );

    harness.connect();
    let groups = harness.chat.model_groups("");
    assert_eq!(groups[0].off, None);
    assert_eq!(groups[0].models[0].id, "haiku");
    assert!(groups[0].models[0].picked, "the default is first");
    assert_eq!(
        groups[1].off.as_deref(),
        Some("Pi: start a new thread to switch")
    );
    assert!(groups[1].models.is_empty());
    harness.chat.pick_model(Provider::Pi, "gemini");
    assert_eq!(
        harness
            .chat
            .agents()
            .settings()
            .settings()
            .model(Provider::Pi),
        None
    );
}

impl Harness {
    fn current_point(&self) -> sound_core::PointId {
        self.project.history().unwrap().current()
    }

    fn go_to(&mut self, point: sound_core::PointId) {
        self.project.go_to(point).unwrap();
        for event in self.project.drain_events() {
            self.chat.heard(&event, &self.project);
        }
    }
}

/// Going back in the history shows where to above the composer, and the agent reads it with
/// the next message, and only that one. The thread does not show it.
#[test]
fn the_message_after_going_back_tells_the_agent_where_to() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    let start = harness.current_point();
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    harness.go_to(start);
    let line = harness
        .chat
        .went_back_line()
        .expect("no line says where to");
    assert!(line.starts_with("Went back to “"), "{line}");

    assert!(harness.send("Make the drums swing"));
    let note = sent_message(&commands)
        .note
        .expect("no note went with the message");
    let folder = harness.project.root().join(HISTORY_FOLDER);
    for said in [start.short(), folder.display().to_string()] {
        assert!(note.contains(&said), "{said:?} is not in {note:?}");
    }
    assert_eq!(harness.chat.went_back_line(), None);
    assert!(!harness.entries().contains("went back"));

    harness.receive([completed()]);
    assert!(harness.send("And the bass"));
    assert_eq!(sent_message(&commands).note, None);
}

/// The × takes the line away, and no note goes.
#[test]
fn a_dismissed_going_back_tells_the_agent_nothing() {
    let mut harness = Harness::new();
    let (_, commands) = harness.connect();
    let start = harness.current_point();
    harness.write_outside("state/bass.json", RECORD, Duration::ZERO);
    harness.go_to(start);
    harness.chat.dismiss_went_back();
    assert_eq!(harness.chat.went_back_line(), None);
    assert!(harness.send("Hello"));
    assert_eq!(sent_message(&commands).note, None);
}

/// A stand-in for `claude` that answers every question with "Name N", N counting up from 1, and
/// keeps question N in `<program>.asked-N`.
#[cfg(unix)]
fn naming_claude(machine: &Path) -> Installed {
    use std::os::unix::fs::PermissionsExt;
    let program = machine.join("claude");
    let script = r#"#!/bin/sh
count=$(($(cat "$0.count" 2>/dev/null || echo 0) + 1))
echo $count >"$0.count"
cat >"$0.asked-$count"
echo "\"Name $count.\""
"#;
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    Installed {
        program,
        environment: HashMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
    }
}

#[cfg(unix)]
impl Harness {
    /// The name of every version, in the order of the history's lines.
    fn version_names(&self) -> Vec<Option<String>> {
        let lines = self.project.history().unwrap().lines();
        (lines.iter().filter(|line| line.is_version()))
            .map(|line| line.name.clone())
            .collect()
    }

    fn wait_for_names(&mut self, names: &[Option<&str>]) {
        let names: Vec<Option<String>> = names.iter().map(|name| name.map(String::from)).collect();
        self.wait_until(|harness| harness.version_names() == names);
    }
}

/// The small model names the versions one at a time once the history is still: the current
/// one first, then the newest. A version is named again when it grows, but never over a name
/// given by hand.
#[cfg(unix)]
#[test]
fn the_small_model_names_versions_once_they_are_still() {
    let machine = tempfile::tempdir().unwrap();
    let installed = naming_claude(machine.path());
    let asked = |count: usize| {
        let file = format!("{}.asked-{count}", installed.program.display());
        std::fs::read_to_string(file).ok()
    };
    let programs = vec![(Provider::Claude, installed.clone())];
    let mut harness = Harness::open(programs, machine, tempfile::tempdir().unwrap());
    // Nothing is asked while the history changes.
    harness.chat.namer.quiet_for = Duration::from_secs(3600);
    let start = harness.current_point();
    harness.write_outside("state/bass.json", RECORD, Duration::from_secs(20));
    harness.go_to(start);
    harness.write_outside("state/lead.json", RECORD, Duration::from_secs(40));
    harness.poll();
    assert!(harness.chat.namer.asking.is_none());
    assert_eq!(harness.version_names(), [None, None]);

    harness.chat.namer.quiet_for = Duration::ZERO;
    harness.chat.wait_for_quiet(&harness.project);
    harness.wait_for_names(&[None, Some("Name 1")]);
    assert!(asked(1).unwrap().contains("state/lead.json (new)"));
    harness.wait_for_names(&[Some("Name 2"), Some("Name 1")]);

    harness.write_outside("state/lead.json", "{}", Duration::from_secs(60));
    harness.wait_for_names(&[Some("Name 2"), Some("Name 3")]);
    assert!(asked(3).unwrap().contains("state/lead.json"));

    let history = harness.project.history().unwrap();
    let current = history.line_of(history.current()).unwrap().unwrap().start;
    harness.project.rename_version(current, "My take").unwrap();
    harness.write_outside("state/lead.json", RECORD, Duration::from_secs(80));
    // Once the quiet wait of this change ended, nothing is asked.
    harness.poll();
    harness.wait_until(|harness| harness.chat.namer.quiet.is_none());
    assert!(harness.chat.namer.asking.is_none());
    assert!(asked(4).is_none(), "asked about a version named by hand");
    assert_eq!(
        harness.version_names(),
        [Some("Name 2".to_string()), Some("My take".to_string())]
    );
}

/// A thread fed recorded events with no agent, as the window's pictures show one: the working
/// line says the running step in the composer's words, and the steps name parts, not paths.
#[test]
fn a_thread_fed_recorded_events_shows_its_steps_and_working_line() {
    let mut harness = Harness::new();
    let started = SystemTime::now() - Duration::from_secs(47);
    (harness.chat).begin_at(
        "Add a clip",
        vec!["Score.png".into()],
        started,
        &mut harness.project,
    );
    harness.receive([
        AgentEvent::TurnStarted,
        write_step("write", "state/bass.json"),
    ]);
    let working = harness.turn(1).working.unwrap();
    assert_eq!(
        (working.icon, working.words.as_str()),
        (Some("pencil"), "Changing the project")
    );
    assert_eq!(working.elapsed, "47 s");
    harness.receive([
        AgentEvent::StepDone {
            id: StepId("write".to_string()),
            outcome: StepOutcome::Denied,
        },
        completed(),
    ]);
    let meta = harness.turn(1).meta.unwrap();
    assert!(meta.opens && meta.line.starts_with("Worked for"));
    harness.chat.toggle_steps(1);
    let steps = harness.turn(1).steps.unwrap();
    // A denied step says what it asked, by the name of the part.
    assert_eq!(steps[0].words, "Change Bass");
    assert_eq!(steps[0].mark.map(Mark::words), Some("· denied"));
}
