//! `history.png`: the History tab on a project with four named versions, work taken back with
//! undo and a version folded away by hand. `history-started-here.png`: on the version the
//! current one started from.

use anyhow::{Context as _, Result};
use gpui::HeadlessAppContext;
use reverb::ReverbState;
use sound_core::{Changes, InstanceId, PointId, Project, ProjectError};
use sound_ui::Session;

use super::{Opened, Save};

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    let opened = Opened::open(cx, &[])?;
    let session = opened.session.clone();
    let tail = cx.update(|cx| session.update(cx, |session, cx| shape(session, cx)))?;
    // Without the agent the panel has History alone.
    opened.key("cmd-b", cx)?;
    save(cx, opened.window.into(), "history")?;
    cx.update(|cx| session.update(cx, |session, cx| session.go_to(tail, cx)));
    save(cx, opened.window.into(), "history-started-here")?;
    Ok(())
}

fn room() -> InstanceId {
    InstanceId::new("room").unwrap_or_else(|_| unreachable!())
}

/// One point: the reverb changed by `change`, named `label`.
fn change(
    label: &'static str,
    change: impl FnOnce(&mut ReverbState),
) -> impl FnOnce(&mut Project) -> Result<(), ProjectError> {
    move |project| {
        let mut changes = Changes::new();
        match project.resolve::<ReverbState>(&room()) {
            Some(instance) => {
                let mut state = project.state(&instance).cloned().unwrap_or_default();
                change(&mut state);
                changes.set(&instance, state);
            }
            None => _ = changes.create(room(), ReverbState::default()),
        }
        project.commit(label, changes)
    }
}

fn edit(
    session: &mut Session,
    cx: &mut gpui::Context<Session>,
    label: &'static str,
    step: fn(&mut ReverbState),
) -> Result<()> {
    session.edit(cx, change(label, step)).context("an edit")
}

fn current(session: &Session) -> Result<PointId> {
    let history = session.project().history().context("no history")?;
    Ok(history.current())
}

/// Names the line the current point is in.
fn name(session: &mut Session, name: &str, cx: &mut gpui::Context<Session>) -> Result<PointId> {
    let history = session.project().history().context("no history")?;
    let line = history.line_of(history.current())?.context("no line")?;
    let start = line.start;
    session.edit(cx, |project| project.rename_version(start, name));
    Ok(start)
}

/// Returns the latest point of the version two others start from.
fn shape(session: &mut Session, cx: &mut gpui::Context<Session>) -> Result<PointId> {
    let start = current(session)?;
    edit(session, cx, "Add a reverb", |_| {})?;
    edit(session, cx, "Change the size", |state| state.size = 0.6)?;
    edit(session, cx, "Change the damping", |state| {
        state.damping = 0.4
    })?;
    let sketch_split = current(session)?;
    edit(session, cx, "Change the pre-delay", |state| {
        state.pre_delay_ms = 20.
    })?;
    edit(session, cx, "Change the diffusion", |state| {
        state.diffusion = 0.8
    })?;
    name(session, "First sketch", cx)?;

    session.go_to(sketch_split, cx);
    edit(session, cx, "Change the decay", |state| {
        state.decay_seconds = 4.
    })?;
    edit(session, cx, "Change the decay", |state| {
        state.decay_seconds = 6.
    })?;
    let tail_split = current(session)?;
    edit(session, cx, "Change the size", |state| state.size = 0.9)?;
    let tail = current(session)?;
    name(session, "Longer tail", cx)?;

    session.go_to(start, cx);
    edit(session, cx, "Add a reverb", |_| {})?;
    edit(session, cx, "Change the size", |state| state.size = 0.1)?;
    let aside = name(session, "Tiny box", cx)?;
    session.edit(cx, |project| project.set_aside(aside, true));

    session.go_to(tail_split, cx);
    edit(session, cx, "Change the damping", |state| {
        state.damping = 0.8
    })?;
    // Two changes taken back and left by a third: undone work.
    edit(session, cx, "Change the size", |state| state.size = 0.3)?;
    edit(session, cx, "Change the size", |state| state.size = 0.2)?;
    session.undo(cx);
    session.undo(cx);
    edit(session, cx, "Change the decay", |state| {
        state.decay_seconds = 8.
    })?;
    edit(session, cx, "Change the diffusion", |state| {
        state.diffusion = 0.5
    })?;
    name(session, "Darker room", cx)?;
    Ok(tail)
}
