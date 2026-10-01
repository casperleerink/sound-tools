//! The agent sidebar in the left panel, with events handed to it and no process:
//!
//! - `agent-sidebar.png`: the piece with the sidebar open: a finished turn, and a second turn
//!   that works and waits on an approval.
//! - `agent-closed-working.png`: the same with the sidebar closed, and the lavender indicator on
//!   its icon in the title row.

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use gpui::{AppContext, HeadlessAppContext};
use runtime::window::{LeftPanel, LeftPanelSlot};
use sound_agent::{AgentEvent, ApprovalId, Installed, Sidebar, StepId, StepOutcome, TurnOutcome};

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
            let sidebar = cx.new(|cx| Sidebar::with_claude(session, Some(installed), cx));
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

    cx.update(|cx| {
        sidebar.update(cx, |sidebar, cx| {
            sidebar.begin("Add a bass line in bars 5 to 8 that follows the piano", cx);
            sidebar.receive(
                [
                    AgentEvent::TurnStarted,
                    AgentEvent::StepStarted {
                        id: step("read"),
                        title: "Read state/arrangement/piano/verse.json".to_string(),
                    },
                    AgentEvent::StepDone {
                        id: step("read"),
                        outcome: StepOutcome::Done,
                    },
                    AgentEvent::StepStarted {
                        id: step("write"),
                        title: "Wrote state/arrangement/bass/clip-005.json".to_string(),
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
                    },
                    AgentEvent::ApprovalRequested {
                        id: ApprovalId("build".to_string()),
                        title: "Run `cargo build`".to_string(),
                    },
                ],
                cx,
            );
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "agent-sidebar")?;

    // Closed while it waits: the icon carries the indicator.
    opened.key("cmd-l", cx)?;
    opened.key("cmd-l", cx)?;
    let open = cx.update(|cx| anyhow::Ok(opened.window.read(cx)?.left_panel_open()))?;
    anyhow::ensure!(!open, "the sidebar did not close");
    save(cx, &opened, "agent-closed-working")?;
    Ok(())
}
