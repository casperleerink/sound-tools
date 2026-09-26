//! The note editor: the notes of one clip as a piano roll, in a panel below the timeline, with
//! the velocity lane at its bottom. Notes are added, selected, moved, resized, copied, pasted
//! and deleted here, their velocities are dragged and drawn in the lane, and a note that is
//! touched sounds for a moment through the instrument of its track.
//!
//! All positions and what a drag does to a note come from [`super::roll`]. The editor keeps
//! no copy of the clip: it reads it when it paints and when a mouse event arrives. The selected
//! notes are kept by value in a [`Selection`], the same one the timeline keeps its clips in,
//! and follow the rules of the clips: a click, shift-click and cmd-click, and a rectangle.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hitbox, HitboxBehavior, Hsla, KeyDownEvent,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels,
    Point, ScrollWheelEvent, SharedString, Subscription, Window, canvas, div, fill, point,
    prelude::*, px, quad, size,
};
use sound_core::{Changes, Instance, InstanceId, ProjectEvent, Ticks};
use sound_notes::{Clip, Note, Pitch, Velocity};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, KeyboardFocus, Session};

use super::clipboard::{Copied, CopiedNotes, SharedClipboard};
use super::gesture::Zone;
use super::layout::{HEADER_WIDTH, RULER_HEIGHT, Rect, Viewport};
use super::paint::{
    Fit, accent, paint_focus_ring, paint_ruler, paint_text, paint_track_label, placed,
};
use super::roll::{
    DRAWN_VELOCITY, KEY_HEIGHT, KEYS_WIDTH, ROLL_HEIGHT, VELOCITY_HEIGHT, clamped, drawn_note,
    is_black_key, key_label, moved_notes, moved_velocity, nearest_pitch, note_at, note_rect,
    notes_in, opened, pitch_at, resized_note, velocity_at, velocity_bar, velocity_bars_at,
    velocity_bars_between, velocity_y, visible_pitches, y_of,
};
use super::scrolled_or_zoomed;
use super::selection::Selection;
use super::snap::{Grid, SharedSnap, snap, snapped_delta};
use crate::{TrackState, preview_note};

/// What the editor asks of the view that holds it.
pub enum EditorEvent {
    /// Escape or the close control.
    Close,
}

/// How far the alt arrows move the velocity of the selected notes.
const VELOCITY_STEP: i64 = 10;

/// What a drag in the editor does.
enum NoteDragKind {
    /// Draws a new note from where it started at mouse down, on the grid.
    Draw { down: Ticks },
    /// Moves the notes in time and pitch, as a whole. The tick and the pitch under the pointer
    /// at mouse down, and the note under it, which sounds when its pitch changes.
    Move {
        grab: Ticks,
        grab_pitch: Pitch,
        grabbed: Note,
    },
    /// Moves the end of one note.
    Resize { grab: Ticks },
    /// Up and down on a bar of the lane: every dragged velocity by the same distance, each from
    /// where it was at mouse down. `grab` is the height of the pointer then.
    Velocity { grab: f32 },
    /// Across the lane: every bar the pointer passes gets the velocity of its height there.
    /// `last` is where the pointer was at the last mouse move, in the lane.
    DrawVelocity { last: (f32, f32) },
}

impl NoteDragKind {
    fn label(&self, count: usize) -> &'static str {
        match self {
            Self::Draw { .. } => "Draw note",
            Self::Move { .. } => plural(count, "Move note", "Move notes"),
            Self::Resize { .. } => "Resize note",
            Self::Velocity { .. } => plural(count, "Change velocity", "Change velocities"),
            Self::DrawVelocity { .. } => "Draw velocities",
        }
    }
}

/// One note a drag changes: as it was at mouse down, which every move starts from, and as the
/// drag wrote it last, which finds it again when something else changed the clip.
#[derive(Copy, Clone)]
struct Tracked {
    origin: Note,
    written: Note,
}

/// What a press on a note that comes up without a move does to the selection, as for clips.
enum OnRelease {
    /// A plain click on one of several selected notes selects it alone.
    SelectAlone(Note),
    /// A cmd-click adds the note to the selection or takes it out.
    Toggle(Note),
}

struct NoteDrag {
    kind: NoteDragKind,
    /// The notes it changes. Empty for a draw in the lane, which finds its bars per move.
    notes: Vec<Tracked>,
    /// Whether the gesture of the session is open. It opens with the first change, so a plain
    /// click on a note is no undo step.
    begun: bool,
    on_release: Option<OnRelease>,
    /// What was selected when the drag began, which escape puts back.
    at_press: Selection<Note>,
}

/// A drag on empty space of the note area: the notes it touches are selected. Its corners are
/// a tick and a height from the top of pitch 127, so a scroll during it keeps its start.
struct Marquee {
    from: (Ticks, f64),
    to: (Ticks, f64),
    /// What was selected before, which a drag with shift or cmd adds to.
    before: Vec<Note>,
    /// What was selected at the press, for escape.
    at_press: Selection<Note>,
}

pub struct NoteEditor {
    session: Entity<Session>,
    clip: Instance<Clip>,
    viewport: Viewport,
    /// The snap of the window, shared with the timeline.
    snap: SharedSnap,
    /// The clipboard of the window, shared with the timeline.
    clipboard: SharedClipboard,
    /// The viewport of the last paint, for the playhead line and the mouse.
    painted: Rc<Cell<Viewport>>,
    /// The width of the note area at the last paint. Before the first paint it is the width
    /// of the timeline above, which is the same.
    painted_width: Rc<Cell<f32>>,
    /// The selected notes, by value and not by index: the clip changes under the editor, by an
    /// agent, an undo or a clip resize, and an index would then name another note. They are
    /// looked up when they are used, and leave the selection when the clip no longer has them.
    selection: Selection<Note>,
    /// The notes of the clip at the last event, and the count of undo and redo of the session
    /// then, to tell what an undo or a redo brought: it selects that.
    known: Vec<Note>,
    seen_history: u64,
    drag: Option<NoteDrag>,
    marquee: Option<Marquee>,
    /// What the pointer is over, so the cursor says what a drag from there does.
    hover: Hover,
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    close_focus: FocusHandle,
    _project_events: Subscription,
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
enum Hover {
    #[default]
    Nothing,
    /// The end of a note: a drag resizes.
    Edge,
    /// A bar of the lane: a drag changes its velocity.
    Bar,
}

impl EventEmitter<EditorEvent> for NoteEditor {}

impl NoteEditor {
    /// `width` is that of the note area, which the first zoom fits the clip into.
    pub(super) fn new(
        session: Entity<Session>,
        clip: Instance<Clip>,
        width: f32,
        snap: SharedSnap,
        clipboard: SharedClipboard,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle().tab_stop(true);
        let project_events = cx.subscribe(&session, |editor, _, event, cx| {
            let track = editor.clip.id().parent();
            let shown = |id: &InstanceId| id == editor.clip.id() || Some(id) == track.as_ref();
            let changed = match event {
                ProjectEvent::Created(id) | ProjectEvent::Changed(id) => shown(id),
                // The view that holds the editor closes it or gives it another clip.
                ProjectEvent::Deleted(_) | ProjectEvent::ProblemsChanged => false,
                ProjectEvent::ProjectFileChanged => true,
            };
            if changed {
                editor.drop_lost_selection(cx);
                editor.select_what_history_brought(cx);
                cx.notify();
            }
        });
        // A drag that is still open when the editor goes away must not leave the gesture of
        // the session open: undo and redo wait for it. The view that closes the editor ends
        // the drag first. This is the net under every other way to go.
        cx.on_release(|editor, cx| {
            if editor.drag.take().is_some_and(|drag| drag.begun) {
                let session = editor.session.clone();
                session.update(cx, |session, cx| session.finish_gesture(cx));
            }
        })
        .detach();
        let mut editor = Self {
            session,
            clip: clip.clone(),
            viewport: Viewport::default(),
            snap,
            clipboard,
            painted: Rc::default(),
            painted_width: Rc::new(Cell::new(width)),
            selection: Selection::default(),
            known: Vec::new(),
            seen_history: 0,
            drag: None,
            marquee: None,
            hover: Hover::Nothing,
            focus_handle,
            keyboard_focus: KeyboardFocus::default(),
            close_focus: cx.focus_handle().tab_stop(true),
            _project_events: project_events,
        };
        editor.set_clip(clip, cx);
        editor
    }

    pub fn clip(&self) -> &Instance<Clip> {
        &self.clip
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub(super) fn painted(&self) -> Rc<Cell<Viewport>> {
        self.painted.clone()
    }

    /// Whether the focus ring shows: the editor has the focus, and it came from the keyboard.
    pub fn shows_focus_ring(&self, window: &Window) -> bool {
        self.keyboard_focus.shows_ring(&self.focus_handle, window)
    }

    /// The index of the first selected note in the clip as it is now. `None` when nothing is
    /// selected or the clip no longer has the note.
    pub fn selected_note(&self, cx: &App) -> Option<usize> {
        let clip = self.session.read(cx).project().state(&self.clip)?;
        let first = self.selection.primary()?;
        clip.notes.iter().position(|note| note == first)
    }

    /// The indices of every selected note in the clip as it is now, in order.
    pub fn selected_notes(&self, cx: &App) -> Vec<usize> {
        let Some(clip) = self.session.read(cx).project().state(&self.clip) else {
            return Vec::new();
        };
        let mut indices: Vec<usize> = located(clip, self.selection.iter().copied())
            .into_iter()
            .flatten()
            .collect();
        indices.sort_unstable();
        indices
    }

    /// Selects the notes at these indices of the clip as it is now, the first of them first.
    pub fn select_notes(&mut self, indices: &[usize], cx: &mut Context<Self>) {
        let clip = self.session.read(cx).project().state(&self.clip);
        let notes: Vec<Note> = indices
            .iter()
            .filter_map(|index| clip?.notes.get(*index).copied())
            .collect();
        let first = notes.first().copied();
        self.set_selection(notes, first, cx);
    }

    fn set_selection(
        &mut self,
        notes: impl IntoIterator<Item = Note>,
        primary: Option<Note>,
        cx: &mut Context<Self>,
    ) {
        let before = self.selection.clone();
        self.selection.set(notes, primary);
        if self.selection != before {
            cx.notify();
        }
    }

    fn select_alone(&mut self, note: Option<Note>, cx: &mut Context<Self>) {
        self.set_selection(note, note, cx);
    }

    fn toggle(&mut self, note: Note, cx: &mut Context<Self>) {
        self.selection.toggle(note);
        cx.notify();
    }

    /// Notes that changed from `old` to `new` stay selected as what they are now.
    fn follow_selection(&mut self, changed: &[(Note, Note)], cx: &mut Context<Self>) {
        let renamed = |note: &Note| {
            let found = changed.iter().find(|(old, _)| old == note);
            found.map_or(*note, |(_, new)| *new)
        };
        let notes: Vec<Note> = self.selection.iter().map(renamed).collect();
        let primary = self.selection.primary().map(renamed);
        self.set_selection(notes, primary, cx);
    }

    /// The clip changed. A selected note that it no longer has is not selected any more, so a
    /// key never edits a note that only took its place. A drag keeps the selection on what it
    /// wrote last.
    fn drop_lost_selection(&mut self, cx: &App) {
        if self.drag.is_some() {
            return;
        }
        let Some(clip) = self.session.read(cx).project().state(&self.clip) else {
            return;
        };
        self.selection.retain(|note| clip.notes.contains(note));
    }

    /// An undo or a redo selects the notes it brought: those the clip has now and did not have
    /// before, such as deleted notes that came back or moved notes where they were. The
    /// selection names notes by value, so without this an undo of a move would leave nothing
    /// selected.
    fn select_what_history_brought(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let history = session.history_moves();
        let Some(clip) = session.project().state(&self.clip) else {
            return;
        };
        let notes = clip.notes.clone();
        let known = std::mem::replace(&mut self.known, notes.clone());
        if std::mem::replace(&mut self.seen_history, history) == history || self.drag.is_some() {
            return;
        }
        let mut before = known;
        let brought: Vec<Note> = notes
            .into_iter()
            .filter(|note| match before.iter().position(|had| had == note) {
                Some(index) => {
                    before.swap_remove(index);
                    false
                }
                None => true,
            })
            .collect();
        if !brought.is_empty() {
            let first = brought.first().copied();
            self.set_selection(brought, first, cx);
        }
    }

    /// Shows another clip: zoomed to fit it, with the middle of its notes in the middle.
    pub fn set_clip(&mut self, clip: Instance<Clip>, cx: &mut Context<Self>) {
        self.end_drag(cx);
        self.marquee = None;
        self.clip = clip;
        self.selection = Selection::default();
        self.seen_history = self.session.read(cx).history_moves();
        if let Some(state) = self.session.read(cx).project().state(&self.clip) {
            self.known = state.notes.clone();
            self.viewport = opened(state, self.painted_width.get(), ROLL_HEIGHT);
            self.painted.set(self.viewport);
        }
        cx.notify();
    }

    fn set_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        let viewport = self.clamped(viewport, cx);
        if self.viewport != viewport {
            self.viewport = viewport;
            cx.notify();
        }
    }

    fn clamped(&self, viewport: Viewport, cx: &App) -> Viewport {
        let project = self.session.read(cx).project();
        let Some(clip) = project.state(&self.clip) else {
            return viewport;
        };
        let time_signature = project.project_file().tempo_map.time_signature();
        let width = self.painted_width.get();
        clamped(&viewport, clip, time_signature, width, ROLL_HEIGHT)
    }

    /// The grid of the snap setting in the time signature of the project.
    fn grid(&self, cx: &App) -> Grid {
        let project = self.session.read(cx).project();
        let time_signature = project.project_file().tempo_map.time_signature();
        self.snap.get().grid(time_signature)
    }

    /// The position of a mouse event in the coordinates of [`super::roll`].
    fn note_area_position(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - bounds.left()) - HEADER_WIDTH,
            f32::from(position.y - bounds.top()) - RULER_HEIGHT,
        )
    }

    /// Sounds a pitch for a moment through the instrument of the track of the clip.
    fn preview(&self, pitch: Pitch, velocity: Velocity, cx: &mut Context<Self>) {
        let Some(track) = self.clip.id().parent() else {
            return;
        };
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| preview_note(project, &track, pitch, velocity))
        });
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, x: f32, y: f32, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        let grid = self.grid(cx);
        if y < 0.0 {
            if x >= 0.0 {
                let tick = snap(viewport.tick_at(x), grid.step);
                self.session
                    .update(cx, |session, _| session.engine().seek(tick));
            }
            return;
        }
        if y >= ROLL_HEIGHT {
            if x >= 0.0 {
                self.press_lane(event.modifiers, x, y - ROLL_HEIGHT, cx);
            }
            return;
        }
        let Some(pitch) = pitch_at(&viewport, y) else {
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
        let hit = note_at(&viewport, clip, x, y);
        let hit = hit.and_then(|(index, zone)| Some((zone, *clip.notes.get(index)?)));
        if let Some((zone, note)) = hit {
            let kind = match zone {
                Zone::RightEdge => NoteDragKind::Resize { grab: pointer },
                Zone::Body | Zone::LeftEdge => NoteDragKind::Move {
                    grab: pointer,
                    grab_pitch: pitch,
                    grabbed: note,
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
        let cmd = modifiers.platform;
        let several = matches!(
            kind,
            NoteDragKind::Move { .. } | NoteDragKind::Velocity { .. }
        );
        let dragged: Vec<Note> = match (several, self.selection.contains(&note) || cmd) {
            (true, true) => {
                let mut notes: Vec<Note> = self.selection.iter().copied().collect();
                if !notes.contains(&note) {
                    notes.push(note);
                }
                notes
            }
            _ => {
                if !cmd {
                    self.select_alone(Some(note), cx);
                }
                vec![note]
            }
        };
        let on_release = match (cmd, dragged.len() > 1) {
            (true, _) => Some(OnRelease::Toggle(note)),
            (false, true) => Some(OnRelease::SelectAlone(note)),
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
                    origin: note,
                    written: note,
                })
                .collect(),
            begun: false,
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
            .copied();
        let Some(note) = nearest else {
            self.drag = Some(NoteDrag {
                kind: NoteDragKind::DrawVelocity { last: (x, y) },
                notes: Vec::new(),
                begun: false,
                on_release: None,
                at_press: self.selection.clone(),
            });
            return;
        };
        self.press_note(note, NoteDragKind::Velocity { grab: y }, modifiers, cx);
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
        let Some(note) = drawn_note(&clip, pointer, pointer, pitch, grid) else {
            return;
        };
        // The start the grid gave at the press, as a project tick. Every move draws from it,
        // so cmd pressed during the draw frees the end and never moves the start.
        let start = clip.start + note.start;
        let instance = self.clip.clone();
        let drawn = self.session.update(cx, |session, cx| {
            session.begin_gesture("Draw note", cx);
            session.gesture(cx, |project, edit| {
                project.update(edit, &instance, |clip| clip.notes.push(note))
            })
        });
        if drawn.is_none() {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
            return;
        }
        let at_press = self.selection.clone();
        self.select_alone(Some(note), cx);
        self.drag = Some(NoteDrag {
            kind: NoteDragKind::Draw { down: start },
            notes: vec![Tracked {
                origin: note,
                written: note,
            }],
            begun: true,
            on_release: None,
            at_press,
        });
        self.preview(note.pitch, note.velocity, cx);
    }

    /// A press on empty space of the note area begins a rectangle that selects the notes it
    /// touches. Without shift or cmd it starts from nothing selected.
    fn start_marquee(&mut self, x: f32, y: f32, modifiers: Modifiers, cx: &mut Context<Self>) {
        let at_press = self.selection.clone();
        if !(modifiers.shift || modifiers.platform) {
            self.select_alone(None, cx);
        }
        let viewport = self.painted.get();
        let corner = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        self.marquee = Some(Marquee {
            from: corner,
            to: corner,
            before: self.selection.iter().copied().collect(),
            at_press,
        });
    }

    /// The rectangle of the marquee in the note area, as the viewport shows it now.
    fn marquee_rect(&self, viewport: &Viewport) -> Option<Rect> {
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
    fn marquee_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
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
            let touched = notes_in(&viewport, clip, area);
            selected.extend(touched.iter().filter_map(|index| clip.notes.get(*index)));
        }
        let primary = self.selection.primary().copied();
        self.set_selection(selected, primary, cx);
        cx.notify();
    }

    /// One mouse move of a drag: the notes become what the pointer says, through the gesture
    /// of the session. `free` is cmd held: the drag does not snap.
    fn drag_to(&mut self, x: f32, y: f32, free: bool, cx: &mut Context<Self>) {
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
        if kept.is_empty() {
            self.drag = Some(drag);
            return self.end_drag(cx);
        }
        drag.notes = kept;
        let origins: Vec<Note> = drag.notes.iter().map(|tracked| tracked.origin).collect();
        let pointer = viewport.tick_at(x);
        let next: Vec<Note> = match &drag.kind {
            NoteDragKind::Draw { down } => {
                let origin = origins[0];
                vec![drawn_note(&clip, *down, pointer, origin.pitch, grid).unwrap_or(origin)]
            }
            NoteDragKind::Move {
                grab, grab_pitch, ..
            } => {
                let semitones = i32::from(nearest_pitch(&viewport, y).number())
                    - i32::from(grab_pitch.number());
                let delta = snapped_delta(*grab, pointer, grid.step);
                moved_notes(clip.length, &origins, delta, semitones)
            }
            NoteDragKind::Resize { grab } => {
                let delta = snapped_delta(*grab, pointer, grid.step);
                vec![resized_note(clip.length, origins[0], delta, grid.unit)]
            }
            NoteDragKind::Velocity { grab } => {
                let dy = y - ROLL_HEIGHT - grab;
                let moved = origins.iter().map(|origin| Note {
                    velocity: moved_velocity(origin.velocity, dy),
                    ..*origin
                });
                moved.collect()
            }
            NoteDragKind::DrawVelocity { .. } => Vec::new(),
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
        let begun = std::mem::replace(&mut drag.begun, true);
        let label = drag.kind.label(drag.notes.len().max(changes.len()));
        let instance = self.clip.clone();
        let written: Vec<(usize, Note)> = changes.iter().map(|(i, _, n)| (*i, *n)).collect();
        let published = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                project.update(edit, &instance, |clip| {
                    for (index, next) in written {
                        if let Some(note) = clip.notes.get_mut(index) {
                            *note = next;
                        }
                    }
                })
            })
        });
        if published.is_some() {
            let pairs: Vec<(Note, Note)> = changes.iter().map(|(_, a, b)| (*a, *b)).collect();
            self.follow_selection(&pairs, cx);
        }
    }

    /// Mouse up, or the notes went away under the drag: the gesture becomes one undo step. The
    /// notes are put in order first, because the file is written now and the agent doc asks
    /// for notes by start. A press that did not move changes the selection as the click it was.
    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.marquee = None;
        if let Some(drag) = self.drag.take() {
            if drag.begun {
                let instance = self.clip.clone();
                self.session.update(cx, |session, cx| {
                    session.gesture(cx, |project, edit| match project.state(&instance) {
                        Some(_) => project.update(edit, &instance, sort_notes),
                        // The clip went away under the drag. There is nothing to put in order.
                        None => Ok(()),
                    });
                    session.finish_gesture(cx);
                });
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
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(marquee) = self.marquee.take() {
            self.selection = marquee.at_press;
            cx.notify();
            return true;
        }
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if drag.begun {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
        }
        self.selection = drag.at_press;
        cx.notify();
        true
    }

    fn hover(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let viewport = self.painted.get();
        let hover = match project.state(&self.clip) {
            Some(clip) if x >= 0.0 && y >= ROLL_HEIGHT => {
                match velocity_bars_at(&viewport, clip, x).is_empty() {
                    true => Hover::Nothing,
                    false => Hover::Bar,
                }
            }
            Some(clip) if x >= 0.0 && y >= 0.0 => match note_at(&viewport, clip, x, y) {
                Some((_, Zone::RightEdge)) => Hover::Edge,
                _ => Hover::Nothing,
            },
            _ => Hover::Nothing,
        };
        if self.hover != hover {
            self.hover = hover;
            cx.notify();
        }
    }

    fn cursor(&self) -> Option<CursorStyle> {
        let hover = match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(NoteDragKind::Resize { .. }) => Hover::Edge,
            Some(NoteDragKind::Velocity { .. } | NoteDragKind::DrawVelocity { .. }) => Hover::Bar,
            Some(_) => Hover::Nothing,
            None => self.hover,
        };
        match hover {
            Hover::Nothing => None,
            Hover::Edge => Some(CursorStyle::ResizeLeftRight),
            Hover::Bar => Some(CursorStyle::ResizeUpDown),
        }
    }

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

    /// Commits a new list of notes for the clip as one undo step, in order.
    fn commit(&mut self, label: &str, mut clip: Clip, cx: &mut Context<Self>) -> bool {
        sort_notes(&mut clip);
        let instance = self.clip.clone();
        let committed = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(&instance, clip);
                project.commit(label, changes)
            })
        });
        committed.is_some()
    }

    /// The keys of the focused editor. Whether the key was one of them.
    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" && !modifiers.shift && !modifiers.platform && !modifiers.alt {
            if !self.cancel_drag(cx) {
                cx.emit(EditorEvent::Close);
            }
            return true;
        }
        // The mouse has the notes: a key would fight the next mouse move.
        if self.drag.is_some() || self.marquee.is_some() {
            return false;
        }
        match (modifiers.platform, modifiers.alt) {
            (true, false) if !modifiers.shift => self.on_command(key, cx),
            (false, true) if !modifiers.shift => match key {
                "up" => self.change_velocity(VELOCITY_STEP, cx),
                "down" => self.change_velocity(-VELOCITY_STEP, cx),
                _ => false,
            },
            (false, false) => self.on_plain_key(key, modifiers.shift, cx),
            _ => false,
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
        let step = self.grid(cx).unit.0 as i64;
        let (delta, semitones) = match (key, shift) {
            ("backspace" | "delete", false) => {
                self.delete(&clip, &selected, "Delete note", "Delete notes", cx);
                return true;
            }
            ("left", false) => (-step, 0),
            ("right", false) => (step, 0),
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

    /// Cmd-v: the copied notes, at the playhead when it is inside the clip, else at the start
    /// of the selected notes, else at the start of the clip. One undo step, and the pasted
    /// notes are selected.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let copied = match self.clipboard.borrow().as_ref() {
            Some(Copied::Notes(copied)) => copied.clone(),
            _ => return,
        };
        let Some((clip, selected)) = self.selected(cx) else {
            return;
        };
        let playhead = self.session.read(cx).playhead().read(cx).tick;
        let at = match (clip.start..clip.end()).contains(&playhead) {
            true => Ticks(playhead.0 - clip.start.0),
            false => selected
                .iter()
                .map(|(_, note)| note.start)
                .min()
                .unwrap_or_default(),
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

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        self.set_viewport(scrolled_or_zoomed(self.viewport, event, x), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
    }

    /// Everything to paint into a panel of this size, read from the project now. `None` when
    /// the clip is gone: the view that holds the editor closes it after the same event.
    fn scene(&self, bounds: Bounds<Pixels>, cx: &App) -> Option<RollScene> {
        let project = self.session.read(cx).project();
        let clip = project.state(&self.clip)?;
        let track = self.clip.id().parent();
        let track = track.and_then(|track| project.resolve::<TrackState>(&track));
        let track = track.and_then(|track| project.state(&track));
        let theme = cx.theme();
        let time_signature = project.project_file().tempo_map.time_signature();
        let width = f32::from(bounds.size.width) - HEADER_WIDTH;
        let height = f32::from(bounds.size.height) - RULER_HEIGHT - VELOCITY_HEIGHT;
        let viewport = clamped(&self.viewport, clip, time_signature, width, height);
        // Only what shows: a long clip has many notes and the editor shows a few bars of it.
        let ticks = viewport.visible_ticks(width);
        let pitches = visible_pitches(&viewport, height);
        let in_time = |note: &&Note| {
            let start = clip.start + note.start;
            start < ticks.end && clip.start + note.end() > ticks.start
        };
        let visible = clip
            .notes
            .iter()
            .filter(in_time)
            .filter(|note| pitches.contains(&note.pitch.number()));
        let selected = |note: &Note| self.selection.contains(note);
        Some(RollScene {
            viewport,
            width,
            height,
            track_name: track
                .map(|track| track.name.clone())
                .unwrap_or_default()
                .into(),
            accent: track.map_or(theme.blue, |track| accent(track.colour, theme)),
            bars: viewport.ruler_bars(time_signature, width).collect(),
            beats: viewport.beat_lines(time_signature, width).collect(),
            clip_start: viewport.x_of(clip.start).clamp(0.0, width),
            clip_end: viewport.x_of(clip.end()).clamp(0.0, width),
            notes: visible
                .map(|note| (note_rect(&viewport, clip, note), selected(note)))
                .collect(),
            velocities: clip
                .notes
                .iter()
                .filter(in_time)
                .map(|note| (velocity_bar(&viewport, clip, note), selected(note)))
                .collect(),
            marquee: self.marquee_rect(&viewport),
        })
    }
}

/// The notes of the clip at these values, as indices: each is the first equal note that no
/// earlier value took, so two equal notes are two places. `None` for a value the clip lacks.
fn located(clip: &Clip, notes: impl IntoIterator<Item = Note>) -> Vec<Option<usize>> {
    let mut taken = vec![false; clip.notes.len()];
    notes
        .into_iter()
        .map(|wanted| {
            let index = clip
                .notes
                .iter()
                .enumerate()
                .position(|(index, note)| *note == wanted && !taken[index])?;
            taken[index] = true;
            Some(index)
        })
        .collect()
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
    let height_at = |x: f32| match to.0 - from.0 {
        across if across.abs() < f32::EPSILON => to.1,
        across => from.1 + (to.1 - from.1) * ((x - from.0) / across).clamp(0.0, 1.0),
    };
    passed
        .into_iter()
        .filter_map(|index| {
            let note = *clip.notes.get(index)?;
            let bar = velocity_bar(viewport, clip, &note);
            let velocity = velocity_at(height_at(bar.x));
            let next = Note { velocity, ..note };
            (next != note).then_some((index, note, next))
        })
        .collect()
}

/// The undo label for one note or several.
fn plural(count: usize, one: &'static str, several: &'static str) -> &'static str {
    match count {
        1 => one,
        _ => several,
    }
}

fn ordered<T: PartialOrd>(a: T, b: T) -> (T, T) {
    if a <= b { (a, b) } else { (b, a) }
}

/// What one paint of the editor shows, in the coordinates of [`super::roll`].
struct RollScene {
    viewport: Viewport,
    width: f32,
    /// The height of the pitch rows, without the lane.
    height: f32,
    track_name: SharedString,
    accent: Hsla,
    bars: Vec<(u64, f32)>,
    beats: Vec<f32>,
    /// The part of the note area that is inside the clip.
    clip_start: f32,
    clip_end: f32,
    /// The visible notes, and whether each is selected.
    notes: Vec<(Rect, bool)>,
    /// The bars of the lane, in its coordinates, and whether the note of each is selected.
    velocities: Vec<(Rect, bool)>,
    /// The rectangle of a drag on empty space.
    marquee: Option<Rect>,
}

fn paint_roll(scene: &RollScene, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let (hairline, beat_line, black_row, outside, white_key, black_key, label, selection) = (
        theme.alpha_at(0.05),
        theme.alpha_at(0.025),
        theme.alpha_at(0.02),
        theme.gray_50.opacity(0.5),
        theme.alpha_at(0.10),
        theme.alpha_at(0.03),
        theme.gray_700,
        theme.gray_950,
    );
    let (marquee_fill, marquee_border) = (theme.alpha_at(0.05), theme.alpha_at(0.20));
    let RollScene {
        viewport,
        width,
        height,
        ..
    } = *scene;
    let ruler = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(0.)),
        size(px(width), px(RULER_HEIGHT)),
    );
    let area = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(px(width), px(height)),
    );
    let lane = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT + height)),
        size(px(width), px(VELOCITY_HEIGHT)),
    );
    let keys = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH - KEYS_WIDTH), px(RULER_HEIGHT)),
        size(px(KEYS_WIDTH), px(height)),
    );
    let names = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT)),
        size(px(HEADER_WIDTH - KEYS_WIDTH), px(height)),
    );
    let pitches = visible_pitches(&viewport, height);
    let row = |pitch: u8, row_width: f32, row_height: f32| Rect {
        x: 0.0,
        y: y_of(&viewport, Pitch::nearest(i64::from(pitch))),
        width: row_width,
        height: row_height,
    };
    let upright = |x: f32, top: Point<Pixels>, tall: f32| {
        Bounds::new(top + point(px(x.round()), px(0.)), size(px(1.), px(tall)))
    };
    // Notes live inside the clip. What is outside is a shade darker, in the rows and the lane.
    let veils = [(0.0, scene.clip_start), (scene.clip_end, width)];
    let paint_veils = |top: Point<Pixels>, tall: f32, window: &mut Window| {
        for (left, right) in veils {
            if right > left {
                let veil = Rect {
                    x: left,
                    y: 0.0,
                    width: right - left,
                    height: tall,
                };
                window.paint_quad(fill(placed(veil, top), outside));
            }
        }
    };

    paint_ruler(&scene.bars, ruler, window, cx);
    // The track of the clip, where the track headers are above.
    let name_width = HEADER_WIDTH - 44. - 40.;
    let name = scene.track_name.clone();
    let top = bounds.origin;
    paint_track_label(
        name,
        scene.accent,
        top,
        RULER_HEIGHT,
        name_width,
        window,
        cx,
    );

    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
        for pitch in pitches.clone().filter(|pitch| is_black_key(*pitch)) {
            let tint = placed(row(pitch, width, KEY_HEIGHT), area.origin);
            window.paint_quad(fill(tint, black_row));
        }
        for x in &scene.beats {
            window.paint_quad(fill(upright(*x, area.origin, height), beat_line));
        }
        for (_, x) in &scene.bars {
            window.paint_quad(fill(upright(*x, area.origin, height), hairline));
        }
        paint_veils(area.origin, height, window);
        // A selected note is filled with the text colour, which is the lightest there is, and
        // keeps its accent as the outline. An outline alone on a pastel fill was hard to see.
        for (rect, selected) in &scene.notes {
            let body = placed(*rect, area.origin);
            let fill_color = if *selected { selection } else { scene.accent };
            let radius = px(3.).min(body.size.width / 2.);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(body, radius, fill_color, px(1.), scene.accent, solid));
        }
        if let Some(rect) = scene.marquee {
            let solid = BorderStyle::Solid;
            let body = placed(rect, area.origin);
            window.paint_quad(quad(
                body,
                px(2.),
                marquee_fill,
                px(1.),
                marquee_border,
                solid,
            ));
        }
    });

    // The lane: a bar at the start of each note in the track colour at 70 %, the selected ones
    // in the text colour and on top.
    window.with_content_mask(Some(ContentMask { bounds: lane }), |window| {
        for (_, x) in &scene.bars {
            window.paint_quad(fill(upright(*x, lane.origin, VELOCITY_HEIGHT), hairline));
        }
        paint_veils(lane.origin, VELOCITY_HEIGHT, window);
        let bar_color = scene.accent.opacity(0.7);
        for selected in [false, true] {
            let bars = scene.velocities.iter().filter(|(_, is)| *is == selected);
            for (rect, _) in bars {
                let color = if selected { selection } else { bar_color };
                window.paint_quad(fill(placed(*rect, lane.origin), color));
            }
        }
    });
    let lane_label = bounds.origin + point(px(24.), px(RULER_HEIGHT + height + 19.));
    let (weight, fit) = (FontWeight::NORMAL, Fit::Truncate(HEADER_WIDTH - 48.));
    paint_text(
        "Velocity".into(),
        lane_label,
        12.,
        weight,
        label,
        fit,
        window,
        cx,
    );

    window.with_content_mask(Some(ContentMask { bounds: keys }), |window| {
        for pitch in pitches.clone() {
            let black = is_black_key(pitch);
            let color = if black { black_key } else { white_key };
            let key = row(pitch, KEYS_WIDTH - 1.0, KEY_HEIGHT - 1.0);
            window.paint_quad(fill(placed(key, keys.origin), color));
        }
    });
    // The names of the Cs, left of the keys. A name may reach over the rows next to it.
    window.with_content_mask(Some(ContentMask { bounds: names }), |window| {
        for pitch in pitches {
            let Some(name) = key_label(pitch) else {
                continue;
            };
            let middle = row(pitch, 0.0, KEY_HEIGHT).y + KEY_HEIGHT / 2.0;
            let origin =
                names.origin + point(names.size.width - px(8.), px((middle - 9.0).round()));
            let (weight, fit) = (FontWeight::NORMAL, Fit::AlignRight);
            paint_text(name.into(), origin, 12., weight, label, fit, window, cx);
        }
    });

    // One hairline above the panel, one under the ruler, one above the lane, one beside the
    // keys.
    let lines = [
        Bounds::new(bounds.origin, size(bounds.size.width, px(1.))),
        Bounds::new(
            bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
            size(bounds.size.width, px(1.)),
        ),
        Bounds::new(
            bounds.origin + point(px(0.), px(RULER_HEIGHT + height)),
            size(bounds.size.width, px(1.)),
        ),
        Bounds::new(
            bounds.origin + point(px(HEADER_WIDTH - 1.), px(0.)),
            size(px(1.), bounds.size.height),
        ),
    ];
    for line in lines {
        window.paint_quad(fill(line, hairline));
    }
}

/// Notes by start and then by pitch, as the agent doc asks of whoever writes a clip.
fn sort_notes(clip: &mut Clip) {
    clip.notes.sort_by_key(|note| (note.start, note.pitch));
}

impl Focusable for NoteEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NoteEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let surface = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let Some(scene) = editor.read(cx).scene(bounds, cx) else {
                    return;
                };
                editor.read(cx).painted.set(scene.viewport);
                editor.read(cx).painted_width.set(scene.width);
                paint_roll(&scene, bounds, window, cx);
                let keyboard_focus = &editor.read(cx).keyboard_focus;
                if keyboard_focus.shows_ring(&focus_handle, window) {
                    paint_focus_ring(bounds, window, cx);
                }
                if let Some(cursor) = editor.read(cx).cursor() {
                    window.set_cursor_style(cursor, &hitbox);
                }
                listen(editor, bounds, hitbox, window);
            },
        );
        let close = Button::icon_only("close-editor", "x")
            // Quiet until it is wanted: the icon is as muted as the ruler numbers.
            .opacity(0.6)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.close_focus)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Close)));
        div()
            .size_full()
            .relative()
            .bg(cx.theme().gray_100)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|editor, event, _, cx| {
                if editor.on_key(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(surface.size_full())
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(px(HEADER_WIDTH - 8. - 24.))
                    .child(close),
            )
    }
}

/// Mouse listeners live for one frame. They read the clip when an event arrives, with the
/// viewport that was painted, so a click hits what is on screen.
fn listen(editor: Entity<NoteEditor>, bounds: Bounds<Pixels>, hitbox: Hitbox, window: &mut Window) {
    window.on_mouse_event({
        let (editor, hitbox) = (editor.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            let hit = phase == DispatchPhase::Bubble && hitbox.is_hovered(window);
            if hit && event.button == MouseButton::Left {
                let (x, y) = NoteEditor::note_area_position(bounds, event.position);
                editor.update(cx, |editor, cx| {
                    window.focus(&editor.focus_handle, cx);
                    editor.keyboard_focus.pressed(cx);
                    editor.on_mouse_down(event, x, y, cx)
                });
            }
        }
    });
    // A drag goes on wherever the pointer is, until the button is up.
    window.on_mouse_event({
        let (editor, hitbox) = (editor.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = NoteEditor::note_area_position(bounds, event.position);
            editor.update(cx, |editor, cx| {
                let pressed = editor.drag.is_some() || editor.marquee.is_some();
                if !pressed {
                    if hitbox.is_hovered(window) {
                        editor.hover(x, y, cx);
                    }
                } else if !event.dragging() {
                    // The button came up somewhere that did not tell this window.
                    editor.end_drag(cx);
                } else if editor.marquee.is_some() {
                    editor.marquee_to(x, y, cx);
                } else {
                    editor.drag_to(x, y, event.modifiers.platform, cx);
                }
            });
        }
    });
    window.on_mouse_event({
        let editor = editor.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                editor.update(cx, |editor, cx| {
                    if editor.drag.is_some() || editor.marquee.is_some() {
                        editor.end_drag(cx);
                    }
                });
            }
        }
    });
    window.on_mouse_event({
        let (editor, hitbox) = (editor.clone(), hitbox.clone());
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                let (x, _) = NoteEditor::note_area_position(bounds, event.position);
                editor.update(cx, |editor, cx| editor.on_scroll(event, x, cx));
            }
        }
    });
    window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
            let (x, _) = NoteEditor::note_area_position(bounds, event.position);
            editor.update(cx, |editor, cx| editor.on_pinch(event, x, cx));
        }
    });
}
