//! The real `claude` in a new project, with the request boundaries the sidebar sets. Ignored
//! by default: it needs a signed-in `claude` on the login shell's `PATH`, and it costs a turn of
//! the default model.
//!
//! ```sh
//! cargo test -p runtime --test projects agent -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use runtime::{OFFLINE, main_arrangement, open_or_create};
use smol::future;
use sound_agent::{
    AgentEvent, ApprovalAnswer, ApprovalMode, Installed, Provider, Thread, ThreadOptions,
    TurnOutcome, login_shell_environment, program_on_path,
};
use sound_core::Engine;

/// Long enough for the agent to read the docs and write one file.
const LONGEST_TURN: Duration = Duration::from_secs(300);

/// Every clip of the first track.
fn clips(project: &sound_core::Project) -> Vec<String> {
    project
        .instances()
        .filter(|(id, tool)| {
            id.as_str().starts_with("arrangement/track-1/") && *tool == "arrangement.clip"
        })
        .map(|(id, _)| id.to_string())
        .collect()
}

#[test]
#[ignore]
fn the_agent_adds_a_clip_as_one_undo_step() {
    let folder = tempfile::tempdir().unwrap();
    let (control, _engine) = Engine::new(OFFLINE);
    let (mut project, _plugins) = open_or_create(folder.path(), control).unwrap();
    // The track the add track button makes. Its step is not the one the test undoes.
    let arrangement = main_arrangement(&project).unwrap();
    runtime::add_track(&mut project, &arrangement).unwrap();
    project.clear_history();
    project.watch().unwrap();
    // Writes the map and the docs the agent reads.
    project.poll().unwrap();

    let environment = smol::block_on(login_shell_environment()).unwrap();
    let program = program_on_path("claude", &environment).expect("claude is not on PATH");
    let (thread, mut events) = Thread::start(ThreadOptions {
        provider: Provider::Claude,
        installed: Installed {
            program,
            environment,
        },
        folder: project.root().to_path_buf(),
        model: None,
        approval_mode: ApprovalMode::AskBeforeCommands,
        resume: None,
    })
    .unwrap();

    let message = "Add a clip of one bar on the first track";
    project.begin_request(message);
    thread.send(message).unwrap();
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < LONGEST_TURN, "the turn took too long");
        // Events and the watcher in turn, as the window's frames and polls do.
        #[allow(clippy::disallowed_methods)]
        let timeout = async {
            smol::Timer::after(Duration::from_millis(50)).await;
            None
        };
        let event = smol::block_on(future::or(async { Some(events.next().await) }, timeout));
        project.poll().unwrap();
        match event {
            Some(Some(AgentEvent::StepStarted { title, .. })) => println!("step: {title}"),
            Some(Some(AgentEvent::TextDone { text })) => println!("text: {text}"),
            // "Ask before commands" asks even for a command that only reads, such as `find`.
            // The composer would allow it.
            Some(Some(AgentEvent::ApprovalRequested { id, title })) => {
                println!("allowed: {title}");
                thread.answer(id, ApprovalAnswer::Allow).unwrap();
            }
            Some(Some(AgentEvent::TurnEnded { outcome })) => {
                assert_eq!(outcome, TurnOutcome::Completed);
                break;
            }
            Some(None) => panic!("the agent exited"),
            _ => {}
        }
    }
    project.end_request();
    // The last write may be heard a little after the end.
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(50));
        project.poll().unwrap();
    }

    let added = clips(&project);
    println!("clips: {added:?}, problems: {:?}", project.problems());
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(project.problems().is_empty(), "{:?}", project.problems());
    assert_eq!(project.undo_label(), Some(message));
    project.undo().unwrap();
    assert_eq!(clips(&project), Vec::<String>::new());
    assert_eq!(project.undo_label(), None);
}
