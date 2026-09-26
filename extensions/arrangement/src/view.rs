//! The arrangement view: track headers, a bar ruler, clips with a miniature of their notes,
//! the playhead, and one detail panel below: the note editor of a clip or the track panel of a
//! track, one at a time. Clips are added, selected, moved, resized, copied, pasted and deleted
//! here with the mouse and the keys, tracks are renamed, and tempo changes are added and
//! removed in the ruler.
//!
//! The views, split so that a moving playhead repaints almost nothing:
//! - [`ArrangementView`] is what the window shows. It stacks the timeline over the detail
//!   panel, and opens, swaps and closes what the panel shows.
//! - [`Timeline`] draws everything that changes with the project, the scroll and the zoom on
//!   one canvas, and only what is visible. GPUI keeps its painted frame while it is not
//!   notified, so playback does not run this code.
//! - [`NoteEditor`] does the same for the notes of one clip.
//! - [`TrackPanel`] shows the devices of one track, each in the view of its own tool.
//! - [`MasterPanel`] shows the master: its volume and its limiter. The master row under the
//!   tracks opens it.
//! - A `PlayheadLine` on top of each draws one line, every frame while the project plays.
//!
//! All positions come from [`layout`], and what a drag does to a clip from [`gesture`]. The
//! timeline gives the [`Scene`] it painted to its mouse listeners, so a click hits exactly
//! what is on screen. Every change goes through the session: a drag is one gesture and one
//! undo step.

pub mod clipboard;
pub mod editor;
pub mod gesture;
pub mod layout;
pub mod master_panel;
mod paint;
pub mod roll;
pub mod selection;
pub mod snap;
pub mod track_panel;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, Hitbox, HitboxBehavior, Hsla, KeyDownEvent,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels,
    Point, ScrollWheelEvent, SharedString, StyleRefinement, Subscription, TextAlign, TextRun,
    Window, canvas, div, fill, point, prelude::*, px, quad, size,
};
use sound_core::{
    Changes, Instance, InstanceId, Project, ProjectError, ProjectEvent, State, Ticks,
    TimeSignature,
};
use sound_notes::Clip;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::{InputSize, TextInput};
use sound_ui::{ActiveTheme, KeyboardFocus, NoticeRoom, Playhead, Session, Views, typography};

use crate::{ArrangementState, TrackState, add_clip, add_clips, free_id_besides, tracks};
use clipboard::CopiedClips;
use editor::EditorEvent;
pub use editor::NoteEditor;
use gesture::{Zone, new_clip, nudged_track, resized_left, resized_right, zone_at};
use layout::{
    Extent, HEADER_WIDTH, RULER_HEIGHT, Rect, TRACK_HEIGHT, Viewport, rows_between, shifted,
};
pub use master_panel::MasterPanel;
use master_panel::{MASTER_NAME, MasterPanelEvent};
use paint::{PlayheadLine, accent, paint_focus_ring, paint_ruler, paint_track_label, placed};
use selection::Selection;
use snap::{Grid, SharedSnap, Snap, snap, snapped_delta};
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

struct OpenMasterPanel {
    panel: Entity<MasterPanel>,
    _events: Subscription,
}

/// What the panel below the timeline shows. One thing at a time: opening another takes its
/// place.
enum Detail {
    Editor(OpenEditor),
    Track(OpenTrackPanel),
    Master(OpenMasterPanel),
}

/// The height of the master row, pinned under the tracks.
pub const MASTER_ROW_HEIGHT: f32 = 40.;

pub struct ArrangementView {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    timeline: Entity<Timeline>,
    playhead_line: Entity<PlayheadLine>,
    detail: Option<Detail>,
    /// The snap setting of the window, shared by the timeline and the note editor.
    snap: SharedSnap,
    /// The master row is a tab stop after the timeline, and enter opens its panel.
    master_focus: FocusHandle,
    master_keyboard: KeyboardFocus,
}

impl ArrangementView {
    pub fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let playhead = session.read(cx).playhead().clone();
        let snap = SharedSnap::default();
        let timeline = cx.new(|cx| {
            Timeline::new(session.clone(), arrangement.clone(), snap.clone(), cx)
        });
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
        // The timeline is a cached view and works out its focus ring while it paints. The
        // master row next to it takes the focus without painting it, so the timeline is told
        // to paint again, or it would still think it had the focus when tab brings it back.
        let focus = timeline.read(cx).focus_handle.clone();
        cx.on_focus_out(&focus, window, |view, _, _, cx| {
            view.timeline.update(cx, |_, cx| cx.notify());
        })
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
        // The notices of the window sit right of the track headers and above the panel below.
        // A view that is gone keeps no room.
        cx.on_release(|view, cx| {
            let room = NoticeRoom::default();
            view.session
                .update(cx, |session, cx| session.set_notice_room(room, cx));
        })
        .detach();
        let view = Self {
            session,
            arrangement,
            timeline,
            playhead_line,
            detail: None,
            snap,
            master_focus: cx.focus_handle().tab_stop(true),
            master_keyboard: KeyboardFocus::default(),
        };
        view.publish_notice_room(cx);
        view
    }

    /// Tells the window where the notices go: right of the header column and above the panel
    /// below, whichever is open, so a notice never covers the mixer strip of a track.
    fn publish_notice_room(&self, cx: &mut Context<Self>) {
        let bottom = match &self.detail {
            Some(Detail::Editor(_)) => EDITOR_HEIGHT,
            Some(Detail::Track(_) | Detail::Master(_)) => track_panel::PANEL_HEIGHT,
            None => 0.,
        };
        let room = NoticeRoom {
            left: HEADER_WIDTH,
            bottom,
        };
        self.session
            .update(cx, |session, cx| session.set_notice_room(room, cx));
    }

    pub fn timeline(&self) -> &Entity<Timeline> {
        &self.timeline
    }

    /// The note editor, while it is open.
    pub fn editor(&self) -> Option<&Entity<NoteEditor>> {
        match &self.detail {
            Some(Detail::Editor(open)) => Some(&open.editor),
            _ => None,
        }
    }

    /// The track panel, while it is open.
    pub fn track_panel(&self) -> Option<&Entity<TrackPanel>> {
        match &self.detail {
            Some(Detail::Track(open)) => Some(&open.panel),
            _ => None,
        }
    }

    /// The master panel, while it is open.
    pub fn master_panel(&self) -> Option<&Entity<MasterPanel>> {
        match &self.detail {
            Some(Detail::Master(open)) => Some(&open.panel),
            _ => None,
        }
    }

    /// Opens the panel of the master. It takes the place of what the panel below showed.
    pub fn open_master_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.master_panel().is_some() {
            return;
        }
        self.close_detail(window, cx);
        let (session, arrangement) = (self.session.clone(), self.arrangement.clone());
        let panel = cx.new(|cx| MasterPanel::new(session, arrangement, cx));
        let events = cx.subscribe_in(&panel, window, |view, _, event, window, cx| {
            let MasterPanelEvent::Close = event;
            view.close_detail(window, cx);
        });
        self.detail = Some(Detail::Master(OpenMasterPanel {
            panel,
            _events: events,
        }));
        self.publish_notice_room(cx);
        cx.notify();
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
            let (session, snap) = (self.session.clone(), self.snap.clone());
            let editor = cx.new(|cx| NoteEditor::new(session, clip, width, snap, cx));
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
            self.publish_notice_room(cx);
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
        self.publish_notice_room(cx);
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
            Some(Detail::Master(open)) => open.panel.focus_handle(cx),
            None => return,
        };
        if focus_handle.contains_focused(window, cx) {
            window.focus(&self.timeline.focus_handle(cx), cx);
        }
        self.publish_notice_room(cx);
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
            Some(Detail::Master(_)) | None => false,
        }
    }

    /// The master row: pinned under the tracks, with a ring where a track has its dot. A click,
    /// or enter when it has the focus, opens the panel of the master.
    fn master_row(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (hairline, ring, text, selected, focus) = (
            theme.alpha_at(0.05),
            theme.gray_800,
            theme.gray_900,
            theme.alpha_at(0.05),
            theme.lavender,
        );
        let open = self.master_panel().is_some();
        let keyboard_ring = self.master_keyboard.shows_ring(&self.master_focus, window);
        let header = div()
            .id("master-row")
            .debug_selector(|| "master-row".to_string())
            .track_focus(&self.master_focus)
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(hairline)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.master_keyboard.pressed(cx)),
            )
            .on_click(cx.listener(|view, _, window, cx| view.open_master_panel(window, cx)))
            .child(
                // The fill of a selected track header, in the same place.
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(4.))
                    .w(px(HEADER_WIDTH - 16.))
                    .h(px(MASTER_ROW_HEIGHT - 8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(match keyboard_ring {
                        true => focus,
                        false => gpui::transparent_black(),
                    })
                    .when(open, |fill| fill.bg(selected)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(MASTER_ROW_HEIGHT / 2. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(ring),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(MASTER_ROW_HEIGHT / 2. - 10.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .child(MASTER_NAME),
            );
        div()
            .flex_none()
            .h(px(MASTER_ROW_HEIGHT))
            .flex()
            .border_t_1()
            .border_color(hairline)
            .child(header)
            .into_any_element()
    }
}

impl Render for ArrangementView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let timeline = self.timeline.clone();
        let fill_parent = || StyleRefinement::default().size_full();
        // Notes need the room and devices do not, so the two details have heights of their
        // own and a swap between them moves the lower edge of the timeline. Both are cached:
        // the playhead line above draws this view again on every frame.
        let detail = self.detail.as_ref().map(|detail| {
            // Named for tests: `note-editor`, `track-panel`.
            let panel = |name: &'static str, height: f32| {
                div()
                    .debug_selector(move || name.to_string())
                    .flex_none()
                    .h(px(height))
                    .relative()
            };
            match detail {
                Detail::Editor(open) => panel("note-editor", EDITOR_HEIGHT)
                    .child(open.editor.clone().cached(fill_parent()))
                    .child(open.playhead_line.clone()),
                Detail::Track(open) => {
                    let height = track_panel::PANEL_HEIGHT;
                    panel("track-panel", height).child(open.panel.clone().cached(fill_parent()))
                }
                Detail::Master(open) => {
                    let height = track_panel::PANEL_HEIGHT;
                    panel("master-panel", height).child(open.panel.clone().cached(fill_parent()))
                }
            }
        });
        let master_row = self.master_row(window, cx);
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
            .child(master_row)
            .children(detail)
    }
}

struct TrackRow {
    y: f32,
    name: SharedString,
    accent: Hsla,
    selected: bool,
    /// The name is being edited: the field of the timeline shows it, not the paint.
    renaming: bool,
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

/// A tempo change after tick 0, in the ruler. The one at tick 0 shows in the transport.
struct TempoMark {
    tick: Ticks,
    x: f32,
    text: SharedString,
    selected: bool,
}

/// What one paint shows: only the visible rows, clips and bars. Later clips are on top.
pub struct Scene {
    pub viewport: Viewport,
    pub clips: Vec<ClipShape>,
    rows: Vec<TrackRow>,
    bars: Vec<(u64, f32)>,
    tempo: Vec<TempoMark>,
    /// Where each tempo change is in the ruler, across: filled by the paint, which measures
    /// the labels, and hit by a press.
    tempo_zones: Vec<(Ticks, Range<f32>)>,
    /// The rectangle of a drag on empty space.
    marquee: Option<Rect>,
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

    /// The tempo change whose mark is at `x` in the ruler.
    fn tempo_at(&self, x: f32) -> Option<Ticks> {
        let mut zones = self.tempo_zones.iter().rev();
        zones
            .find(|(_, across)| across.contains(&x))
            .map(|(tick, _)| *tick)
    }
}

/// One selected clip during a move: where it is now, the id it had at mouse down, the row of
/// its track then, and its start. When the live clip is not what the drag wrote, something else
/// changed it: an undo between mouse down and the first move, or an agent.
struct MovedClip {
    /// The clip now. Its id changes when the drag takes it to another track.
    clip: Instance<Clip>,
    /// A drag that comes back to the first track takes this id again, so a drag there and back
    /// leaves the file where it was.
    home: InstanceId,
    row: usize,
    start: Ticks,
    written: Ticks,
}

/// What a drag of clips does.
enum ClipDragKind {
    /// Every selected clip, by the same distance in time and in rows. A move writes only the
    /// start and the track, so it keeps what else changed.
    Move {
        clips: Vec<MovedClip>,
        /// The row of the clip under the pointer at mouse down, and its place in `clips`.
        grab_row: usize,
        grabbed: usize,
    },
    /// Only a resize keeps a whole clip, because `Clip::set_length` drops notes for good:
    /// every move starts from `origin` again, so going in and out loses nothing. One clip.
    Resize {
        clip: Instance<Clip>,
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

/// A drag of clips, from mouse down to mouse up.
struct ClipDrag {
    kind: ClipDragKind,
    /// The tick under the pointer at mouse down.
    grab: Ticks,
    /// Whether the gesture of the session is open. It opens with the first move that changes
    /// something, so a plain click is no undo step.
    begun: bool,
    /// A press on one clip of several selected ones. When it comes up without a move, that clip
    /// is selected alone, as in the Finder.
    select_on_release: Option<InstanceId>,
}

impl ClipDrag {
    fn label(&self) -> &'static str {
        match &self.kind {
            ClipDragKind::Move { clips, .. } => plural(clips.len(), "Move clip", "Move clips"),
            ClipDragKind::Resize { .. } => "Resize clip",
        }
    }

    /// The clips the drag holds now.
    fn holds(&self, id: &InstanceId) -> bool {
        match &self.kind {
            ClipDragKind::Move { clips, .. } => clips.iter().any(|moved| moved.clip.id() == id),
            ClipDragKind::Resize { clip, .. } => clip.id() == id,
        }
    }
}

/// A drag on empty space: the clips it touches are selected. Its corners are a tick and a
/// height from the top of the first track, so a scroll during it keeps its start in place.
struct Marquee {
    from: (Ticks, f64),
    to: (Ticks, f64),
    /// What was selected before, which a drag with shift or cmd adds to.
    before: Vec<InstanceId>,
}

/// The name of a track while it is being edited in its header.
struct Rename {
    track: Instance<TrackState>,
    input: Entity<TextInput>,
    /// A click anywhere else finishes the edit, as in the Finder.
    _blur: Subscription,
}

/// One clip of a move to another place: the clip now, the id it had when the move began,
/// the track it goes to and what it becomes there.
struct ClipMove {
    clip: Instance<Clip>,
    home: InstanceId,
    to: Instance<TrackState>,
    next: Clip,
}

/// Moves clips in one group of changes. A clip that stays on its track gets its new record. One
/// that goes to another track is a delete and a create, like moving a file: back on the track of
/// its `home` it takes that id again, elsewhere its name, or the next free one. Gives the clips
/// at their ids after the move, in the order of `moves`.
fn move_clips(
    project: &Project,
    changes: &mut Changes,
    moves: Vec<ClipMove>,
) -> Result<Vec<Instance<Clip>>, ProjectError> {
    let mut taken = BTreeSet::new();
    let mut moved = Vec::new();
    for ClipMove {
        clip,
        home,
        to,
        next,
    } in moves
    {
        if clip.id().parent().as_ref() == Some(to.id()) {
            changes.set(&clip, next);
            moved.push(clip);
            continue;
        }
        changes.delete(clip.id());
        let id = match home.parent().as_ref() == Some(to.id()) {
            true => home,
            false => free_id_besides(project, &to.id().child(home.name())?, &taken)?,
        };
        taken.insert(id.clone());
        moved.push(changes.create(id, next));
    }
    Ok(moved)
}

/// The undo label for one thing or several.
fn plural(count: usize, one: &'static str, several: &'static str) -> &'static str {
    match count {
        1 => one,
        _ => several,
    }
}

/// What of the kept track order and clip ends has to be read again. A drag changes a few clips
/// per mouse move, so only their tracks are walked then, not every clip of the project.
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
    /// A click on a track header, or cmd-down on the selected track: show its panel.
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
    /// The selected clips. The first one is what the note editor shows.
    clips: Selection<InstanceId>,
    /// The track whose header was clicked last. The track panel shows it. The keys go to the
    /// selected clips first, and to this track when no clip is selected.
    selected_track: Option<InstanceId>,
    /// The selected tempo change, by its tick. Selecting one selects no clip.
    selected_tempo: Option<Ticks>,
    /// Selected clips that were deleted in the event group that is arriving, each with whether
    /// it came first, and what the same group created. See [`Self::reselect`].
    lost_selection: Vec<(InstanceId, bool)>,
    created_in_group: Vec<InstanceId>,
    forgets_group_later: bool,
    drag: Option<ClipDrag>,
    marquee: Option<Marquee>,
    /// What cmd-c and cmd-x kept, for cmd-v. In the app only.
    clipboard: Option<CopiedClips>,
    rename: Option<Rename>,
    snap: SharedSnap,
    /// The snap setting, in the corner above the track headers.
    snap_menu: Entity<DropdownMenu>,
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
        snap: SharedSnap,
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
                    if timeline.rename.as_ref().is_some_and(|rename| rename.track.id() == id) {
                        timeline.rename = None;
                    }
                    let first = timeline.clips.primary() == Some(id);
                    if timeline.clips.remove(id) {
                        timeline.publish_selection(cx);
                        timeline.lost_selection.push((id.clone(), first));
                        timeline.reselect(cx);
                        timeline.forget_group_later(cx);
                    }
                    // Deleted under the drag, from outside. A drag to another track is not
                    // this: it names its new clips before this event arrives.
                    if timeline.drag.as_ref().is_some_and(|drag| drag.holds(id)) {
                        timeline.end_drag(cx);
                    }
                    changed
                }
                // The time signature places the bars, and the tempo map the tempo marks.
                ProjectEvent::ProjectFileChanged => {
                    timeline.forget_lost_tempo(cx);
                    true
                }
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
        let snap_menu = cx.new(|cx| {
            let items = Snap::ALL.map(|snap| MenuItem::new(snap.label(), snap.label()));
            let entries = vec![MenuEntry::Group(MenuGroup::new().label("Snap").items(items))];
            DropdownMenu::new("Snap", entries, cx)
                .debug_name("snap")
                .trigger(Trigger::Select)
                .width(160.)
                .selected(snap.get().label())
        });
        cx.subscribe(&snap_menu, |timeline, _, picked: &MenuPicked, _| {
            if let Some(picked) = Snap::from_label(&picked.0) {
                timeline.snap.set(picked);
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
            clips: Selection::default(),
            selected_track: None,
            selected_tempo: None,
            lost_selection: Vec::new(),
            created_in_group: Vec::new(),
            forgets_group_later: false,
            drag: None,
            marquee: None,
            clipboard: None,
            rename: None,
            snap,
            snap_menu,
            over_edge: false,
            focus_handle,
            keyboard_focus: KeyboardFocus::default(),
            _project_events: project_events,
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// The snap setting of the window.
    pub fn snap(&self) -> Snap {
        self.snap.get()
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

    /// The row of a track in the order that was read last.
    fn row_of(&self, track: &InstanceId) -> Option<usize> {
        self.order.iter().position(|row| row.id() == track)
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
    /// its redo. The selection goes with the clips: when a selected clip is deleted and the
    /// same group creates a clip of the same name, that one is selected.
    fn reselect(&mut self, cx: &mut Context<Self>) {
        if self.lost_selection.is_empty() {
            return;
        }
        let project = self.session.read(cx).project();
        let mut found = Vec::new();
        self.lost_selection.retain(|(lost, first)| {
            let mut created = self.created_in_group.iter();
            let same = created.find(|id| {
                id.name() == lost.name()
                    && project.resolve::<Clip>(id).is_some()
                    && !found.iter().any(|(found, _)| found == *id)
            });
            match same {
                Some(same) => {
                    found.push((same.clone(), *first));
                    false
                }
                None => true,
            }
        });
        if found.is_empty() {
            return;
        }
        let first = found.iter().find(|(_, first)| *first).map(|(id, _)| id.clone());
        let mut selected: Vec<_> = self.clips.iter().cloned().collect();
        selected.extend(found.into_iter().map(|(id, _)| id));
        let primary = first.or_else(|| self.clips.primary().cloned());
        self.set_clips(selected, primary, cx);
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
                    timeline.lost_selection.clear();
                    timeline.created_in_group.clear();
                    timeline.forgets_group_later = false;
                });
            }
        });
    }

    /// The first selected clip: the one the note editor shows.
    pub fn selected_clip(&self) -> Option<&InstanceId> {
        self.clips.primary()
    }

    /// Every selected clip, by id.
    pub fn selected_clips(&self) -> impl Iterator<Item = &InstanceId> {
        self.clips.iter()
    }

    /// Selects one clip, or none.
    pub fn select_clip(&mut self, clip: Option<InstanceId>, cx: &mut Context<Self>) {
        self.set_clips(clip.clone(), clip, cx);
    }

    /// Selects exactly these clips, `primary` first.
    pub fn set_clips(
        &mut self,
        clips: impl IntoIterator<Item = InstanceId>,
        primary: Option<InstanceId>,
        cx: &mut Context<Self>,
    ) {
        let before = self.clips.clone();
        self.clips.set(clips, primary);
        if self.clips != before {
            if !self.clips.is_empty() {
                self.selected_tempo = None;
            }
            self.publish_selection(cx);
            cx.notify();
        }
    }

    /// The first selected clip goes to the session too, as the selected track does: the window
    /// offers to fit the project tempo to the take of the selected clip, and the arrangement
    /// knows nothing of takes or of fitting.
    fn publish_selection(&self, cx: &mut Context<Self>) {
        let primary = self.clips.primary().cloned();
        let published = self.session.read(cx).selected_clip().cloned();
        if primary != published {
            self.session
                .update(cx, |session, cx| session.select_clip(primary, cx));
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
            self.select_tempo(None, cx);
        }
        if self.selected_track != track {
            self.selected_track = track.clone();
            self.session
                .update(cx, |session, cx| session.select(track, cx));
            cx.notify();
        }
    }

    /// The selected tempo change, by its tick.
    pub fn selected_tempo(&self) -> Option<Ticks> {
        self.selected_tempo
    }

    /// Selects a tempo change and no clip, so delete removes it.
    pub fn select_tempo(&mut self, tick: Option<Ticks>, cx: &mut Context<Self>) {
        if tick.is_some() {
            self.select_clip(None, cx);
        }
        if self.selected_tempo != tick {
            self.selected_tempo = tick;
            cx.notify();
        }
    }

    /// A tempo change removed from outside is not selected any more.
    fn forget_lost_tempo(&mut self, cx: &App) {
        let Some(tick) = self.selected_tempo else {
            return;
        };
        let project = self.session.read(cx).project();
        if project.project_file().tempo_map.change_at(tick).tick != tick {
            self.selected_tempo = None;
        }
    }

    /// The name of the track being edited, while it is.
    pub fn renaming(&self) -> Option<&Instance<TrackState>> {
        self.rename.as_ref().map(|rename| &rename.track)
    }

    fn time_signature(&self, cx: &App) -> TimeSignature {
        let project = self.session.read(cx).project();
        project.project_file().tempo_map.time_signature()
    }

    /// The grid of the snap setting in the time signature of the project.
    fn grid(&self, cx: &App) -> Grid {
        self.snap.get().grid(self.time_signature(cx))
    }

    /// Everything to paint into a timeline area of this size, read from the project now.
    fn scene(&self, width: f32, height: f32, cx: &App) -> Scene {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let tempo_map = &project.project_file().tempo_map;
        let time_signature = tempo_map.time_signature();
        // Clamped again for this size: the window may have grown since the last scroll.
        let viewport = self.clamped(self.viewport, width, height, cx);
        let visible_ticks = viewport.visible_ticks(width);
        let renaming = self.rename.as_ref().map(|rename| rename.track.id());

        let changes = tempo_map.tempo_changes().iter();
        let tempo = changes
            .filter(|change| change.tick > Ticks(0))
            .map(|change| TempoMark {
                tick: change.tick,
                x: viewport.x_of(change.tick),
                text: change.bpm.to_string().into(),
                selected: self.selected_tempo == Some(change.tick),
            })
            // A label reaches right of its tick, so one that starts just left of the area
            // still shows its end.
            .filter(|mark| (-TEMPO_LABEL_ROOM..width).contains(&mark.x))
            .collect();
        let marquee = self.marquee.as_ref().map(|marquee| {
            let (left, right) = ordered(marquee.from.0, marquee.to.0);
            let (top, bottom) = ordered(marquee.from.1, marquee.to.1);
            let x = viewport.x_of(left);
            Rect {
                x,
                y: (top - viewport.scroll_y) as f32,
                width: viewport.x_of(right) - x,
                height: (bottom - top) as f32,
            }
        });

        let mut scene = Scene {
            viewport,
            clips: Vec::new(),
            rows: Vec::new(),
            bars: viewport.ruler_bars(time_signature, width).collect(),
            tempo,
            tempo_zones: Vec::new(),
            marquee,
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
                renaming: renaming == Some(track.id()),
            });
            let first = scene.clips.len();
            for (clip, state) in project.children::<Clip>(track.id()) {
                if state.start >= visible_ticks.end || state.end() <= visible_ticks.start {
                    continue;
                }
                let rect = viewport.clip_rect(index, state);
                scene.clips.push(ClipShape {
                    notes: viewport.miniature(state, rect).collect(),
                    selected: self.clips.contains(clip.id()),
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
        (x, y): (f32, f32),
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let double = event.click_count == 2;
        // Shift and cmd add to the selection or take out of it, as in the Finder.
        let adds = event.modifiers.shift || event.modifiers.platform;
        if x < 0.0 {
            // The corner above the headers holds the snap setting, which takes its own clicks.
            if y < 0.0 {
                return;
            }
            // A track header. A double click edits its name.
            let row = scene.viewport.track_at(y, self.order.len());
            if let Some(track) = row.and_then(|row| self.order.get(row)).cloned() {
                self.select_track(Some(track.id().clone()), cx);
                cx.emit(TimelineEvent::OpenTrack(track.clone()));
                if double {
                    self.start_rename(track, window, cx);
                }
            }
            return;
        }
        if y < 0.0 {
            self.on_ruler(x, double, scene, cx);
            return;
        }
        let Some((shape, zone)) = scene.zone_at(x, y) else {
            if double {
                self.add_clip_at(x, y, scene, cx);
            } else {
                self.start_marquee(x, y, adds, cx);
            }
            return;
        };
        let clip = shape.clip.clone();
        if adds {
            let mut clips = self.clips.clone();
            clips.toggle(clip.id().clone());
            let primary = clips.primary().cloned();
            self.set_clips(clips.iter().cloned().collect::<Vec<_>>(), primary, cx);
            return;
        }
        if double {
            self.select_clip(Some(clip.id().clone()), cx);
            cx.emit(TimelineEvent::OpenEditor(clip));
            return;
        }
        let grab = self.painted.get().tick_at(x);
        let kind = match zone {
            Zone::Body => self.start_move(&clip, cx),
            Zone::LeftEdge | Zone::RightEdge => {
                self.select_clip(Some(clip.id().clone()), cx);
                let Some(state) = self.session.read(cx).project().state(&clip).cloned() else {
                    return;
                };
                let edge = match zone {
                    Zone::LeftEdge => Edge::Left,
                    _ => Edge::Right,
                };
                Some(ClipDragKind::Resize {
                    clip,
                    edge,
                    origin: state.clone(),
                    written: state,
                    delta: 0,
                })
            }
        };
        let Some(kind) = kind else {
            return;
        };
        let several = matches!(&kind, ClipDragKind::Move { clips, .. } if clips.len() > 1);
        self.drag = Some(ClipDrag {
            kind,
            grab,
            begun: false,
            select_on_release: several.then(|| shape.clip.id().clone()),
        });
    }

    /// A press on the body of a clip: a move of it, or of every selected clip when it is one of
    /// them. `None` when the project has none of them any more.
    fn start_move(&mut self, pressed: &Instance<Clip>, cx: &mut Context<Self>) -> Option<ClipDragKind> {
        self.refresh_order(cx);
        if self.clips.contains(pressed.id()) {
            let selected: Vec<_> = self.clips.iter().cloned().collect();
            self.set_clips(selected, Some(pressed.id().clone()), cx);
        } else {
            self.select_clip(Some(pressed.id().clone()), cx);
        }
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for id in self.clips.iter() {
            let clip = project.resolve::<Clip>(id)?;
            let start = project.state(&clip)?.start;
            let row = self.row_of(&id.parent()?)?;
            clips.push(MovedClip {
                home: id.clone(),
                clip,
                row,
                start,
                written: start,
            });
        }
        let grabbed = clips.iter().position(|moved| moved.clip.id() == pressed.id())?;
        let grab_row = clips.get(grabbed)?.row;
        Some(ClipDragKind::Move {
            clips,
            grab_row,
            grabbed,
        })
    }

    /// A press in the ruler: on a tempo mark it selects the tempo change and moves the playhead
    /// onto it, so the tempo of the transport is its tempo and a drag there edits it. Anywhere
    /// else it moves the playhead to the grid, and a double click adds a tempo change there.
    fn on_ruler(&mut self, x: f32, double: bool, scene: &Scene, cx: &mut Context<Self>) {
        if let Some(tick) = scene.tempo_at(x) {
            self.select_tempo(Some(tick), cx);
            self.seek(tick, cx);
            return;
        }
        let tick = snap(scene.viewport.tick_at(x), self.grid(cx).step);
        if double {
            self.add_tempo_change(tick, cx);
        } else {
            self.select_tempo(None, cx);
        }
        self.seek(tick, cx);
    }

    fn seek(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.engine().seek(tick));
    }

    /// Adds a tempo change at `tick` with the tempo that plays there, and selects it. A change
    /// that is there already is selected.
    pub fn add_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let Some(tempo_map) = project.project_file().tempo_map.with_change_at(tick) else {
                    return Ok(());
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit("Add tempo change", changes)
            })
        });
        let project = self.session.read(cx).project();
        if project.project_file().tempo_map.change_at(tick).tick == tick && tick > Ticks(0) {
            self.select_tempo(Some(tick), cx);
        }
    }

    fn remove_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let map = &project.project_file().tempo_map;
                let Some(tempo_map) = map.without_change_at(tick) else {
                    return Ok(());
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit("Remove tempo change", changes)
            })
        });
        self.select_tempo(None, cx);
    }

    /// A double click on empty track space: a clip of one bar in the cell under the pointer.
    fn add_clip_at(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let row = scene.viewport.track_at(y, self.order.len());
        let Some(track) = row.and_then(|row| self.order.get(row)).cloned() else {
            return;
        };
        let grid = self.grid(cx);
        let clip = new_clip(scene.viewport.tick_at(x), self.time_signature(cx), grid.step);
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

    /// A press on empty track space begins a rectangle that selects what it touches. Without
    /// shift or cmd it starts from nothing selected.
    fn start_marquee(&mut self, x: f32, y: f32, adds: bool, cx: &mut Context<Self>) {
        if !adds {
            self.select_clip(None, cx);
        }
        self.select_tempo(None, cx);
        let viewport = self.painted.get();
        let corner = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        self.marquee = Some(Marquee {
            from: corner,
            to: corner,
            before: self.clips.iter().cloned().collect(),
        });
    }

    /// One mouse move of the rectangle: the clips it touches and what was selected before.
    fn marquee_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let viewport = self.painted.get();
        let Some(marquee) = &mut self.marquee else {
            return;
        };
        marquee.to = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        let (left, right) = ordered(marquee.from.0, marquee.to.0);
        let rows = rows_between(marquee.from.1, marquee.to.1, self.order.len());
        let project = self.session.read(cx).project();
        let mut selected = marquee.before.clone();
        for track in self.order.get(rows).unwrap_or_default() {
            let clips = project.children::<Clip>(track.id());
            let touched = clips.filter(|(_, clip)| clip.start <= right && clip.end() > left);
            selected.extend(touched.map(|(clip, _)| clip.id().clone()));
        }
        let primary = self.clips.primary().cloned();
        self.set_clips(selected, primary, cx);
        cx.notify();
    }

    /// One mouse move of a drag: the clips become what the pointer says, through the gesture
    /// of the session, so sound and every other view follow. `free` is cmd held: no snap.
    fn drag_to(&mut self, x: f32, y: f32, free: bool, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let grid = match free {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(ClipDragKind::Move { .. }) => self.drag_move(x, y, grid, cx),
            Some(ClipDragKind::Resize { .. }) => self.drag_resize(x, grid, cx),
            None => {}
        }
    }

    /// A move of the selected clips: all by the same distance in time and in track rows. The
    /// earliest stops at tick 0 and the outer ones at the first and the last track, and the
    /// others keep their distance to them.
    fn drag_move(&mut self, x: f32, y: f32, grid: Grid, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let label = drag.label();
        let ClipDragKind::Move {
            clips,
            grab_row,
            grabbed,
        } = &mut drag.kind
        else {
            self.drag = Some(drag);
            return;
        };
        let project = self.session.read(cx).project();
        let lives: Option<Vec<Clip>> = clips
            .iter()
            .map(|moved| project.state(&moved.clip).cloned())
            .collect();
        let Some(lives) = lives else {
            // A clip is gone: deleted from outside.
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        // Once it moves, the drag owns the starts. Before that, an undo under the press may
        // have moved a clip.
        if !drag.begun {
            for (moved, live) in clips.iter_mut().zip(&lives) {
                if live.start != moved.written {
                    (moved.start, moved.written) = (live.start, live.start);
                }
            }
        }
        let viewport = self.painted.get();
        let earliest = clips.iter().map(|moved| moved.start.0).min().unwrap_or(0);
        let delta = snapped_delta(drag.grab, viewport.tick_at(x), grid.step);
        let delta = delta.max(-(earliest as i64));
        let rows = self.order.len();
        let (top, bottom) = (
            clips.iter().map(|moved| moved.row).min().unwrap_or(0),
            clips.iter().map(|moved| moved.row).max().unwrap_or(0),
        );
        let under_pointer = viewport.nearest_track(y, rows).unwrap_or(*grab_row);
        let row_delta = (under_pointer as i64 - *grab_row as i64)
            .clamp(-(top as i64), rows.saturating_sub(1 + bottom) as i64);
        let mut moves = Vec::new();
        for (moved, live) in clips.iter().zip(lives) {
            let row = moved.row.saturating_add_signed(row_delta as isize);
            let Some(to) = self.order.get(row).cloned() else {
                self.drag = Some(drag);
                return;
            };
            let next = Clip {
                start: shifted(moved.start, delta),
                ..live
            };
            moves.push(ClipMove {
                clip: moved.clip.clone(),
                home: moved.home.clone(),
                to,
                next,
            });
        }
        let project = self.session.read(cx).project();
        let unchanged = moves.iter().all(|step| {
            step.clip.id().parent().as_ref() == Some(step.to.id())
                && project.state(&step.clip).map(|live| live.start) == Some(step.next.start)
        });
        if unchanged {
            self.drag = Some(drag);
            return;
        }
        let starts: Vec<Ticks> = moves.iter().map(|step| step.next.start).collect();
        let begun = std::mem::replace(&mut drag.begun, true);
        let moved = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                let moved = move_clips(project, &mut changes, moves)?;
                project.publish(edit, changes)?;
                Ok(moved)
            })
        });
        if let Some(moved) = moved {
            for ((clip, now), start) in clips.iter_mut().zip(moved).zip(starts) {
                (clip.clip, clip.written) = (now, start);
            }
        }
        let selected: Vec<_> = clips.iter().map(|moved| moved.clip.id().clone()).collect();
        let primary = selected.get(*grabbed).cloned();
        self.drag = Some(drag);
        self.set_clips(selected, primary, cx);
    }

    /// A move of an edge of one clip. It goes on from the live clip when that is not what the
    /// drag wrote last.
    fn drag_resize(&mut self, x: f32, grid: Grid, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let label = drag.label();
        let ClipDragKind::Resize {
            clip,
            edge,
            origin,
            written,
            delta,
        } = &mut drag.kind
        else {
            self.drag = Some(drag);
            return;
        };
        let project = self.session.read(cx).project();
        let Some(live) = project.state(clip).cloned() else {
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        let pointer = self.painted.get().tick_at(x);
        // Something else wrote the clip. The drag goes on from that clip, and the grab moves
        // by what the drag had done to its edge, so the pointer still means the same distance.
        let rebased = live != *written;
        if rebased {
            let ticks = |ticks: Ticks| ticks.0 as i64;
            let done = match edge {
                Edge::Left => ticks(written.start) - ticks(origin.start),
                Edge::Right => ticks(written.length.ticks()) - ticks(origin.length.ticks()),
            };
            drag.grab = shifted(drag.grab, done);
            (*origin, *written) = (live.clone(), live);
        }
        let next_delta = snapped_delta(drag.grab, pointer, grid.step);
        if !rebased && next_delta == *delta {
            self.drag = Some(drag);
            return;
        }
        *delta = next_delta;
        let next = match edge {
            Edge::Left => resized_left(origin, next_delta, grid.unit),
            Edge::Right => resized_right(origin, next_delta, grid.unit),
        };
        if next == *written {
            self.drag = Some(drag);
            return;
        }
        let begun = std::mem::replace(&mut drag.begun, true);
        let (instance, wrote) = (clip.clone(), next.clone());
        let published = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&instance, next);
                project.publish(edit, changes)
            })
        });
        if published.is_some() {
            *written = wrote;
        }
        self.drag = Some(drag);
    }

    /// Mouse up, or a clip went away under the drag: the gesture becomes one undo step. A press
    /// on one of several selected clips that did not move selects that clip alone.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.marquee = None;
        if let Some(drag) = self.drag.take() {
            if drag.begun {
                self.session
                    .update(cx, |session, cx| session.finish_gesture(cx));
            } else if let Some(pressed) = drag.select_on_release {
                self.select_clip(Some(pressed), cx);
            }
        }
        cx.notify();
    }

    /// Escape: the clips go back to where they were at mouse down. Whether there was a drag.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        if self.marquee.take().is_some() {
            cx.notify();
            return true;
        }
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if drag.begun {
            self.session
                .update(cx, |session, cx| session.cancel_gesture(cx));
            if let ClipDragKind::Move { clips, grabbed, .. } = drag.kind {
                let homes: Vec<_> = clips.into_iter().map(|moved| moved.home).collect();
                let primary = homes.get(grabbed).cloned();
                self.set_clips(homes, primary, cx);
            }
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
        project.resolve(self.clips.primary()?)
    }

    /// The selected clips that the project still has, with their state.
    fn selected_states(&self, cx: &App) -> Vec<(Instance<Clip>, Clip)> {
        let project = self.session.read(cx).project();
        let clips = self.clips.iter().filter_map(|id| {
            let clip = project.resolve::<Clip>(id)?;
            let state = project.state(&clip)?.clone();
            Some((clip, state))
        });
        clips.collect()
    }

    /// The keys of the focused timeline. Whether the key was one of them.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Keys that bubble up from a control inside, such as the name field of a track or the
        // snap setting, are that control's.
        if !self.focus_handle.is_focused(window) {
            return false;
        }
        let Modifiers {
            control,
            alt,
            shift,
            platform,
            ..
        } = event.keystroke.modifiers;
        if control || alt || shift {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" && !platform {
            return self.cancel_drag(cx);
        }
        // The mouse has the clips: a key would fight the next mouse move.
        if self.drag.is_some() || self.marquee.is_some() {
            return false;
        }
        if platform {
            return self.on_command(key, cx);
        }
        if key == "t" {
            let tick = self.playhead.read(cx).tick;
            self.add_tempo_change(tick, cx);
            return true;
        }
        if let Some(tick) = self.selected_tempo
            && matches!(key, "backspace" | "delete")
        {
            self.remove_tempo_change(tick, cx);
            return true;
        }
        let Some(clip) = self.selected_instance(cx) else {
            return self.on_track_key(key, window, cx);
        };
        let unit = self.grid(cx).unit.0 as i64;
        match key {
            "enter" => cx.emit(TimelineEvent::OpenEditor(clip)),
            "backspace" | "delete" => self.delete_clips("Delete clip", "Delete clips", cx),
            "left" => self.nudge_in_time(-unit, cx),
            "right" => self.nudge_in_time(unit, cx),
            "up" => self.nudge_to_track(-1, cx),
            "down" => self.nudge_to_track(1, cx),
            _ => return false,
        }
        true
    }

    /// The keys with cmd: select all, copy, cut, paste, duplicate, and cmd-down, which opens
    /// what is selected as enter does in the Finder.
    fn on_command(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "a" => self.select_all(cx),
            "c" => {
                self.copy(cx);
            }
            "x" => {
                if self.copy(cx) {
                    self.delete_clips("Cut clip", "Cut clips", cx);
                }
            }
            "v" => self.paste(cx),
            "d" => self.duplicate(cx),
            "down" => {
                if let Some(clip) = self.selected_instance(cx) {
                    cx.emit(TimelineEvent::OpenEditor(clip));
                } else {
                    let project = self.session.read(cx).project();
                    let selected = self.selected_track.as_ref();
                    let track = selected.and_then(|track| project.resolve::<TrackState>(track));
                    let Some(track) = track else {
                        return false;
                    };
                    cx.emit(TimelineEvent::OpenTrack(track));
                }
            }
            _ => return false,
        }
        true
    }

    /// The keys of the selected track, which it gets while no clip is selected: up and down
    /// select the track above or below, and enter edits its name.
    fn on_track_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.refresh_order(cx);
        let selected = self.selected_track.as_ref();
        let Some(current) = selected.and_then(|id| self.row_of(id)) else {
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
        if key == "enter" {
            self.start_rename(track, window, cx);
        } else {
            self.select_track(Some(track.id().clone()), cx);
        }
        true
    }

    /// Opens the name field of a track in its header, with the name selected.
    pub fn start_rename(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self.session.read(cx).project().state(&track).map(|state| state.name.clone())
        else {
            return;
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx).size(InputSize::Sm);
            input.set_text(name, cx);
            input.select_all_text(cx);
            input
        });
        let this = cx.weak_entity();
        input.update(cx, |input, _| {
            let submit = this.clone();
            input.set_on_submit(move |text, window, cx| {
                if let Some(timeline) = submit.upgrade() {
                    let text = text.to_string();
                    timeline.update(cx, |timeline, cx| {
                        timeline.finish_rename(Some(text), window, cx)
                    });
                }
            });
            input.set_on_cancel(move |_, window, cx| {
                if let Some(timeline) = this.upgrade() {
                    timeline.update(cx, |timeline, cx| timeline.finish_rename(None, window, cx));
                }
            });
        });
        let focus = input.focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |timeline, window, cx| {
            let text = timeline.rename.as_ref().map(|rename| rename.input.read(cx).text().to_string());
            timeline.finish_rename(text, window, cx);
        });
        window.focus(&focus, cx);
        self.rename = Some(Rename {
            track,
            input,
            _blur: blur,
        });
        cx.notify();
    }

    /// Enter or a click elsewhere gives the name, one undo step; escape gives nothing. A name of
    /// only spaces is no name, and the track keeps the one it had.
    fn finish_rename(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        if rename.input.focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        let project = self.session.read(cx).project();
        let state = project.state(&rename.track).cloned();
        let name = name.map(|name| name.trim().to_string());
        if let (Some(mut state), Some(name)) = (state, name)
            && !name.is_empty()
            && name != state.name
        {
            state.name = name;
            let track = rename.track.clone();
            self.session.update(cx, |session, cx| {
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    changes.set(&track, state);
                    project.commit("Rename track", changes)
                })
            });
        }
        cx.notify();
    }

    /// Cmd-a: every clip of the arrangement.
    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let clips = self
            .order
            .iter()
            .flat_map(|track| project.children::<Clip>(track.id()))
            .map(|(clip, _)| clip.id().clone());
        let clips: Vec<_> = clips.collect();
        let primary = self.clips.primary().cloned();
        self.set_clips(clips, primary, cx);
    }

    /// What the selected clips are for the clipboard: each with its row and its name.
    fn copied(&mut self, cx: &mut Context<Self>) -> Option<CopiedClips> {
        self.refresh_order(cx);
        let clips = self.selected_states(cx).into_iter().filter_map(|(clip, state)| {
            let row = self.row_of(&clip.id().parent()?)?;
            Some((row, clip.id().name().to_string(), state))
        });
        CopiedClips::new(clips.collect::<Vec<_>>())
    }

    /// Cmd-c. Whether there was something to copy.
    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(copied) = self.copied(cx) else {
            return false;
        };
        self.clipboard = Some(copied);
        true
    }

    /// Cmd-v: the copied clips at the playhead, the top one on the track of the first selected
    /// clip, else on the selected track, else on the first track. One undo step.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(copied) = self.clipboard.clone() else {
            return;
        };
        self.refresh_order(cx);
        let track = match self.clips.primary() {
            Some(clip) => clip.parent(),
            None => self.selected_track.clone(),
        };
        let top = track.and_then(|track| self.row_of(&track)).unwrap_or(0);
        let at = self.playhead.read(cx).tick;
        let label = plural(copied.len(), "Paste clip", "Paste clips");
        self.add_copies(&copied, at, top, label, cx);
    }

    /// Cmd-d: a copy of the selected clips right after them, on the same tracks. The clipboard
    /// keeps what it had.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(copied) = self.copied(cx) else {
            return;
        };
        let (start, top) = copied.origin();
        let label = plural(copied.len(), "Duplicate clip", "Duplicate clips");
        self.add_copies(&copied, start + copied.span(), top, label, cx);
    }

    /// Adds copies of clips as one undo step and selects them.
    fn add_copies(
        &mut self,
        copied: &CopiedClips,
        at: Ticks,
        top: usize,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let order = self.order.clone();
        let placed = copied.placed(at, top, order.len());
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let clips = placed.into_iter().filter_map(|(row, name, clip)| {
                    Some((order.get(row)?, name, clip))
                });
                let added = add_clips(project, &mut changes, clips)?;
                project.commit(label, changes)?;
                Ok(added)
            })
        });
        if let Some(added) = added {
            let ids: Vec<_> = added.iter().map(|clip| clip.id().clone()).collect();
            let primary = ids.first().cloned();
            self.set_clips(ids, primary, cx);
        }
    }

    fn delete_clips(&mut self, one: &'static str, several: &'static str, cx: &mut Context<Self>) {
        let selected: Vec<_> = self.clips.iter().cloned().collect();
        let label = plural(selected.len(), one, several);
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for clip in &selected {
                    changes.delete(clip);
                }
                project.commit(label, changes)
            })
        });
    }

    /// The arrows left and right: every selected clip by one unit of the grid, as one undo
    /// step. The earliest stops at tick 0.
    fn nudge_in_time(&mut self, delta: i64, cx: &mut Context<Self>) {
        let selected = self.selected_states(cx);
        let earliest = selected.iter().map(|(_, clip)| clip.start.0).min();
        let delta = delta.max(-(earliest.unwrap_or(0) as i64));
        if delta == 0 || selected.is_empty() {
            return;
        }
        let label = plural(selected.len(), "Nudge clip", "Nudge clips");
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for (clip, state) in selected {
                    let start = shifted(state.start, delta);
                    changes.set(&clip, Clip { start, ..state });
                }
                project.commit(label, changes)
            })
        });
    }

    /// The arrows up and down: every selected clip to the track above or below, as one undo
    /// step. Nothing moves when one of them is on the first or the last track already.
    fn nudge_to_track(&mut self, step: i64, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let mut moves = Vec::new();
        for (clip, next) in self.selected_states(cx) {
            let row = clip.id().parent().and_then(|track| self.row_of(&track));
            let row = row.and_then(|row| row.checked_add_signed(step as isize));
            let Some(to) = row.and_then(|row| self.order.get(row)).cloned() else {
                return;
            };
            let home = clip.id().clone();
            moves.push(ClipMove {
                clip,
                home,
                to,
                next,
            });
        }
        if moves.is_empty() {
            return;
        }
        let label = plural(moves.len(), "Nudge clip", "Nudge clips");
        let primary = self.clips.primary().cloned();
        let index = moves.iter().position(|step| Some(step.clip.id()) == primary.as_ref());
        let moved = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let moved = move_clips(project, &mut changes, moves)?;
                project.commit(label, changes)?;
                Ok(moved)
            })
        });
        if let Some(moved) = moved {
            let ids: Vec<_> = moved.iter().map(|clip| clip.id().clone()).collect();
            let primary = index.and_then(|index| ids.get(index).cloned());
            self.set_clips(ids, primary, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        self.set_viewport(scrolled_or_zoomed(self.viewport, event, x), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
    }

    /// The name field over the header of the track being renamed, where the name is painted.
    fn rename_field(&self) -> Option<gpui::AnyElement> {
        let rename = self.rename.as_ref()?;
        let row = self.row_of(rename.track.id())?;
        let top = RULER_HEIGHT + self.painted.get().y_of(row) + (TRACK_HEIGHT - RENAME_HEIGHT) / 2.;
        Some(
            div()
                .debug_selector(|| "rename-track".to_string())
                .absolute()
                .top(px(top))
                .left(px(RENAME_LEFT))
                .w(px(HEADER_WIDTH - RENAME_LEFT - 12.))
                .occlude()
                .child(rename.input.clone())
                .into_any_element(),
        )
    }

    /// The snap setting in the corner above the track headers, on the line of the ruler.
    fn snap_corner(&self, cx: &App) -> impl IntoElement {
        let muted = cx.theme().gray_700;
        div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(HEADER_WIDTH - 1.))
            .h(px(RULER_HEIGHT - 1.))
            .flex()
            .items_center()
            .justify_between()
            .pl(px(24.))
            .pr(px(12.))
            .occlude()
            .child(div().text_size(px(12.)).text_color(muted).child("Snap"))
            .child(self.snap_menu.clone())
    }
}

/// How far right of its tick a tempo label may start to still be seen when its tick is off the
/// left edge.
const TEMPO_LABEL_ROOM: f32 = 80.;
/// The name field of a renamed track: a small text input where the name is painted.
const RENAME_HEIGHT: f32 = 28.;
/// Its left edge, so that its text starts where the painted name does, 44 pt in.
const RENAME_LEFT: f32 = 44. - 8.;

/// Two values, the smaller first.
fn ordered<T: PartialOrd>(a: T, b: T) -> (T, T) {
    match a <= b {
        true => (a, b),
        false => (b, a),
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
                let mut scene = timeline.read(cx).scene(width, height, cx);
                timeline.read(cx).painted.set(scene.viewport);
                timeline.read(cx).painted_size.set((width, height));
                paint_scene(&mut scene, bounds, window, cx);
                let keyboard_focus = &timeline.read(cx).keyboard_focus;
                if keyboard_focus.shows_ring(&focus_handle, window) {
                    paint_focus_ring(bounds, window, cx);
                }
                if timeline.read(cx).resize_cursor() {
                    window.set_cursor_style(CursorStyle::ResizeLeftRight, &hitbox);
                }
                listen(timeline, Rc::new(scene), bounds, hitbox, window);
            },
        );
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|timeline, event, window, cx| {
                if timeline.on_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(surface.size_full())
            .child(self.snap_corner(cx))
            .children(self.rename_field())
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
                let position = Timeline::timeline_position(bounds, event.position);
                timeline.update(cx, |timeline, cx| {
                    window.focus(&timeline.focus_handle, cx);
                    timeline.keyboard_focus.pressed(cx);
                    timeline.on_mouse_down(event, position, &scene, window, cx)
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
                let dragging = timeline.drag.is_some() || timeline.marquee.is_some();
                if !dragging {
                    if hitbox.is_hovered(window) {
                        timeline.hover(x, y, &scene, cx);
                    }
                } else if !event.dragging() {
                    // The button came up somewhere that did not tell this window.
                    timeline.end_drag(cx);
                } else if timeline.marquee.is_some() {
                    timeline.marquee_to(x, y, cx);
                } else {
                    timeline.drag_to(x, y, event.modifiers.platform, cx);
                }
            });
        }
    });
    window.on_mouse_event({
        let timeline = timeline.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                timeline.update(cx, |timeline, cx| {
                    if timeline.drag.is_some() || timeline.marquee.is_some() {
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

fn paint_scene(scene: &mut Scene, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let (hairline, clip_fill, clip_border) = (
        theme.alpha_at(0.05),
        theme.alpha_at(0.05),
        theme.alpha_at(0.10),
    );
    let (selection, selected_header) = (theme.gray_950, theme.alpha_at(0.05));
    let (marquee_fill, marquee_border) = (theme.alpha_at(0.05), theme.alpha_at(0.20));
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

    // The ruler: a short mark and a number per bar, and the tempo changes. No grid below it.
    paint_ruler(&scene.bars, ruler, window, cx);
    scene.tempo_zones = paint_tempo_marks(&scene.tempo, &scene.bars, ruler, window, cx);

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
            // The field over the header shows the name that is being edited.
            let name = match row.renaming {
                true => SharedString::default(),
                false => row.name.clone(),
            };
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
        if let Some(marquee) = scene.marquee {
            let area = placed(marquee, timeline.origin);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(area, px(2.), marquee_fill, px(1.), marquee_border, solid));
        }
    });
}

/// The tempo changes after tick 0 in the ruler: a line at the tick and a label, `140 bpm`, in
/// a box of the window colour that covers the bar numbers under it. On a bar line it starts
/// after the number of the bar. The selected one has a light border, as a selected clip. Gives
/// where each label is across, for the hit test of a press.
fn paint_tempo_marks(
    marks: &[TempoMark],
    bars: &[(u64, f32)],
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<(Ticks, Range<f32>)> {
    let theme = cx.theme();
    let (line, background, border, selected_fill, selected_border, value, unit) = (
        theme.gray_700,
        theme.gray_100,
        theme.alpha_at(0.10),
        theme.alpha_at(0.10),
        theme.gray_950,
        theme.gray_950,
        theme.gray_700,
    );
    let font = typography::tabular();
    let font_size = px(12.);
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut zones = Vec::new();
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        for mark in marks {
            let x = mark.x.round();
            // A bar number at the same place stays readable: the label goes after it.
            let bar = bars.iter().find(|(_, bar_x)| (bar_x.round() - x).abs() < 1.);
            let after = match bar {
                Some((number, _)) => {
                    let text: SharedString = number.to_string().into();
                    let runs = [run(text.len(), unit)];
                    let shaped = window.text_system().shape_line(text, font_size, &runs, None);
                    8. + f32::from(shaped.width) + 4.
                }
                None => 4.,
            };
            let text: SharedString = format!("{} bpm", mark.text).into();
            let runs = [
                run(mark.text.len(), value),
                run(text.len() - mark.text.len(), unit),
            ];
            let shaped = window.text_system().shape_line(text, font_size, &runs, None);
            let (left, width) = (x + after, f32::from(shaped.width) + 12.);
            let tick_line = Bounds::new(
                ruler.origin + point(px(x), px(0.)),
                size(px(1.), px(RULER_HEIGHT)),
            );
            window.paint_quad(fill(tick_line, line));
            let label = Bounds::new(
                ruler.origin + point(px(left), px(6.)),
                size(px(width), px(20.)),
            );
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(label, px(6.), background, px(1.), border, solid));
            if mark.selected {
                let (fill, edge) = (selected_fill, selected_border);
                window.paint_quad(quad(label, px(6.), fill, px(1.), edge, solid));
            }
            let origin = ruler.origin + point(px(left + 6.), px(8.));
            // A glyph that cannot be painted leaves a gap in a label. Nothing else depends on it.
            if let Err(error) = shaped.paint(origin, px(17.), TextAlign::Left, None, window, cx) {
                eprintln!("arrangement view: {error}");
            }
            zones.push((mark.tick, x - 4.0..left + width));
        }
    });
    zones
}
