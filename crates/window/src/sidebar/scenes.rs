//! The sidebar in each of its states for a run without a window (`--scene`), which has no
//! agent: a chat signed in with programs that never start, fed recorded events. For pictures,
//! and for `--tree` and `--act` on a thread an agent made.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sound_agent::{
    Account, AgentEvent, AgentSettings, Agents, ApprovalId, Chat, ExitReason, Installed, Model,
    Provider, Scope, Service, Setup, SignedIn, SigningIn, StepId, StepKind, StepOutcome, Thread,
    TurnOutcome,
};
use sound_core::Project;

use super::{Menu, Sidebar};
use crate::app::{State, Tab};

pub(crate) const SCENES: &[(&str, fn(&mut State))] = &[
    ("agent-empty", empty),
    ("agent-thread", thread),
    ("agent-steps", steps),
    ("agent-approval", approval),
    ("agent-failed", failed),
    ("agent-problems", problems),
    ("agent-cannot-continue", cannot_continue),
    ("agent-attachments", attachments),
    ("agent-models", models),
    ("agent-access", access),
    ("agent-new-menu", new_menu),
    ("agent-instructions", instructions),
    ("agent-no-agent", no_agent),
    ("agent-looking", looking),
    ("agent-settings", settings_signed_in),
    ("agent-settings-setup", settings_setup),
];

/// What the first answer says, with the markdown an answer has.
const ANSWER: &str = "Added a **bass line** in bars 5 to 8:

- It plays the root of each piano chord on the beat, an octave down.
- The clip is `bass/clip-005.json`.

Say *softer* if it sits too loud.";
const BUILD: &str = "cargo build --release -p drums && cargo test -p drums --no-fail-fast";

fn installed(name: &str) -> Installed {
    Installed {
        program: PathBuf::from(format!("/nonexistent/{name}")),
        environment: HashMap::new(),
    }
}

/// Signed in to Claude Code and Pi, whose programs never start.
fn both() -> Vec<(Provider, Installed)> {
    vec![
        (Provider::Claude, installed("claude")),
        (Provider::Pi, installed("pi")),
    ]
}

/// The window with the sidebar open on a chat on `agents`, its project's own.
fn open(state: &mut State, agents: impl FnOnce(AgentSettings, sound_agent::Waker) -> Agents) {
    open_saving(state, None, agents);
}

/// [`open`], keeping the threads in `threads`.
fn open_saving(
    state: &mut State,
    threads: Option<PathBuf>,
    agents: impl FnOnce(AgentSettings, sound_agent::Waker) -> Agents,
) {
    let waker = sound_agent::Waker::new(|| {});
    let settings = AgentSettings::new(None, waker.clone());
    let chat = Chat::new(agents(settings, waker.clone()), &state.project, threads, waker);
    let woken = Arc::default();
    state.panel.agent = Some(Box::new(Sidebar::new(chat, woken, Arc::new(|| {}))));
    state.show_tab(Tab::Agent);
    state.set_panel_open(true);
}

/// For a test: signed in to both, the threads kept in `threads`.
#[cfg(test)]
pub(crate) fn signed_in_saving(state: &mut State, threads: Option<PathBuf>) {
    open_saving(state, threads, |settings, waker| {
        Agents::with_programs(both(), settings, waker).with_services(services())
    });
}

/// For a test: the scene called `name`.
#[cfg(test)]
pub(crate) fn scene(state: &mut State, name: &str) {
    if let Some((_, scene)) = SCENES.iter().find(|(scene, _)| *scene == name) {
        scene(state);
    }
}

fn signed_in(state: &mut State) {
    open(state, |settings, waker| {
        Agents::with_programs(both(), settings, waker).with_services(services())
    });
}

fn chat(state: &mut State, f: impl FnOnce(&mut Chat, &mut Project)) {
    super::with_chat(state, |sidebar, project| f(&mut sidebar.chat, project));
}

fn sidebar(state: &mut State, f: impl FnOnce(&mut Sidebar)) {
    super::with_chat(state, |sidebar, _| f(sidebar));
}

fn step(id: &str) -> StepId {
    StepId(id.to_string())
}

fn ago(seconds: u64) -> SystemTime {
    SystemTime::now()
        .checked_sub(Duration::from_secs(seconds))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn model(id: &str, name: &str, description: &str, short_name: &str) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        short_name: short_name.into(),
        source: None,
    }
}

pub(crate) fn started() -> AgentEvent {
    AgentEvent::Started {
        account: Some(Account {
            email: Some("composer@example.com".into()),
            plan: Some("Claude Max".into()),
        }),
        models: vec![
            model(
                "default",
                "Default (recommended)",
                "Opus 5.5 · Best for everyday, complex tasks",
                "Opus 5.5",
            ),
            model("sonnet", "Sonnet 5.5", "Most efficient for simpler tasks", "Sonnet 5.5"),
            model("haiku", "Haiku 4.5", "Fastest for quick answers", "Haiku 4.5"),
        ],
    }
}

/// A turn that read the piano, wrote the bass and answered, then one that runs a command.
fn two_turns(chat: &mut Chat, project: &mut Project) {
    let (thread, commands) = Thread::without_agent(None);
    // Nothing reads what it is asked: a picture has no agent.
    std::mem::forget(commands);
    chat.connect(thread);
    let message = "Add a bass line in bars 5 to 8 that follows the piano";
    chat.begin_at(message, Vec::new(), ago(12), project);
    chat.receive(
        [
            started(),
            AgentEvent::TurnStarted,
            started_step("read", "Read state/song/piano/verse.json", StepKind::Read {
                path: "state/song/piano/verse.json".into(),
            }),
            done("read", StepOutcome::Done),
            started_step("write", "Wrote state/song/bass/clip-005.json", StepKind::Change {
                path: "state/song/bass/clip-005.json".into(),
            }),
            done("write", StepOutcome::Done),
            AgentEvent::TextDone {
                text: ANSWER.into(),
            },
            AgentEvent::TurnEnded {
                outcome: TurnOutcome::Completed,
            },
        ],
        project,
    );
    chat.begin_at("Check that the project still builds", Vec::new(), ago(4), project);
    chat.receive(
        [
            AgentEvent::TurnStarted,
            AgentEvent::StepStarted {
                id: step("build"),
                title: "Build and test the drums".into(),
                running_title: "Build and test the drums".into(),
                request_title: "Build and test the drums".into(),
                command: Some(BUILD.into()),
                kind: StepKind::Run,
            },
        ],
        project,
    );
}

fn started_step(id: &str, title: &str, kind: StepKind) -> AgentEvent {
    AgentEvent::StepStarted {
        id: step(id),
        title: title.into(),
        running_title: title.into(),
        request_title: title.into(),
        command: None,
        kind,
    }
}

fn done(id: &str, outcome: StepOutcome) -> AgentEvent {
    AgentEvent::StepDone {
        id: step(id),
        outcome,
    }
}

fn empty(state: &mut State) {
    signed_in(state);
}

fn thread(state: &mut State) {
    signed_in(state);
    chat(state, two_turns);
}

fn steps(state: &mut State) {
    thread(state);
    chat(state, |chat, _| chat.toggle_steps(1));
}

fn approval(state: &mut State) {
    thread(state);
    chat(state, |chat, project| {
        let question = AgentEvent::ApprovalRequested {
            id: ApprovalId("build".into()),
            title: "Run `cargo build` and `cargo test` on the drums".into(),
            command: Some(BUILD.into()),
        };
        chat.receive([question], project);
    });
}

fn failed(state: &mut State) {
    thread(state);
    chat(state, |chat, project| {
        chat.receive(
            [
                done("build", StepOutcome::Denied),
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        message: "Claude is busy. Try again in a moment.".into(),
                    },
                },
            ],
            project,
        );
        chat.toggle_steps(3);
    });
}

/// A turn with a failed step that left two files not live, its steps and problems open.
fn problems(state: &mut State) {
    signed_in(state);
    let root = state.project.root().to_path_buf();
    chat(state, |chat, project| {
        let (thread, commands) = Thread::without_agent(None);
        std::mem::forget(commands);
        chat.connect(thread);
        chat.begin_at("Make the drums swing", Vec::new(), ago(47), project);
        chat.receive(
            [
                started(),
                AgentEvent::TurnStarted,
                started_step("edit", "Edited state/song/drums/clip-001.json", StepKind::Change {
                    path: "state/song/drums/clip-001.json".into(),
                }),
                done("edit", StepOutcome::Done),
                AgentEvent::StepStarted {
                    id: step("test"),
                    title: "Ran a command".into(),
                    running_title: "Running a command".into(),
                    request_title: "Run a command".into(),
                    command: Some("cargo test -p drums".into()),
                    kind: StepKind::Run,
                },
                done("test", StepOutcome::Failed),
            ],
            project,
        );
        let broken: Vec<PathBuf> = ["state/song/swing.json", "state/song/kick.json"]
            .into_iter()
            .map(|file| root.join(file))
            .collect();
        for file in &broken {
            if std::fs::write(file, "{").is_err() {
                eprintln!("{} could not be written", file.display());
            }
        }
        if let Err(error) = project.apply_outside_changes(&broken) {
            eprintln!("{error}");
        }
        chat.receive(
            [
                AgentEvent::TextDone {
                    text: "The drums swing now: every second 16th is 30 ticks late. \
                           `cargo test` failed on the drums, so I left their code as it was."
                        .into(),
                },
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                },
            ],
            project,
        );
        chat.toggle_steps(1);
        chat.toggle_problems(1);
    });
}

fn cannot_continue(state: &mut State) {
    thread(state);
    chat(state, |chat, project| {
        chat.receive(
            [AgentEvent::TurnEnded {
                outcome: TurnOutcome::Completed,
            }],
            project,
        );
        chat.begin_at("Which clips did you add?", Vec::new(), ago(1), project);
        chat.receive(
            [
                AgentEvent::TurnStarted,
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Failed {
                        message: "Claude no longer has this thread.".into(),
                    },
                },
                AgentEvent::Exited {
                    reason: ExitReason::SessionNotFound,
                },
            ],
            project,
        );
    });
}

/// A message sent with a picture and a file, and a picture and a file for the next.
fn attachments(state: &mut State) {
    signed_in(state);
    chat(state, |chat, project| {
        let (thread, commands) = Thread::without_agent(None);
        std::mem::forget(commands);
        chat.connect(thread);
        let names = vec!["Groove sketch.png".to_string(), "Vocal take 3.wav".to_string()];
        let message = "Make the bass follow this groove, and tune it to the take";
        chat.begin_at(message, names, ago(9), project);
        chat.receive(
            [
                AgentEvent::TurnStarted,
                AgentEvent::TextDone {
                    text: "The bass follows the kick of the sketch now, in the key of the take."
                        .into(),
                },
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Completed,
                },
            ],
            project,
        );
        chat.attach_picture(include_bytes!("../../../../tooling/icon/sound-tools-512.png").to_vec());
    });
    sidebar(state, |sidebar| {
        sidebar.set_composer("And this one?");
        sidebar.sync_thumbs();
    });
}

fn models(state: &mut State) {
    signed_in(state);
    chat(state, |chat, project| chat.receive([started()], project));
    sidebar(state, |sidebar| {
        sidebar.menu = Some(Menu::Model);
        sidebar.search.focused = true;
    });
}

fn access(state: &mut State) {
    signed_in(state);
    sidebar(state, |sidebar| sidebar.menu = Some(Menu::Access));
}

fn new_menu(state: &mut State) {
    signed_in(state);
    sidebar(state, |sidebar| sidebar.menu = Some(Menu::New));
}

fn instructions(state: &mut State) {
    signed_in(state);
    let file = state.project.root().join(sound_core::INSTRUCTIONS_FILE);
    if let Err(error) = std::fs::write(&file, "Keep the strings out of the intro.\n") {
        eprintln!("{} could not be written: {error}", file.display());
    }
    chat(state, |chat, project| {
        chat.open_instructions(Scope::Project);
        // Read in the background: the picture waits for it.
        for _ in 0..200 {
            chat.poll(project);
            if chat.instructions().is_some_and(|shown| shown.text().is_some()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    sidebar(state, |sidebar| {
        sidebar.follow_instructions();
    });
}

fn no_agent(state: &mut State) {
    open(state, |settings, waker| Agents::with_programs(Vec::new(), settings, waker));
}

fn looking(state: &mut State) {
    open(state, |settings, waker| {
        Agents::with_programs(both(), settings, waker)
            .with_setup(Provider::Claude, Setup::Checking)
            .with_setup(Provider::Pi, Setup::Checking)
    });
}

fn services() -> Vec<Service> {
    vec![
        Service::example("Anthropic", Some("Anthropic (Claude Pro/Max)"), None),
        Service::example("GitHub Copilot", Some("GitHub Copilot"), None),
        Service::example("OpenAI", Some("Sign in with ChatGPT"), Some(SignedIn::Here)),
        Service::example("OpenRouter", Some("Sign in with OpenRouter"), Some(SignedIn::Here)),
        Service::example("DeepSeek", None, None),
        Service::example("Google", None, Some(SignedIn::Elsewhere)),
        Service::example("Groq", None, None),
        Service::example("Mistral", None, None),
    ]
}

/// Claude Code signed in, and Pi to three services.
fn settings_signed_in(state: &mut State) {
    open(state, |settings, waker| {
        let account = Account {
            email: Some("composer@example.com".into()),
            plan: Some("Claude Max".into()),
        };
        Agents::with_programs(both(), settings, waker)
            .with_services(services())
            .with_setup(Provider::Claude, Setup::Ready {
                account,
                reason: None,
            })
    });
    sidebar(state, |sidebar| sidebar.shows_settings = true);
}

/// Claude Code mid sign-in with a code to enter, and Pi not on the computer yet.
fn settings_setup(state: &mut State) {
    open(state, |settings, waker| {
        let signing_in = SigningIn {
            page: Some("https://claude.ai/oauth/code".into()),
            code: Some("WDJB-MJHT".into()),
            note: None,
            question: None,
        };
        Agents::with_programs(both(), settings, waker)
            .with_setup(Provider::Claude, Setup::SigningIn(signing_in))
            .with_setup(Provider::Pi, Setup::NotInstalled)
    });
    sidebar(state, |sidebar| sidebar.shows_settings = true);
}
