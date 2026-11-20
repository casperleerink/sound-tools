//! The agent sidebar, driven by key and label on a chat with no agent: what it is asked goes
//! out as the commands of a thread nobody runs, and what the agent says is fed as events.

use std::time::{Duration, Instant, SystemTime};

use rune::{ClipboardContent, ClipboardImage, Key, KeyEvent, Mods};
use smol::channel::Receiver;
use sound_agent::{
    AgentEvent, ApprovalAnswer, ApprovalId, Command, FIX_NOT_LIVE, StepId, StepKind, StepOutcome,
    Thread, TurnOutcome,
};

use super::*;
use crate::app::MAIN_MODIFIER;
use crate::sidebar;

/// A window on a new project with the sidebar in `scene`, open.
fn window(scene: &str) -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    sidebar::scene(&mut state, scene);
    (scratch, keyed(state))
}

fn keyed(state: State) -> rune::Runtime<State> {
    let mut window = laid_out(state);
    redraw(&mut window);
    window
}

/// The chat talks to a thread whose commands come out here.
fn connect(window: &mut rune::Runtime<State>) -> Receiver<Command> {
    let (thread, commands) = Thread::without_agent(None);
    sidebar::chat(&mut window.state, |chat, _| chat.connect(thread));
    commands
}

fn feed(window: &mut rune::Runtime<State>, events: impl IntoIterator<Item = AgentEvent>) {
    let events: Vec<AgentEvent> = events.into_iter().collect();
    sidebar::chat(&mut window.state, |chat, project| {
        chat.receive(events, project)
    });
    redraw(window);
}

/// A turn that began `seconds` ago, the agent working on it.
fn begin(window: &mut rune::Runtime<State>, message: &str, seconds: u64) {
    let started = SystemTime::now() - Duration::from_secs(seconds);
    sidebar::chat(&mut window.state, |chat, project| {
        chat.begin_at(message, Vec::new(), started, project);
    });
    feed(window, [AgentEvent::TurnStarted]);
}

fn sent(commands: &Receiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| commands.try_recv().ok()).collect()
}

fn chord(window: &mut rune::Runtime<State>, c: char) {
    let mods = MAIN_MODIFIER;
    window.key(KeyEvent {
        key: Key::Command(c),
        mods,
    });
    redraw(window);
}

fn shift_tab(window: &mut rune::Runtime<State>) {
    let mods = Mods {
        shift: true,
        ..Mods::default()
    };
    window.key(KeyEvent {
        key: Key::Tab,
        mods,
    });
    redraw(window);
}

fn typed(window: &mut rune::Runtime<State>, words: &str) {
    key(window, Key::Text(words.into()));
}

/// What the composer says now.
fn composer(window: &rune::Runtime<State>) -> String {
    let controls = chrome_controls(&window.state);
    let field = controls
        .iter()
        .find(|control| control.key == "agent-composer");
    let value = field.and_then(|field| field.value.clone());
    value
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Frames until `done`: the chat's background work hands in what it read.
fn until(
    window: &mut rune::Runtime<State>,
    mut done: impl FnMut(&mut rune::Runtime<State>) -> bool,
) {
    let start = Instant::now();
    while !done(window) {
        assert!(start.elapsed() < Duration::from_secs(10), "it never came");
        std::thread::sleep(Duration::from_millis(2));
        redraw(window);
    }
}

/// The thread's entries, as words a test can read.
fn entries(window: &mut rune::Runtime<State>) -> Vec<String> {
    sidebar::chat(&mut window.state, |chat, _| {
        let entries = chat.conversation().entries().iter();
        entries.map(|entry| format!("{entry:?}")).collect()
    })
}

#[test]
fn return_sends_what_was_typed_and_the_composer_keeps_the_keys() {
    let (_scratch, mut window) = window("agent-empty");
    let commands = connect(&mut window);
    chord(&mut window, 'l');
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    typed(&mut window, "Add a bass line");
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    window.key(KeyEvent {
        key: Key::Enter,
        mods: shift,
    });
    typed(&mut window, "that follows the piano");
    assert_eq!(composer(&window), "Add a bass line\nthat follows the piano");

    key(&mut window, Key::Enter);
    let [Command::Send(message)] = &sent(&commands)[..] else {
        panic!("no message went");
    };
    assert_eq!(message.text, "Add a bass line\nthat follows the piano");
    assert_eq!(composer(&window), "");
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    // While it works, the button stops it.
    let controls = chrome_controls(&window.state);
    let send = controls.iter().find(|control| control.key == "agent-send");
    assert_eq!(send.map(|send| send.label.as_str()), Some("Stop the agent"));
}

#[test]
fn up_and_down_in_the_composer_go_through_the_messages_sent() {
    let (_scratch, mut window) = window("agent-empty");
    let _commands = connect(&mut window);
    chord(&mut window, 'l');
    for message in ["First", "Second"] {
        typed(&mut window, message);
        key(&mut window, Key::Enter);
        feed(
            &mut window,
            [AgentEvent::TurnEnded {
                outcome: TurnOutcome::Completed,
            }],
        );
    }
    key(&mut window, Key::Up);
    assert_eq!(composer(&window), "Second");
    key(&mut window, Key::Up);
    assert_eq!(composer(&window), "First");
    key(&mut window, Key::Down);
    key(&mut window, Key::Down);
    assert_eq!(composer(&window), "");
}

#[test]
fn cmd_period_and_the_stop_button_stop_the_agent() {
    let (_scratch, mut window) = window("agent-empty");
    let commands = connect(&mut window);
    begin(&mut window, "Make the drums swing", 3);
    assert!(window.state.panel.busy());
    chord(&mut window, 'l');
    chord(&mut window, '.');
    assert_eq!(sent(&commands), [Command::Interrupt]);
    click(&mut window, "agent-send");
    assert_eq!(sent(&commands), [Command::Interrupt]);
}

/// One undo step per message: every file the agent changes from the send to the end of its
/// turn goes back with one undo, through the window's session.
#[test]
fn a_turn_that_changes_two_files_is_one_undo_step() {
    let (_scratch, mut window) = window("agent-empty");
    let _commands = connect(&mut window);
    chord(&mut window, 'l');
    typed(&mut window, "Add a bass and a lead");
    key(&mut window, Key::Enter);
    feed(&mut window, [AgentEvent::TurnStarted]);
    let root = window.state.project().root().to_path_buf();
    for (path, after) in [("state/bass.json", 0), ("state/lead.json", 20)] {
        let path = root.join(path);
        std::fs::write(&path, "{}").unwrap();
        let at = Instant::now() + Duration::from_secs(after);
        (window.state.session)
            .background(|project| project.apply_outside_changes_at(&[path], at))
            .unwrap();
        redraw(&mut window);
    }
    feed(
        &mut window,
        [AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }],
    );
    assert_eq!(
        window.state.project().undo_label().as_deref(),
        Some("Add a bass and a lead")
    );
    assert!(window.state.session.undo());
    assert!(!root.join("state/bass.json").exists() && !root.join("state/lead.json").exists());
}

/// The composer is a text field: a key it does not take is no shortcut of the window, but the
/// panel's own keys work from inside it.
#[test]
fn a_closed_panel_shows_the_agent_at_work_on_its_icon() {
    let (_scratch, mut window) = window("agent-empty");
    let _commands = connect(&mut window);
    chord(&mut window, 'b');
    assert!(window.find("left-panel-busy").is_none());
    begin(&mut window, "Make the drums swing", 3);
    assert!(window.find("left-panel-busy").is_some());
    feed(
        &mut window,
        [AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }],
    );
    assert!(window.find("left-panel-busy").is_none());
}

#[test]
fn shift_tab_from_the_composer_reaches_worked_for_whose_keys_open_the_steps() {
    let (_scratch, mut window) = window("agent-problems");
    chord(&mut window, 'l');
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    while focused(&window).as_deref() != Some("agent-worked-for-1") {
        shift_tab(&mut window);
        assert!(focused(&window).is_some(), "shift-tab left the sidebar");
    }
    // The scene shows the steps; the keys fold them and open them again.
    assert!(window.find("agent-steps-1").is_some());
    key(&mut window, Key::Enter);
    assert!(window.find("agent-steps-1").is_none());
    key(&mut window, Key::Text(" ".into()));
    assert!(window.find("agent-steps-1").is_some());
}

#[test]
fn the_question_of_the_agent_is_answered_by_label_and_goes() {
    let (_scratch, mut window) = window("agent-empty");
    let commands = connect(&mut window);
    begin(&mut window, "Check that the project still builds", 2);
    feed(
        &mut window,
        [AgentEvent::ApprovalRequested {
            id: ApprovalId("build".into()),
            title: "Run `cargo build`".into(),
            command: Some("cargo build".into()),
        }],
    );
    assert!(window.find("agent-approval").is_some());
    act(
        &mut window.state,
        json!({ "label": "Allow for this thread", "action": "press" }),
    );
    assert_eq!(
        sent(&commands),
        [Command::Answer(
            ApprovalId("build".into()),
            ApprovalAnswer::AllowForThread
        )]
    );
    redraw(&mut window);
    assert!(window.find("agent-approval").is_none());
}

#[test]
fn fix_sends_as_if_typed_keeps_the_draft_and_gives_the_composer_the_keys() {
    let (_scratch, mut window) = window("agent-problems");
    let commands = connect(&mut window);
    chord(&mut window, 'l');
    typed(&mut window, "Then make it louder");
    click(&mut window, "agent-fix");
    let [Command::Send(message)] = &sent(&commands)[..] else {
        panic!("Fix sent nothing");
    };
    assert_eq!(message.text, FIX_NOT_LIVE);
    assert_eq!(composer(&window), "Then make it louder");
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    // Asked once: the line goes while the agent works on it.
    assert!(window.find("agent-fix").is_none());
}

#[test]
fn try_again_sends_the_failed_message_again() {
    let (_scratch, mut window) = window("agent-empty");
    let commands = connect(&mut window);
    begin(&mut window, "Make the drums swing", 2);
    feed(
        &mut window,
        [AgentEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                message: "Claude is busy. Try again in a moment.".into(),
            },
        }],
    );
    // The thread scrolls to its end first.
    for _ in 0..30 {
        redraw(&mut window);
    }
    click(&mut window, "agent-try-again");
    let [Command::Send(message)] = &sent(&commands)[..] else {
        panic!("Try again sent nothing");
    };
    assert_eq!(message.text, "Make the drums swing");
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
}

#[test]
fn the_window_list_asks_the_agent_over_the_instructions_and_keeps_the_draft() {
    let scratch = new_project();
    std::fs::write(scratch.path().join("song/state/song/quiet.json"), "{").unwrap();
    let mut state = opened(&scratch.path().join("song"));
    sidebar::scene(&mut state, "agent-empty");
    let mut window = keyed(state);
    let commands = connect(&mut window);
    chord(&mut window, 'l');
    typed(&mut window, "A draft");
    act(
        &mut window.state,
        json!({ "label": "Recent threads and instructions", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "label": "This project", "action": "press" }),
    );
    until(&mut window, |window| {
        window.find("agent-instructions-page").is_some()
    });

    click(&mut window, "problems");
    click(&mut window, "problems-ask");
    let [Command::Send(message)] = &sent(&commands)[..] else {
        panic!("the list asked nothing");
    };
    assert_eq!(message.text, FIX_NOT_LIVE);
    assert!(window.find("agent-instructions-page").is_none());
    assert_eq!(composer(&window), "A draft");
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
}

#[test]
fn a_pasted_picture_is_a_chip_text_pastes_as_text_and_the_x_gives_the_keys_back() {
    let (_scratch, mut window) = window("agent-empty");
    let commands = connect(&mut window);
    chord(&mut window, 'l');
    let png = include_bytes!("../../../../../tooling/icon/sound-tools-512.png");
    let picture = ClipboardImage::from_png(png.to_vec()).unwrap();
    window.paste_content(ClipboardContent::new("").with_image(picture));
    redraw(&mut window);
    window.paste("a groove");
    redraw(&mut window);
    assert_eq!(composer(&window), "a groove");
    assert!(window.find("agent-chip-0").is_some());

    // Dropped files come after it, once read.
    let outside = tempfile::tempdir().unwrap();
    let take = outside.path().join("Vocal take 3.wav");
    std::fs::write(&take, "").unwrap();
    let (x, y) = window.find("agent-composer-box").unwrap();
    window.drop_files(vec![take], Some((x, y)));
    until(&mut window, |window| window.find("agent-chip-1").is_some());

    click(&mut window, "agent-remove-0");
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    key(&mut window, Key::Enter);
    let [Command::Send(message)] = &sent(&commands)[..] else {
        panic!("no message went");
    };
    assert_eq!(message.attachments.len(), 1);
    let first = entries(&mut window).remove(0);
    assert!(
        first.contains(r#"attachments: ["Vocal take 3.wav"]"#),
        "{first}"
    );
}

#[test]
fn return_adds_a_line_to_the_instructions_and_escape_closes_them_on_the_composer() {
    let (_scratch, mut window) = window("agent-empty");
    act(
        &mut window.state,
        json!({ "label": "Recent threads and instructions", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "label": "This project", "action": "press" }),
    );
    until(&mut window, |window| {
        window.find("agent-instructions").is_some()
    });
    redraw(&mut window);
    assert_eq!(focused(&window).as_deref(), Some("agent-instructions"));
    typed(&mut window, "Keep the strings");
    key(&mut window, Key::Enter);
    typed(&mut window, "out of the intro.");
    key(&mut window, Key::Escape);
    assert_eq!(focused(&window).as_deref(), Some("agent-composer"));
    let file = window
        .state
        .project()
        .root()
        .join(sound_core::INSTRUCTIONS_FILE);
    until(&mut window, |_| {
        std::fs::read_to_string(&file)
            .is_ok_and(|text| text == "Keep the strings\nout of the intro.\n")
    });
}

#[test]
fn the_keys_pick_a_model_in_the_picker_and_escape_goes_back_to_the_pill() {
    let (_scratch, mut window) = window("agent-empty");
    feed(&mut window, [sidebar::started()]);
    click(&mut window, "agent-model");
    until(&mut window, |window| {
        focused(window).as_deref() == Some("agent-model-search")
    });
    typed(&mut window, "ha");
    key(&mut window, Key::Enter);
    assert_eq!(
        control(&chrome_controls(&window.state), "Haiku 4.5").key,
        "agent-model"
    );
    assert_eq!(focused(&window).as_deref(), Some("agent-model"));

    click(&mut window, "agent-model");
    key(&mut window, Key::Down);
    key(&mut window, Key::Escape);
    assert!(window.find("agent-model-picker").is_none());
    assert_eq!(focused(&window).as_deref(), Some("agent-model"));
}

#[test]
fn with_no_agent_cmd_l_takes_the_sidebar_itself_and_closes_it_again() {
    let (_scratch, mut window) = window("agent-no-agent");
    tab_to(&mut window, "track-t00");
    chord(&mut window, 'l');
    assert_eq!(focused(&window).as_deref(), Some("left-panel-body"));
    assert!(window.find("agent-open-settings").is_some());
    chord(&mut window, 'l');
    assert!(!window.state.panel.open);
    assert_eq!(focused(&window).as_deref(), Some("track-t00"));
}

#[test]
fn every_control_the_sidebar_lists_is_in_the_window() {
    for (scene, _) in sidebar::SCENES {
        let (_scratch, window) = window(scene);
        for control in chrome_controls(&window.state) {
            // A menu keys its own rows: an agent finds them by label.
            let menu = matches!(
                control.role.as_deref(),
                Some("menuitem" | "menuitemcheckbox")
            );
            assert!(
                menu || window.find(&control.key).is_some(),
                "{scene}: {} is listed but not drawn",
                control.key
            );
        }
    }
}

#[test]
fn new_while_the_last_thread_loads_wins_over_it() {
    let (scratch, machine) = (new_project(), tempfile::tempdir().unwrap());
    let threads = machine.path().join("threads");
    let mut state = opened(&scratch.path().join("song"));
    sidebar::signed_in_saving(&mut state, Some(threads.clone()));
    let mut window = keyed(state);
    let _commands = connect(&mut window);
    begin(&mut window, "Make the drums swing", 2);
    feed(
        &mut window,
        [AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }],
    );
    until(&mut window, |window| {
        !sidebar::chat(&mut window.state, |chat, _| chat.is_saving())
    });
    drop(window);

    let mut state = opened(&scratch.path().join("song"));
    sidebar::signed_in_saving(&mut state, Some(threads));
    // Before any frame, so before the last thread is in.
    act(&mut state, json!({ "label": "New", "action": "press" }));
    let mut window = keyed(state);
    until(&mut window, |window| {
        !sidebar::chat(&mut window.state, |chat, _| chat.is_loading())
    });
    assert!(
        entries(&mut window).is_empty(),
        "the thread that loaded came back"
    );
}

#[test]
fn a_recent_thread_removed_meanwhile_says_so() {
    let (scratch, machine) = (new_project(), tempfile::tempdir().unwrap());
    let threads = machine.path().join("threads");
    let mut state = opened(&scratch.path().join("song"));
    sidebar::signed_in_saving(&mut state, Some(threads.clone()));
    let mut window = keyed(state);
    until(&mut window, |window| {
        !sidebar::chat(&mut window.state, |chat, _| chat.is_loading())
    });
    let _commands = connect(&mut window);
    begin(&mut window, "Make the drums swing", 2);
    feed(
        &mut window,
        [AgentEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
        }],
    );
    act(
        &mut window.state,
        json!({ "label": "New", "action": "press" }),
    );
    until(&mut window, |window| {
        !sidebar::chat(&mut window.state, |chat, _| chat.is_saving())
    });
    std::fs::remove_dir_all(&threads).unwrap();

    act(
        &mut window.state,
        json!({ "label": "Recent threads and instructions", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "label": "Open recent", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "label": "Make the drums swing", "action": "press" }),
    );
    until(&mut window, |window| {
        !sidebar::chat(&mut window.state, |chat, _| chat.is_loading())
    });
    let entries = entries(&mut window);
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("That thread is not saved any more")),
        "{entries:?}"
    );
}

#[test]
fn a_step_that_runs_shows_its_words_on_the_working_line() {
    let (_scratch, mut window) = window("agent-empty");
    let _commands = connect(&mut window);
    begin(&mut window, "Build the drums", 2);
    feed(
        &mut window,
        [AgentEvent::StepStarted {
            id: StepId("read".into()),
            title: "Read state/song/song.json".into(),
            running_title: "Reading state/song/song.json".into(),
            request_title: "Read state/song/song.json".into(),
            command: None,
            kind: StepKind::Read {
                path: "state/song/song.json".into(),
            },
        }],
    );
    let working = window
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("agent-working-1"));
    assert!(working.is_some());
    feed(
        &mut window,
        [
            AgentEvent::StepDone {
                id: StepId("read".into()),
                outcome: StepOutcome::Done,
            },
            AgentEvent::TurnEnded {
                outcome: TurnOutcome::Completed,
            },
        ],
    );
    assert!(window.find("agent-working-1").is_none());
    assert!(window.find("agent-worked-for-1").is_some());
}

#[test]
fn open_settings_opens_the_settings_page_on_the_agents_and_its_cards_work() {
    let (_scratch, mut window) = window("agent-no-agent");
    act(
        &mut window.state,
        json!({ "label": sound_agent::OPEN_SETTINGS, "action": "press" }),
    );
    redraw(&mut window);
    let page = window.state.settings.page.as_ref().expect("the page");
    assert_eq!(page.section, super::super::settings::Section::Agents);
    let controls = chrome_controls(&window.state);
    let cards: Vec<&str> = (controls.iter())
        .map(|control| control.key.as_str())
        .filter(|key| key.starts_with("agent-"))
        .collect();
    assert!(!cards.is_empty(), "the agents' cards list their buttons");
    for key in cards {
        assert!(window.find(key).is_some(), "{key} is listed but not drawn");
    }
}

#[test]
fn settings_opens_on_the_agents_whose_fields_type_and_pass_on_cmd_k() {
    let (_scratch, mut window) = window("agent-empty");
    chord(&mut window, ',');
    let page = window.state.settings.page.as_ref().expect("the page");
    assert_eq!(page.section, super::super::settings::Section::Agents);
    click(&mut window, "agent-services-search");
    typed(&mut window, " ");
    chord(&mut window, 'k');
    assert!(!window.state.playing, "Space typed, not played");
    assert!(window.state.session.transport().keys, "cmd-K never types");
    assert!(window.state.settings.page.is_some());
}

#[test]
fn the_composer_passes_on_the_apps_keys_that_never_type() {
    let (_scratch, mut window) = window("agent-empty");
    chord(&mut window, 'l');
    typed(&mut window, "hi ");
    assert!(!window.state.playing);
    chord(&mut window, 'k');
    assert!(window.state.session.transport().keys);
    typed(&mut window, "a");
    assert_eq!(composer(&window), "hi a");
    chord(&mut window, ',');
    assert!(window.state.settings.page.is_some());
}

#[test]
fn the_access_menu_shows_what_each_mode_means_in_full() {
    let (_scratch, mut window) = window("agent-empty");
    click(&mut window, "agent-access-trigger");
    redraw(&mut window);
    let small = window.state.theme.kit.ty.small;
    let descriptions: Vec<_> = (window.elements().into_iter())
        .filter(|element| element.key.as_deref() == Some("description"))
        .collect();
    assert_eq!(descriptions.len(), 3);
    for description in descriptions {
        let words = description.text.unwrap();
        let (width, _) = rune::text::measure(&words, &[], small, None);
        assert!(
            description.rect[2] >= width,
            "{words} cut: {width} in {:?}",
            description.rect
        );
    }
}
