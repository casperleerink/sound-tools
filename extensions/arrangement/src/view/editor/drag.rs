//! The mouse moves of a drag: notes drawn, moved and resized, velocities dragged and drawn,
//! and a line drawn or erased in an expression lane.

use gpui::Context;
use sound_notes::{Clip, Note};

use super::{NoteDrag, NoteDragKind, NoteEditor, located};
use crate::view::lanes::height_at;
use crate::view::layout::Viewport;
use crate::view::roll::{
    ROLL_HEIGHT, drawn_note, moved_notes, moved_velocity, nearest_pitch, resized_note, velocity_at,
    velocity_bar, velocity_bars_between,
};
use crate::view::snap::Grid;

impl NoteEditor {
    /// One mouse move of a gesture in an expression lane, into the gesture of the session,
    /// which opens with the first change.
    fn drag_expression(
        &mut self,
        drag: &mut NoteDrag,
        clip: &Clip,
        (x, y): (f32, f32),
        grid: &Grid,
        cx: &mut Context<Self>,
    ) {
        let viewport = self.painted.get();
        let NoteDragKind::Expression {
            lane,
            origin,
            stroke,
        } = &mut drag.kind
        else {
            return;
        };
        // An undo between mouse down and the first change may have changed the lane: the
        // stroke works from the clip as it is now, so it does not write the old lane back.
        if !drag.edit.is_open() {
            *origin = clip.clone();
        }
        if !stroke.moved(&viewport, grid, (x, y)) {
            return;
        }
        let (lane, origin, edit) = (*lane, &*origin, stroke.edit());
        let label = lane.label(&edit);
        let mut next = clip.clone();
        lane.edit(&mut next, origin, &edit);
        if next == *clip {
            return;
        }
        let instance = self.clip.clone();
        drag.edit
            .publish(&self.session, label, cx, |project, edit_of| {
                project.update(edit_of, &instance, |clip| lane.edit(clip, origin, &edit))
            });
    }

    /// One mouse move of a drag: the notes become what the pointer says, through the gesture
    /// of the session. `free` is cmd held: the drag does not snap.
    pub(super) fn drag_to(&mut self, x: f32, y: f32, free: bool, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        let grid = match free {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let project = self.session.read(cx).project();
        let Some(clip) = project.state(&self.clip).cloned() else {
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        if matches!(drag.kind, NoteDragKind::Expression { .. }) {
            self.drag_expression(&mut drag, &clip, (x, y - ROLL_HEIGHT), &grid, cx);
            self.drag = Some(drag);
            return;
        }
        if let NoteDragKind::DrawVelocity { last } = &mut drag.kind {
            let lane_y = y - ROLL_HEIGHT;
            let changes = drawn_velocities(&viewport, &clip, *last, (x, lane_y));
            *last = (x, lane_y);
            self.drag = Some(drag);
            return self.write(changes, cx);
        }
        // Something else may have changed the clip. Each note is found by what this drag wrote
        // last. A note that is gone is left out, and when none is left the drag ends.
        let found = located(&clip, drag.notes.iter().map(|tracked| tracked.written));
        let mut kept = Vec::new();
        let mut indices = Vec::new();
        for (tracked, index) in drag.notes.iter().zip(found) {
            if let Some(index) = index {
                kept.push(*tracked);
                indices.push(index);
            }
        }
        let Some(first) = kept.first().map(|tracked| tracked.origin) else {
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        drag.notes = kept;
        let origins: Vec<Note> = drag.notes.iter().map(|tracked| tracked.origin).collect();
        let pointer = viewport.tick_at(x);
        let next: Vec<Note> = match &drag.kind {
            NoteDragKind::Draw { down } => {
                vec![drawn_note(&clip, *down, pointer, first.pitch, &grid).unwrap_or(first)]
            }
            NoteDragKind::Move {
                grab,
                grab_pitch,
                grabbed,
            } => {
                let semitones = i32::from(nearest_pitch(&viewport, y).number())
                    - i32::from(grab_pitch.number());
                let delta = grid.delta(clip.start + grabbed.start, *grab, pointer);
                moved_notes(clip.length, &origins, delta, semitones)
            }
            NoteDragKind::Resize { grab } => {
                let end = clip.start + first.end();
                let delta = grid.delta(end, *grab, pointer);
                vec![resized_note(clip.length, first, delta, grid.unit_at(end))]
            }
            NoteDragKind::Velocity { grab } => {
                let dy = y - ROLL_HEIGHT - grab;
                let moved = origins.iter().map(|origin| Note {
                    velocity: moved_velocity(origin.velocity, dy),
                    ..*origin
                });
                moved.collect()
            }
            NoteDragKind::DrawVelocity { .. } | NoteDragKind::Expression { .. } => Vec::new(),
        };
        let changes: Vec<(usize, Note, Note)> = indices
            .into_iter()
            .zip(&drag.notes)
            .zip(next)
            .map(|((index, tracked), next)| (index, tracked.written, next))
            .filter(|(_, written, next)| written != next)
            .collect();
        // A pitch that changed under the pointer sounds.
        let sounds = match &drag.kind {
            NoteDragKind::Move { grabbed, .. } => {
                let grabbed = drag.notes.iter().find(|tracked| tracked.origin == *grabbed);
                let grabbed = grabbed.map(|tracked| tracked.written);
                let moved = changes
                    .iter()
                    .find(|(_, written, _)| Some(*written) == grabbed);
                moved
                    .filter(|(_, written, next)| written.pitch != next.pitch)
                    .map(|(_, _, next)| *next)
            }
            _ => None,
        };
        for tracked in &mut drag.notes {
            if let Some((_, _, next)) = changes.iter().find(|(_, old, _)| *old == tracked.written) {
                tracked.written = *next;
            }
        }
        self.drag = Some(drag);
        self.write(changes, cx);
        if let Some(note) = sounds {
            self.preview(note.pitch, note.velocity, cx);
        }
    }

    /// Publishes notes of the clip that a drag changed, `(index, was, is)`, into the gesture,
    /// which opens with the first change. The selection follows them.
    fn write(&mut self, changes: Vec<(usize, Note, Note)>, cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        let Some(drag) = &mut self.drag else {
            return;
        };
        let label = drag.kind.label(drag.notes.len().max(changes.len()));
        let instance = self.clip.clone();
        let written: Vec<(usize, Note)> = changes.iter().map(|(i, _, n)| (*i, *n)).collect();
        let published = drag
            .edit
            .publish(&self.session, label, cx, |project, edit| {
                project.update(edit, &instance, |clip| {
                    for (index, next) in written {
                        if let Some(note) = clip.notes.get_mut(index) {
                            *note = next;
                        }
                    }
                })
            });
        if published.is_some() {
            let pairs: Vec<(Note, Note)> = changes.iter().map(|(_, a, b)| (*a, *b)).collect();
            self.follow_selection(&pairs, cx);
        }
    }
}

/// What a draw in the lane from `from` to `to` does: every bar between the two places gets the
/// velocity of the height of the line between them there. `(index, was, is)` of what changes.
fn drawn_velocities(
    viewport: &Viewport,
    clip: &Clip,
    from: (f32, f32),
    to: (f32, f32),
) -> Vec<(usize, Note, Note)> {
    let passed = velocity_bars_between(viewport, clip, from.0, to.0);
    passed
        .into_iter()
        .filter_map(|index| {
            let note = *clip.notes.get(index)?;
            let bar = velocity_bar(viewport, clip, &note);
            let velocity = velocity_at(height_at(from, to, bar.x));
            let next = Note { velocity, ..note };
            (next != note).then_some((index, note, next))
        })
        .collect()
}
