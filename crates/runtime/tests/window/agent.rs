//! The agent sidebar in the left panel: a turn fed as events with no process, the request it
//! makes one undo step, the approval row, cmd-L and escape, the panel remembered closed, and
//! the thread saved and opened again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{AppContext, Entity, Focusable, TestAppContext};
use runtime::window::{LeftPanel, LeftPanelSlot};
use runtime::{OFFLINE, open_or_create};
use sound_agent::{
    Account, AgentEvent, AgentSettings, ApprovalId, ApprovalMode, Command, Entry, ExitReason,
    Installed, Model, Sidebar, StepId, StepOutcome, Thread, TurnOutcome,
};
use sound_core::Engine;
use tempfile::TempDir;

use crate::support::{self, Opened, mark, one_undo_step, write_outside};

/// The sidebar with a `claude` that cannot start, so no test ever runs the real one: a test
/// hands the sidebar the events instead. `support` is the support folder of the machine.
fn install_sidebar(cx: &mut TestAppContext, support: &Path) -> Entity<AgentSettings> {
    install_sidebar_with(cx, support, Some(nonexistent_claude()))
}

fn nonexistent_claude() -> Installed {
    Installed {
        program: PathBuf::from("/nonexistent/claude"),
        environment: HashMap::new(),
    }
}

/// The same with `installed` as the program, or not installed. The threads are kept in
/// `support`. Every window shares the settings it gives, which are not saved.
fn install_sidebar_with(
    cx: &mut TestAppContext,
    support: &Path,
    installed: Option<Installed>,
) -> Entity<AgentSettings> {
    let remembered = runtime::app::left_panel_file(support);
    let threads = runtime::app::threads_folder(support);
    cx.update(|cx| {
        let settings = cx.new(|cx| AgentSettings::new(None, cx));
        let shared = settings.clone();
        LeftPanelSlot::new(Some(remembered), move |session, _, cx| {
            let (installed, threads) = (installed.clone(), Some(threads.clone()));
            let settings = shared.clone();
            let sidebar =
                cx.new(|cx| Sidebar::with_program(session, installed, threads, settings, cx));
            LeftPanel::new(sidebar, Sidebar::is_busy, cx)
        })
        .install(cx);
        settings
    })
}

/// The same project folder in a new window, as quitting and starting the app gives.
fn open_again(cx: &mut TestAppContext, folder: TempDir) -> Opened<'_> {
    let (control, engine) = Engine::new(OFFLINE);
    let (project, plugins) = open_or_create(folder.path(), control).unwrap();
    let opened = support::open_project(cx, folder, project, engine, plugins.downgrade());
    // The sidebar reads the saved thread in the background.
    opened.cx.run_until_parked();
    opened
}

impl Opened<'_> {
    fn sidebar(&mut self) -> Entity<Sidebar> {
        let shell = self.shell.clone();
        let view = self.cx.read(|cx| shell.read(cx).left_panel().cloned());
        view.unwrap().downcast::<Sidebar>().ok().unwrap()
    }

    fn panel_open(&mut self) -> bool {
        let shell = self.shell.clone();
        self.cx.read(|cx| shell.read(cx).left_panel_open())
    }

    fn composer_focused(&mut self) -> bool {
        let sidebar = self.sidebar();
        self.cx
            .update(|window, cx| sidebar.focus_handle(cx).is_focused(window))
    }

    fn begin(&mut self, message: &str) {
        let sidebar = self.sidebar();
        self.cx
            .update(|_, cx| sidebar.update(cx, |sidebar, cx| sidebar.begin(message, cx)));
        self.cx.run_until_parked();
    }

    fn receive(&mut self, events: impl IntoIterator<Item = AgentEvent>) {
        let sidebar = self.sidebar();
        self.cx
            .update(|_, cx| sidebar.update(cx, |sidebar, cx| sidebar.receive(events, cx)));
        self.cx.run_until_parked();
    }
}

fn step(id: &str) -> StepId {
    StepId(id.to_string())
}

const MESSAGE: &str = "Add a bass line in bars 5 to 8 that follows the piano";
const LABEL: &str = "Add a bass line in bars 5 to 8 that…";

/// A clip in the form the runtime writes, so a redo gives the same bytes back.
const BASS_CLIP: &str = "{\n  \"tool\": \"arrangement.clip\",\n  \"state\": {\n    \"start\": 15360,\n    \"length\": 15360,\n    \"notes\": [{\"start\": 0, \"length\": 960, \"pitch\": 36, \"velocity\": 100}]\n  }\n}\n";

/// An empty clip in bar 1, written after the request.
const LATER_CLIP: &str =
    r#"{"tool": "arrangement.clip", "state": {"start": 0, "length": 3840, "notes": []}}"#;

/// Longer than the grouping window after the end of a request, so a write after it is no part
/// of the request.
fn after_the_request() {
    std::thread::sleep(Duration::from_millis(150));
}

#[gpui::test]
fn a_request_of_the_agent_is_one_undo_step_named_after_the_message(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let before = mark(&mut opened);

    opened.begin(MESSAGE);
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::StepStarted {
            id: step("write"),
            title: "Wrote state/arrangement/track-1/bass.json".to_string(),
            request_title: "Write state/arrangement/track-1/bass.json".to_string(),
            running_title: "Writing state/arrangement/track-1/bass.json".to_string(),
        },
    ]);
    assert!(opened.panel_open());
    // The writes of one request, seconds apart as an agent makes them: the watcher hears each.
    write_outside(
        &mut opened,
        "state/arrangement/track-1/bass.json",
        r#"{"tool": "arrangement.clip", "state": {"start": 15360, "length": 15360, "notes": []}}"#,
    );
    opened.receive([
        AgentEvent::StepDone {
            id: step("write"),
            outcome: StepOutcome::Done,
        },
        AgentEvent::StepStarted {
            id: step("edit"),
            title: "Edited state/arrangement/track-1/bass.json".to_string(),
            request_title: "Edit state/arrangement/track-1/bass.json".to_string(),
            running_title: "Editing state/arrangement/track-1/bass.json".to_string(),
        },
    ]);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/bass.json",
        BASS_CLIP,
    );
    opened.receive([
        AgentEvent::StepDone {
            id: step("edit"),
            outcome: StepOutcome::Done,
        },
        AgentEvent::TextDelta {
            text: "Added a bass".to_string(),
        },
        AgentEvent::TextDelta {
            text: " line.".to_string(),
        },
        AgentEvent::TextDone {
            text: "Added a bass line.".to_string(),
        },
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let conversation = sidebar.read(cx).conversation();
        assert!(!conversation.is_working());
        let [Entry::Message(message), Entry::Turn(turn)] = conversation.entries() else {
            panic!("{:?}", conversation.entries());
        };
        assert_eq!(message, MESSAGE);
        assert_eq!(turn.blocks, ["Added a bass line."]);
        let steps: Vec<_> = turn.steps.iter().map(|step| step.title.as_str()).collect();
        assert_eq!(
            steps,
            [
                "Wrote state/arrangement/track-1/bass.json",
                "Edited state/arrangement/track-1/bass.json"
            ]
        );
        let outcome = turn.end.as_ref().map(|end| &end.outcome);
        assert_eq!(outcome, Some(&TurnOutcome::Completed));
    });
    one_undo_step(&mut opened, LABEL, &before);
}

#[gpui::test]
fn the_approval_row_answers_and_goes_when_the_turn_ends(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let sidebar = opened.sidebar();
    let approval = |opened: &mut Opened<'_>| {
        opened.cx.read(|cx| {
            let conversation = sidebar.read(cx).conversation();
            conversation
                .approval()
                .map(|approval| approval.title.clone())
        })
    };

    opened.begin("Build the extension");
    let question = AgentEvent::ApprovalRequested {
        id: ApprovalId("question".to_string()),
        title: "Run `cargo build`".to_string(),
    };
    opened.receive([AgentEvent::TurnStarted, question.clone()]);
    assert_eq!(approval(&mut opened).as_deref(), Some("Run `cargo build`"));
    let allow = opened.control("approval-allow");
    opened.click(allow);
    assert_eq!(approval(&mut opened), None);
    assert!(opened.find("approval-allow").is_none());

    // A question still open when the turn ends is void.
    opened.receive([question]);
    assert!(opened.find("approval-deny").is_some());
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Interrupted,
    }]);
    assert_eq!(approval(&mut opened), None);
    assert!(opened.find("approval-deny").is_none());
}

#[gpui::test]
fn a_claude_that_does_not_start_is_a_quiet_line_and_no_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let undo_label = opened.undo_label();
    opened.keys("cmd-l");
    assert!(opened.composer_focused());
    opened.cx.simulate_input("Hello");
    opened.keys("enter");

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let entries = sidebar.read(cx).conversation().entries();
        let [Entry::Notice(line)] = entries else {
            panic!("{entries:?}");
        };
        assert!(line.starts_with("Claude Code did not start"), "{line}");
    });
    assert_eq!(opened.undo_label(), undo_label);
}

#[gpui::test]
fn the_panel_stays_closed_once_closed_and_cmd_l_opens_it_on_the_composer(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    // Open at the first start.
    assert!(opened.panel_open());
    let timeline = opened.timeline.clone();
    opened
        .cx
        .update(|window, cx| window.focus(&timeline.focus_handle(cx), cx));

    // cmd-L focuses the composer of the open panel, escape gives the focus back.
    opened.keys("cmd-l");
    assert!(opened.composer_focused());
    opened.keys("escape");
    assert!(opened.panel_open());
    assert!(
        opened
            .cx
            .update(|window, cx| timeline.focus_handle(cx).is_focused(window))
    );

    // cmd-L again in the composer closes it, and the focus goes back.
    opened.keys("cmd-l");
    opened.keys("cmd-l");
    assert!(!opened.panel_open());
    assert!(!opened.composer_focused());
    drop(timeline);
    drop(opened.close());

    // Another window, on another project: closed, as the machine remembers it.
    let mut opened = support::open_with(cx, |_| {});
    assert!(!opened.panel_open());
    opened.keys("cmd-l");
    assert!(opened.panel_open());
    assert!(opened.composer_focused());
}

/// The account menu sits in the composer and offers **Sign out**.
#[gpui::test]
fn the_account_menu_in_the_composer_offers_sign_out(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    assert!(opened.find("menu-sign-out").is_none());
    let menu = opened.control("account-menu");
    opened.click(menu);
    assert!(opened.find("menu-sign-out").is_some());
}

/// Sign out ends the agent. The turn it worked on ends as stopped, and its request with it,
/// so the sidebar is not left working.
#[gpui::test]
fn signing_out_mid_turn_stops_the_turn_and_its_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let before = mark(&mut opened);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/bass.json",
        BASS_CLIP,
    );

    let menu = opened.control("account-menu");
    opened.click(menu);
    let sign_out = opened.control("menu-sign-out");
    opened.click(sign_out);

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let conversation = sidebar.read(cx).conversation();
        assert!(!conversation.is_working());
        let Some(Entry::Turn(turn)) = conversation.entries().get(1) else {
            panic!("{:?}", conversation.entries());
        };
        let outcome = turn.end.as_ref().map(|end| &end.outcome);
        assert_eq!(outcome, Some(&TurnOutcome::Interrupted));
    });
    assert!(opened.find("left-panel-busy").is_none());
    one_undo_step(&mut opened, "Add a bass line in bars 5 to 8 that…", &before);
}

/// While the panel is closed and the agent works, the icon in the title row says so.
#[gpui::test]
fn a_closed_panel_shows_the_agent_working_on_its_icon(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let icon = opened.control("left-panel-icon");
    opened.click(icon);
    assert!(!opened.panel_open());
    assert!(opened.find("left-panel-busy").is_none());
    opened.begin("Add a clip");
    assert!(opened.find("left-panel-busy").is_some());
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    assert!(opened.find("left-panel-busy").is_none());
}

/// With no composer, the sidebar itself takes the focus, so cmd-L still closes it.
#[gpui::test]
fn with_no_claude_cmd_l_focuses_the_sidebar_and_closes_it_again(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar_with(cx, machine.path(), None);
    let mut opened = support::open_with(cx, |_| {});
    assert!(opened.panel_open());
    opened.keys("cmd-l");
    assert!(opened.composer_focused());
    opened.keys("cmd-l");
    assert!(!opened.panel_open());
}

/// The process dies in the middle of a turn: the turn says so, the request ends there, and
/// what the agent wrote before is still one undo step.
#[gpui::test]
fn an_exit_mid_turn_ends_the_turn_and_its_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let before = mark(&mut opened);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/bass.json",
        BASS_CLIP,
    );
    let message = "Claude Code stopped: Killed".to_string();
    opened.receive([AgentEvent::Exited {
        reason: ExitReason::Failed {
            message: message.clone(),
        },
    }]);

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let conversation = sidebar.read(cx).conversation();
        assert!(!conversation.is_working());
        let [Entry::Message(_), Entry::Turn(turn)] = conversation.entries() else {
            panic!("{:?}", conversation.entries());
        };
        let outcome = turn.end.as_ref().map(|end| &end.outcome);
        assert_eq!(outcome, Some(&TurnOutcome::Failed { message }));
    });
    one_undo_step(&mut opened, LABEL, &before);

    after_the_request();
    write_outside(
        &mut opened,
        "state/arrangement/track-1/later.json",
        LATER_CLIP,
    );
    assert_eq!(opened.undo_label().as_deref(), Some("File change"));
}

/// **+** in the middle of a turn ends its request with the thread.
#[gpui::test]
fn a_new_thread_mid_turn_ends_the_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(
        &mut opened,
        "state/arrangement/track-1/bass.json",
        BASS_CLIP,
    );
    let new_thread = opened.control("agent-new-thread");
    opened.click(new_thread);

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let conversation = sidebar.read(cx).conversation();
        assert!(conversation.entries().is_empty());
        assert!(!conversation.is_working());
    });
    after_the_request();
    write_outside(
        &mut opened,
        "state/arrangement/track-1/later.json",
        LATER_CLIP,
    );
    assert_eq!(opened.undo_label().as_deref(), Some("File change"));
    // The composer has the focus, where cmd-z belongs to the text.
    opened.edit(|project| project.undo());
    assert_eq!(opened.undo_label().as_deref(), Some(LABEL));
}

/// cmd-period and the stop button both send the interrupt, to a thread with no process.
#[gpui::test]
fn cmd_period_and_the_stop_button_send_the_interrupt(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));

    opened.keys("cmd-l");
    opened.cx.simulate_input("Hello");
    opened.keys("enter");
    assert!(opened.cx.read(|cx| sidebar.read(cx).is_busy(cx)));
    opened.keys("cmd-.");
    let stop = opened.control("agent-stop");
    opened.click(stop);

    let sent: Vec<Command> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
    assert_eq!(
        sent,
        [
            Command::Send("Hello".to_string()),
            Command::Interrupt,
            Command::Interrupt
        ]
    );
}

fn entries(opened: &mut Opened<'_>) -> String {
    let sidebar = opened.sidebar();
    opened
        .cx
        .read(|cx| format!("{:?}", sidebar.read(cx).conversation().entries()))
}

fn resume(opened: &mut Opened<'_>) -> Option<String> {
    let sidebar = opened.sidebar();
    opened.cx.read(|cx| sidebar.read(cx).resume())
}

/// The window closes and the project opens again on the same machine: the sidebar shows the
/// thread as it was, and the next message resumes the agent's session.
#[gpui::test]
fn a_thread_opens_again_as_it_was_and_resumes_its_session(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    assert_eq!(resume(&mut opened), None);
    // The agent's session is known as it starts, before it says anything.
    let (thread, _commands) = Thread::without_agent(None);
    let session = Some(thread.session_id().to_string());
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.begin(MESSAGE);
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::StepStarted {
            id: step("write"),
            title: "Wrote state/arrangement/track-1/bass.json".to_string(),
            request_title: "Write state/arrangement/track-1/bass.json".to_string(),
            running_title: "Writing state/arrangement/track-1/bass.json".to_string(),
        },
    ]);
    opened.receive([
        AgentEvent::StepDone {
            id: step("write"),
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
    ]);
    let shown = entries(&mut opened);
    assert_eq!(resume(&mut opened), session);
    drop(sidebar);
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    assert_eq!(entries(&mut opened), shown);
    assert_eq!(resume(&mut opened), session);

    // The next message goes on in the same thread, with an agent in the same session.
    let (thread, commands) = Thread::without_agent(resume(&mut opened));
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.keys("cmd-l");
    opened.cx.simulate_input("Which file was it?");
    opened.keys("enter");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::Send("Which file was it?".to_string()))
    );
    opened.cx.read(|cx| {
        let entries = sidebar.read(cx).conversation().entries();
        let [
            Entry::Message(_),
            Entry::Turn(_),
            Entry::Message(next),
            Entry::Turn(_),
        ] = entries
        else {
            panic!("{entries:?}");
        };
        assert_eq!(next, "Which file was it?");
    });
}

/// The agent no longer has the session: the thread stays to read, also when the project opens
/// again, and only **+** goes on.
#[gpui::test]
fn a_lost_session_ends_the_thread_until_plus(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    opened.begin("Hello");
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "No conversation found with session ID: gone".to_string(),
            },
        },
        AgentEvent::Exited {
            reason: ExitReason::SessionNotFound,
        },
    ]);
    assert!(opened.find("agent-cannot-continue").is_some());
    assert!(opened.find("agent-stop").is_none());
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    assert!(opened.find("agent-cannot-continue").is_some());
    // With no composer, cmd-L focuses the sidebar itself.
    opened.keys("cmd-l");
    assert!(opened.composer_focused());
    let plus = opened.control("agent-cannot-continue");
    opened.click(plus);
    assert!(opened.find("agent-cannot-continue").is_none());
    assert_eq!(entries(&mut opened), "[]");
    assert_eq!(resume(&mut opened), None);

    // Opened again before a message: the new, empty thread, not the old one.
    let folder = opened.close();
    let mut opened = open_again(cx, folder);
    assert!(opened.find("agent-cannot-continue").is_none());
    assert_eq!(entries(&mut opened), "[]");
}

/// The real `claude` remembers across a closed window. Ignored: it needs a signed-in `claude`
/// on the login shell's `PATH`, and runs two real turns.
///
/// ```sh
/// cargo test -p runtime --test window -- --ignored the_real_agent
/// ```
#[gpui::test]
#[ignore]
fn the_real_agent_resumes_a_thread_after_the_window_closed(cx: &mut TestAppContext) {
    // The process wakes the sidebar's tasks from smol's own thread.
    cx.executor().allow_parking();
    let environment = smol::block_on(sound_agent::login_shell_environment()).unwrap();
    let program = sound_agent::program_on_path("claude", &environment).expect("no claude");
    let machine = tempfile::tempdir().unwrap();
    let installed = Installed {
        program,
        environment,
    };
    install_sidebar_with(cx, machine.path(), Some(installed));

    let mut opened = support::open_with(cx, |_| {});
    ask(&mut opened, "Remember the word lantern. Reply only: ok.");
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    assert!(resume(&mut opened).is_some());
    let answer = ask(
        &mut opened,
        "Which word did I ask you to remember? Answer with the word only. Use no tools.",
    );
    assert!(answer.to_lowercase().contains("lantern"), "{answer}");
}

/// Sends `message` from the composer and gives the text of the answer once the turn ends.
fn ask(opened: &mut Opened<'_>, message: &str) -> String {
    opened.keys("cmd-l");
    opened.cx.simulate_input(message);
    opened.keys("enter");
    let sidebar = opened.sidebar();
    let started = std::time::Instant::now();
    while opened.cx.read(|cx| sidebar.read(cx).is_busy(cx)) {
        assert!(started.elapsed() < Duration::from_secs(180), "no answer");
        // The process is real: give it time, and the sidebar its frame.
        std::thread::sleep(Duration::from_millis(50));
        opened
            .cx
            .executor()
            .advance_clock(Duration::from_millis(16));
        opened.cx.run_until_parked();
    }
    opened.cx.read(
        |cx| match sidebar.read(cx).conversation().entries().last() {
            Some(Entry::Turn(turn)) => {
                let outcome = turn.end.as_ref().map(|end| &end.outcome);
                assert_eq!(outcome, Some(&TurnOutcome::Completed));
                turn.blocks.join("\n")
            }
            other => panic!("{other:?}"),
        },
    )
}

/// A turn that leaves a file that is not live says so under its answer, and the line opens to
/// why. A turn that fixes it says nothing, and the problem that was there before it is no
/// news.
#[gpui::test]
fn a_turn_shows_the_problems_it_left(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let file = "state/arrangement/track-1/bass.json";

    opened.begin("Add a bass line");
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, file, "{");
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    let line = opened.control("agent-problems-1");
    assert!(opened.find("agent-problem-1-0").is_none());
    opened.click(line);
    assert!(opened.find("agent-problem-1-0").is_some());

    opened.begin("Fix the bass line");
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, file, BASS_CLIP);
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    assert!(opened.find("agent-problems-3").is_none());
    // The first turn still says what it left.
    assert!(opened.find("agent-problems-1").is_some());
}

/// The watcher applies a write after its grouping window, so the last write of a turn can
/// come just after the turn ended. Its problem is the turn's; one heard later is not.
#[gpui::test]
fn a_write_heard_just_after_the_turn_is_the_turns(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});

    opened.begin("Add a bass line");
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);
    assert!(opened.find("agent-problems-1").is_none());
    write_outside(&mut opened, "state/arrangement/track-1/bass.json", "{");
    let line = opened.control("agent-problems-1");
    opened.click(line);
    assert!(opened.find("agent-problem-1-0").is_some());

    after_the_request();
    write_outside(&mut opened, "state/arrangement/track-1/drums.json", "{");
    assert!(opened.find("agent-problem-1-1").is_none());
}

/// Up in an empty composer recalls the earlier messages of the thread, newest first; down
/// goes forward again and back to empty.
#[gpui::test]
fn up_in_the_composer_recalls_earlier_messages(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    for message in ["Add a bass line", "Make it louder"] {
        opened.begin(message);
        opened.receive([AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }]);
    }
    opened.keys("cmd-l");
    assert_eq!(press(&mut opened, "up"), "Make it louder");
    assert_eq!(press(&mut opened, "up"), "Add a bass line");
    assert_eq!(press(&mut opened, "down"), "Make it louder");
    assert_eq!(press(&mut opened, "down"), "");
}

/// Presses `key` and gives what the composer holds then.
fn press(opened: &mut Opened<'_>, key: &str) -> String {
    opened.keys(key);
    let sidebar = opened.sidebar();
    opened
        .cx
        .read(|cx| sidebar.read(cx).composer().read(cx).text().to_string())
}

/// A thread opened again from disk recalls its messages too.
#[gpui::test]
fn up_recalls_the_messages_of_a_thread_opened_again(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    opened.keys("cmd-l");
    assert_eq!(press(&mut opened, "up"), MESSAGE);
}

fn model(id: &str) -> Model {
    Model {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        short_name: id.to_string(),
    }
}

/// Picks `row` in the composer's menu.
fn pick(opened: &mut Opened<'_>, row: &str) {
    let menu = opened.control("account-menu");
    opened.click(menu);
    let row = opened.control(row);
    opened.click(row);
}

/// The approval mode and the model picked in the menu go to the running agent at once.
/// "Never ask" says so above the composer until the thread's first message.
#[gpui::test]
fn the_menu_settings_go_to_the_agent_at_once(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.receive([AgentEvent::Started {
        account: Account::default(),
        models: vec![model("default"), model("haiku")],
    }]);

    assert!(opened.find("agent-never-ask").is_none());
    pick(&mut opened, "menu-approval-never-ask");
    pick(&mut opened, "menu-model-haiku");
    assert!(opened.find("agent-never-ask").is_some());
    let sent: Vec<Command> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
    assert_eq!(
        sent,
        [
            Command::SetApprovalMode(ApprovalMode::NeverAsk),
            Command::SetModel("haiku".to_string()),
        ]
    );

    // Into the composer, from the menu's trigger where the focus went back to.
    opened
        .cx
        .update(|window, cx| window.focus(&sidebar.focus_handle(cx), cx));
    opened.cx.simulate_input("Hello");
    opened.keys("enter");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::Send("Hello".to_string()))
    );
    assert!(opened.find("agent-never-ask").is_none());

    // While the turn runs, a change goes to the agent at once too.
    assert!(opened.cx.read(|cx| sidebar.read(cx).is_busy(cx)));
    pick(&mut opened, "menu-approval-ask-for-everything");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::SetApprovalMode(ApprovalMode::AskForEverything))
    );
}

/// Every sidebar of the app shows one setting: a change in one goes to the agents of all.
#[gpui::test]
fn two_sidebars_share_one_setting(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    let settings = install_sidebar(cx, machine.path());
    let mut opened = support::open_with(cx, |_| {});
    let (thread, commands) = Thread::without_agent(None);
    let (other_thread, other_commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    let session = opened.session.clone();
    let shared = settings.clone();
    let other = opened.cx.update(|_, cx| {
        sidebar.update(cx, |sidebar, _| sidebar.connect(thread));
        let other = cx
            .new(|cx| Sidebar::with_program(session, Some(nonexistent_claude()), None, shared, cx));
        other.update(cx, |other, _| other.connect(other_thread));
        other
    });

    pick(&mut opened, "menu-approval-never-ask");
    let never_ask = Some(Command::SetApprovalMode(ApprovalMode::NeverAsk));
    assert_eq!(commands.try_recv().ok(), never_ask);
    assert_eq!(other_commands.try_recv().ok(), never_ask);
    let approval_mode = opened
        .cx
        .read(|cx| settings.read(cx).settings().approval_mode);
    assert_eq!(approval_mode, ApprovalMode::NeverAsk);
    drop(other);
}
