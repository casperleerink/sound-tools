//! The keys of the note editor: the arrows, delete, the velocity steps, and copy, cut, paste,
//! duplicate and select all.

use gpui::{App, Context, KeyDownEvent, Modifiers};
use sound_core::Ticks;
use sound_notes::{Clip, Note, Velocity};

use super::{EditorEvent, NoteEditor, located};
use crate::view::clipboard::{Copied, CopiedNotes};
use crate::view::plural;
use crate::view::roll::moved_notes;

/// How far the alt arrows move the velocity of the selected notes.
const VELOCITY_STEP: i64 = 10;

impl NoteEditor {
    /// The selected notes that the clip has now, each with its index, and the clip.
    fn selected(&self, cx: &App) -> Option<(Clip, Vec<(usize, Note)>)> {
        let clip = self.session.read(cx).project().state(&self.clip)?.clone();
        let notes: Vec<Note> = self.selection.iter().copied().collect();
        let found = located(&clip, notes.iter().copied());
        let selected: Vec<(usize, Note)> = found
            .into_iter()
            .zip(notes)
            .filter_map(|(index, note)| Some((index?, note)))
            .collect();
        Some((clip, selected))
    }

    /// The keys of the focused editor. Whether the key was one of them.
    pub(super) fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if key == "escape" && !modifiers.modified() {
            if !self.cancel_drag(cx) {
                cx.emit(EditorEvent::Close);
            }
            return true;
        }
        // The mouse has the notes: a key would fight the next mouse move.
        if self.drag.is_some() || self.marquee.is_some() {
            return false;
        }
        if modifiers == Modifiers::secondary_key() {
            self.on_command(key, cx)
        } else if modifiers == Modifiers::alt() {
            match key {
                "up" => self.change_velocity(VELOCITY_STEP, cx),
                "down" => self.change_velocity(-VELOCITY_STEP, cx),
                _ => false,
            }
        } else if !modifiers.modified() || modifiers == Modifiers::shift() {
            self.on_plain_key(key, modifiers.shift, cx)
        } else {
            false
        }
    }

    /// Delete and the arrows, on every selected note.
    fn on_plain_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let Some((clip, selected)) = self.selected(cx) else {
            return false;
        };
        if selected.is_empty() {
            return false;
        }
        let grid = self.grid(cx);
        let earliest = selected.iter().map(|(_, note)| note.start).min();
        let anchor = clip.start + earliest.unwrap_or_default();
        let (delta, semitones) = match (key, shift) {
            ("backspace" | "delete", false) => {
                self.delete(&clip, &selected, "Delete note", "Delete notes", cx);
                return true;
            }
            ("left", false) => (grid.nudge(anchor, false), 0),
            ("right", false) => (grid.nudge(anchor, true), 0),
            ("up", false) => (0, 1),
            ("down", false) => (0, -1),
            ("up", true) => (0, 12),
            ("down", true) => (0, -12),
            _ => return false,
        };
        let origins: Vec<Note> = selected.iter().map(|(_, note)| *note).collect();
        let next = moved_notes(clip.length, &origins, delta, semitones);
        if next == origins {
            return true;
        }
        let label = plural(next.len(), "Nudge note", "Nudge notes");
        let mut edited = clip.clone();
        for ((index, _), note) in selected.iter().zip(&next) {
            if let Some(slot) = edited.notes.get_mut(*index) {
                *slot = *note;
            }
        }
        if self.commit(label, edited, cx) {
            let pairs: Vec<(Note, Note)> = origins.iter().copied().zip(next).collect();
            self.follow_selection(&pairs, cx);
            let first = self.selection.primary().copied();
            let moved_first = pairs.iter().find(|(_, new)| Some(*new) == first);
            if let Some((old, new)) = moved_first
                && old.pitch != new.pitch
            {
                self.preview(new.pitch, new.velocity, cx);
            }
        }
        true
    }

    /// Alt with up or down: the velocity of every selected note, by a step.
    fn change_velocity(&mut self, delta: i64, cx: &mut Context<Self>) -> bool {
        let Some((mut clip, selected)) = self.selected(cx) else {
            return false;
        };
        if selected.is_empty() {
            return false;
        }
        let mut pairs = Vec::new();
        for (index, note) in &selected {
            let velocity = Velocity::nearest(i64::from(note.velocity.value()) + delta);
            let next = Note { velocity, ..*note };
            if let Some(slot) = clip.notes.get_mut(*index) {
                *slot = next;
            }
            pairs.push((*note, next));
        }
        if pairs.iter().all(|(old, new)| old == new) {
            return true;
        }
        let label = plural(pairs.len(), "Change velocity", "Change velocities");
        if self.commit(label, clip, cx) {
            self.follow_selection(&pairs, cx);
        }
        true
    }

    /// The keys with cmd: select all, copy, cut, paste and duplicate.
    fn on_command(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "a" => {
                let clip = self.session.read(cx).project().state(&self.clip);
                let notes: Vec<Note> = clip.map(|clip| clip.notes.clone()).unwrap_or_default();
                let primary = self.selection.primary().copied();
                self.set_selection(notes, primary, cx);
            }
            "c" => {
                self.copy(cx);
            }
            "x" => {
                if self.copy(cx)
                    && let Some((clip, selected)) = self.selected(cx)
                {
                    self.delete(&clip, &selected, "Cut note", "Cut notes", cx);
                }
            }
            "v" => self.paste(cx),
            "d" => self.duplicate(cx),
            _ => return false,
        }
        true
    }

    /// Cmd-c: the selected notes into the clipboard of the window. Whether there were any.
    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((_, selected)) = self.selected(cx) else {
            return false;
        };
        let Some(copied) = CopiedNotes::new(selected.into_iter().map(|(_, note)| note)) else {
            return false;
        };
        *self.clipboard.borrow_mut() = Some(Copied::Notes(copied));
        true
    }

    /// Takes the selected notes out of the clip, as one undo step. An undo brings them back
    /// selected, see [`Self::select_what_history_brought`].
    fn delete(
        &mut self,
        clip: &Clip,
        selected: &[(usize, Note)],
        one: &'static str,
        several: &'static str,
        cx: &mut Context<Self>,
    ) {
        let label = plural(selected.len(), one, several);
        let mut edited = clip.clone();
        let mut indices: Vec<usize> = selected.iter().map(|(index, _)| *index).collect();
        indices.sort_unstable();
        for index in indices.into_iter().rev() {
            edited.notes.remove(index);
        }
        if self.commit(label, edited, cx) {
            self.select_alone(None, cx);
        }
    }

    /// Cmd-v: the copied notes, at the playhead when it is inside the clip, else right after
    /// the selected notes, as a duplicate goes, else at the start of the clip. Never on the
    /// selection itself: a copy and a paste would stack exact copies on the notes. One undo
    /// step, and the pasted notes are selected.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let copied = match self.clipboard.borrow().as_ref() {
            Some(Copied::Notes(copied)) => copied.clone(),
            _ => return,
        };
        let Some((clip, selected)) = self.selected(cx) else {
            return;
        };
        let playhead = self.session.read(cx).playhead().read(cx).tick;
        let after_selection = CopiedNotes::new(selected.into_iter().map(|(_, note)| note))
            .map(|selected| selected.start() + selected.span());
        let at = match (clip.start..clip.end()).contains(&playhead) {
            true => Ticks(playhead.0 - clip.start.0),
            false => after_selection.unwrap_or_default(),
        };
        let notes = copied.placed(at, clip.length);
        self.add_notes(clip, notes, "Paste note", "Paste notes", cx);
    }

    /// Cmd-d: a copy of the selected notes right after them. The clipboard keeps what it had.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some((clip, selected)) = self.selected(cx) else {
            return;
        };
        let Some(copied) = CopiedNotes::new(selected.into_iter().map(|(_, note)| note)) else {
            return;
        };
        let notes = copied.placed(copied.start() + copied.span(), clip.length);
        self.add_notes(clip, notes, "Duplicate note", "Duplicate notes", cx);
    }

    fn add_notes(
        &mut self,
        mut clip: Clip,
        notes: Vec<Note>,
        one: &'static str,
        several: &'static str,
        cx: &mut Context<Self>,
    ) {
        if notes.is_empty() {
            return;
        }
        let label = plural(notes.len(), one, several);
        clip.notes.extend(notes.iter().copied());
        if self.commit(label, clip, cx) {
            let first = notes.first().copied();
            self.set_selection(notes, first, cx);
        }
    }
}
