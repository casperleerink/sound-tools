//! The arrangement view: track headers, a bar ruler, clips with a miniature of their notes,
//! the playhead, and the note editor as a panel below. Clips are added, moved, resized and
//! deleted here with the mouse and the keys.
//!
//! The views, split so that a moving playhead repaints almost nothing:
//! - [`ArrangementView`] is what the window shows. It stacks the timeline over the note editor
//!   and opens and closes the editor.
//! - [`Timeline`] draws everything that changes with the project, the scroll and the zoom on
//!   one canvas, and only what is visible. GPUI keeps its painted frame while it is not
//!   notified, so playback does not run this code.
//! - [`NoteEditor`] does the same for the notes of one clip.
//! - A `PlayheadLine` on top of each draws one line, every frame while the project plays.
//!
//! All positions come from [`layout`], and what a drag does to a clip from [`gesture`]. The
//! timeline gives the [`Scene`] it painted to its mouse listeners, so a click hits exactly
//! what is on screen. Every change goes through the session: a drag is one gesture and one
//! undo step.

pub mod editor;
pub mod gesture;
pub mod layout;
mod paint;
pub mod roll;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, Hitbox, HitboxBehavior, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point, ScrollWheelEvent,
    SharedString, StyleRefinement, Subscription, Window, canvas, div, fill, point, prelude::*, px,
    quad, size,
};
use sound_core::{Changes, Instance, InstanceId, ProjectEvent, Ticks, TimeSignature};
use sound_notes::Clip;
use sound_ui::{ActiveTheme, Session, Views};

use crate::{ArrangementState, TrackState, add_clip, move_clip, tracks};
use editor::EditorEvent;
pub use editor::NoteEditor;
use gesture::{Zone, new_clip, nudged_track, resized_left, resized_right, zone_at};
use layout::{
    Extent, HEADER_WIDTH, RULER_HEIGHT, Rect, SNAP, TRACK_HEIGHT, Viewport, shifted, snap,
    snapped_delta,
};
use paint::{
    KeyboardFocus, PlayheadLine, accent, paint_focus_ring, paint_ruler, paint_track_label, placed,
};
use roll::EDITOR_HEIGHT;

/// Registers the view of the `arrangement` tool.
pub fn register(views: &mut Views) {
    views.register(ArrangementView::new);
}

/// The note editor while it is open, with its own playhead line.
struct OpenEditor {
    editor: Entity<NoteEditor>,
    playhead_line: Entity<PlayheadLine>,
    _events: Subscription,
}

pub struct ArrangementView {
    session: Entity<Session>,
    timeline: Entity<Timeline>,
    playhead_line: Entity<PlayheadLine>,
    editor: Option<OpenEditor>,
}

impl ArrangementView {
    pub fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let playhead = session.read(cx).playhead().clone();
        let timeline = cx.new(|cx| Timeline::new(session.clone(), arrangement, cx));
        let painted = timeline.read(cx).painted.clone();
        let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &timeline, painted, cx));

        cx.subscribe_in(&timeline, window, |view, _, event, window, cx| {
            let TimelineEvent::OpenEditor(clip) = event;
            view.open_editor(clip.clone(), window, cx);
        })
        .detach();
        // The open editor follows the selection to another clip.
        cx.observe(&timeline, |view, _, cx| {
            view.follow_selection(cx);
        })
        .detach();
        cx.subscribe_in(&session, window, |view, _, event, window, cx| {
            let shown = view.editor_clip(cx);
            if matches!(event, ProjectEvent::Deleted(id) if Some(id) == shown.as_ref()) {
                // A drag to another track deletes the clip at its old id. The timeline has
                // selected the new one by now, and the editor goes with it.
                if !view.follow_selection(cx) {
                    view.close_editor(window, cx);
                }
            }
        })
        .detach();

        Self {
            session,
            timeline,
            playhead_line,
            editor: None,
        }
    }

    pub fn timeline(&self) -> &Entity<Timeline> {
        &self.timeline
    }

    /// The note editor, while it is open.
    pub fn editor(&self) -> Option<&Entity<NoteEditor>> {
        self.editor.as_ref().map(|open| &open.editor)
    }

    fn editor_clip(&self, cx: &App) -> Option<InstanceId> {
        let editor = self.editor()?.read(cx);
        Some(editor.clip().id().clone())
    }

    /// Opens the note editor for a clip and gives it the focus, so the keys edit notes.
    pub fn open_editor(
        &mut self,
        clip: Instance<Clip>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = &self.editor {
            open.editor
                .update(cx, |editor, cx| editor.set_clip(clip, cx));
        } else {
            let (width, _) = self.timeline.read(cx).painted_size.get();
            let session = self.session.clone();
            let editor = cx.new(|cx| NoteEditor::new(session, clip, width, cx));
            let playhead = self.session.read(cx).playhead().clone();
            let painted = editor.read(cx).painted();
            let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &editor, painted, cx));
            let events = cx.subscribe_in(&editor, window, |view, _, event, window, cx| {
                let EditorEvent::Close = event;
                view.close_editor(window, cx);
            });
            self.editor = Some(OpenEditor {
                editor,
                playhead_line,
                _events: events,
            });
            cx.notify();
        }
        if let Some(editor) = self.editor() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Closes the editor. The focus goes back to the timeline when the editor had it.
    pub fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.editor.take() else {
            return;
        };
        if open.editor.focus_handle(cx).contains_focused(window, cx) {
            window.focus(&self.timeline.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Shows the selected clip in the open editor. Whether there was one to show.
    fn follow_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.editor else {
            return false;
        };
        let Some(selected) = self.timeline.read(cx).selected_clip().cloned() else {
            return false;
        };
        let project = self.session.read(cx).project();
        let Some(clip) = project.resolve::<Clip>(&selected) else {
            return false;
        };
        if open.editor.read(cx).clip().id() != &selected {
            open.editor
                .update(cx, |editor, cx| editor.set_clip(clip, cx));
        }
        true
    }
}

impl Render for ArrangementView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let timeline = self.timeline.clone();
        let fill_parent = || StyleRefinement::default().size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Cached: a frame that only moves the playhead reuses what was painted.
                    .child(timeline.cached(fill_parent()))
                    .child(self.playhead_line.clone()),
            )
            .children(self.editor.as_ref().map(|open| {
                div()
                    .flex_none()
                    .h(px(EDITOR_HEIGHT))
                    .relative()
                    .child(open.editor.clone().cached(fill_parent()))
                    .child(open.playhead_line.clone())
            }))
    }
}

struct TrackRow {
    y: f32,
    name: SharedString,
    accent: Hsla,
}

/// A clip as it is on screen, in the coordinates of [`layout`].
pub struct ClipShape {
    pub clip: Instance<Clip>,
    pub rect: Rect,
    start: Ticks,
    notes: Vec<Rect>,
    accent: Hsla,
    selected: bool,
}

/// What one paint shows: only the visible rows, clips and bars. Later clips are on top.
pub struct Scene {
    pub viewport: Viewport,
    pub clips: Vec<ClipShape>,
    rows: Vec<TrackRow>,
    bars: Vec<(u64, f32)>,
}

impl Scene {
    /// The clip on top at a position in the timeline area.
    pub fn clip_at(&self, x: f32, y: f32) -> Option<&ClipShape> {
        self.clips
            .iter()
            .rev()
            .find(|shape| shape.rect.contains(x, y))
    }

    /// The clip on top at a position, with the part of it that is there: its body or an edge.
    pub fn zone_at(&self, x: f32, y: f32) -> Option<(&ClipShape, Zone)> {
        let shape = self.clip_at(x, y)?;
        Some((shape, zone_at(shape.rect, x)))
    }
}

/// What a drag of a clip does. Only a resize keeps the clip as it was at mouse down, because
/// `Clip::set_length` drops notes for good: every move starts from that clip again.
enum ClipDragKind {
    Move { start: Ticks },
    ResizeLeft { origin: Clip },
    ResizeRight { origin: Clip },
}

impl ClipDragKind {
    fn label(&self) -> &'static str {
        match self {
            Self::Move { .. } => "Move clip",
            Self::ResizeLeft { .. } | Self::ResizeRight { .. } => "Resize clip",
        }
    }
}

/// A drag of a clip, from mouse down to mouse up.
struct ClipDrag {
    /// The clip now. Its id changes when the drag takes it to another track.
    clip: Instance<Clip>,
    /// The id at mouse down. A drag that comes back to its first track takes this id again, so
    /// a drag there and back leaves the file where it was.
    home: InstanceId,
    kind: ClipDragKind,
    /// The tick under the pointer at mouse down.
    grab: Ticks,
    /// Whether the gesture of the session is open. It opens with the first move that changes
    /// something, so a plain click is no undo step.
    begun: bool,
}

/// What one mouse move of a drag asks for.
enum DragStep {
    /// The clip is gone: deleted from outside.
    Gone,
    Unchanged,
    Publish {
        next: Clip,
        /// Another track than the clip is on now.
        to_track: Option<Instance<TrackState>>,
    },
}

/// What of the kept track order and clip ends has to be read again. A drag changes one clip
/// per mouse move, so only its track is walked then, not every clip of the project.
#[derive(Default)]
enum Stale {
    #[default]
    Nothing,
    Tracks(BTreeSet<InstanceId>),
    Everything,
}

impl Stale {
    /// An event named `id`. `track` is the track it is or is inside of. Without one, the event
    /// is about the arrangement itself.
    fn add(&mut self, track: Option<InstanceId>, id: &InstanceId) {
        match (&mut *self, track) {
            (Self::Everything, _) => {}
            // The record of a track holds its order. A track that comes or goes changes it.
            (_, Some(track)) if track != *id => match self {
                Self::Tracks(tracks) => {
                    tracks.insert(track);
                }
                _ => *self = Self::Tracks(BTreeSet::from([track])),
            },
            _ => *self = Self::Everything,
        }
    }
}

/// What the timeline asks of the view that holds it.
pub enum TimelineEvent {
    /// A double click on a clip, or enter: show its notes.
    OpenEditor(Instance<Clip>),
}

pub struct Timeline {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    /// Zoom and scroll, kept inside the content by [`Self::set_viewport`]. Scroll and pinch go
    /// on from here, not from what was painted: several events may arrive between two frames.
    viewport: Viewport,
    /// The viewport of the last paint, for the playhead line. Filled by paint, like a GPUI
    /// scroll handle. It differs from `viewport` only while the window or the project changed
    /// size under it.
    painted: Rc<Cell<Viewport>>,
    /// The size of the timeline area at the last paint, which the scroll limits depend on.
    painted_size: Rc<Cell<(f32, f32)>>,
    /// The tracks in display order, each with the end of its last clip. Finding the ends walks
    /// every clip, so they are kept between the project events that can change them and are
    /// not read again per paint. Nothing else of the project is kept.
    order: Vec<Instance<TrackState>>,
    ends: BTreeMap<InstanceId, Ticks>,
    /// What the events since the last render may have changed.
    stale: Stale,
    selected_clip: Option<InstanceId>,
    drag: Option<ClipDrag>,
    /// The pointer is over an edge of a clip, so the cursor says that a drag resizes.
    over_edge: bool,
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    _project_events: Subscription,
}

impl EventEmitter<TimelineEvent> for Timeline {}

impl Timeline {
    fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle().tab_stop(true);
        let project_events = cx.subscribe(&session, |timeline, _, event, cx| {
            let shown = |id: &InstanceId| {
                id == timeline.arrangement.id() || id.is_inside(timeline.arrangement.id())
            };
            let changed = match event {
                ProjectEvent::Created(id) | ProjectEvent::Changed(id) => shown(id),
                ProjectEvent::Deleted(id) => {
                    let changed = shown(id);
                    if timeline.selected_clip.as_ref() == Some(id) {
                        timeline.selected_clip = None;
                    }
                    // Deleted under the drag, from outside. A drag to another track is not
                    // this: it names its new clip before this event arrives.
                    let dragged = timeline.drag.as_ref().map(|drag| drag.clip.id());
                    if dragged == Some(id) {
                        timeline.end_drag(cx);
                    }
                    changed
                }
                // The time signature places the bars.
                ProjectEvent::ProjectFileChanged => true,
                ProjectEvent::ProblemsChanged => false,
            };
            if changed {
                // Read again at the next render, once for all events of a group.
                match event {
                    ProjectEvent::Created(id)
                    | ProjectEvent::Changed(id)
                    | ProjectEvent::Deleted(id) => {
                        let track = timeline.track_of(id);
                        timeline.stale.add(track, id);
                    }
                    ProjectEvent::ProjectFileChanged | ProjectEvent::ProblemsChanged => {}
                }
                cx.notify();
            }
        });
        Self {
            session,
            arrangement,
            viewport: Viewport::default(),
            painted: Rc::default(),
            painted_size: Rc::default(),
            order: Vec::new(),
            ends: BTreeMap::new(),
            stale: Stale::Everything,
            selected_clip: None,
            drag: None,
            over_edge: false,
            focus_handle,
            keyboard_focus: KeyboardFocus::default(),
            _project_events: project_events,
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Sets zoom and scroll, kept inside the content for the size that was last painted.
    pub fn set_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(viewport, width, height, cx);
        if self.viewport != viewport {
            self.viewport = viewport;
            cx.notify();
        }
    }

    fn clamped(&self, viewport: Viewport, width: f32, height: f32, cx: &App) -> Viewport {
        let extent = Extent {
            end: self.ends.values().max().copied().unwrap_or_default(),
            tracks: self.order.len(),
        };
        viewport.clamped(extent, self.time_signature(cx), width, height)
    }

    /// The track that an id of this arrangement is, or is inside of.
    fn track_of(&self, id: &InstanceId) -> Option<InstanceId> {
        let arrangement = self.arrangement.id();
        let mut inside = id.ancestors().chain([id.clone()]);
        inside.find(|ancestor| ancestor.parent().as_ref() == Some(arrangement))
    }

    fn refresh_order(&mut self, cx: &App) {
        let project = self.session.read(cx).project();
        let end_of = |track: &InstanceId| {
            let clips = project.children::<Clip>(track);
            clips.map(|(_, clip)| clip.end()).max()
        };
        match std::mem::take(&mut self.stale) {
            Stale::Nothing => {}
            Stale::Tracks(tracks) => {
                for track in tracks {
                    match end_of(&track) {
                        Some(end) => self.ends.insert(track, end),
                        None => self.ends.remove(&track),
                    };
                }
            }
            Stale::Everything => {
                let tracks = tracks(project, self.arrangement.id());
                self.order = tracks.into_iter().map(|(track, _)| track).collect();
                let ends = self.order.iter();
                self.ends = ends
                    .filter_map(|track| Some((track.id().clone(), end_of(track.id())?)))
                    .collect();
            }
        }
    }

    /// Whether the focus ring shows: the timeline has the focus, and it came from the keyboard.
    pub fn shows_focus_ring(&self, window: &Window) -> bool {
        self.keyboard_focus.shows_ring(&self.focus_handle, window)
    }

    pub fn selected_clip(&self) -> Option<&InstanceId> {
        self.selected_clip.as_ref()
    }

    pub fn select_clip(&mut self, clip: Option<InstanceId>, cx: &mut Context<Self>) {
        if self.selected_clip != clip {
            self.selected_clip = clip;
            cx.notify();
        }
    }

    fn time_signature(&self, cx: &App) -> TimeSignature {
        let project = self.session.read(cx).project();
        project.project_file().tempo_map.time_signature()
    }

    /// Everything to paint into a timeline area of this size, read from the project now.
    fn scene(&self, width: f32, height: f32, cx: &App) -> Scene {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let time_signature = self.time_signature(cx);
        // Clamped again for this size: the window may have grown since the last scroll.
        let viewport = self.clamped(self.viewport, width, height, cx);
        let visible_ticks = viewport.visible_ticks(width);

        let mut scene = Scene {
            viewport,
            clips: Vec::new(),
            rows: Vec::new(),
            bars: viewport.ruler_bars(time_signature, width).collect(),
        };
        for index in viewport.visible_tracks(height, self.order.len()) {
            let Some(track) = self.order.get(index) else {
                break;
            };
            // Gone since the order was read: the render after its event leaves it out.
            let Some(state) = project.state(track) else {
                continue;
            };
            let accent = accent(state.colour, theme);
            scene.rows.push(TrackRow {
                y: viewport.y_of(index),
                name: state.name.clone().into(),
                accent,
            });
            let first = scene.clips.len();
            for (clip, state) in project.children::<Clip>(track.id()) {
                if state.start >= visible_ticks.end || state.end() <= visible_ticks.start {
                    continue;
                }
                let rect = viewport.clip_rect(index, state);
                scene.clips.push(ClipShape {
                    notes: viewport.miniature(state, rect).collect(),
                    selected: self.selected_clip.as_ref() == Some(clip.id()),
                    start: state.start,
                    clip,
                    rect,
                    accent,
                });
            }
            // The order of `clips()`, by start and then by id, for the few that are visible:
            // it decides which of two overlapping clips is on top.
            scene.clips[first..]
                .sort_by(|a, b| (a.start, a.clip.id()).cmp(&(b.start, b.clip.id())));
        }
        scene
    }

    /// The position of a mouse event in the coordinates of [`layout`].
    fn timeline_position(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - bounds.left()) - HEADER_WIDTH,
            f32::from(position.y - bounds.top()) - RULER_HEIGHT,
        )
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        scene: &Scene,
        cx: &mut Context<Self>,
    ) {
        if x < 0.0 {
            return;
        }
        if y < 0.0 {
            let tick = snap(scene.viewport.tick_at(x));
            self.session
                .update(cx, |session, _| session.engine().seek(tick));
            return;
        }
        let double = event.click_count == 2;
        let Some((shape, zone)) = scene.zone_at(x, y) else {
            self.select_clip(None, cx);
            if double {
                self.add_clip_at(x, y, scene, cx);
            }
            return;
        };
        let clip = shape.clip.clone();
        self.select_clip(Some(clip.id().clone()), cx);
        if double {
            cx.emit(TimelineEvent::OpenEditor(clip));
            return;
        }
        let Some(state) = self.session.read(cx).project().state(&clip) else {
            return;
        };
        let kind = match zone {
            Zone::Body => ClipDragKind::Move { start: state.start },
            Zone::LeftEdge => ClipDragKind::ResizeLeft {
                origin: state.clone(),
            },
            Zone::RightEdge => ClipDragKind::ResizeRight {
                origin: state.clone(),
            },
        };
        self.drag = Some(ClipDrag {
            home: clip.id().clone(),
            clip,
            kind,
            grab: scene.viewport.tick_at(x),
            begun: false,
        });
    }

    /// A double click on empty track space: a clip of one bar in the cell under the pointer.
    fn add_clip_at(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let row = scene.viewport.track_at(y, self.order.len());
        let Some(track) = row.and_then(|row| self.order.get(row)).cloned() else {
            return;
        };
        let clip = new_clip(scene.viewport.tick_at(x), self.time_signature(cx));
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let added = add_clip(project, &mut changes, &track, "clip", clip)?;
                project.commit("Add clip", changes)?;
                Ok(added)
            })
        });
        if let Some(added) = added {
            self.select_clip(Some(added.id().clone()), cx);
        }
    }

    /// What the pointer asks of the dragged clip now.
    fn drag_step(&self, drag: &ClipDrag, x: f32, y: f32, cx: &App) -> DragStep {
        let project = self.session.read(cx).project();
        let Some(live) = project.state(&drag.clip) else {
            return DragStep::Gone;
        };
        let delta = snapped_delta(drag.grab, self.viewport.tick_at(x));
        let mut to_track = None;
        let next = match &drag.kind {
            ClipDragKind::Move { start } => {
                let row = self.viewport.nearest_track(y, self.order.len());
                let under_pointer = row.and_then(|row| self.order.get(row));
                let on_now = drag.clip.id().parent();
                to_track = under_pointer
                    .filter(|track| Some(track.id()) != on_now.as_ref())
                    .cloned();
                Clip {
                    start: shifted(*start, delta),
                    ..live.clone()
                }
            }
            ClipDragKind::ResizeLeft { origin } => resized_left(origin, delta),
            ClipDragKind::ResizeRight { origin } => resized_right(origin, delta),
        };
        if to_track.is_none() && next == *live {
            return DragStep::Unchanged;
        }
        DragStep::Publish { next, to_track }
    }

    /// One mouse move of a drag: the clip becomes what the pointer says, through the gesture
    /// of the session, so sound and every other view follow.
    fn drag_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let Some(drag) = &self.drag else {
            return;
        };
        let (next, to_track) = match self.drag_step(drag, x, y, cx) {
            DragStep::Gone => return self.end_drag(cx),
            DragStep::Unchanged => return,
            DragStep::Publish { next, to_track } => (next, to_track),
        };
        let (clip, home, label) = (drag.clip.clone(), drag.home.clone(), drag.kind.label());
        let begun = drag.begun;
        let moved = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                let clip = match &to_track {
                    Some(track) if home.parent().as_ref() == Some(track.id()) => {
                        changes.delete(clip.id());
                        changes.create(home.clone(), next)
                    }
                    Some(track) => {
                        let moved = move_clip(project, &mut changes, &clip, track)?;
                        changes.set(&moved, next);
                        moved
                    }
                    None => {
                        changes.set(&clip, next);
                        clip.clone()
                    }
                };
                project.publish(edit, changes)?;
                Ok(clip)
            })
        });
        if let Some(drag) = &mut self.drag {
            drag.begun = true;
            if let Some(moved) = moved {
                drag.clip = moved;
            }
        }
        let dragged = self.drag.as_ref().map(|drag| drag.clip.id().clone());
        self.select_clip(dragged, cx);
    }

    /// Mouse up, or the clip went away under the drag: the gesture becomes one undo step.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some_and(|drag| drag.begun) {
            self.session
                .update(cx, |session, cx| session.finish_gesture(cx));
        }
        cx.notify();
    }

    /// Escape: the clip goes back to where it was at mouse down. Whether there was a drag.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if drag.begun {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
            self.select_clip(Some(drag.home), cx);
        }
        cx.notify();
        true
    }

    /// The cursor says that a drag from here resizes.
    fn hover(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let inside = x >= 0.0 && y >= 0.0;
        let zone = scene.zone_at(x, y).filter(|_| inside);
        let over_edge = zone.is_some_and(|(_, zone)| zone != Zone::Body);
        if self.over_edge != over_edge {
            self.over_edge = over_edge;
            cx.notify();
        }
    }

    fn resize_cursor(&self) -> bool {
        match &self.drag {
            Some(drag) => !matches!(drag.kind, ClipDragKind::Move { .. }),
            None => self.over_edge,
        }
    }

    fn selected_instance(&self, cx: &App) -> Option<Instance<Clip>> {
        let project = self.session.read(cx).project();
        project.resolve(self.selected_clip.as_ref()?)
    }

    /// The keys of the focused timeline. Whether the key was one of them.
    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            return self.cancel_drag(cx);
        }
        // The mouse has the clip: a key would fight the next mouse move.
        if self.drag.is_some() {
            return false;
        }
        let Some(clip) = self.selected_instance(cx) else {
            return false;
        };
        let step = SNAP.0 as i64;
        match key {
            "enter" => cx.emit(TimelineEvent::OpenEditor(clip)),
            "backspace" | "delete" => self.delete_clip(&clip, cx),
            "left" => self.nudge_in_time(&clip, -step, cx),
            "right" => self.nudge_in_time(&clip, step, cx),
            "up" => self.nudge_to_track(&clip, -1, cx),
            "down" => self.nudge_to_track(&clip, 1, cx),
            _ => return false,
        }
        true
    }

    fn delete_clip(&mut self, clip: &Instance<Clip>, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.delete(clip.id());
                project.commit("Delete clip", changes)
            })
        });
    }

    fn nudge_in_time(&mut self, clip: &Instance<Clip>, delta: i64, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some(start) = project.state(clip).map(|state| state.start) else {
            return;
        };
        let next = shifted(start, delta);
        if next == start {
            return;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Nudge clip");
                project.update(&mut edit, clip, |clip| clip.start = next)?;
                project.finish(edit)
            })
        });
    }

    fn nudge_to_track(&mut self, clip: &Instance<Clip>, step: i64, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let on_now = clip.id().parent();
        let mut rows = self.order.iter();
        let Some(current) = rows.position(|track| Some(track.id()) == on_now.as_ref()) else {
            return;
        };
        let next = nudged_track(current, self.order.len(), step);
        let Some(track) = self.order.get(next).filter(|_| next != current).cloned() else {
            return;
        };
        let moved = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let moved = move_clip(project, &mut changes, clip, &track)?;
                project.commit("Nudge clip", changes)?;
                Ok(moved)
            })
        });
        if let Some(moved) = moved {
            self.select_clip(Some(moved.id().clone()), cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        self.set_viewport(scrolled_or_zoomed(self.viewport, event, x), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
    }
}

/// Scroll pans. With cmd it zooms in time about the pointer.
fn scrolled_or_zoomed(viewport: Viewport, event: &ScrollWheelEvent, x: f32) -> Viewport {
    let delta = event.delta.pixel_delta(px(32.));
    if event.modifiers.secondary() {
        let factor = (f64::from(f32::from(delta.y)) * 0.01).exp();
        viewport.zoomed(factor, x.max(0.0))
    } else {
        viewport.scrolled(f32::from(delta.x), f32::from(delta.y))
    }
}

impl Focusable for Timeline {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Timeline {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_order(cx);
        let timeline = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let surface = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let width = f32::from(bounds.size.width) - HEADER_WIDTH;
                let height = f32::from(bounds.size.height) - RULER_HEIGHT;
                let scene = Rc::new(timeline.read(cx).scene(width, height, cx));
                timeline.read(cx).painted.set(scene.viewport);
                timeline.read(cx).painted_size.set((width, height));
                paint_scene(&scene, bounds, window, cx);
                let keyboard_focus = &timeline.read(cx).keyboard_focus;
                if keyboard_focus.shows_ring(&focus_handle, window) {
                    paint_focus_ring(bounds, window, cx);
                }
                if timeline.read(cx).resize_cursor() {
                    window.set_cursor_style(CursorStyle::ResizeLeftRight, &hitbox);
                }
                listen(timeline, scene, bounds, hitbox, window);
            },
        );
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|timeline, event, _, cx| {
                if timeline.on_key(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(surface.size_full())
    }
}

/// Mouse listeners live for one frame and know what that frame painted.
fn listen(
    timeline: Entity<Timeline>,
    scene: Rc<Scene>,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    window: &mut Window,
) {
    window.on_mouse_event({
        let (timeline, hitbox, scene) = (timeline.clone(), hitbox.clone(), scene.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            let hit = phase == DispatchPhase::Bubble && hitbox.is_hovered(window);
            if hit && event.button == MouseButton::Left {
                let (x, y) = Timeline::timeline_position(bounds, event.position);
                timeline.update(cx, |timeline, cx| {
                    window.focus(&timeline.focus_handle, cx);
                    timeline.keyboard_focus.pressed(cx);
                    timeline.on_mouse_down(event, x, y, &scene, cx)
                });
            }
        }
    });
    // A drag that starts on a clip goes on wherever the pointer is, until the button is up.
    window.on_mouse_event({
        let (timeline, hitbox) = (timeline.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = Timeline::timeline_position(bounds, event.position);
            timeline.update(cx, |timeline, cx| {
                if timeline.drag.is_none() {
                    if hitbox.is_hovered(window) {
                        timeline.hover(x, y, &scene, cx);
                    }
                } else if event.dragging() {
                    timeline.drag_to(x, y, cx);
                } else {
                    // The button came up somewhere that did not tell this window.
                    timeline.end_drag(cx);
                }
            });
        }
    });
    window.on_mouse_event({
        let timeline = timeline.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                timeline.update(cx, |timeline, cx| {
                    if timeline.drag.is_some() {
                        timeline.end_drag(cx);
                    }
                });
            }
        }
    });
    window.on_mouse_event({
        let (timeline, hitbox) = (timeline.clone(), hitbox.clone());
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                let (x, _) = Timeline::timeline_position(bounds, event.position);
                timeline.update(cx, |timeline, cx| timeline.on_scroll(event, x, cx));
            }
        }
    });
    window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
            let (x, _) = Timeline::timeline_position(bounds, event.position);
            timeline.update(cx, |timeline, cx| timeline.on_pinch(event, x, cx));
        }
    });
}

fn paint_scene(scene: &Scene, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let (hairline, clip_fill, clip_border) = (
        theme.alpha_at(0.05),
        theme.alpha_at(0.05),
        theme.alpha_at(0.10),
    );
    let selection = theme.gray_950;
    let headers = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT)),
        size(px(HEADER_WIDTH), bounds.size.height - px(RULER_HEIGHT)),
    );
    let ruler = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(0.)),
        size(bounds.size.width - px(HEADER_WIDTH), px(RULER_HEIGHT)),
    );
    let timeline = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(ruler.size.width, headers.size.height),
    );

    // The ruler: a short mark and a number per bar. No grid below it.
    paint_ruler(&scene.bars, ruler, window, cx);

    window.with_content_mask(Some(ContentMask { bounds: headers }), |window| {
        for row in &scene.rows {
            let top = headers.origin + point(px(0.), px(row.y.round()));
            let name_width = HEADER_WIDTH - 44. - 16.;
            let name = row.name.clone();
            paint_track_label(name, row.accent, top, TRACK_HEIGHT, name_width, window, cx);
        }
    });

    // One hairline under the ruler and one between the headers and the timeline.
    let under_ruler = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
        size(bounds.size.width, px(1.)),
    );
    let beside_headers = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH - 1.), px(0.)),
        size(px(1.), bounds.size.height),
    );
    window.paint_quad(fill(under_ruler, hairline));
    window.paint_quad(fill(beside_headers, hairline));

    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for shape in &scene.clips {
            let border = if shape.selected {
                selection
            } else {
                clip_border
            };
            let body = placed(shape.rect, timeline.origin);
            let radius = px(6.).min(body.size.width / 2.);
            window.paint_quad(quad(
                body,
                radius,
                clip_fill,
                px(1.),
                border,
                BorderStyle::Solid,
            ));
            for note in &shape.notes {
                window.paint_quad(fill(placed(*note, timeline.origin), shape.accent));
            }
        }
    });
}
