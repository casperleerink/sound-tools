//! The note editor: the notes of one clip as a piano roll, in a panel below the timeline, with
//! a lane at its bottom. Notes are added, selected, moved, resized, copied, pasted and deleted
//! here, and a note that is touched sounds for a moment through the instrument of its track.
//! The lane shows the velocities, which are dragged and drawn there, or one expression lane of
//! the clip, which is drawn, erased and cleared there ([`super::lanes`]).
//!
//! All positions and what a drag does to a note come from [`super::roll`]. The editor keeps
//! no copy of the clip: it reads it when it paints and when a mouse event arrives. The selected
//! notes are kept by value in a [`Selection`], the same one the timeline keeps its clips in,
//! and follow the rules of the clips: a click, shift-click and cmd-click, and a rectangle.
//!
//! This file holds the view itself: its fields, what the mouse holds, the project events, the
//! selection, the viewport, the render and the mouse listeners. The rest is in `editor/`, as
//! more `impl NoteEditor`:
//! - `mouse`: what a press takes, the marquee, the end of a drag, hover and the cursor.
//! - `drag`: the mouse moves of a drag of notes, of velocities or in an expression lane.
//! - `keys`: the keys, and copy, cut, paste, duplicate and delete.
//! - `scene`: what one paint shows, and its painting.

mod drag;
mod keys;
mod mouse;
mod scene;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, EventEmitter, FocusHandle, Focusable, Hitbox,
    HitboxBehavior, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels,
    Point, ScrollWheelEvent, Subscription, Window, canvas, div, prelude::*, px,
};
use sound_core::{Changes, Instance, InstanceId, ProjectEvent, Ticks};
use sound_notes::{Clip, Note, Pitch, Velocity};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::{ActiveTheme, DragEdit, KeyboardFocus, Session};

use self::scene::paint_roll;
use super::clipboard::SharedClipboard;
use super::lanes::{Lane, Shown, Stroke};
use super::layout::{HEADER_WIDTH, RULER_HEIGHT, Viewport};
use super::paint::paint_focus_ring;
use super::plural;
use super::roll::{ROLL_HEIGHT, VELOCITY_HEIGHT, clamped, opened};
use super::scrolled_or_zoomed;
use super::selection::Selection;
use super::snap::{Grid, SharedSnap};
use crate::preview_note;

/// What the editor asks of the view that holds it.
pub enum EditorEvent {
    /// Escape or the close control.
    Close,
}

/// What a drag in the editor does.
enum NoteDragKind {
    /// Draws a new note from where it started at mouse down, on the grid.
    Draw { down: Ticks },
    /// Moves the notes in time and pitch, as a whole. The tick and the pitch under the pointer
    /// at mouse down, and the note under it, which sounds when its pitch changes. With `copy`,
    /// alt held at the press, it moves copies and the notes stay.
    Move {
        grab: Ticks,
        grab_pitch: Pitch,
        grabbed: Note,
        copy: bool,
    },
    /// Moves the end of one note.
    Resize { grab: Ticks },
    /// Up and down on a bar of the lane: every dragged velocity by the same distance, each from
    /// where it was at mouse down. `grab` is the height of the pointer then.
    Velocity { grab: f32 },
    /// Across the lane: every bar the pointer passes gets the velocity of its height there.
    /// `last` is where the pointer was at the last mouse move, in the lane.
    DrawVelocity { last: (f32, f32) },
    /// Across an expression lane: the line the pointer draws, or with alt the points it
    /// erases, over the lane of the clip at mouse down.
    Expression {
        lane: Lane,
        origin: Box<Clip>,
        stroke: Stroke,
    },
}

impl NoteDragKind {
    fn label(&self, count: usize) -> &'static str {
        match self {
            Self::Draw { .. } => "Draw note",
            Self::Move { copy: true, .. } => plural(count, "Copy note", "Copy notes"),
            Self::Move { .. } => plural(count, "Move note", "Move notes"),
            Self::Resize { .. } => "Resize note",
            Self::Velocity { .. } => plural(count, "Change velocity", "Change velocities"),
            Self::DrawVelocity { .. } => "Draw velocities",
            Self::Expression { lane, stroke, .. } => lane.label(&stroke.edit()),
        }
    }
}

/// One note a drag changes: as it was at mouse down, which every move starts from, and as the
/// drag wrote it last, which finds it again when something else changed the clip.
#[derive(Clone)]
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
    /// The notes it changes. Empty for a draw in the lane, which finds its bars per move, and
    /// for a gesture in an expression lane.
    notes: Vec<Tracked>,
    /// It opens with the first change, so a plain click on a note is no undo step.
    edit: DragEdit,
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
    /// The events arriving now come from an undo or a redo: the count moved at the first of
    /// them, and it is cleared when the group is over.
    history_group: bool,
    /// What was selected before the last single press, which a double click that draws puts
    /// back on escape: its first click already let go of the selection.
    before_click: Selection<Note>,
    drag: Option<NoteDrag>,
    marquee: Option<Marquee>,
    /// What the lane at the bottom shows. Kept when another clip opens.
    shown: Shown,
    /// The select left of the lane that picks what it shows.
    lane_menu: Entity<DropdownMenu>,
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
    /// An expression lane: a drag draws.
    Draw,
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
            // Every event, not only those of this clip, sees the count of undo and redo, so an
            // undo elsewhere is not taken for one of this clip later.
            editor.note_history(cx);
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
            if let Some(mut drag) = editor.drag.take() {
                drag.edit.finish(&editor.session, cx);
            }
        })
        .detach();
        let lane_menu = cx.new(|cx| {
            let items = Shown::ALL.map(|(_, value, label)| MenuItem::new(value, label));
            let entries = vec![MenuEntry::Group(
                MenuGroup::new().label("Lane").items(items),
            )];
            DropdownMenu::new("Lane", entries, cx)
                .debug_name("editor-lane")
                .trigger(Trigger::Select)
                .width(160.)
                .selected(Shown::default().value())
        });
        cx.subscribe(&lane_menu, |editor, _, picked: &MenuPicked, cx| {
            if let Some(shown) = Shown::from_value(&picked.0) {
                editor.show(shown, cx);
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
            history_group: false,
            before_click: Selection::default(),
            drag: None,
            marquee: None,
            shown: Shown::default(),
            lane_menu,
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

    /// What the lane at the bottom shows.
    pub fn shown(&self) -> Shown {
        self.shown
    }

    /// Shows the velocities or an expression lane. A drag in the lane ends first.
    pub fn show(&mut self, shown: Shown, cx: &mut Context<Self>) {
        if self.shown != shown {
            self.end_drag(cx);
            self.shown = shown;
            let value = shown.value();
            self.lane_menu
                .update(cx, |menu, cx| menu.set_selected(value, cx));
            cx.notify();
        }
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
        let mut indices: Vec<usize> = located(clip, self.selection.iter().cloned())
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
            .filter_map(|index| clip?.notes.get(*index).cloned())
            .collect();
        let first = notes.first().cloned();
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
        self.set_selection(note.clone(), note, cx);
    }

    fn toggle(&mut self, note: Note, cx: &mut Context<Self>) {
        self.selection.toggle(note);
        cx.notify();
    }

    /// Notes that changed from `old` to `new` stay selected as what they are now.
    fn follow_selection(&mut self, changed: &[(Note, Note)], cx: &mut Context<Self>) {
        let renamed = |note: &Note| {
            let found = changed.iter().find(|(old, _)| old == note);
            found.map_or_else(|| note.clone(), |(_, new)| new.clone())
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
        let Some(clip) = self.session.read(cx).project().state(&self.clip) else {
            return;
        };
        let notes = clip.notes.clone();
        let known = std::mem::replace(&mut self.known, notes.clone());
        if !self.history_group || self.drag.is_some() {
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
            let first = brought.first().cloned();
            self.set_selection(brought, first, cx);
        }
    }

    /// Whether the group of events that arrives now comes from an undo or a redo: the count of
    /// the session moved since the last event. It holds until the group is over.
    fn note_history(&mut self, cx: &mut Context<Self>) {
        let history = self.session.read(cx).history_moves();
        if std::mem::replace(&mut self.seen_history, history) == history {
            return;
        }
        self.history_group = true;
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |editor, _| editor.history_group = false);
            }
        });
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
        let time_signatures = project.project_file().tempo_map.time_signatures();
        let width = self.painted_width.get();
        clamped(&viewport, clip, time_signatures, width, ROLL_HEIGHT)
    }

    /// The grid of the snap setting over the time signatures of the project.
    fn grid(&self, cx: &App) -> Grid {
        let project = self.session.read(cx).project();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        self.snap.get().grid(time_signatures)
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

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        self.set_viewport(scrolled_or_zoomed(self.viewport, event, x), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
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
            // The select of the lane, left of it, in the middle of its height.
            .child(
                div()
                    .absolute()
                    .bottom(px((VELOCITY_HEIGHT - 24.) / 2.))
                    .left(px(16.))
                    .child(self.lane_menu.clone()),
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
                    editor.drag_to(x, y, event.modifiers.secondary(), cx);
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
