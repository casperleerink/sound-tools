//! `history.png`: the History tab on a project with two versions, work taken back with undo,
//! a version folded away by hand and folds between the rows.

use anyhow::{Context as _, Result};
use gpui::HeadlessAppContext;
use reverb::ReverbState;
use sound_core::{Changes, InstanceId, PointId, Project, ProjectError};
use sound_ui::Session;

use super::{Opened, Save};

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    let opened = Opened::open(cx, &[])?;
    let session = opened.session.clone();
    cx.update(|cx| session.update(cx, |session, cx| shape(session, cx)))?;
    // Without the agent the panel has History alone.
    opened.key("cmd-b", cx)?;
    save(cx, opened.window.into(), "history")?;
    Ok(())
}

fn room() -> InstanceId {
    InstanceId::new("room").unwrap_or_else(|_| unreachable!())
}

/// One point: the reverb with this decay.
fn decay(seconds: f32) -> impl FnOnce(&mut Project) -> Result<(), ProjectError> {
    move |project| {
        let mut changes = Changes::new();
        let state = ReverbState {
            decay_seconds: seconds,
            ..ReverbState::default()
        };
        match project.resolve::<ReverbState>(&room()) {
            Some(instance) => changes.set(&instance, state),
            None => _ = changes.create(room(), state),
        }
        project.commit("Change the decay", changes)
    }
}

fn current(session: &Session) -> Result<PointId> {
    let history = session.project().history().context("no history")?;
    Ok(history.current())
}

fn shape(session: &mut Session, cx: &mut gpui::Context<Session>) -> Result<()> {
    let start = current(session)?;
    for step in 1..=8 {
        session.edit(cx, decay(step as f32)).context("an edit")?;
    }
    let split = current(session)?;
    // Two changes taken back and left by a third: undone work.
    session.edit(cx, decay(20.)).context("an edit")?;
    session.edit(cx, decay(21.)).context("an edit")?;
    session.undo(cx);
    session.undo(cx);
    for step in 9..=12 {
        session.edit(cx, decay(step as f32)).context("an edit")?;
    }
    let tip = current(session)?;
    // A version that starts at the split, named by hand.
    session.go_to(split, cx);
    for step in 30..=32 {
        session.edit(cx, decay(step as f32)).context("an edit")?;
    }
    let brighter = current(session)?;
    let line = (session.project().history())
        .and_then(|history| history.line_of(brighter).ok().flatten())
        .map(|line| line.start)
        .context("no line")?;
    let name = Some("Longer tail".to_string());
    session.edit(cx, |project| project.rename_version(line, name));
    // A version from the start, folded away by hand.
    session.go_to(start, cx);
    session.edit(cx, decay(40.)).context("an edit")?;
    let aside = current(session)?;
    let aside = (session.project().history())
        .and_then(|history| history.line_of(aside).ok().flatten())
        .map(|line| line.start)
        .context("no line")?;
    session.edit(cx, |project| project.set_aside(aside, true));
    session.go_to(tip, cx);
    session.edit(cx, decay(13.)).context("an edit")?;
    Ok(())
}
