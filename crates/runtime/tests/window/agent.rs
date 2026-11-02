//! The agent sidebar in the left panel: a turn fed as events with no process, the request it
//! makes one undo step, the approval row, cmd-L and escape, the panel remembered closed, the
//! thread saved and opened again, pictures and files attached, and the project's instructions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{AppContext, ClipboardItem, Entity, Focusable, Modifiers, TestAppContext};
use runtime::window::{LeftPanel, LeftPanelSlot};
use sound_agent::{
    Account, AgentEvent, AgentSettings, ApprovalId, ApprovalMode, Attachment, Command, Entry,
    ExitReason, Image, ImageFormat, Installed, Message, Model, Sidebar, StepId, StepKind,
    StepOutcome, Thread, TurnOutcome,
};
use tempfile::TempDir;

use crate::support::{self, Opened, mark, one_undo_step, write_outside};

/// The sidebar with a `claude` that cannot start, so no test ever runs the real one: a test
/// hands the sidebar the events instead. `support` is the support folder of the machine.
pub(crate) fn install_sidebar(cx: &mut TestAppContext, support: &Path) -> Entity<AgentSettings> {
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
        LeftPanelSlot::new(Some(remembered), move |session, window, cx| {
            let (installed, threads) = (installed.clone(), Some(threads.clone()));
            let settings = shared.clone();
            let sidebar = cx
                .new(|cx| Sidebar::with_program(session, installed, threads, settings, window, cx));
            let (composer, asks) = (sidebar.clone(), sidebar.clone());
            LeftPanel::new(sidebar, Sidebar::is_busy, cx)
                .with_composer(move |cx| composer.read(cx).composer().clone())
                .with_ask(move |message, window, cx| {
                    asks.update(cx, |sidebar, cx| {
                        sidebar.ask_from_outside(message, window, cx)
                    })
                })
        })
        .install(cx);
        settings
    })
}

/// The same project folder in a new window, as quitting and starting the app gives.
fn open_again(cx: &mut TestAppContext, folder: TempDir) -> Opened<'_> {
    let opened = support::open_folder(cx, folder);
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

/// A record the agent writes during its turn, whole, as the project writes it again on a redo.
const BASS_CLIP: &str = r#"{"tool": "reverb", "state": {"pre_delay_ms": 20.0, "decay_seconds": 2.0, "size": 0.5, "damping": 0.5, "diffusion": 0.7, "low_cut_hz": 100.0, "high_cut_hz": 8000.0, "width": 1.0, "mix": 0.5, "freeze": false}}"#;

/// A record written after the request.
const LATER_CLIP: &str = r#"{"tool": "reverb", "state": {}}"#;

/// Longer than the grouping window after the end of a request, so a write after it is no part
/// of the request.
fn after_the_request() {
    std::thread::sleep(Duration::from_millis(150));
}

#[gpui::test]
fn a_request_of_the_agent_is_one_undo_step_named_after_the_message(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let before = mark(&mut opened);

    opened.begin(MESSAGE);
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::StepStarted {
            id: step("write"),
            title: "Wrote state/bass.json".to_string(),
            request_title: "Write state/bass.json".to_string(),
            command: None,
            running_title: "Writing state/bass.json".to_string(),
            kind: StepKind::Change {
                path: "state/bass.json".to_string(),
            },
        },
    ]);
    assert!(opened.panel_open());
    // The writes of one request, seconds apart as an agent makes them: the watcher hears each.
    write_outside(
        &mut opened,
        "state/bass.json",
        r#"{"tool": "reverb", "state": {}}"#,
    );
    opened.receive([
        AgentEvent::StepDone {
            id: step("write"),
            outcome: StepOutcome::Done,
        },
        AgentEvent::StepStarted {
            id: step("edit"),
            title: "Edited state/bass.json".to_string(),
            request_title: "Edit state/bass.json".to_string(),
            command: None,
            running_title: "Editing state/bass.json".to_string(),
            kind: StepKind::Change {
                path: "state/bass.json".to_string(),
            },
        },
    ]);
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);
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
        let [Entry::Message { text: message, .. }, Entry::Turn(turn)] = conversation.entries()
        else {
            panic!("{:?}", conversation.entries());
        };
        assert_eq!(message, MESSAGE);
        assert_eq!(turn.blocks, ["Added a bass line."]);
        let steps: Vec<_> = turn.steps.iter().map(|step| step.title.as_str()).collect();
        assert_eq!(steps, ["Wrote state/bass.json", "Edited state/bass.json"]);
        let outcome = turn.end.as_ref().map(|end| &end.outcome);
        assert_eq!(outcome, Some(&TurnOutcome::Completed));
    });
    one_undo_step(&mut opened, LABEL, &before);
}

#[gpui::test]
fn the_approval_row_answers_and_goes_when_the_turn_ends(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
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
        title: "Run".to_string(),
        command: Some("cargo build".to_string()),
    };
    opened.receive([AgentEvent::TurnStarted, question.clone()]);
    assert_eq!(approval(&mut opened).as_deref(), Some("Run"));
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
fn try_again_sends_the_message_of_a_turn_that_failed_and_keeps_the_draft(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.keys("secondary-l");
    opened.cx.simulate_input("Make the drums swing");
    opened.keys("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::Send(_))));
    assert!(opened.find("agent-try-again").is_none());
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "Claude is busy. Try again in a moment.".to_string(),
            },
        },
    ]);

    // A draft and a picture for the next message are in the composer.
    opened.cx.simulate_input("Make it softer");
    let pasted = gpui::Image::from_bytes(gpui::ImageFormat::Bmp, BMP.to_vec());
    opened
        .cx
        .write_to_clipboard(ClipboardItem::new_image(&pasted));
    opened.keys("secondary-v");
    // From the composer, past the picture's remove button, the keys reach it.
    opened.keys("shift-tab");
    opened.keys("shift-tab");
    press_enter(&mut opened);
    let Ok(Command::Send(Message {
        text, attachments, ..
    })) = commands.try_recv()
    else {
        panic!("the message did not go again");
    };
    assert_eq!(
        (text.as_str(), attachments.len()),
        ("Make the drums swing", 0)
    );
    // The new turn works, so the failed one is no longer the last.
    assert!(opened.find("agent-try-again").is_none());
    let draft = opened
        .cx
        .read(|cx| sidebar.read(cx).composer().read(cx).text().to_string());
    assert_eq!(draft, "Make it softer");
    assert!(opened.composer_focused());
}

/// Enter going down and up on what has the focus, which GPUI takes as a click of it.
fn press_enter(opened: &mut Opened<'_>) {
    opened.keys("enter");
    let up = gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("enter").unwrap(),
    };
    opened.cx.simulate_event(up);
    opened.cx.run_until_parked();
}

#[gpui::test]
fn the_keys_open_the_steps_of_a_turn(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, _commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.keys("secondary-l");
    opened.cx.simulate_input("Read the bass");
    opened.keys("enter");
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::StepStarted {
            id: step("read"),
            title: "Read state/bass.json".to_string(),
            request_title: "Read state/bass.json".to_string(),
            command: None,
            running_title: "Reading state/bass.json".to_string(),
            kind: StepKind::Read {
                path: "state/bass.json".to_string(),
            },
        },
        AgentEvent::StepDone {
            id: step("read"),
            outcome: StepOutcome::Done,
        },
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);
    assert!(opened.find("agent-steps-1").is_none());
    // From the composer, "Worked for" is the stop before it.
    assert!(opened.composer_focused());
    opened.keys("shift-tab");
    press_enter(&mut opened);
    assert!(opened.find("agent-steps-1").is_some());
    press_enter(&mut opened);
    assert!(opened.find("agent-steps-1").is_none());
}

#[gpui::test]
fn a_claude_that_does_not_start_is_a_quiet_line_and_no_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let undo_label = opened.undo_label();
    opened.keys("secondary-l");
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
    let mut opened = support::open_new(cx);
    // Open at the first start.
    assert!(opened.panel_open());
    let shell = opened.shell.clone();
    opened
        .cx
        .update(|window, cx| window.focus(&shell.focus_handle(cx), cx));

    // cmd-L focuses the composer of the open panel, escape gives the focus back.
    opened.keys("secondary-l");
    assert!(opened.composer_focused());
    opened.keys("escape");
    assert!(opened.panel_open());
    assert!(
        opened
            .cx
            .update(|window, cx| shell.focus_handle(cx).is_focused(window))
    );

    // cmd-L again in the composer closes it, and the focus goes back.
    opened.keys("secondary-l");
    opened.keys("secondary-l");
    assert!(!opened.panel_open());
    assert!(!opened.composer_focused());
    drop(shell);
    drop(opened.close());

    // Another window, on another project: closed, as the machine remembers it.
    let mut opened = support::open_new(cx);
    assert!(!opened.panel_open());
    opened.keys("secondary-l");
    assert!(opened.panel_open());
    assert!(opened.composer_focused());
}

#[gpui::test]
fn cmd_b_opens_and_closes_the_panel_and_leaves_the_focus(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    assert!(opened.panel_open());
    let shell = opened.shell.clone();
    opened
        .cx
        .update(|window, cx| window.focus(&shell.focus_handle(cx), cx));
    let shell_focused = |opened: &mut support::Opened<'_>| {
        opened
            .cx
            .update(|window, cx| shell.focus_handle(cx).is_focused(window))
    };

    opened.keys("secondary-b");
    assert!(!opened.panel_open());
    assert!(shell_focused(&mut opened));
    opened.keys("secondary-b");
    assert!(opened.panel_open());
    assert!(shell_focused(&mut opened));

    // From the composer, closing gives the focus back.
    opened.keys("secondary-l");
    opened.keys("secondary-b");
    assert!(!opened.panel_open());
    assert!(shell_focused(&mut opened));
}

/// The account menu sits in the composer and offers **Sign out**.
#[gpui::test]
fn the_account_menu_in_the_composer_offers_sign_out(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    assert!(opened.find("menu-sign-out").is_none());
    let menu = opened.control("model-menu");
    opened.click(menu);
    assert!(opened.find("menu-sign-out").is_some());
}

/// Sign out ends the agent. The turn it worked on ends as stopped, and its request with it,
/// so the sidebar is not left working.
#[gpui::test]
fn signing_out_mid_turn_stops_the_turn_and_its_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let before = mark(&mut opened);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);

    let menu = opened.control("model-menu");
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
    let mut opened = support::open_new(cx);
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
    let mut opened = support::open_new(cx);
    assert!(opened.panel_open());
    opened.keys("secondary-l");
    assert!(opened.composer_focused());
    opened.keys("secondary-l");
    assert!(!opened.panel_open());
}

/// The process dies in the middle of a turn: the turn says so, the request ends there, and
/// what the agent wrote before is still one undo step.
#[gpui::test]
fn an_exit_mid_turn_ends_the_turn_and_its_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let before = mark(&mut opened);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);
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
        let [Entry::Message { .. }, Entry::Turn(turn)] = conversation.entries() else {
            panic!("{:?}", conversation.entries());
        };
        let outcome = turn.end.as_ref().map(|end| &end.outcome);
        assert_eq!(outcome, Some(&TurnOutcome::Failed { message }));
    });
    one_undo_step(&mut opened, LABEL, &before);

    after_the_request();
    write_outside(&mut opened, "state/later.json", LATER_CLIP);
    assert_eq!(opened.undo_label().as_deref(), Some("File change"));
}

/// **+** in the middle of a turn ends its request with the thread.
#[gpui::test]
fn a_new_thread_mid_turn_ends_the_request(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);
    let new_thread = opened.control("agent-new-thread");
    opened.click(new_thread);

    let sidebar = opened.sidebar();
    opened.cx.read(|cx| {
        let conversation = sidebar.read(cx).conversation();
        assert!(conversation.entries().is_empty());
        assert!(!conversation.is_working());
    });
    after_the_request();
    write_outside(&mut opened, "state/later.json", LATER_CLIP);
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
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));

    opened.keys("secondary-l");
    opened.cx.simulate_input("Hello");
    opened.keys("enter");
    assert!(opened.cx.read(|cx| sidebar.read(cx).is_busy()));
    opened.keys("secondary-.");
    let stop = opened.control("agent-stop");
    opened.click(stop);

    let sent: Vec<Command> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
    assert_eq!(
        sent,
        [
            Command::Send("Hello".into()),
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
    let mut opened = support::open_new(cx);
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
            title: "Wrote state/bass.json".to_string(),
            request_title: "Write state/bass.json".to_string(),
            command: None,
            running_title: "Writing state/bass.json".to_string(),
            kind: StepKind::Change {
                path: "state/bass.json".to_string(),
            },
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
    opened.keys("secondary-l");
    opened.cx.simulate_input("Which file was it?");
    opened.keys("enter");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::Send("Which file was it?".into()))
    );
    opened.cx.read(|cx| {
        let entries = sidebar.read(cx).conversation().entries();
        let [
            Entry::Message { .. },
            Entry::Turn(_),
            Entry::Message { text: next, .. },
            Entry::Turn(_),
        ] = entries
        else {
            panic!("{entries:?}");
        };
        assert_eq!(next, "Which file was it?");
    });
}

/// One red pixel.
const PNG: [u8; 69] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// One red pixel, as the clipboard of Windows holds a screenshot.
const BMP: [u8; 58] = [
    66, 77, 58, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 24, 0,
    0, 0, 0, 0, 4, 0, 0, 0, 19, 11, 0, 0, 19, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0,
];

/// A picture pasted and files dropped on the composer go with the next message: a picture as
/// itself, or as PNG when the agent cannot see its format, and any other file as its path. One
/// taken out stays behind. The message shows their names, also when the thread opens again.
#[gpui::test]
fn pasted_and_dropped_files_go_with_the_next_message(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    let outside = tempfile::tempdir().unwrap();
    let [score, take, notes] = ["Score.PNG", "Take 3.wav", "notes.pdf"].map(|name| {
        let path = outside.path().join(name);
        std::fs::write(&path, PNG).unwrap();
        path
    });

    opened.keys("secondary-l");
    let pasted = gpui::Image::from_bytes(gpui::ImageFormat::Bmp, BMP.to_vec());
    opened
        .cx
        .write_to_clipboard(ClipboardItem::new_image(&pasted));
    opened.keys("secondary-v");
    let composer = opened.control("agent-composer");
    // GPUI takes a drop only when the last input was the pointer's, not the keys'.
    opened
        .cx
        .simulate_mouse_move(composer, None, Modifiers::default());
    opened.drop_files(&[score, take.clone(), notes], composer);
    let remove_notes = opened.control("agent-remove-attachment-3");
    opened.click(remove_notes);
    // The paste went to the chips, not into the text.
    opened.cx.simulate_input("Make the bass follow this");
    opened.keys("enter");

    let Ok(Command::Send(Message {
        text, attachments, ..
    })) = commands.try_recv()
    else {
        panic!("no message went");
    };
    assert_eq!(text, "Make the bass follow this");
    let [
        Attachment::Image(Image {
            format: ImageFormat::Png,
            bytes: pasted,
        }),
        score,
        file,
    ] = attachments.as_slice()
    else {
        panic!("{attachments:?}");
    };
    assert!(
        pasted.starts_with(&PNG[..8]),
        "the pasted picture is no PNG"
    );
    let picture = Attachment::Image(Image {
        format: ImageFormat::Png,
        bytes: PNG.to_vec(),
    });
    assert_eq!((score, file), (&picture, &Attachment::File(take)));
    assert_eq!(opened.find("agent-remove-attachment-0"), None);
    opened.cx.read(|cx| {
        let entries = sidebar.read(cx).conversation().entries();
        let [Entry::Message { attachments, .. }, Entry::Turn(_)] = entries else {
            panic!("{entries:?}");
        };
        assert_eq!(attachments, &["Pasted image", "Score.PNG", "Take 3.wav"]);
    });
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);
    let shown = entries(&mut opened);
    drop(sidebar);
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    assert_eq!(entries(&mut opened), shown);
}

/// A message sent while a dropped file is still being read waits for it, so the file never goes
/// with the next message instead.
#[gpui::test]
fn a_message_waits_for_a_file_still_being_read(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    let outside = tempfile::tempdir().unwrap();
    let take = outside.path().join("take.wav");
    std::fs::write(&take, "").unwrap();
    opened.keys("secondary-l");
    opened.cx.simulate_input("Tune the bass to this");

    let attach = vec![take.clone()];
    opened.cx.update(|_, cx| {
        sidebar.update(cx, |sidebar, cx| sidebar.attach_files(attach, cx));
    });
    opened
        .cx
        .dispatch_action(sound_ui::components::text_input::Submit);
    assert!(
        commands.try_recv().is_err(),
        "it went before the file was in"
    );
    opened.cx.run_until_parked();
    opened.keys("enter");
    let message = Message {
        text: "Tune the bass to this".to_string(),
        attachments: vec![Attachment::File(take)],
        note: None,
    };
    assert_eq!(commands.try_recv().ok(), Some(Command::Send(message)));
}

/// The thread the sidebar shows, by its id in the index of the project's threads.
fn current_thread(machine: &Path) -> String {
    let projects = std::fs::read_dir(machine.join("agent/threads")).unwrap();
    let [project] = projects.collect::<Vec<_>>().try_into().unwrap();
    let index = std::fs::read_to_string(project.unwrap().path().join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    index["current"].as_str().unwrap().to_string()
}

/// A thread left with **New** is in **Open recent**, and opens again from there.
#[gpui::test]
fn a_thread_left_with_new_opens_again_from_open_recent(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    opened.begin(MESSAGE);
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TextDone {
            text: "Added a bass line.".to_string(),
        },
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);
    let shown = entries(&mut opened);
    let thread = current_thread(machine.path());
    let new_thread = opened.control("agent-new-thread");
    opened.click(new_thread);
    assert_eq!(entries(&mut opened), "[]");

    for control in [
        "agent-new-thread-menu".to_string(),
        "menu-recent".to_string(),
        format!("menu-thread-{thread}"),
    ] {
        let control = opened.control(&control);
        opened.click(control);
    }
    opened.cx.run_until_parked();
    assert_eq!(entries(&mut opened), shown);
    assert_eq!(current_thread(machine.path()), thread);
}

/// The project's instructions are typed in the sidebar and saved in the project folder, where
/// every agent reads them. Enter adds a line. An empty text removes the file.
#[gpui::test]
fn instructions_typed_in_the_sidebar_are_saved_in_the_project(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let file = opened.folder.path().join("instructions.md");
    let open_instructions = |opened: &mut Opened<'_>| {
        for control in ["agent-new-thread-menu", "menu-instructions-project"] {
            let control = opened.control(control);
            opened.click(control);
        }
        // The file is read in the background.
        opened.cx.run_until_parked();
    };

    open_instructions(&mut opened);
    opened
        .cx
        .simulate_input("Never automate the volume directly.");
    opened.keys("enter");
    opened.cx.simulate_input("Use a utility.");
    let back = opened.control("agent-instructions-back");
    opened.click(back);
    opened.cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Never automate the volume directly.\nUse a utility.\n"
    );

    // Opened again it shows the file; emptied and closed with escape, the file goes.
    open_instructions(&mut opened);
    opened.keys("secondary-a backspace escape");
    opened.cx.run_until_parked();
    assert!(!file.exists());
    assert!(opened.composer_focused());
}

/// The agent no longer has the session: the thread stays to read, also when the project opens
/// again, and only **+** goes on.
#[gpui::test]
fn a_lost_session_ends_the_thread_until_plus(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
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
    opened.keys("secondary-l");
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

    let mut opened = support::open_new(cx);
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
    opened.keys("secondary-l");
    opened.cx.simulate_input(message);
    opened.keys("enter");
    let sidebar = opened.sidebar();
    let started = std::time::Instant::now();
    while opened.cx.read(|cx| sidebar.read(cx).is_busy()) {
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
    let mut opened = support::open_new(cx);
    let file = "state/bass.json";

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

/// Under the problems the last turn left, one click asks the agent to fix them, as if typed.
#[gpui::test]
fn the_problems_of_the_last_turn_ask_the_agent_to_fix_them(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));

    opened.begin("Add a bass line");
    opened.receive([AgentEvent::TurnStarted]);
    write_outside(&mut opened, "state/bass.json", "{");
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    let fix = opened.control("agent-fix");
    opened.click(fix);
    let Ok(Command::Send(Message { text, .. })) = commands.try_recv() else {
        panic!("nothing went to the agent");
    };
    assert_eq!(text, "Fix the files that are not live.");
    // The agent works on it, so the line goes.
    assert!(opened.find("agent-fix").is_none());
}

/// The window's notice of files that are not live asks the agent to fix them, as the line
/// under a turn does: the thread shows, over the instructions too, and a draft stays.
#[gpui::test]
fn the_notice_of_files_not_live_asks_the_agent_to_fix_them(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.keys("secondary-l");
    opened.cx.simulate_input("Make the bass softer");
    for control in ["agent-new-thread-menu", "menu-instructions-project"] {
        let control = opened.control(control);
        opened.click(control);
    }
    opened.cx.run_until_parked();
    write_outside(&mut opened, "state/bass.json", "{");

    let ask = opened.control("problems-ask");
    opened.click(ask);
    let Ok(Command::Send(Message { text, .. })) = commands.try_recv() else {
        panic!("nothing went to the agent");
    };
    assert_eq!(text, sound_agent::FIX_NOT_LIVE);
    assert!(opened.find("agent-instructions-back").is_none());
    assert!(opened.composer_focused());
    let draft = opened
        .cx
        .read(|cx| sidebar.read(cx).composer().read(cx).text().to_string());
    assert_eq!(draft, "Make the bass softer");
}

/// The watcher applies a write after its grouping window, so the last write of a turn can
/// come just after the turn ended. Its problem is the turn's; one heard later is not.
#[gpui::test]
fn a_write_heard_just_after_the_turn_is_the_turns(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);

    opened.begin("Add a bass line");
    opened.receive([
        AgentEvent::TurnStarted,
        AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        },
    ]);
    assert!(opened.find("agent-problems-1").is_none());
    write_outside(&mut opened, "state/bass.json", "{");
    let line = opened.control("agent-problems-1");
    opened.click(line);
    assert!(opened.find("agent-problem-1-0").is_some());

    after_the_request();
    write_outside(&mut opened, "state/drums.json", "{");
    assert!(opened.find("agent-problem-1-1").is_none());
}

/// Up in an empty composer recalls the earlier messages of the thread, newest first; down
/// goes forward again and back to empty.
#[gpui::test]
fn up_in_the_composer_recalls_earlier_messages(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    for message in ["Add a bass line", "Make it louder"] {
        opened.begin(message);
        opened.receive([AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }]);
    }
    opened.keys("secondary-l");
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
    let mut opened = support::open_new(cx);
    opened.begin(MESSAGE);
    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    let folder = opened.close();

    let mut opened = open_again(cx, folder);
    opened.keys("secondary-l");
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

/// Picks `row` in the composer's access menu, or its model menu.
fn pick(opened: &mut Opened<'_>, row: &str) {
    let name = if row.starts_with("menu-approval") {
        "access-menu"
    } else {
        "model-menu"
    };
    let menu = opened.control(name);
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
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    opened.receive([AgentEvent::Started {
        account: Account::default(),
        models: vec![model("default"), model("haiku")],
    }]);

    pick(&mut opened, "menu-approval-never-ask");
    pick(&mut opened, "menu-model-haiku");
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
        Some(Command::Send("Hello".into()))
    );

    // While the turn runs, a change goes to the agent at once too.
    assert!(opened.cx.read(|cx| sidebar.read(cx).is_busy()));
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
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let (other_thread, other_commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    let session = opened.session.clone();
    let shared = settings.clone();
    let other = opened.cx.update(|window, cx| {
        sidebar.update(cx, |sidebar, _| sidebar.connect(thread));
        let installed = Some(nonexistent_claude());
        let other =
            cx.new(|cx| Sidebar::with_program(session, installed, None, shared, window, cx));
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

impl Opened<'_> {
    fn go_to(&mut self, point: sound_core::PointId) {
        let session = self.session.clone();
        self.cx
            .update(|_, cx| session.update(cx, |session, cx| session.go_to(point, cx)));
        self.cx.run_until_parked();
    }

    fn current_point(&mut self) -> sound_core::PointId {
        self.project(|project| project.history().unwrap().current())
    }
}

/// Going back in the history shows where to above the composer, and the agent reads it with
/// the next message, and only that one.
#[gpui::test]
fn the_message_after_going_back_tells_the_agent_where_to(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    let start = opened.current_point();
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);

    opened.go_to(start);
    assert!(opened.find("agent-went-back").is_some());
    opened.keys("secondary-l");
    opened.cx.simulate_input("Make the drums swing");
    opened.keys("enter");
    let Ok(Command::Send(Message { text, note, .. })) = commands.try_recv() else {
        panic!("no message went");
    };
    assert_eq!(text, "Make the drums swing");
    let note = note.expect("no note went with the message");
    let folder = opened.path(sound_core::HISTORY_FOLDER);
    for said in [
        start.short(),
        "Version 1".to_string(),
        folder.display().to_string(),
    ] {
        assert!(note.contains(&said), "{said:?} is not in {note:?}");
    }
    assert!(opened.find("agent-went-back").is_none());
    assert!(!entries(&mut opened).contains("went back"));

    opened.receive([AgentEvent::TurnEnded {
        outcome: TurnOutcome::Completed,
    }]);
    opened.cx.simulate_input("And the bass");
    opened.keys("enter");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::Send("And the bass".into()))
    );
}

/// The × takes the line away, and no note goes.
#[gpui::test]
fn a_dismissed_going_back_tells_the_agent_nothing(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    install_sidebar(cx, machine.path());
    let mut opened = support::open_new(cx);
    let (thread, commands) = Thread::without_agent(None);
    let sidebar = opened.sidebar();
    opened
        .cx
        .update(|_, cx| sidebar.update(cx, |sidebar, _| sidebar.connect(thread)));
    let start = opened.current_point();
    write_outside(&mut opened, "state/bass.json", BASS_CLIP);
    opened.go_to(start);

    let dismiss = opened.control("agent-went-back-dismiss");
    opened.click(dismiss);
    assert!(opened.find("agent-went-back").is_none());
    opened.keys("secondary-l");
    opened.cx.simulate_input("Hello");
    opened.keys("enter");
    assert_eq!(
        commands.try_recv().ok(),
        Some(Command::Send("Hello".into()))
    );
}
