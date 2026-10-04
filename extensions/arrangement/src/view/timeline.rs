//! The timeline of the arrangement: track headers, the bar ruler with its tempo changes, and
//! the clips, painted on one canvas. Clips of notes and of audio are added, selected, moved,
//! resized, copied, pasted and deleted here with the mouse and the keys, an audio clip is
//! trimmed, faded and turned up or down from its handles, audio files are dropped in from the
//! Finder, a track is renamed in its header and deleted with backspace, tempo changes are added
//! and removed in the ruler, and the snap setting sits in the corner. Under a track its
//! automation lanes show at the toggle in its header, where lanes are added and their points
//! added, moved, deleted and erased ([`super::track_lanes`]).
//!
//! This file holds the view itself: its fields, the project events, the viewport, the selection,
//! the render and the mouse listeners. The rest is in `timeline/`, as more `impl Timeline`:
//! - `scene`: what one paint shows, the hit tests on it and its painting.
//! - `build`: the scene of a paint, from the project.
//! - `state`: the types of what the mouse drags, a rename and files dragged in.
//! - `edits`: moves of clips and their automation, as pure functions.
//! - `mouse`: the press, the ruler, the marquee, the end of a drag, hover and the cursor.
//! - `drag`: the mouse moves of a drag of clips or of a track.
//! - `keys`: the keys, and deleting and renaming a track.
//! - `clipboard`: copy, paste, duplicate, delete and nudge.
//! - `file_drop`: audio files dropped from the Finder.
//! - `lanes`: the automation lanes, their add menus and their points.

mod build;
mod clipboard;
mod drag;
mod edits;
mod file_drop;
mod keys;
mod lanes;
mod mouse;
mod scene;
mod state;

pub(super) use scene::{ARM_LEFT, ARMED_METER_HEIGHT, ARMED_METER_LEFT, paint_takes};
pub use scene::{ClipShape, Scene};
pub use state::DropTarget;
pub(crate) use state::{FADE_IN_LABEL, FADE_OUT_LABEL, GAIN_LABEL};

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, CursorStyle, DispatchPhase, Entity, EventEmitter, ExternalPaths,
    FileDropEvent, FocusHandle, Focusable, Hitbox, HitboxBehavior, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels,
    ScrollWheelEvent, Subscription, Window, canvas, div, prelude::*, px,
};
use sound_core::{Instance, InstanceId, ProjectEvent, State, Ticks, TimeSignatures};
use sound_notes::Clip;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::TextInput;
use sound_ui::{ActiveTheme, KeyboardFocus, Playhead, Recording, Session, Waveforms};

use super::clipboard::SharedClipboard;
use super::clips::{AnyClip, shown_end};
use super::layout::{
    ADD_ROW_HEIGHT, Extent, HEADER_INSET, HEADER_WIDTH, NAME_LEFT, RULER_HEIGHT, Rows, Viewport,
};
use super::paint::paint_focus_ring;
use super::selection::Selection;
use super::snap::{Grid, SharedSnap, Snap};
use crate::{ArrangementState, AudioClip, TrackKind, TrackState, tracks, unnumbered};
use edits::is_clip_tool;
use lanes::LaneMenu;
use scene::{PointKey, paint_scene};
use state::{Held, Incoming, Rename};

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
    /// A click on a track header, or cmd-down on the selected track: show its panel. Also a
    /// double click on an audio clip, or enter: the Clip card of its track shows it.
    OpenTrack(Instance<TrackState>),
}

pub struct Timeline {
    session: Entity<Session>,
    playhead: Entity<Playhead>,
    /// The armed tracks and the takes while they record.
    recording: Entity<Recording>,
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
    /// Where the whole timeline was painted in the window, for a drop, which says only where
    /// the pointer is.
    painted_bounds: Rc<Cell<Bounds<Pixels>>>,
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
    reselects_later: bool,
    held: Held,
    /// The selected point of an automation lane, which delete removes, and the one under the
    /// pointer.
    selected_point: Option<PointKey>,
    hovered_point: Option<PointKey>,
    /// The tracks that show their automation lanes. Interface state: not saved, no undo step.
    expanded: BTreeSet<InstanceId>,
    /// The select that adds a lane, of each track that shows its lanes.
    lane_menus: BTreeMap<InstanceId, LaneMenu>,
    /// What cmd-c and cmd-x kept, for cmd-v. In the app only, and shared with the note
    /// editor.
    clipboard: SharedClipboard,
    /// The clips the last delete or cut of the timeline took away, so that they are selected
    /// again when they come back, which is what undo does. The first of them first.
    deleted: Vec<InstanceId>,
    rename: Option<Rename>,
    snap: SharedSnap,
    /// The snap setting, in the corner above the track headers.
    snap_menu: Entity<DropdownMenu>,
    /// What the cursor says of a drag from where the pointer is: that it resizes, trims, fades
    /// or changes the gain.
    hover_cursor: Option<CursorStyle>,
    /// The clip under the pointer, whose handles show when it is an audio clip.
    hovered: Option<InstanceId>,
    /// Files from the Finder while they are dragged over the timeline.
    incoming: Option<Incoming>,
    /// What a drag of files carries, as GPUI gives it while it draws the timeline under one.
    dragged_paths: Rc<RefCell<Vec<PathBuf>>>,
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    _project_events: Subscription,
}

impl EventEmitter<TimelineEvent> for Timeline {}

impl Timeline {
    pub(super) fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        (snap, clipboard): (SharedSnap, SharedClipboard),
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
        // An armed track has a shorter name. The level and the growing takes are drawn over
        // the timeline by `RecordingOverlay`, which is not the timeline, so they paint nothing
        // here.
        let recording = session.read(cx).recording().clone();
        cx.observe(&recording, |_, _, cx| cx.notify()).detach();
        // A waveform whose overview was being made is drawn when it is ready.
        let waveforms = Waveforms::entity(cx);
        cx.observe(&waveforms, |_, _, cx| cx.notify()).detach();
        // The add track button and the recording overlay read the order as they render, and
        // the timeline is cached, so it renders after them in the frame. Read it again here,
        // once the events of a group are all in, or they show the tracks of the last frame.
        cx.observe_self(|timeline, cx| timeline.refresh_order(cx))
            .detach();
        let project_events = cx.subscribe(&session, |timeline, _, event, cx| {
            let shown = |id: &InstanceId| timeline.shows(id, cx);
            let changed = match event {
                ProjectEvent::Changed(id) => shown(id),
                ProjectEvent::Created(id) => {
                    let changed = shown(id);
                    if changed {
                        timeline.created_in_group.push(id.clone());
                        timeline.reselect_later(cx);
                    }
                    changed
                }
                ProjectEvent::Deleted(id) => {
                    let changed = shown(id);
                    if timeline.selected_track.as_ref() == Some(id) {
                        timeline.select_track(None, cx);
                    }
                    if timeline
                        .rename
                        .as_ref()
                        .is_some_and(|rename| rename.track.id() == id)
                    {
                        timeline.rename = None;
                    }
                    let first = timeline.clips.primary() == Some(id);
                    if timeline.clips.remove(id) {
                        timeline.publish_selection(cx);
                        timeline.lost_selection.push((id.clone(), first));
                        timeline.reselect_later(cx);
                    }
                    let ends = match &timeline.held {
                        // Deleted under the drag, from outside. A drag to another track is
                        // not this: it names its new clips before this event arrives.
                        Held::Clips(_, kind) => kind.ends_without(id),
                        // The dragged track deleted from outside: that was the last write.
                        // So is the track of a lane being drawn.
                        Held::Track(drag) => drag.track == *id,
                        Held::Lane(drag) => drag.track.id() == id,
                        Held::Nothing | Held::Marquee(_) => false,
                    };
                    if ends {
                        timeline.end_drag(cx);
                    }
                    timeline.expanded.remove(id);
                    timeline.lane_menus.remove(id);
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
                // Read again once for all events of a group, see `observe_self` above.
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
            if let Some(edit) = std::mem::take(&mut timeline.held).edit() {
                edit.finish(&timeline.session, cx);
            }
        })
        .detach();
        let snap_menu = cx.new(|cx| {
            let items = Snap::ALL.map(|snap| MenuItem::new(snap.label(), snap.label()));
            let entries = vec![MenuEntry::Group(
                MenuGroup::new().label("Snap").items(items),
            )];
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
            recording,
            arrangement,
            viewport: Viewport::default(),
            painted: Rc::default(),
            painted_size: Rc::default(),
            painted_bounds: Rc::default(),
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
            reselects_later: false,
            held: Held::Nothing,
            selected_point: None,
            hovered_point: None,
            expanded: BTreeSet::new(),
            lane_menus: BTreeMap::new(),
            clipboard,
            deleted: Vec::new(),
            rename: None,
            snap,
            snap_menu,
            hover_cursor: None,
            hovered: None,
            incoming: None,
            dragged_paths: Rc::default(),
            focus_handle,
            keyboard_focus: KeyboardFocus::default(),
            _project_events: project_events,
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// The viewport of the last paint, for the playhead line.
    pub(super) fn painted(&self) -> Rc<Cell<Viewport>> {
        self.painted.clone()
    }

    /// The width of the timeline area at the last paint.
    pub(super) fn painted_width(&self) -> f32 {
        self.painted_size.get().0
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
            height: self.rows(cx).height(),
        };
        viewport.clamped(extent, self.time_signatures(cx), width, height)
    }

    /// How tall each track row is now: a track, and the lanes it shows under it.
    pub fn rows(&self, cx: &App) -> Rows {
        let project = self.session.read(cx).project();
        let lanes = |track: &Instance<TrackState>| {
            let state = project.state(track);
            state.map_or(0, |state| state.automation.len())
        };
        let shown = self.order.iter().map(|track| {
            let expanded = self.expanded.contains(track.id());
            expanded.then(|| lanes(track))
        });
        Rows::new(shown)
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
            && project
                .tool_of(id)
                .is_none_or(|tool| tool == Clip::TOOL || tool == AudioClip::TOOL)
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
        // The last point of a lane is as far as a clip end: the scroll reaches it.
        let end_of = |track: &InstanceId| {
            let notes = project.children::<Clip>(track).map(|(_, clip)| clip.end());
            let audio = project.children::<AudioClip>(track);
            let audio = audio.map(|(_, clip)| shown_end(project, clip));
            let state = project.resolve::<TrackState>(track);
            let state = state.and_then(|track| project.state(&track));
            let lanes = state.iter().flat_map(|state| state.automation.iter());
            let lanes = lanes.filter_map(|lane| lane.points.last().map(|point| point.tick));
            notes.chain(audio).chain(lanes).max()
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
    /// its redo. The selection goes with the clips: at the end of a group that deleted selected
    /// clips, each of them is selected again where the group created it. The same id first, and
    /// else a clip of the same name, its number left out, since a clip that goes back to its
    /// track takes the name it had there.
    ///
    /// A group that brings back the clips of the last delete or cut of the timeline, which is
    /// what undo does, selects them.
    fn reselect(&mut self, cx: &mut Context<Self>) {
        let lost = std::mem::take(&mut self.lost_selection);
        let created = std::mem::take(&mut self.created_in_group);
        let project = self.session.read(cx).project();
        let is_clip = |id: &InstanceId| is_clip_tool(project, id);
        let back: Vec<InstanceId> = self
            .deleted
            .iter()
            .filter(|id| created.contains(id) && is_clip(id))
            .cloned()
            .collect();
        if !back.is_empty() {
            let first = back.first().cloned();
            self.set_clips(back, first, cx);
            return;
        }
        if lost.is_empty() {
            return;
        }
        let (mut found, mut by_name) = (Vec::new(), Vec::new());
        for (id, first) in lost {
            match created.contains(&id) && is_clip(&id) {
                true => found.push((id, first)),
                false => by_name.push((id, first)),
            }
        }
        for (id, first) in by_name {
            let same = created.iter().find(|created| {
                unnumbered(created.name()) == unnumbered(id.name())
                    && is_clip(created)
                    && !found.iter().any(|(found, _)| found == *created)
            });
            if let Some(same) = same {
                found.push((same.clone(), first));
            }
        }
        if found.is_empty() {
            return;
        }
        let first = found
            .iter()
            .find(|(_, first)| *first)
            .map(|(id, _)| id.clone());
        let mut selected: Vec<_> = self.clips.iter().cloned().collect();
        selected.extend(found.into_iter().map(|(id, _)| id));
        let primary = first.or_else(|| self.clips.primary().cloned());
        self.set_clips(selected, primary, cx);
    }

    /// Runs [`Self::reselect`] once per group, when the group is over. Deferred work runs after
    /// the events that are waiting, which are the rest of the group.
    fn reselect_later(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.reselects_later, true) {
            return;
        }
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |timeline, cx| {
                    timeline.reselects_later = false;
                    timeline.reselect(cx);
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
                self.selected_point = None;
            }
            self.publish_selection(cx);
            cx.notify();
        }
    }

    /// The selected clips go to the session too, the first one first, as the selected track
    /// does: the window offers to fit the project tempo to the take of the selected clip and to
    /// export the time the selected clips cover, and the arrangement knows nothing of takes,
    /// fitting or exports.
    fn publish_selection(&self, cx: &mut Context<Self>) {
        let primary = self.clips.primary();
        let others = self.clips.iter().filter(|id| Some(*id) != primary);
        let clips: Vec<_> = primary.into_iter().chain(others).cloned().collect();
        if self.session.read(cx).selected_clips() != clips {
            self.session
                .update(cx, |session, cx| session.select_clips(clips, cx));
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
    fn select_tempo(&mut self, tick: Option<Ticks>, cx: &mut Context<Self>) {
        if tick.is_some() {
            self.select_clip(None, cx);
            self.selected_point = None;
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

    /// The track whose name is being edited and the field that edits it, while it is.
    pub fn name_field(&self) -> Option<(&Instance<TrackState>, &Entity<TextInput>)> {
        let rename = self.rename.as_ref()?;
        Some((&rename.track, &rename.input))
    }

    fn time_signatures<'a>(&self, cx: &'a App) -> &'a TimeSignatures {
        let project = self.session.read(cx).project();
        project.project_file().tempo_map.time_signatures()
    }

    /// The grid of the snap setting over the time signatures of the project.
    fn grid(&self, cx: &App) -> Grid {
        self.snap.get().grid(self.time_signatures(cx))
    }

    /// Where each audio track is in the header column, from the top of the first row, and
    /// whether it is armed: where the overlay puts its arm toggle and its input meter.
    pub(super) fn audio_rows(&self, cx: &App) -> Vec<(f32, Instance<TrackState>)> {
        let project = self.session.read(cx).project();
        let rows = self.order.iter().enumerate().filter(|(_, track)| {
            project
                .state(*track)
                .is_some_and(|state| state.kind == TrackKind::Audio)
        });
        let layout = self.rows(cx);
        rows.map(|(index, track)| (self.viewport.y_of(&layout, index), track.clone()))
            .collect()
    }

    /// The top of the row under the last track, from the top of the first row: where the add
    /// track button is. Clamped as the paint clamps, so the button stays with the headers when
    /// tracks go or the window grows.
    pub(super) fn add_row_top(&self, cx: &App) -> f32 {
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(self.viewport, width, height, cx);
        viewport.y_at(self.rows(cx).height())
    }

    /// Scrolls just enough to show the whole row of the add track button, for a focus that
    /// the keys moved onto it.
    pub(super) fn reveal_add_row(&mut self, cx: &mut Context<Self>) {
        let (_, height) = self.painted_size.get();
        let top = f64::from(self.add_row_top(cx));
        let bottom = top + f64::from(ADD_ROW_HEIGHT) - f64::from(height);
        let scroll_by = top.min(0.) + bottom.max(0.);
        if scroll_by != 0. {
            let viewport = Viewport {
                scroll_y: self.viewport.scroll_y + scroll_by,
                ..self.viewport
            };
            self.set_viewport(viewport, cx);
        }
    }

    /// The first selected clip, while the project has it and it is a clip of notes: the one
    /// the note editor shows.
    pub(super) fn selected_instance(&self, cx: &App) -> Option<Instance<Clip>> {
        let project = self.session.read(cx).project();
        project.resolve(self.clips.primary()?)
    }

    /// The first selected clip, while the project has it and it is an audio clip: the one the
    /// Clip card of its track shows.
    pub fn selected_audio_clip(&self, cx: &App) -> Option<Instance<AudioClip>> {
        let project = self.session.read(cx).project();
        project.resolve(self.clips.primary()?)
    }

    /// The selected clips that the project still has, with their state.
    fn selected_states(&self, cx: &App) -> Vec<(InstanceId, AnyClip)> {
        let project = self.session.read(cx).project();
        let clips = self.clips.iter().filter_map(|id| {
            let clip = AnyClip::read(project, id)?;
            Some((id.clone(), clip))
        });
        clips.collect()
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        self.set_viewport(scrolled_or_zoomed(self.viewport, event, x), cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
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
            .pl(px(NAME_LEFT))
            .pr(px(HEADER_INSET))
            .occlude()
            .child(div().text_size(px(12.)).text_color(muted).child("Snap"))
            .child(self.snap_menu.clone())
    }
}
/// Scroll pans. With cmd it zooms in time about the pointer.
pub(super) fn scrolled_or_zoomed(viewport: Viewport, event: &ScrollWheelEvent, x: f32) -> Viewport {
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
                timeline.read(cx).painted_bounds.set(bounds);
                paint_scene(&mut scene, bounds, window, cx);
                let keyboard_focus = &timeline.read(cx).keyboard_focus;
                if keyboard_focus.shows_ring(&focus_handle, window) {
                    paint_focus_ring(bounds, window, cx);
                }
                if let Some(cursor) = timeline.read(cx).cursor() {
                    window.set_cursor_style(cursor, &hitbox);
                }
                listen(timeline, Rc::new(scene), bounds, hitbox, window);
            },
        );
        let dragged_paths = self.dragged_paths.clone();
        div()
            .id("timeline")
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            // GPUI gives what a drag of files carries only here, while it draws the timeline
            // under one. The mouse moves of the drag read it from there.
            .drag_over::<ExternalPaths>(move |style, paths, _, _| {
                let mut dragged = dragged_paths.borrow_mut();
                if dragged.as_slice() != paths.paths() {
                    *dragged = paths.paths().to_vec();
                }
                style
            })
            .on_drop(cx.listener(|timeline, paths: &ExternalPaths, window, cx| {
                // Where the files are let go of, not where the last move of the drag was.
                let bounds = timeline.painted_bounds.get();
                let (x, y) = Timeline::timeline_position(bounds, window.mouse_position());
                let target = timeline.drop_target_at(x, y, cx);
                timeline.forget_files(cx);
                if let Some(target) = target {
                    timeline.drop_files(paths.paths().to_vec(), target, cx);
                }
            }))
            .on_key_down(cx.listener(|timeline, event, window, cx| {
                if timeline.on_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_modifiers_changed(cx.listener(
                |timeline, event: &ModifiersChangedEvent, window, cx| {
                    timeline.modifiers_changed(event.modifiers, window, cx);
                },
            ))
            .child(surface.size_full())
            .child(self.snap_corner(cx))
            .child(self.lane_menu_column(cx))
            .children(self.rename_field(cx))
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
                // The timeline gave the focus itself, maybe on to the name field of a track.
                // Its root must not take it back.
                window.prevent_default();
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
            // Files from the Finder: the moves of their drag show where they would go.
            let paths = timeline.read(cx).dragged_paths.borrow().clone();
            if cx.has_active_drag() && !paths.is_empty() {
                let over = hitbox.is_hovered(window);
                let target = timeline.update(cx, |timeline, cx| {
                    match over {
                        true => timeline.drag_files_over(paths, x, y, cx),
                        false => timeline.forget_files(cx),
                    }
                    timeline.incoming_target().is_some()
                });
                // Over an instrument track the cursor says no.
                let cursor = match target {
                    true => CursorStyle::DragCopy,
                    false => CursorStyle::OperationNotAllowed,
                };
                cx.set_active_drag_cursor_style(cursor, window);
                return;
            }
            timeline.update(cx, |timeline, cx| {
                if !paths.is_empty() {
                    timeline.forget_files(cx);
                }
                if !timeline.dragging() {
                    // Something from elsewhere is dragged over, such as files from the
                    // Finder before the timeline knows them: no clip is under the pointer.
                    match hitbox.is_hovered(window) && !cx.has_active_drag() {
                        true => timeline.hover(x, y, &scene, cx),
                        false => timeline.unhover(cx),
                    }
                } else if !event.dragging() {
                    // The button came up somewhere that did not tell this window.
                    timeline.end_drag(cx);
                } else {
                    timeline.drag_held(x, y, event.modifiers, cx);
                }
            });
        }
    });
    // A drag of files that leaves the window, or ends somewhere else, takes its ghosts along.
    window.on_mouse_event({
        let timeline = timeline.clone();
        move |event: &FileDropEvent, _, _, cx| {
            if matches!(event, FileDropEvent::Exited | FileDropEvent::Ended) {
                timeline.update(cx, |timeline, cx| timeline.forget_files(cx));
            }
        }
    });
    window.on_mouse_event({
        let timeline = timeline.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                timeline.update(cx, |timeline, cx| {
                    if timeline.dragging() {
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
