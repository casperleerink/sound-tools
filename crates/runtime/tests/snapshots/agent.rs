//! The agent sidebar in the left panel, with events handed to it and no process:
//!
//! - `agent-sidebar.png`: the piece with the sidebar open: a turn that worked for 12 s, and a
//!   second turn that runs a command.
//! - `agent-approval.png`: the same turn waiting on an approval.
//! - `agent-closed-working.png`: the same with the sidebar closed, and the lavender indicator on
//!   its icon in the title row.
//! - `agent-cannot-continue.png`: open again, the turn stopped, and a message the agent no
//!   longer has the session for: the composer gives way to the line with **+**.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use gpui::{AppContext, HeadlessAppContext};
use runtime::window::{LeftPanel, LeftPanelSlot};
use sound_agent::{
    AgentEvent, ApprovalId, ExitReason, Installed, Sidebar, StepId, StepOutcome, TurnOutcome,
};

use super::{Opened, piece};

pub fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    // A `claude` that cannot start: the snapshot hands the sidebar its events. Nothing is
    // remembered, so the panel opens, as at the first start.
    cx.update(|cx| {
        LeftPanelSlot::new(None, |session, _, cx| {
            let installed = Installed {
                program: "/nonexistent/claude".into(),
                environment: HashMap::new(),
            };
            let sidebar = cx.new(|cx| Sidebar::with_program(session, Some(installed), None, cx));
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
    let step = |id: &str| StepId(id.to_string());
    // The first turn ends now, so it worked for 12 s.
    let started = SystemTime::now()
        .checked_sub(Duration::from_secs(12))
        .context("a clock this early")?;

    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            let message = "Add a bass line in bars 5 to 8 that follows the piano";
            sidebar.begin_at(message, started, cx);
            sidebar.receive(
                [
                    AgentEvent::TurnStarted,
                    AgentEvent::StepStarted {
                        id: step("read"),
                        title: "Read state/arrangement/piano/verse.json".to_string(),
                        running_title: "Reading state/arrangement/piano/verse.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("read"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::StepStarted {
                        id: step("write"),
                        title: "Wrote state/arrangement/bass/clip-005.json".to_string(),
                        running_title: "Writing state/arrangement/bass/clip-005.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("write"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::TextDone {
                        text: "Added a clip on the bass in bars 5 to 8. It plays the root of each piano chord on the beat, an octave down.".to_string(),
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
                        title: "Ran cargo build".to_string(),
                        running_title: "Running cargo build".to_string(),
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
        title: "Run `cargo build`".to_string(),
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

    // Opened again, stopped, and a message the agent no longer has the session for.
    opened.key("cmd-l", cx)?;
    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            let stopped = AgentEvent::TurnEnded {
                outcome: TurnOutcome::Interrupted,
            };
            sidebar.receive([stopped], cx);
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
    Ok(())
}
