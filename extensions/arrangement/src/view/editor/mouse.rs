//! The mouse on the note editor: what a press takes, the marquee, the end of a drag, hover
//! and the cursor.

use gpui::{Context, CursorStyle, Modifiers, MouseDownEvent};
use sound_core::Ticks;
use sound_notes::{Clip, Note, Pitch, Velocity};
use sound_ui::DragEdit;

use super::{Hover, Marquee, NoteDrag, NoteDragKind, NoteEditor, OnRelease, Tracked, sort_notes};
use crate::view::gesture::Zone;
use crate::view::lanes::{Lane, LaneEdit, Shown, Stroke};
use crate::view::layout::{Rect, Viewport, ordered};
use crate::view::roll::{
    DRAWN_VELOCITY, KEYS_WIDTH, ROLL_HEIGHT, drawn_note, note_at, notes_in, velocity_bars_at,
    velocity_y,
};
use crate::view::snap::Grid;

impl NoteEditor {
    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        if event.click_count < 2 {
            self.before_click = self.selection.clone();
        }
        let viewport = self.painted.get();
        let grid = self.grid(cx);
        if y < 0.0 {
            if x >= 0.0 {
                let tick = grid.snap(viewport.tick_at(x));
                self.session
                    .update(cx, |session, _| session.engine().seek(tick));
            }
            return;
        }
        if y >= ROLL_HEIGHT {
            match self.shown {
                _ if x < 0.0 => {}
                Shown::Velocity => self.press_lane(event.modifiers, x, y - ROLL_HEIGHT, cx),
                Shown::Lane(lane) => self.press_expression(lane, event, x, y - ROLL_HEIGHT, cx),
            }
            return;
        }
        let rows = self.rows();
        let Some(pitch) = rows.pitch_at(&viewport, y) else {
            return;
        };
        if x < 0.0 {
            // A key of the strip sounds its pitch.
            if x >= -KEYS_WIDTH {
                self.preview(pitch, Velocity::nearest(DRAWN_VELOCITY), cx);
            }
            return;
        }
        let project = self.session.read(cx).project();
        let Some(clip) = project.state(&self.clip) else {
            return;
        };
        let pointer = viewport.tick_at(x);
        let hit = note_at(&viewport, &rows, clip, x, y);
        let hit = hit.and_then(|(index, zone)| Some((zone, clip.notes.get(index)?.clone())));
        if let Some((zone, note)) = hit {
            let kind = match zone {
                Zone::RightEdge => NoteDragKind::Resize { grab: pointer },
                Zone::Body | Zone::LeftEdge => NoteDragKind::Move {
                    grab: pointer,
                    grab_pitch: pitch,
                    grabbed: note.clone(),
                    copy: event.modifiers.alt,
                },
            };
            self.press_note(note, kind, event.modifiers, cx);
            return;
        }
        if event.click_count == 2 {
            self.draw_note(clip.clone(), pointer, pitch, grid, cx);
        } else {
            self.start_marquee(x, y, event.modifiers, cx);
        }
    }

    /// A press on a note, or on its bar in the lane. Shift-click takes it in or out of the
    /// selection. A press on a selected note drags every selected note, and a cmd press drags
    /// them with this one; any other press selects it alone and drags it. A resize is of the
    /// pressed note only.
    fn press_note(
        &mut self,
        note: Note,
        kind: NoteDragKind,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if modifiers.shift {
            self.toggle(note, cx);
            return;
        }
        let cmd = modifiers.secondary();
        let several = matches!(
            kind,
            NoteDragKind::Move { .. } | NoteDragKind::Velocity { .. }
        );
        let dragged: Vec<Note> = match (several, self.selection.contains(&note) || cmd) {
            (true, true) => {
                let mut notes: Vec<Note> = self.selection.iter().cloned().collect();
                if !notes.contains(&note) {
                    notes.push(note.clone());
                }
                notes
            }
            _ => {
                if !cmd {
                    self.select_alone(Some(note.clone()), cx);
                }
                vec![note.clone()]
            }
        };
        let on_release = match (cmd, dragged.len() > 1) {
            (true, _) => Some(OnRelease::Toggle(note.clone())),
            (false, true) => Some(OnRelease::SelectAlone(note.clone())),
            (false, false) => None,
        };
        if matches!(
            kind,
            NoteDragKind::Move { .. } | NoteDragKind::Resize { .. }
        ) {
            self.preview(note.pitch, note.velocity, cx);
        }
        self.drag = Some(NoteDrag {
            kind,
            notes: dragged
                .into_iter()
                .map(|note| Tracked {
                    origin: note.clone(),
                    written: note,
                })
                .collect(),
            edit: DragEdit::default(),
            on_release,
            at_press: self.selection.clone(),
        });
    }

    /// A press in the velocity lane, at `y` from its top. On a bar it drags that velocity, and
    /// the other selected ones with it when its note is selected. Of the bars of a chord, which
    /// share a place, it takes the one whose top is nearest to the pointer. Anywhere else it
    /// draws: every bar the pointer passes gets the velocity of its height.
    fn press_lane(&mut self, modifiers: Modifiers, x: f32, y: f32, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        let project = self.session.read(cx).project();
        let Some(clip) = project.state(&self.clip) else {
            return;
        };
        let hit = velocity_bars_at(&viewport, clip, x);
        let nearest = hit
            .iter()
            .filter_map(|index| clip.notes.get(*index))
            .min_by(|a, b| {
                let distance = |note: &Note| (velocity_y(note.velocity) - y).abs();
                distance(a).total_cmp(&distance(b))
            })
            .cloned();
        let Some(note) = nearest else {
            self.drag = Some(NoteDrag {
                kind: NoteDragKind::DrawVelocity { last: (x, y) },
                notes: Vec::new(),
                edit: DragEdit::default(),
                on_release: None,
                at_press: self.selection.clone(),
            });
            return;
        };
        self.press_note(note, NoteDragKind::Velocity { grab: y }, modifiers, cx);
    }

    /// A press in an expression lane, at `y` from its top. A drag from it draws the line of the
    /// pointer, with alt it erases, and a double click clears the lane. A press alone changes
    /// nothing, so the first click of a double click is no undo step.
    fn press_expression(
        &mut self,
        lane: Lane,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        let Some(origin) = project.state(&self.clip).cloned() else {
            return;
        };
        if event.click_count == 2 {
            if !lane.is_empty(&origin) {
                let mut cleared = origin.clone();
                lane.edit(&mut cleared, &origin, &LaneEdit::Clear);
                self.commit(lane.label(&LaneEdit::Clear), cleared, cx);
            }
            return;
        }
        let inside = origin.start..origin.end();
        let viewport = self.painted.get();
        let stroke = Stroke::new(event.modifiers.alt, origin.start, inside, &viewport, (x, y));
        let kind = NoteDragKind::Expression {
            lane,
            origin: Box::new(origin),
            stroke,
        };
        self.drag = Some(NoteDrag {
            kind,
            notes: Vec::new(),
            edit: DragEdit::default(),
            on_release: None,
            at_press: self.selection.clone(),
        });
    }

    /// A double click on empty space inside the clip adds a note of one unit of the grid, which
    /// a drag of the second press draws longer. Outside the clip there is nothing to add into.
    fn draw_note(
        &mut self,
        clip: Clip,
        pointer: Ticks,
        pitch: Pitch,
        grid: Grid,
        cx: &mut Context<Self>,
    ) {
        let shape = self.rows().shape();
        let Some(note) = drawn_note(&clip, pointer, pointer, pitch, &grid, shape) else {
            return;
        };
        // The start the grid gave at the press, as a project tick. Every move draws from it,
        // so cmd pressed during the draw frees the end and never moves the start.
        let start = clip.start + note.start;
        let instance = self.clip.clone();
        // The note is the first change, so this drag opens at the press.
        let mut edit = DragEdit::default();
        let drawn = edit.publish(&self.session, "Draw note", cx, |project, edit| {
            project.update(edit, &instance, |clip| clip.notes.push(note.clone()))
        });
        if drawn.is_none() {
            edit.cancel(&self.session, cx);
            return;
        }
        // Escape gives back what was selected before the first click of the double click.
        let at_press = self.before_click.clone();
        self.select_alone(Some(note.clone()), cx);
        self.drag = Some(NoteDrag {
            kind: NoteDragKind::Draw { down: start },
            notes: vec![Tracked {
                origin: note.clone(),
                written: note.clone(),
            }],
            edit,
            on_release: None,
            at_press,
        });
        self.preview(note.pitch, note.velocity, cx);
    }

    /// A press on empty space of the note area begins a rectangle that selects the notes it
    /// touches. Without shift or cmd it starts from nothing selected.
    fn start_marquee(&mut self, x: f32, y: f32, modifiers: Modifiers, cx: &mut Context<Self>) {
        let at_press = self.selection.clone();
        if !(modifiers.shift || modifiers.secondary()) {
            self.select_alone(None, cx);
        }
        let viewport = self.painted.get();
        let corner = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        self.marquee = Some(Marquee {
            from: corner,
            to: corner,
            before: self.selection.iter().cloned().collect(),
            at_press,
        });
    }

    /// The rectangle of the marquee in the note area, as the viewport shows it now.
    pub(super) fn marquee_rect(&self, viewport: &Viewport) -> Option<Rect> {
        let marquee = self.marquee.as_ref()?;
        let (left, right) = ordered(marquee.from.0, marquee.to.0);
        let (top, bottom) = ordered(marquee.from.1, marquee.to.1);
        let x = viewport.x_of(left);
        Some(Rect {
            x,
            y: (top - viewport.scroll_y) as f32,
            width: viewport.x_of(right) - x,
            height: (bottom - top) as f32,
        })
    }

    /// One mouse move of the rectangle: the notes it touches and what was selected before.
    pub(super) fn marquee_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        let Some(marquee) = &mut self.marquee else {
            return;
        };
        // Down into the lane it stops at the lowest row that shows.
        let y = y.min(ROLL_HEIGHT);
        marquee.to = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        let mut selected = marquee.before.clone();
        let Some(area) = self.marquee_rect(&viewport) else {
            return;
        };
        if let Some(clip) = self.session.read(cx).project().state(&self.clip) {
            let touched = notes_in(&viewport, &self.rows(), clip, area);
            selected.extend(
                touched
                    .iter()
                    .filter_map(|index| clip.notes.get(*index).cloned()),
            );
        }
        let primary = self.selection.primary().cloned();
        self.set_selection(selected, primary, cx);
        cx.notify();
    }

    /// Mouse up, or the notes went away under the drag: the gesture becomes one undo step. The
    /// notes are put in order first, because the file is written now and the agent doc asks
    /// for notes by start. A press that did not move changes the selection as the click it was.
    pub(in crate::view) fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.marquee = None;
        if let Some(mut drag) = self.drag.take() {
            if drag.edit.is_open() {
                let instance = self.clip.clone();
                self.session.update(cx, |session, cx| {
                    session.gesture(cx, |project, edit| match project.state(&instance) {
                        Some(_) => project.update(edit, &instance, sort_notes),
                        // The clip went away under the drag. There is nothing to put in order.
                        None => Ok(()),
                    });
                });
                drag.edit.finish(&self.session, cx);
            } else {
                match drag.on_release {
                    Some(OnRelease::SelectAlone(note)) => self.select_alone(Some(note), cx),
                    Some(OnRelease::Toggle(note)) => self.toggle(note, cx),
                    None => {}
                }
            }
        }
        self.drop_lost_selection(cx);
        cx.notify();
    }

    /// Escape during a drag: the clip goes back to what it was at mouse down, and so does the
    /// selection. Whether there was a drag or a rectangle.
    pub(super) fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(marquee) = self.marquee.take() {
            self.selection = marquee.at_press;
            cx.notify();
            return true;
        }
        let Some(mut drag) = self.drag.take() else {
            return false;
        };
        drag.edit.cancel(&self.session, cx);
        self.selection = drag.at_press;
        cx.notify();
        true
    }

    pub(super) fn hover(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let viewport = self.painted.get();
        let hover = match project.state(&self.clip) {
            Some(_) if x >= 0.0 && y >= ROLL_HEIGHT && self.shown != Shown::Velocity => Hover::Draw,
            Some(clip) if x >= 0.0 && y >= ROLL_HEIGHT => {
                match velocity_bars_at(&viewport, clip, x).is_empty() {
                    true => Hover::Nothing,
                    false => Hover::Bar,
                }
            }
            Some(clip) if x >= 0.0 && y >= 0.0 => {
                match note_at(&viewport, &self.rows(), clip, x, y) {
                    Some((_, Zone::RightEdge)) => Hover::Edge,
                    _ => Hover::Nothing,
                }
            }
            _ => Hover::Nothing,
        };
        if self.hover != hover {
            self.hover = hover;
            cx.notify();
        }
    }

    pub(super) fn cursor(&self) -> Option<CursorStyle> {
        let hover = match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(NoteDragKind::Resize { .. }) => Hover::Edge,
            Some(NoteDragKind::Velocity { .. } | NoteDragKind::DrawVelocity { .. }) => Hover::Bar,
            Some(NoteDragKind::Expression { .. }) => Hover::Draw,
            Some(_) => Hover::Nothing,
            None => self.hover,
        };
        match hover {
            Hover::Nothing => None,
            Hover::Edge => Some(CursorStyle::ResizeLeftRight),
            Hover::Bar => Some(CursorStyle::ResizeUpDown),
            Hover::Draw => Some(CursorStyle::Crosshair),
        }
    }
}
