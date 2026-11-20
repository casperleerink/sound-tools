//! A drag of an element that `plays` numbers: each move plays them into the session's gesture,
//! so the sound follows the hand and no code runs; the code's `update` at the end names the one
//! undo step, from before the drag to what the update left.

use runtime::session::Drag;
use sound_core::{InstanceId, Project, Ticks};

use sound_typescript::next::Tools;

use super::State;

/// The step of a drag that writes into a lane without an update of code.
const STEP: &str = "Move a control";

/// The drag that plays now, if one does.
#[derive(Default)]
pub(crate) struct Playing {
    drag: Option<Drag>,
    /// A lane refused what it plays: the rest of the drag does nothing.
    refused: bool,
    /// It was let go: it ends once the code answered its release.
    let_go: bool,
    /// How far what a lane played into each number it plays, by path, was from where the hand
    /// began: the drag plays from what played, not from what the record says.
    from: Vec<(String, f64)>,
}

impl Playing {
    /// No drag plays, nor waits for the code to answer its release.
    pub(crate) fn idle(&self) -> bool {
        self.drag.is_none()
    }

    /// `values` moved by how far what a lane played was from where the hand began.
    fn from_played<'a>(&self, values: &[(&'a str, f64)]) -> Vec<(&'a str, f64)> {
        let from = |path: &str| {
            (self.from.iter())
                .find(|(each, _)| each == path)
                .map_or(0.0, |(_, by)| *by)
        };
        (values.iter())
            .map(|(path, value)| (*path, value + from(path)))
            .collect()
    }
}

/// A drag that plays `values` of `instance` begins, the hand where it was pressed: a number a
/// lane plays into moves from what the lane plays now.
pub(crate) fn grab(state: &mut State, instance: &InstanceId, values: &[(&str, f64)]) {
    let (project, tick) = (state.session.project(), state.playhead);
    let from = (values.iter()).filter_map(|(path, rest)| {
        let played = lane_played(project, instance, path, tick)?;
        Some((path.to_string(), f64::from(played) - rest))
    });
    state.plays.from = from.collect();
}

/// What a lane plays into the number `path` of `instance` at `tick`, if one does.
fn lane_played(project: &Project, instance: &InstanceId, path: &str, tick: Ticks) -> Option<f32> {
    // As `live.field` reads a path: `t00/mix.gain_db` is `gain_db` of the child `t00/mix`.
    let (child, name) = match path.split_once('.') {
        Some((child, name)) if !child.contains('[') => (instance.child(child).ok()?, name),
        _ => (instance.clone(), path),
    };
    let mut values = Vec::new();
    project.lanes(&child)?.values_at(tick, &mut values);
    let (_, value) = values.into_iter().find(|(each, _)| **each == *name)?;
    Some(value)
}

/// One move of the drag: `values` by their paths in `instance`, as `drag.plays` names them.
/// Whether the drag goes on: a number a lane holds refuses it, in the session's words.
pub(crate) fn moved(state: &mut State, instance: &InstanceId, values: &[(&str, f64)]) -> bool {
    let playing = &mut state.plays;
    if playing.refused {
        return false;
    }
    let values = playing.from_played(values);
    let values = values.as_slice();
    let mut drag = (playing.drag.take()).unwrap_or_else(|| Drag::new(STEP));
    match state.session.play(&mut drag, instance, values) {
        Ok(()) => {
            playing.drag = Some(drag);
            defer(state, true);
            true
        }
        Err(held) => {
            // The session ended the gesture as the step it was so far.
            state.session.finish(drag);
            state.session.report(held);
            playing.refused = true;
            defer(state, false);
            false
        }
    }
}

/// The drag was let go: whether its release goes to the code. A refused one ends here.
pub(crate) fn let_go(state: &mut State) -> bool {
    let playing = &mut state.plays;
    if std::mem::take(&mut playing.refused) {
        return false;
    }
    if playing.drag.is_some() {
        playing.let_go = true;
        defer(state, false);
        // What an earlier action updated is not this release's.
        state.tools.as_mut().map(Tools::take_update);
    }
    true
}

/// Each frame: a drag let go ends once the code answered its release. One whose record was
/// deleted ended in the session; its element went with it, and Rune lets go of no element
/// that is gone.
pub(crate) fn settle(state: &mut State) {
    if state.plays.drag.is_some() && !state.session.gesture_open() {
        state.plays = Playing::default();
        defer(state, false);
        return;
    }
    if state.plays.let_go && state.tools.as_ref().is_none_or(Tools::answered) {
        let update = state.tools.as_mut().and_then(Tools::take_update);
        ended(state, update.as_deref());
    }
}

/// The drag let go, and the code answered: `update` names the update it sent for the end,
/// which landed, and the drag's one step; none puts back the sound from before the drag.
pub(crate) fn ended(state: &mut State, update: Option<&str>) {
    state.plays.let_go = false;
    let Some(drag) = state.plays.drag.take() else {
        return;
    };
    match update {
        Some(label) => state.session.finish_as(drag, label),
        None => state.session.cancel(drag),
    }
}

/// Escape: the sound from before the drag.
pub(crate) fn cancelled(state: &mut State) {
    (state.plays.refused, state.plays.let_go) = (false, false);
    defer(state, false);
    if let Some(drag) = state.plays.drag.take() {
        state.session.cancel(drag);
    }
}

/// An update of code that set numbers a hand writes into a lane now: into the drag that ends,
/// so it is one step with it, else as a value typed.
pub(crate) fn write(
    state: &mut State,
    instance: &InstanceId,
    label: &str,
    values: &[(String, f64)],
) {
    let values: Vec<(&str, f64)> = (values.iter())
        .map(|(path, value)| (path.as_str(), *value))
        .collect();
    let playing = &mut state.plays;
    // The code worked from where the hand began, as the drag did.
    let shifted = playing.from_played(&values);
    let played = match playing.drag.as_mut().filter(|_| playing.let_go) {
        Some(drag) => state.session.play(drag, instance, &shifted),
        None => state.session.set(label, instance, &values),
    };
    if let Err(held) = played {
        state.session.report(held);
    }
}

/// While a drag plays, the code hears none of the records it changes, so no view runs.
fn defer(state: &mut State, deferred: bool) {
    if let Some(tools) = &mut state.tools {
        tools.defer_records(deferred);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::{Value, json};

    use super::*;
    use crate::app::tests::{automated_cutoff, new_project, opened};

    fn track() -> InstanceId {
        InstanceId::new("song/t00").unwrap()
    }

    fn cutoff(state: &State) -> Option<f64> {
        let synth = InstanceId::new("song/t00/synth").unwrap();
        let record: Value =
            serde_json::from_str(&state.project().state_json(&synth).unwrap()).unwrap();
        record["cutoff_hz"].as_f64()
    }

    /// What the code does with the update at the end of a drag: an edit of its own.
    fn update(state: &mut State, label: &str, hz: u64) {
        let synth = InstanceId::new("song/t00/synth").unwrap();
        state.session.background(|project| {
            let mut edit = project.begin(label);
            (project.update_json(&mut edit, &synth, |s| s["cutoff_hz"] = json!(hz))).unwrap();
            project.finish(edit).unwrap();
        });
    }

    /// Whether the code keeps the value the hand left or another, such as a rounded one.
    #[test]
    fn the_update_at_the_end_of_a_drag_names_its_one_undo_step() {
        for kept in [1200, 1250] {
            let scratch = new_project();
            let mut state = opened(&scratch.path().join("song"));
            let before = (cutoff(&state), state.project().undo_label());
            moved(&mut state, &track(), &[("synth.cutoff_hz", 900.0)]);
            moved(&mut state, &track(), &[("synth.cutoff_hz", 1200.0)]);
            assert_eq!(cutoff(&state), Some(1200.0));
            assert_eq!(state.project().undo_label(), before.1);
            update(&mut state, "Turn the cutoff", kept);
            ended(&mut state, Some("Turn the cutoff"));
            assert_eq!(cutoff(&state), Some(kept as f64));
            let label = state.project().undo_label();
            assert_eq!(label.as_deref(), Some("Turn the cutoff"), "{kept}");
            state.undo().unwrap();
            assert_eq!(
                (cutoff(&state), state.project().undo_label()),
                before,
                "{kept}"
            );
        }
    }

    #[test]
    fn a_drag_with_no_update_or_an_escape_puts_the_sound_back() {
        let scratch = new_project();
        let mut state = opened(&scratch.path().join("song"));
        let before = (cutoff(&state), state.project().undo_label());
        moved(&mut state, &track(), &[("synth.cutoff_hz", 900.0)]);
        ended(&mut state, None);
        assert_eq!((cutoff(&state), state.project().undo_label()), before);
        moved(&mut state, &track(), &[("synth.cutoff_hz", 900.0)]);
        cancelled(&mut state);
        assert_eq!((cutoff(&state), state.project().undo_label()), before);
    }

    #[test]
    fn a_number_a_lane_holds_ends_the_drag_in_the_sessions_words() {
        let scratch = automated_cutoff();
        let mut state = opened(&scratch.path().join("song"));
        moved(&mut state, &track(), &[("synth.cutoff_hz", 900.0)]);
        let said = state.session.notice().unwrap().to_string();
        assert!(said.contains("turn on Write automation"), "{said}");
        // The rest of the drag does nothing.
        moved(&mut state, &track(), &[("synth.cutoff_hz", 1000.0)]);
        ended(&mut state, None);
        assert_eq!(cutoff(&state), Some(500.0));
        assert!(state.plays.drag.is_none());
    }
}
