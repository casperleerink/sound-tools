//! The note editor: the notes of one clip as a piano roll, in a panel below the timeline.
//! Notes are drawn, moved, resized and deleted here, and a note that is touched sounds for a
//! moment through the instrument of its track.
//!
//! All positions and what a drag does to a note come from [`super::roll`]. The editor keeps
//! no copy of the clip: it reads it when it paints and when a mouse event arrives. One note is
//! selected at a time, by its index in the clip.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hitbox, HitboxBehavior, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Subscription, Window, canvas, div, fill, point, prelude::*, px,
    quad, size,
};
use sound_core::{Instance, InstanceId, ProjectEvent, Ticks};
use sound_notes::{Clip, Length, Note, Pitch, Velocity};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, Session};

use super::gesture::Zone;
use super::layout::{HEADER_WIDTH, RULER_HEIGHT, Rect, SNAP, Viewport, snap, snapped_delta};
use super::paint::{
    Fit, accent, paint_focus_ring, paint_ruler, paint_text, paint_track_label, placed,
};
use super::roll::{
    DRAWN_VELOCITY, EDITOR_HEIGHT, KEY_HEIGHT, KEYS_WIDTH, clamped, drawn_note, is_black_key,
    key_label, moved_note, nearest_pitch, note_at, note_rect, opened, pitch_at, resized_note,
    visible_pitches, y_of,
};
use super::scrolled_or_zoomed;
use crate::{TrackState, preview_note};

/// What the editor asks of the view that holds it.
pub enum EditorEvent {
    /// Escape or the close control.
    Close,
}

/// What a drag in the note area does.
enum NoteDragKind {
    /// Draws a new note from the tick where the mouse went down.
    Draw { down: Ticks },
    /// Moves a note in time and pitch. The tick and the pitch under the pointer at mouse down.
    Move { grab: Ticks, grab_pitch: Pitch },
    /// Moves the end of a note.
    Resize { grab: Ticks },
}

impl NoteDragKind {
    fn label(&self) -> &'static str {
        match self {
            Self::Draw { .. } => "Draw note",
            Self::Move { .. } => "Move note",
            Self::Resize { .. } => "Resize note",
        }
    }
}

struct NoteDrag {
    kind: NoteDragKind,
    index: usize,
    /// The note at mouse down. Every move starts from it.
    origin: Note,
    /// The note as this drag wrote it last. It finds the note again when something else
    /// changed the clip during the drag, and it ends the drag when the note is gone.
    written: Note,
    /// Whether the gesture of the session is open. It opens with the first change, so a plain
    /// click on a note is no undo step.
    begun: bool,
}

pub struct NoteEditor {
    session: Entity<Session>,
    clip: Instance<Clip>,
    viewport: Viewport,
    /// The viewport of the last paint, for the playhead line and the mouse.
    painted: Rc<Cell<Viewport>>,
    /// The width of the note area at the last paint. Before the first paint it is the width
    /// of the timeline above, which is the same.
    painted_width: Rc<Cell<f32>>,
    selected_note: Option<usize>,
    drag: Option<NoteDrag>,
    /// The pointer is over the end of a note, so the cursor says that a drag resizes.
    over_edge: bool,
    focus_handle: FocusHandle,
    close_focus: FocusHandle,
    _project_events: Subscription,
}

impl EventEmitter<EditorEvent> for NoteEditor {}

impl NoteEditor {
    /// `width` is that of the note area, which the first zoom fits the clip into.
    pub(super) fn new(
        session: Entity<Session>,
        clip: Instance<Clip>,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Self {
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
                cx.notify();
            }
        });
        let mut editor = Self {
            session,
            clip: clip.clone(),
            viewport: Viewport::default(),
            painted: Rc::default(),
            painted_width: Rc::new(Cell::new(width)),
            selected_note: None,
            drag: None,
            over_edge: false,
            focus_handle: cx.focus_handle().tab_stop(true),
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

    /// The index of the selected note in the clip.
    pub fn selected_note(&self) -> Option<usize> {
        self.selected_note
    }

    pub fn select_note(&mut self, note: Option<usize>, cx: &mut Context<Self>) {
        if self.selected_note != note {
            self.selected_note = note;
            cx.notify();
        }
    }

    /// Shows another clip: zoomed to fit it, with the middle of its notes in the middle.
    pub fn set_clip(&mut self, clip: Instance<Clip>, cx: &mut Context<Self>) {
        self.end_drag(cx);
        self.clip = clip;
        self.selected_note = None;
        if let Some(state) = self.session.read(cx).project().state(&self.clip) {
            self.viewport = opened(state, self.painted_width.get(), note_area_height());
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
        clamped(&viewport, clip, time_signature, width, note_area_height())
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

    fn on_mouse_down(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        if y < 0.0 {
            if x >= 0.0 {
                let tick = snap(viewport.tick_at(x));
                self.session
                    .update(cx, |session, _| session.engine().seek(tick));
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
        let hit = hit.and_then(|(index, zone)| Some((index, zone, *clip.notes.get(index)?)));
        if let Some((index, zone, note)) = hit {
            let kind = match zone {
                Zone::RightEdge => NoteDragKind::Resize { grab: pointer },
                Zone::Body | Zone::LeftEdge => NoteDragKind::Move {
                    grab: pointer,
                    grab_pitch: pitch,
                },
            };
            self.drag = Some(NoteDrag {
                kind,
                index,
                origin: note,
                written: note,
                begun: false,
            });
            self.select_note(Some(index), cx);
            self.preview(note.pitch, note.velocity, cx);
            return;
        }

        let Some(note) = drawn_note(clip, pointer, pointer, pitch) else {
            // Outside the clip there is nothing to draw into.
            self.select_note(None, cx);
            return;
        };
        let index = clip.notes.len();
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
        self.drag = Some(NoteDrag {
            kind: NoteDragKind::Draw { down: pointer },
            index,
            origin: note,
            written: note,
            begun: true,
        });
        self.select_note(Some(index), cx);
        self.preview(note.pitch, note.velocity, cx);
    }

    /// One mouse move of a drag: the note becomes what the pointer says, through the gesture
    /// of the session.
    fn drag_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let viewport = self.painted.get();
        let Some(drag) = &self.drag else {
            return;
        };
        let project = self.session.read(cx).project();
        let Some(clip) = project.state(&self.clip) else {
            return self.end_drag(cx);
        };
        // Something else may have changed the clip. The note is where it was, or it is found
        // by what this drag wrote last, or it is gone.
        let index = match clip.notes.get(drag.index) {
            Some(note) if *note == drag.written => Some(drag.index),
            _ => clip.notes.iter().position(|note| *note == drag.written),
        };
        let Some(index) = index else {
            return self.end_drag(cx);
        };
        let pointer = viewport.tick_at(x);
        let next = match &drag.kind {
            NoteDragKind::Draw { down } => {
                drawn_note(clip, *down, pointer, drag.origin.pitch).unwrap_or(drag.origin)
            }
            NoteDragKind::Move { grab, grab_pitch } => {
                let semitones = i32::from(nearest_pitch(&viewport, y).number())
                    - i32::from(grab_pitch.number());
                let delta = snapped_delta(*grab, pointer);
                moved_note(clip.length, drag.origin, delta, semitones)
            }
            NoteDragKind::Resize { grab } => {
                resized_note(clip.length, drag.origin, snapped_delta(*grab, pointer))
            }
        };
        if next == drag.written {
            return;
        }

        let (begun, label, before) = (drag.begun, drag.kind.label(), drag.written);
        let instance = self.clip.clone();
        let published = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                project.update(edit, &instance, |clip| {
                    if let Some(note) = clip.notes.get_mut(index) {
                        *note = next;
                    }
                })
            })
        });
        if let Some(drag) = &mut self.drag {
            drag.begun = true;
            if published.is_some() {
                drag.index = index;
                drag.written = next;
            }
        }
        self.select_note(Some(index), cx);
        if published.is_some() && next.pitch != before.pitch {
            self.preview(next.pitch, next.velocity, cx);
        }
    }

    /// Mouse up, or the note went away under the drag: the gesture becomes one undo step. The
    /// notes are put in order first, because the file is written now and the agent doc asks
    /// for notes by start.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take().filter(|drag| drag.begun) {
            let instance = self.clip.clone();
            self.session.update(cx, |session, cx| {
                session.gesture(cx, |project, edit| match project.state(&instance) {
                    Some(_) => project.update(edit, &instance, sort_notes),
                    // The clip went away under the drag. There is nothing to put in order.
                    None => Ok(()),
                });
                session.finish_gesture(cx);
            });
            let project = self.session.read(cx).project();
            let notes = project.state(&self.clip).map(|clip| clip.notes.as_slice());
            let mut notes = notes.unwrap_or_default().iter();
            self.selected_note = notes.position(|note| *note == drag.written);
        }
        cx.notify();
    }

    /// Escape during a drag: the clip goes back to what it was at mouse down.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.begun {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
        }
        // A drawn note is gone again. Any other keeps its place in the clip.
        let drawn = matches!(drag.kind, NoteDragKind::Draw { .. });
        self.selected_note = (!drawn).then_some(drag.index);
        cx.notify();
    }

    fn hover(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let inside = x >= 0.0 && y >= 0.0;
        let zone = project
            .state(&self.clip)
            .filter(|_| inside)
            .and_then(|clip| note_at(&self.painted.get(), clip, x, y));
        let over_edge = zone.is_some_and(|(_, zone)| zone == Zone::RightEdge);
        if self.over_edge != over_edge {
            self.over_edge = over_edge;
            cx.notify();
        }
    }

    fn resize_cursor(&self) -> bool {
        match &self.drag {
            Some(drag) => matches!(drag.kind, NoteDragKind::Resize { .. }),
            None => self.over_edge,
        }
    }

    /// The selected note, when the clip still has a note at its index.
    fn selected(&self, cx: &App) -> Option<(usize, Note, Length)> {
        let index = self.selected_note?;
        let clip = self.session.read(cx).project().state(&self.clip)?;
        Some((index, *clip.notes.get(index)?, clip.length))
    }

    /// The keys of the focused editor. Whether the key was one of them.
    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" && !modifiers.shift {
            if self.drag.is_some() {
                self.cancel_drag(cx);
            } else {
                cx.emit(EditorEvent::Close);
            }
            return true;
        }
        // The mouse has the note: a key would fight the next mouse move.
        if self.drag.is_some() {
            return false;
        }
        let Some((index, note, clip_length)) = self.selected(cx) else {
            return false;
        };
        let step = SNAP.0 as i64;
        let next = match (key, modifiers.shift) {
            ("backspace" | "delete", false) => None,
            ("left", false) => Some(moved_note(clip_length, note, -step, 0)),
            ("right", false) => Some(moved_note(clip_length, note, step, 0)),
            ("up", false) => Some(moved_note(clip_length, note, 0, 1)),
            ("down", false) => Some(moved_note(clip_length, note, 0, -1)),
            ("up", true) => Some(moved_note(clip_length, note, 0, 12)),
            ("down", true) => Some(moved_note(clip_length, note, 0, -12)),
            _ => return false,
        };
        if next == Some(note) {
            return true;
        }
        let label = if next.is_some() {
            "Nudge note"
        } else {
            "Delete note"
        };
        let instance = self.clip.clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin(label);
                project.update(&mut edit, &instance, |clip| match next {
                    Some(next) => {
                        if let Some(note) = clip.notes.get_mut(index) {
                            *note = next;
                        }
                    }
                    None => {
                        clip.notes.remove(index);
                    }
                })?;
                project.update(&mut edit, &instance, sort_notes)?;
                project.finish(edit)
            })
        });
        let project = self.session.read(cx).project();
        let notes = project.state(&self.clip).map(|clip| clip.notes.as_slice());
        let mut notes = notes.unwrap_or_default().iter();
        let selected = next.and_then(|next| notes.position(|note| *note == next));
        self.select_note(selected, cx);
        if let Some(next) = next.filter(|next| next.pitch != note.pitch) {
            self.preview(next.pitch, next.velocity, cx);
        }
        true
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
        let height = f32::from(bounds.size.height) - RULER_HEIGHT;
        let viewport = clamped(&self.viewport, clip, time_signature, width, height);
        let notes = clip.notes.iter();
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
            notes: notes.map(|note| note_rect(&viewport, clip, note)).collect(),
            selected: self.selected_note,
        })
    }
}

/// What one paint of the editor shows, in the coordinates of [`super::roll`].
struct RollScene {
    viewport: Viewport,
    width: f32,
    height: f32,
    track_name: SharedString,
    accent: Hsla,
    bars: Vec<(u64, f32)>,
    beats: Vec<f32>,
    /// The part of the note area that is inside the clip.
    clip_start: f32,
    clip_end: f32,
    notes: Vec<Rect>,
    selected: Option<usize>,
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
    let upright = |x: f32| {
        Bounds::new(
            area.origin + point(px(x.round()), px(0.)),
            size(px(1.), px(height)),
        )
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
            window.paint_quad(fill(upright(*x), beat_line));
        }
        for (_, x) in &scene.bars {
            window.paint_quad(fill(upright(*x), hairline));
        }
        // Notes live inside the clip. What is outside is a shade darker.
        for (left, right) in [(0.0, scene.clip_start), (scene.clip_end, width)] {
            if right > left {
                let veil = Rect {
                    x: left,
                    y: 0.0,
                    width: right - left,
                    height,
                };
                window.paint_quad(fill(placed(veil, area.origin), outside));
            }
        }
        for (index, rect) in scene.notes.iter().enumerate() {
            let body = placed(*rect, area.origin);
            let selected = scene.selected == Some(index);
            let border = if selected { selection } else { scene.accent };
            let radius = px(3.).min(body.size.width / 2.);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(body, radius, scene.accent, px(1.), border, solid));
        }
    });

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

    // One hairline above the panel, one under the ruler, one beside the keys.
    let lines = [
        Bounds::new(bounds.origin, size(bounds.size.width, px(1.))),
        Bounds::new(
            bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
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

fn note_area_height() -> f32 {
    EDITOR_HEIGHT - RULER_HEIGHT
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
                paint_focus_ring(bounds, &focus_handle, window, cx);
                if editor.read(cx).resize_cursor() {
                    window.set_cursor_style(CursorStyle::ResizeLeftRight, &hitbox);
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
                    editor.on_mouse_down(x, y, cx)
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
                if editor.drag.is_none() {
                    if hitbox.is_hovered(window) {
                        editor.hover(x, y, cx);
                    }
                } else if event.dragging() {
                    editor.drag_to(x, y, cx);
                } else {
                    // The button came up somewhere that did not tell this window.
                    editor.end_drag(cx);
                }
            });
        }
    });
    window.on_mouse_event({
        let editor = editor.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                editor.update(cx, |editor, cx| {
                    if editor.drag.is_some() {
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
