//! The agent sidebar in the left panel, with events handed to it and no process:
//!
//! - `agent-sidebar.png`: the piece with the sidebar open: a markdown answer that worked for
//!   12 s, and a second turn that runs a command.
//! - `agent-approval.png`: the same turn waiting on an approval, its command as code.
//! - `agent-closed-working.png`: the same with the sidebar closed, and the lavender indicator on
//!   its icon in the title row.
//! - `agent-failed.png`: open again, the command denied and the turn failed, its steps open.
//! - `agent-steps-problems.png`: a turn with a failed step that left two files not live, its
//!   steps and its problems open.
//! - `agent-menu.png`: the composer's menu: the approvals, the models and the account.
//! - `agent-cannot-continue.png`: a message the agent no longer has the session for: the
//!   composer gives way to the line with **+**.
//! - `agent-never-ask.png`: a new thread under "Never ask", which says so above a composer of
//!   three lines.
//! - `agent-composer-full.png`: the composer at its eight rows, scrolled to the caret.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use gpui::{AppContext, Entity, HeadlessAppContext};
use runtime::window::{LeftPanel, LeftPanelSlot};
use sound_agent::{
    Account, AgentEvent, AgentSettings, ApprovalId, ExitReason, Installed, Model, Sidebar, StepId,
    StepOutcome, TurnOutcome,
};
use sound_ui::components::dropdown_menu::MenuPicked;

use super::{Opened, piece};

/// The first answer, with the markdown an answer has.
const ANSWER: &str = "Added a **bass line** in bars 5 to 8:

- It plays the root of each piano chord on the beat, an octave down.
- The clip is `bass/clip-005.json`.

Say *softer* if it sits too loud.";

pub fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let (opened, sidebar) = open_with_sidebar(cx)?;
    let step = |id: &str| StepId(id.to_string());
    // The first turn ends now, so it worked for 12 s.
    let started = SystemTime::now()
        .checked_sub(Duration::from_secs(12))
        .context("a clock this early")?;

    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            let model = |id: &str, name: &str, description: &str| Model {
                id: id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                short_name: name.to_string(),
            };
            let account = Account {
                email: Some("composer@example.com".to_string()),
                plan: Some("Claude Max".to_string()),
            };
            let models = vec![
                Model {
                    short_name: "Opus 5.5".to_string(),
                    ..model(
                        "default",
                        "Default (recommended)",
                        "Opus 5.5 · Best for everyday, complex tasks",
                    )
                },
                model("sonnet", "Sonnet 5.5", "Most efficient for simpler tasks"),
                model("haiku", "Haiku 4.5", "Fastest for quick answers"),
            ];
            let message = "Add a bass line in bars 5 to 8 that follows the piano";
            sidebar.begin_at(message, started, cx);
            sidebar.receive(
                [
                    AgentEvent::Started {
                        session_id: "session".to_string(),
                        account,
                        models,
                    },
                    AgentEvent::TurnStarted,
                    AgentEvent::StepStarted {
                        id: step("read"),
                        title: "Read state/arrangement/piano/verse.json".to_string(),
                        request_title: "Read state/arrangement/piano/verse.json".to_string(),
                        running_title: "Reading state/arrangement/piano/verse.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("read"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::StepStarted {
                        id: step("write"),
                        title: "Wrote state/arrangement/bass/clip-005.json".to_string(),
                        request_title: "Write state/arrangement/bass/clip-005.json".to_string(),
                        running_title: "Writing state/arrangement/bass/clip-005.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("write"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::TextDone {
                        text: ANSWER.to_string(),
                    },
                    AgentEvent::TurnEnded {
                        outcome: TurnOutcome::Completed,
                    },
                ],
                cx,
            );
            sidebar.begin("Check that the project still builds", cx);
            sidebar.receive(
                [
                    AgentEvent::TurnStarted,
                    AgentEvent::StepStarted {
                        id: step("build"),
                        title: "Ran `cargo build --release -p drums && cargo test -p drums --no-fail…`".to_string(),
                        request_title: "Run `cargo build --release -p drums && cargo test -p drums --no-fail…`".to_string(),
                        // Longer than the line: it ends in an ellipsis.
                        running_title: "Running `cargo build --release -p drums && cargo test -p drums --no-fail…`".to_string(),
                    },
                ],
                cx,
            );
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-sidebar")?;

    let question = AgentEvent::ApprovalRequested {
        id: ApprovalId("build".to_string()),
        title: "Run `cargo build --release -p drums && cargo test -p drums --no-fail…`".to_string(),
    };
    cx.update(|cx| sidebar.update(cx, |sidebar, cx| sidebar.receive([question], cx)));
    cx.run_until_parked();
    save(cx, &opened, "agent-approval")?;

    // Closed while it waits: the icon carries the indicator.
    opened.key("cmd-l", cx)?;
    opened.key("cmd-l", cx)?;
    let open = cx.update(|cx| anyhow::Ok(opened.window.read(cx)?.left_panel_open()))?;
    anyhow::ensure!(!open, "the sidebar did not close");
    save(cx, &opened, "agent-closed-working")?;

    // Opened again: the command was denied, and the turn failed.
    opened.key("cmd-l", cx)?;
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.receive(
                [
                    AgentEvent::StepDone {
                        id: step("build"),
                        outcome: StepOutcome::Denied,
                    },
                    AgentEvent::TurnEnded {
                        outcome: TurnOutcome::Failed {
                            message: "API Error: 529 Overloaded. Try again in a moment."
                                .to_string(),
                        },
                    },
                ],
                cx,
            );
            sidebar.open_details(3, cx);
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-failed")?;

    // A turn with a failed step, which leaves two files that are not live. It worked for 47 s.
    let drums_started = SystemTime::now()
        .checked_sub(Duration::from_secs(47))
        .context("a clock this early")?;
    let root = cx.update(|cx| opened.session.read(cx).project().root().to_path_buf());
    let broken: Vec<PathBuf> = ["state/arrangement/drums/swing.json", "state/tone/kick.json"]
        .into_iter()
        .map(|file| root.join(file))
        .collect();
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.begin_at("Make the drums swing", drums_started, cx);
            sidebar.receive(
                [
                    AgentEvent::TurnStarted,
                    AgentEvent::StepStarted {
                        id: step("edit"),
                        title: "Edited state/arrangement/drums/clip-001.json".to_string(),
                        request_title: "Edit state/arrangement/drums/clip-001.json".to_string(),
                        running_title: "Editing state/arrangement/drums/clip-001.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("edit"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::StepStarted {
                        id: step("test"),
                        title: "Ran `cargo test -p drums`".to_string(),
                        request_title: "Run `cargo test -p drums`".to_string(),
                        running_title: "Running `cargo test -p drums`".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("test"),
                        outcome: StepOutcome::Failed,
                    },
                ],
                cx,
            );
        })
    });
    for file in &broken {
        std::fs::create_dir_all(file.parent().context("a file with no folder")?)?;
        std::fs::write(file, "{")?;
    }
    cx.update(|cx| {
        opened.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.apply_outside_changes(&broken))
        })
    });
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.receive(
                [
                    AgentEvent::TextDone {
                        text: "The drums swing now: every second 16th is 30 ticks late. \
                               `cargo test` failed on the drums, so I left their code as it was."
                            .to_string(),
                    },
                    AgentEvent::TurnEnded {
                        outcome: TurnOutcome::Completed,
                    },
                ],
                cx,
            );
            sidebar.open_details(5, cx);
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-steps-problems")?;

    let menu = cx.update(|cx| sidebar.read(cx).menu().clone());
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "agent-menu")?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.close(window, cx));
    })?;

    // A message the agent no longer has the session for.
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.begin("Which clips did you add?", cx);
            let lost =
                "No conversation found with session ID: 0b6c2a4e-6f0f-4c43-9d43-6f5ad1c5a0e2";
            sidebar.receive(
                [
                    AgentEvent::TurnStarted,
                    AgentEvent::TurnEnded {
                        outcome: TurnOutcome::Failed {
                            message: lost.to_string(),
                        },
                    },
                    AgentEvent::Exited {
                        reason: ExitReason::SessionNotFound,
                    },
                ],
                cx,
            );
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-cannot-continue")?;
    drop(opened);

    // A new thread under "Never ask".
    let (opened, sidebar) = open_with_sidebar(cx)?;
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            // As a pick in the menu does.
            sidebar.menu().update(cx, |_, cx| {
                cx.emit(MenuPicked("approval-never-ask".into()));
            });
            sidebar.composer().update(cx, |composer, cx| {
                composer.set_text(
                    "Give the three voices their own rhythms.\nKeep the kick as it is.\nThen bounce bars 5 to 8.",
                    cx,
                );
            });
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-never-ask")?;

    let long = (1..=12)
        .map(|line| format!("Line {line} of a long message to the agent."))
        .collect::<Vec<_>>()
        .join("\n");
    cx.update(|cx| {
        let composer = sidebar.read(cx).composer().clone();
        composer.update(cx, |composer, cx| composer.set_text(long, cx));
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-composer-full")?;
    Ok(())
}

/// The piece in a window with the sidebar open in its left panel. Its `claude` cannot start:
/// the snapshot hands the sidebar its events. Nothing is remembered, so the panel opens, as
/// at the first start.
fn open_with_sidebar(cx: &mut HeadlessAppContext) -> Result<(Opened, Entity<Sidebar>)> {
    cx.update(|cx| {
        let settings = cx.new(|cx| AgentSettings::new(None, cx));
        LeftPanelSlot::new(None, move |session, _, cx| {
            let installed = Installed {
                program: "/nonexistent/claude".into(),
                environment: HashMap::new(),
            };
            let settings = settings.clone();
            let sidebar =
                cx.new(|cx| Sidebar::with_program(session, Some(installed), None, settings, cx));
            LeftPanel::new(sidebar, Sidebar::is_busy, cx)
        })
        .install(cx)
    });
    let opened = Opened::new(cx, piece);
    // The other snapshots show the window with no panel.
    cx.update(|cx| cx.remove_global::<LeftPanelSlot>());
    let opened = opened?;
    let sidebar = cx.update(|cx| {
        let shell = opened.window.read(cx)?;
        let view = shell.left_panel().cloned().context("no left panel")?;
        view.downcast::<Sidebar>()
            .map_err(|_| anyhow::anyhow!("the left panel is not the sidebar"))
    })?;
    Ok((opened, sidebar))
}
