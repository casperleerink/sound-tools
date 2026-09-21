//! The arrangement view: track headers, a bar ruler, clips with a miniature of their notes,
//! the playhead, and one detail panel below: the note editor of a clip or the track panel of a
//! track, one at a time. Clips are added, moved, resized and deleted here with the mouse and
//! the keys.
//!
//! The views, split so that a moving playhead repaints almost nothing:
//! - [`ArrangementView`] is what the window shows. It stacks the timeline over the detail
//!   panel, and opens, swaps and closes what the panel shows.
//! - [`Timeline`] draws everything that changes with the project, the scroll and the zoom on
//!   one canvas, and only what is visible. GPUI keeps its painted frame while it is not
//!   notified, so playback does not run this code.
//! - [`NoteEditor`] does the same for the notes of one clip.
//! - [`TrackPanel`] shows the devices of one track, each in the view of its own tool.
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
pub mod track_panel;

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
use sound_core::{Changes, Instance, InstanceId, ProjectEvent, State, Ticks, TimeSignature};
use sound_notes::Clip;
use sound_ui::{ActiveTheme, KeyboardFocus, Playhead, Session, Views};

use crate::{ArrangementState, TrackState, add_clip, move_clip, tracks};
use editor::EditorEvent;
pub use editor::NoteEditor;
use gesture::{Zone, new_clip, nudged_track, resized_left, resized_right, zone_at};
use layout::{
    Extent, HEADER_WIDTH, RULER_HEIGHT, Rect, SNAP, TRACK_HEIGHT, Viewport, shifted, snap,
    snapped_delta,
};
use paint::{PlayheadLine, accent, paint_focus_ring, paint_ruler, paint_track_label, placed};
use roll::EDITOR_HEIGHT;
pub use track_panel::TrackPanel;
use track_panel::TrackPanelEvent;

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

struct OpenTrackPanel {
    panel: Entity<TrackPanel>,
    _events: Subscription,
}

/// What the panel below the timeline shows. One thing at a time: opening the other takes its
/// place.
enum Detail {
    Editor(OpenEditor),
    Track(OpenTrackPanel),
}

pub struct ArrangementView {
    session: Entity<Session>,
    timeline: Entity<Timeline>,
    playhead_line: Entity<PlayheadLine>,
    detail: Option<Detail>,
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

        cx.subscribe_in(
            &timeline,
            window,
            |view, _, event, window, cx| match event {
                TimelineEvent::OpenEditor(clip) => view.open_editor(clip.clone(), window, cx),
                TimelineEvent::OpenTrack(track) => view.open_track_panel(track.clone(), window, cx),
            },
        )
        .detach();
        // What is open follows the selection to another clip or another track.
        cx.observe_in(&timeline, window, |view, _, window, cx| {
            view.follow_selection(window, cx);
        })
        .detach();
        // A move to another track, and the undo of one, delete the clip at one id and create
        // it at another in one group of events. So the editor is not closed at the delete, but
        // after the group: the timeline has selected the clip at its new id by then, and the
        // editor goes with it. Only a clip that is really gone closes the editor. A track
        // keeps its id, so its panel closes with it at once.
        cx.subscribe_in(&session, window, |view, _, event, window, cx| {
            let ProjectEvent::Deleted(id) = event else {
                return;
            };
            if Some(id) == view.editor_clip(cx).as_ref() {
                cx.defer_in(window, |view, window, cx| {
                    let gone = view.editor_clip(cx).is_some_and(|clip| {
                        let project = view.session.read(cx).project();
                        project.resolve::<Clip>(&clip).is_none()
                    });
                    if gone && !view.follow_selection(window, cx) {
                        view.close_detail(window, cx);
                    }
                });
            }
            let shown = view.track_panel().map(|panel| panel.read(cx).track().id());
            if Some(id) == shown {
                view.close_detail(window, cx);
            }
        })
        .detach();

        Self {
            session,
            timeline,
            playhead_line,
            detail: None,
        }
    }

    pub fn timeline(&self) -> &Entity<Timeline> {
        &self.timeline
    }

    /// The note editor, while it is open.
    pub fn editor(&self) -> Option<&Entity<NoteEditor>> {
        match &self.detail {
            Some(Detail::Editor(open)) => Some(&open.editor),
            Some(Detail::Track(_)) | None => None,
        }
    }

    /// The track panel, while it is open.
    pub fn track_panel(&self) -> Option<&Entity<TrackPanel>> {
        match &self.detail {
            Some(Detail::Track(open)) => Some(&open.panel),
            Some(Detail::Editor(_)) | None => None,
        }
    }

    fn editor_clip(&self, cx: &App) -> Option<InstanceId> {
        let editor = self.editor()?.read(cx);
        Some(editor.clip().id().clone())
    }

    /// Opens the note editor for a clip and gives it the focus, so the keys edit notes. It
    /// takes the place of an open track panel.
    pub fn open_editor(
        &mut self,
        clip: Instance<Clip>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.editor() {
            editor.update(cx, |editor, cx| editor.set_clip(clip, cx));
        } else {
            self.close_detail(window, cx);
            let (width, _) = self.timeline.read(cx).painted_size.get();
            let session = self.session.clone();
            let editor = cx.new(|cx| NoteEditor::new(session, clip, width, cx));
            let playhead = self.session.read(cx).playhead().clone();
            let painted = editor.read(cx).painted();
            let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &editor, painted, cx));
            let events = cx.subscribe_in(&editor, window, |view, _, event, window, cx| {
                let EditorEvent::Close = event;
                view.close_detail(window, cx);
            });
            self.detail = Some(Detail::Editor(OpenEditor {
                editor,
                playhead_line,
                _events: events,
            }));
            cx.notify();
        }
        if let Some(editor) = self.editor() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Opens the track panel for a track. It takes the place of an open note editor. The
    /// focus stays where it is: the timeline keeps the keys that pick another track, and tab
    /// goes into the panel.
    pub fn open_track_panel(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(panel) = self.track_panel() {
            if panel.read(cx).track().id() != track.id() {
                panel.update(cx, |panel, cx| panel.set_track(track, window, cx));
            }
            return;
        }
        self.close_detail(window, cx);
        let session = self.session.clone();
        let panel = cx.new(|cx| TrackPanel::new(session, track, window, cx));
        let events = cx.subscribe_in(&panel, window, |view, _, event, window, cx| {
            let TrackPanelEvent::Close = event;
            view.close_detail(window, cx);
        });
        self.detail = Some(Detail::Track(OpenTrackPanel {
            panel,
            _events: events,
        }));
        cx.notify();
    }

    /// Closes what the panel below the timeline shows. The focus goes back to the timeline
    /// when it was inside.
    pub fn close_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = match self.detail.take() {
            Some(Detail::Editor(open)) => {
                // A note drag may be going on: its gesture ends here, not with the editor.
                open.editor.update(cx, |editor, cx| editor.end_drag(cx));
                open.editor.focus_handle(cx)
            }
            // A knob drag of a device ends when its view is released with the panel.
            Some(Detail::Track(open)) => open.panel.focus_handle(cx),
            None => return,
        };
        if focus_handle.contains_focused(window, cx) {
            window.focus(&self.timeline.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Shows the selected clip in the open editor, or the selected track in the open track
    /// panel. Whether there was one to show.
    fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let project = self.session.read(cx).project();
        let timeline = self.timeline.read(cx);
        match &self.detail {
            Some(Detail::Editor(open)) => {
                let Some(clip) = timeline.selected_instance(cx) else {
                    return false;
                };
                if open.editor.read(cx).clip().id() != clip.id() {
                    open.editor
                        .update(cx, |editor, cx| editor.set_clip(clip, cx));
                }
                true
            }
            Some(Detail::Track(_)) => {
                let selected = timeline.selected_track.as_ref();
                let Some(track) = selected.and_then(|track| project.resolve(track)) else {
                    return false;
                };
                self.open_track_panel(track, window, cx);
                true
            }
            None => false,
        }
    }
}

impl Render for ArrangementView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let timeline = self.timeline.clone();
        let fill_parent = || StyleRefinement::default().size_full();
        // Both details have one height, so a swap between them does not move the timeline.
        // Both are cached: the playhead line above draws this view again on every frame.
        let detail = self.detail.as_ref().map(|detail| {
            let panel = div().flex_none().h(px(EDITOR_HEIGHT)).relative();
            match detail {
                Detail::Editor(open) => panel
                    .child(open.editor.clone().cached(fill_parent()))
                    .child(open.playhead_line.clone()),
                Detail::Track(open) => panel.child(open.panel.clone().cached(fill_parent())),
            }
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            // Escape closes the detail. It comes here from the timeline and from inside the
            // track panel when nothing there used it, as a knob does to cancel its drag. The
            // note editor handles its own.
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let escape =
                    event.keystroke.key == "escape" && !event.keystroke.modifiers.modified();
                if escape && view.detail.is_some() {
                    view.close_detail(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Cached: a frame that only moves the playhead reuses what was painted.
                    .child(timeline.cached(fill_parent()))
                    .child(self.playhead_line.clone()),
            )
            .children(detail)
    }
}

struct TrackRow {
    y: f32,
    name: SharedString,
    accent: Hsla,
    selected: bool,
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

/// What a drag of a clip does, with what it starts every move from and what it wrote last.
/// When the live clip is not what the drag wrote, something else changed it: an undo between
/// mouse down and the first move, or an agent. A resize then goes on from the live clip, so it
/// never writes an old copy with old notes over a newer clip. A move writes only the start.
enum ClipDragKind {
    Move {
        start: Ticks,
        written: Ticks,
    },
    /// Only a resize keeps a whole clip, because `Clip::set_length` drops notes for good:
    /// every move starts from `origin` again, so going in and out loses nothing.
    Resize {
        edge: Edge,
        origin: Clip,
        written: Clip,
        /// The delta of the last move, to skip a move inside the same snap step cheaply.
        delta: i64,
    },
}

#[derive(Copy, Clone)]
enum Edge {
    Left,
    Right,
}

impl ClipDragKind {
    fn label(&self) -> &'static str {
        match self {
            Self::Move { .. } => "Move clip",
            Self::Resize { .. } => "Resize clip",
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
    /// A click on a track header, or enter on the selected track: show its panel.
    OpenTrack(Instance<TrackState>),
}

pub struct Timeline {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
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
    /// Whether the view follows the playhead. It does while the playhead is on screen, and it
    /// stops when the composer scrolls it off screen, until the next jump brings it back.
    follows_playhead: bool,
    /// The jump count of the last playhead this view saw, see [`Playhead::jumps`].
    seen_jumps: u64,
    /// The tracks in display order, each with the end of its last clip. Finding the ends walks
    /// every clip, so they are kept between the project events that can change them and are
    /// not read again per paint. Nothing else of the project is kept.
    order: Vec<Instance<TrackState>>,
    ends: BTreeMap<InstanceId, Ticks>,
    /// What the events since the last render may have changed.
    stale: Stale,
    selected_clip: Option<InstanceId>,
    /// The track whose header was clicked last. The track panel shows it. The keys go to the
    /// selected clip first, and to this track when no clip is selected.
    selected_track: Option<InstanceId>,
    /// The selected clip was deleted in the event group that is arriving, and what the same
    /// group created. See [`Self::reselect`].
    lost_selection: Option<InstanceId>,
    created_in_group: Vec<InstanceId>,
    forgets_group_later: bool,
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
        let playhead = session.read(cx).playhead().clone();
        let seen_jumps = playhead.read(cx).jumps;
        // The view follows the playhead. This runs on every playhead change, which is every
        // frame while playing, and notifies only when the view really moves, so the timeline
        // is still not painted per frame.
        cx.observe(&playhead, |timeline, _, cx| timeline.follow_playhead(cx))
            .detach();
        let project_events = cx.subscribe(&session, |timeline, _, event, cx| {
            let shown = |id: &InstanceId| timeline.shows(id, cx);
            let changed = match event {
                ProjectEvent::Changed(id) => shown(id),
                ProjectEvent::Created(id) => {
                    let changed = shown(id);
                    if changed {
                        timeline.created_in_group.push(id.clone());
                        timeline.reselect(cx);
                        timeline.forget_group_later(cx);
                    }
                    changed
                }
                ProjectEvent::Deleted(id) => {
                    let changed = shown(id);
                    if timeline.selected_track.as_ref() == Some(id) {
                        timeline.select_track(None, cx);
                    }
                    if timeline.selected_clip.as_ref() == Some(id) {
                        timeline.select_clip(None, cx);
                        timeline.lost_selection = Some(id.clone());
                        timeline.reselect(cx);
                        timeline.forget_group_later(cx);
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
        // A drag that is still open when the timeline goes away must not leave the gesture
        // of the session open: undo and redo wait for it.
        cx.on_release(|timeline, cx| {
            if timeline.drag.take().is_some_and(|drag| drag.begun) {
                let session = timeline.session.clone();
                session.update(cx, |session, cx| session.finish_gesture(cx));
            }
        })
        .detach();
        Self {
            session,
            playhead,
            arrangement,
            viewport: Viewport::default(),
            painted: Rc::default(),
            painted_size: Rc::default(),
            follows_playhead: true,
            seen_jumps,
            order: Vec::new(),
            ends: BTreeMap::new(),
            stale: Stale::Everything,
            selected_clip: None,
            selected_track: None,
            lost_selection: None,
            created_in_group: Vec::new(),
            forgets_group_later: false,
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

    /// Sets zoom and scroll, kept inside the content for the size that was last painted. The
    /// composer decides here whether the view follows the playhead: scrolling it off screen
    /// stops the following, and scrolling it back in starts it again.
    pub fn set_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        let placed = self.place_viewport(viewport, cx);
        let (width, _) = self.painted_size.get();
        self.follows_playhead = placed.shows(self.playhead.read(cx).tick, width);
    }

    /// Clamps and applies a viewport, and notifies when it moved. Gives what was applied.
    fn place_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) -> Viewport {
        self.refresh_order(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(viewport, width, height, cx);
        if self.viewport != viewport {
            self.viewport = viewport;
            cx.notify();
        }
        viewport
    }

    /// Keeps the playhead on screen while the project plays, and brings it back after a jump.
    /// Nothing pulls the view back while the composer has scrolled the playhead off screen:
    /// the next stop or seek does that.
    fn follow_playhead(&mut self, cx: &mut Context<Self>) {
        let playhead = *self.playhead.read(cx);
        let jumped = playhead.jumps != self.seen_jumps;
        self.seen_jumps = playhead.jumps;
        if jumped {
            self.follows_playhead = true;
        } else if !playhead.playing || !self.follows_playhead {
            return;
        }
        let (width, _) = self.painted_size.get();
        if width <= 0.0 {
            return;
        }
        self.place_viewport(self.viewport.following(playhead.tick, width), cx);
    }

    fn clamped(&self, viewport: Viewport, width: f32, height: f32, cx: &App) -> Viewport {
        // The playhead runs past the end of the piece, and the view follows it there, so the
        // scroll room reaches at least that far. Playback does not stop at the end yet.
        let end = self.ends.values().max().copied().unwrap_or_default();
        let extent = Extent {
            end: end.max(self.playhead.read(cx).tick),
            tracks: self.order.len(),
        };
        viewport.clamped(extent, self.time_signature(cx), width, height)
    }

    /// Whether an event about `id` can change what the timeline paints: the arrangement, a
    /// track, or a clip of a track. Another child of a track shows nowhere here. So a knob
    /// drag on the instrument of a track, which changes it per mouse move, reads no clips
    /// again and paints nothing. What a deleted id was is not known any more, so it counts.
    fn shows(&self, id: &InstanceId, cx: &App) -> bool {
        let arrangement = self.arrangement.id();
        let Some(parent) = id.parent() else {
            return id == arrangement;
        };
        if parent == *arrangement {
            return true;
        }
        let project = self.session.read(cx).project();
        parent.parent().as_ref() == Some(arrangement)
            && project.tool_of(id).is_none_or(|tool| tool == Clip::TOOL)
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

    /// A move to another track is a delete and a create in one group, and so is its undo and
    /// its redo. The selection goes with the clip: when the selected clip is deleted and the
    /// same group creates a clip of the same name, that one is selected.
    fn reselect(&mut self, cx: &mut Context<Self>) {
        let Some(lost) = &self.lost_selection else {
            return;
        };
        let project = self.session.read(cx).project();
        let mut created = self.created_in_group.iter();
        let found =
            created.find(|id| id.name() == lost.name() && project.resolve::<Clip>(id).is_some());
        let found = found.cloned();
        if let Some(found) = found {
            // Through the one path, so the session hears it too: what the timeline shows as
            // selected and what the rest of the window offers for it are one thing.
            self.select_clip(Some(found), cx);
            self.lost_selection = None;
        }
    }

    /// Forgets what `reselect` keeps, once per group. Deferred work runs after the events
    /// that are waiting, which are the rest of the group.
    fn forget_group_later(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.forgets_group_later, true) {
            return;
        }
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |timeline, _| {
                    timeline.lost_selection = None;
                    timeline.created_in_group.clear();
                    timeline.forgets_group_later = false;
                });
            }
        });
    }

    pub fn selected_clip(&self) -> Option<&InstanceId> {
        self.selected_clip.as_ref()
    }

    /// Selects a clip. It goes to the session too, as the selected track does: the window
    /// offers to fit the project tempo to the take of the selected clip, and the arrangement
    /// knows nothing of takes or of fitting.
    pub fn select_clip(&mut self, clip: Option<InstanceId>, cx: &mut Context<Self>) {
        if self.selected_clip != clip {
            self.selected_clip = clip.clone();
            self.session
                .update(cx, |session, cx| session.select_clip(clip, cx));
            cx.notify();
        }
    }

    pub fn selected_track(&self) -> Option<&InstanceId> {
        self.selected_track.as_ref()
    }

    /// Selects a track and no clip, so that the keys are about the track.
    ///
    /// It goes to the session too. The selected track is what a keyboard plays into and what a
    /// recording is written to, and the window wires that: the arrangement knows nothing of
    /// MIDI, and MIDI nothing of tracks.
    pub fn select_track(&mut self, track: Option<InstanceId>, cx: &mut Context<Self>) {
        if track.is_some() {
            self.select_clip(None, cx);
        }
        if self.selected_track != track {
            self.selected_track = track.clone();
            self.session
                .update(cx, |session, cx| session.select(track, cx));
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
                selected: self.selected_track.as_ref() == Some(track.id()),
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
            // A track header.
            let row = scene.viewport.track_at(y, self.order.len());
            if let Some(track) = row.and_then(|row| self.order.get(row)).cloned() {
                self.select_track(Some(track.id().clone()), cx);
                cx.emit(TimelineEvent::OpenTrack(track));
            }
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
        let resize = |edge| ClipDragKind::Resize {
            edge,
            origin: state.clone(),
            written: state.clone(),
            delta: 0,
        };
        let kind = match zone {
            Zone::Body => ClipDragKind::Move {
                start: state.start,
                written: state.start,
            },
            Zone::LeftEdge => resize(Edge::Left),
            Zone::RightEdge => resize(Edge::Right),
        };
        self.drag = Some(ClipDrag {
            home: clip.id().clone(),
            clip,
            kind,
            grab: self.painted.get().tick_at(x),
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

    /// What the pointer asks of the dragged clip now. It goes on from the live clip when that
    /// is not what the drag wrote last.
    fn drag_step(&self, drag: &mut ClipDrag, x: f32, y: f32, cx: &App) -> DragStep {
        let project = self.session.read(cx).project();
        let Some(live) = project.state(&drag.clip) else {
            return DragStep::Gone;
        };
        let viewport = self.painted.get();
        let pointer = viewport.tick_at(x);
        match &mut drag.kind {
            ClipDragKind::Move { start, written } => {
                // Once it moves, the drag owns the start: the clip stays under the pointer,
                // and the rest of the clip is the live one. Before that, an undo under the
                // press may have moved the clip.
                if !drag.begun && live.start != *written {
                    (*start, *written) = (live.start, live.start);
                }
                let row = viewport.nearest_track(y, self.order.len());
                let under_pointer = row.and_then(|row| self.order.get(row));
                let on_now = drag.clip.id().parent();
                let to_track = under_pointer
                    .filter(|track| Some(track.id()) != on_now.as_ref())
                    .cloned();
                let next_start = shifted(*start, snapped_delta(drag.grab, pointer));
                if to_track.is_none() && next_start == live.start {
                    return DragStep::Unchanged;
                }
                let next = Clip {
                    start: next_start,
                    ..live.clone()
                };
                DragStep::Publish { next, to_track }
            }
            ClipDragKind::Resize {
                edge,
                origin,
                written,
                delta,
            } => {
                // Something else wrote the clip. The drag goes on from that clip, and the grab
                // moves by what the drag had done to its edge, so the pointer still means the
                // same distance.
                let rebased = live != written;
                if rebased {
                    let ticks = |ticks: Ticks| ticks.0 as i64;
                    let done = match edge {
                        Edge::Left => ticks(written.start) - ticks(origin.start),
                        Edge::Right => ticks(written.length.ticks()) - ticks(origin.length.ticks()),
                    };
                    drag.grab = shifted(drag.grab, done);
                    (*origin, *written) = (live.clone(), live.clone());
                }
                let next_delta = snapped_delta(drag.grab, pointer);
                if !rebased && next_delta == *delta {
                    return DragStep::Unchanged;
                }
                *delta = next_delta;
                let next = match edge {
                    Edge::Left => resized_left(origin, next_delta),
                    Edge::Right => resized_right(origin, next_delta),
                };
                if next == *written {
                    return DragStep::Unchanged;
                }
                DragStep::Publish {
                    next,
                    to_track: None,
                }
            }
        }
    }

    /// One mouse move of a drag: the clip becomes what the pointer says, through the gesture
    /// of the session, so sound and every other view follow.
    fn drag_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let step = self.drag_step(&mut drag, x, y, cx);
        let DragStep::Publish { next, to_track } = step else {
            self.drag = Some(drag);
            if matches!(step, DragStep::Gone) {
                self.end_drag(cx);
            }
            return;
        };
        let (clip, home, label) = (drag.clip.clone(), drag.home.clone(), drag.kind.label());
        let begun = std::mem::replace(&mut drag.begun, true);
        let wrote = match &drag.kind {
            ClipDragKind::Move { .. } => None,
            ClipDragKind::Resize { .. } => Some(next.clone()),
        };
        let wrote_start = next.start;
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
        if let Some(moved) = moved {
            drag.clip = moved;
            match (&mut drag.kind, wrote) {
                (ClipDragKind::Move { written, .. }, _) => *written = wrote_start,
                (ClipDragKind::Resize { written, .. }, Some(wrote)) => *written = wrote,
                (ClipDragKind::Resize { .. }, None) => {}
            }
        }
        let dragged = drag.clip.id().clone();
        self.drag = Some(drag);
        self.select_clip(Some(dragged), cx);
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
            Some(drag) => matches!(drag.kind, ClipDragKind::Resize { .. }),
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
            return self.on_track_key(key, cx);
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

    /// The keys of the selected track, which it gets while no clip is selected: up and down
    /// select the track above or below, and enter opens its panel.
    fn on_track_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        self.refresh_order(cx);
        let mut rows = self.order.iter();
        let selected = self.selected_track.as_ref();
        let Some(current) = selected.and_then(|id| rows.position(|track| track.id() == id)) else {
            return false;
        };
        let next = match key {
            "enter" => current,
            "up" => nudged_track(current, self.order.len(), -1),
            "down" => nudged_track(current, self.order.len(), 1),
            _ => return false,
        };
        let Some(track) = self.order.get(next).cloned() else {
            return false;
        };
        self.select_track(Some(track.id().clone()), cx);
        if key == "enter" {
            cx.emit(TimelineEvent::OpenTrack(track));
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
    let (selection, selected_header) = (theme.gray_950, theme.alpha_at(0.05));
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
            if row.selected {
                // The shape of a clip, in the same place of the row. No accent: it is a fill.
                let inside = Bounds::new(
                    top + point(px(8.), px(4.)),
                    size(px(HEADER_WIDTH - 16.), px(TRACK_HEIGHT - 8.)),
                );
                window.paint_quad(quad(
                    inside,
                    px(6.),
                    selected_header,
                    px(0.),
                    selected_header,
                    BorderStyle::Solid,
                ));
            }
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
