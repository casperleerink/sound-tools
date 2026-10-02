//! The timeline of the arrangement: track headers, the bar ruler with its tempo changes, and
//! the clips, painted on one canvas. Clips of notes and of audio are added, selected, moved,
//! resized, copied, pasted and deleted here with the mouse and the keys, an audio clip is
//! trimmed, faded and turned up or down from its handles, audio files are dropped in from the
//! Finder, a track is renamed in its header, tempo changes are added and removed in the ruler,
//! and the snap setting sits in the corner. Under a track its automation lanes show at the
//! toggle in its header, where lanes are added and their points added, moved, deleted and
//! erased ([`super::track_lanes`]).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DispatchPhase, Entity,
    EventEmitter, ExternalPaths, FileDropEvent, FocusHandle, Focusable, FontWeight, Hitbox,
    HitboxBehavior, Hsla, KeyDownEvent, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PinchEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Subscription, TextAlign, TextRun, Window, canvas, div, fill,
    point, prelude::*, px, quad, size,
};
use sound_core::{
    Assets, Changes, Instance, InstanceId, Project, ProjectError, ProjectEvent, State, Ticks,
    TimeSignatures, ValueRange,
};
use sound_media::{AudioAsset, Cached, Info, TakeOverview};
use sound_notes::Clip;
use sound_ui::components::audio_clip::{
    AudioClipLook, ClipHandle, ClipHandles, Columns, paint_audio_clip,
};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::text_input::{InputSize, TextInput};
use sound_ui::{
    ActiveTheme, Devices, KeyboardFocus, LiveTake, Playhead, Recording, Session, Waveforms,
    typography,
};

use super::clipboard::{Copied, CopiedClips, SharedClipboard};
use super::clips::{
    AnyClip, GAIN_DB, GAIN_KEY_STEP_DB, GAIN_TRAVEL, fade_in, fade_out, fitted, gain_label,
    gain_moved, shown_end, time_label, trimmed_left, trimmed_right,
};
use super::gesture::{Zone, new_clip, nudged_track, resized_left, resized_right, zone_at};
use super::lanes::{DRAG_THRESHOLD, erase_range};
use super::layout::{
    ADD_LANE_HEIGHT, ADD_ROW_HEIGHT, DOT_LEFT, Extent, HEADER_INSET, HEADER_WIDTH, LANE_HEIGHT,
    LANES_MIDDLE, NAME_LEFT, NAME_MIDDLE, Part, RULER_HEIGHT, Rect, Rows, RulerBar, TRACK_HEIGHT,
    Viewport, ordered, shifted,
};
use super::paint::{
    Fit, accent, paint_focus_ring, paint_ruler, paint_text, paint_track_label, placed,
};
use super::plural;
use super::selection::Selection;
use super::snap::{Grid, SharedSnap, Snap};
use super::track_lanes::{self, LANE_BOX};
use crate::{
    ArrangementState, AudioClip, AutomationLane, AutomationValue, Carried, Colour, FreeIds,
    LaneMove, Moved, TrackKind, TrackState, Travel, add_audio_clips, add_audio_track, add_clip,
    add_clips, automatable, automation, move_track, moved, top_layer, track_orders, tracks,
    travel_in, unnumbered,
};

struct TrackRow {
    y: f32,
    name: SharedString,
    accent: Hsla,
    kind: TrackKind,
    selected: bool,
    /// A muted track has its name, dot and clips at 40 %.
    muted: bool,
    /// The name is being edited: the field of the timeline shows it, not the paint.
    renaming: bool,
    /// An armed audio track shows the level of its input in its header, and its name is
    /// shorter.
    armed: bool,
    /// Its header is being dragged: the ring of a drag shows where it lands.
    lifted: bool,
    /// Its lanes show under it, and how many it has: the toggle in its header says both.
    expanded: bool,
    automated: bool,
    /// What the toggle says: `Automation`, and how many lanes when there are any.
    lanes_label: SharedString,
}

/// An automation lane under a track, as one paint shows it.
struct LaneShape {
    /// The top of the lane in the timeline area.
    y: f32,
    name: SharedString,
    accent: Hsla,
    muted: bool,
    /// The line in the lane, from the left edge to the right one. Empty for a number the
    /// project does not know.
    line: Vec<(f32, f32)>,
    /// While clips are dragged: where across each lands with the line it takes along, and the
    /// line that was there before, faded.
    ghosts: Vec<(Range<f32>, Vec<(f32, f32)>)>,
    /// The dots of the points that show.
    points: Vec<LanePoint>,
}

/// The dot of a point of an automation lane, in the lane.
struct LanePoint {
    x: f32,
    y: f32,
    /// Under the pointer or selected: the dot is bigger, and a selected one has a ring.
    hovered: bool,
    selected: bool,
}

/// A point of an automation lane: the track, the number of the lane and the tick of the point.
/// What is selected and what is under the pointer. Interface state: not saved, no undo step.
#[derive(Clone, Debug, PartialEq)]
struct PointKey {
    track: InstanceId,
    device: Option<String>,
    parameter: String,
    tick: Ticks,
}

impl PointKey {
    fn of(track: &InstanceId, lane: &AutomationLane, tick: Ticks) -> Self {
        Self {
            track: track.clone(),
            device: lane.device.clone(),
            parameter: lane.parameter.clone(),
            tick,
        }
    }

    /// Whether it is a point of `lane` of `track`.
    fn is_in(&self, track: &InstanceId, lane: &AutomationLane) -> bool {
        self.track == *track && self.device == lane.device && self.parameter == lane.parameter
    }
}

/// What a clip shows: the notes of a note clip, or the waveform of an audio clip.
enum Body {
    Notes(Vec<Rect>),
    Audio(Box<AudioShape>),
}

/// Where the waveform of an audio shape comes from.
enum Sound {
    /// The file of a clip, whose overview is asked for while painting.
    File(AudioAsset),
    /// A take while it records: what its file holds so far, once it is lined up.
    Take(Option<TakeOverview>),
}

/// An audio clip as one paint shows it. The waveform needs the overview of its file, which is
/// asked for while painting, so here are the times of the file each column of the clip covers.
struct AudioShape {
    sound: Sound,
    /// The first column on screen, and the time in the file at each column edge from there.
    first: f32,
    edges: Vec<f64>,
    gain: f32,
    fade_in: f32,
    fade_out: f32,
    /// The part of the file past the edge that is dragged: its first column and column edges.
    hidden: Option<(f32, Vec<f64>)>,
    /// The value of a fade or of the gain while it is dragged.
    label: Option<(SharedString, ClipHandle)>,
    missing: Option<SharedString>,
    /// The pointer is on it or it is selected: its handles show and can be pressed.
    handles: bool,
}

/// A clip as it is on screen, in the coordinates of [`layout`].
pub struct ClipShape {
    pub id: InstanceId,
    pub rect: Rect,
    body: Body,
    accent: Hsla,
    selected: bool,
    muted: bool,
    /// It is dragged with automation that goes along, on a track whose lanes are folded away.
    carries: bool,
}

impl ClipShape {
    /// The handle of an audio clip at a place, when its handles show.
    fn handle_at(&self, x: f32, y: f32) -> Option<ClipHandle> {
        let Body::Audio(audio) = &self.body else {
            return None;
        };
        let Rect {
            x: left,
            y: top,
            width,
            ..
        } = self.rect;
        let handles = ClipHandles::of(left, top, width, audio.fade_in, audio.fade_out)?;
        handles.at(x, y)
    }
}

/// A tempo change after tick 0, in the ruler. The one at tick 0 shows in the transport.
struct TempoMark {
    tick: Ticks,
    x: f32,
    text: SharedString,
    selected: bool,
}

/// Where a drop of files would go: the ghost of each clip it makes, and whether it makes a
/// track under the last one.
struct Ghosts {
    clips: Vec<(Rect, SharedString)>,
    new_track: Option<f32>,
}

/// What one paint shows: only the visible rows, clips and bars. Later clips are on top.
pub struct Scene {
    pub viewport: Viewport,
    pub clips: Vec<ClipShape>,
    rows: Vec<TrackRow>,
    bars: Vec<RulerBar>,
    tempo: Vec<TempoMark>,
    /// Where each tempo change is in the ruler, across: filled by the paint, which measures
    /// the labels, and hit by a press.
    tempo_zones: Vec<(Ticks, Range<f32>)>,
    /// The rectangle of a drag on empty space.
    marquee: Option<Rect>,
    /// Where dropped files would go.
    ghosts: Option<Ghosts>,
    /// The folder of the files the audio clips name, for their waveforms.
    assets: Assets,
    /// How tall each track row is, with the lanes it shows.
    pub layout: Rows,
    /// The automation lanes that show.
    lanes: Vec<LaneShape>,
    /// Where the hint of a drag of clips that takes automation along goes: under the clip under
    /// the pointer.
    hint: Option<(f32, f32)>,
}

/// What a press on a clip took: its body, an edge, or one of the handles of an audio clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Grip {
    Zone(Zone),
    Handle(ClipHandle),
}

impl Scene {
    /// The clip on top at a position in the timeline area.
    pub fn clip_at(&self, x: f32, y: f32) -> Option<&ClipShape> {
        self.clips
            .iter()
            .rev()
            .find(|shape| shape.rect.contains(x, y))
    }

    /// The clip on top at a position, with the part of it that is there: a handle of an audio
    /// clip, its body or an edge.
    pub fn zone_at(&self, x: f32, y: f32) -> Option<(&ClipShape, Grip)> {
        let shape = self.clip_at(x, y)?;
        let grip = match shape.handle_at(x, y) {
            Some(handle) => Grip::Handle(handle),
            None => Grip::Zone(zone_at(shape.rect, x)),
        };
        Some((shape, grip))
    }

    /// The tempo change whose mark is at `x` in the ruler.
    fn tempo_at(&self, x: f32) -> Option<Ticks> {
        let mut zones = self.tempo_zones.iter().rev();
        zones
            .find(|(_, across)| across.contains(&x))
            .map(|(tick, _)| *tick)
    }
}

#[derive(Clone)]
/// One selected clip during a move: where it is now, the id it had at mouse down, the row of
/// its track then, and where it was. When the live clip is not what the drag wrote, something else
/// changed it: an undo between mouse down and the first move, or an agent.
struct MovedClip {
    /// The clip now. Its id changes when the drag takes it to another track.
    clip: InstanceId,
    /// A drag that comes back to the first track takes this id again, so a drag there and back
    /// leaves the file where it was.
    home: InstanceId,
    /// The kind of track it goes on, which is the kind it is.
    kind: TrackKind,
    row: usize,
    /// From its start to its end before the move. The length of an audio clip in ticks depends
    /// on the tempo where it is, so it is measured there and not where the drag has it now.
    range: Range<Ticks>,
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
        /// The last distance in rows at which every clip was on a track of its kind. The move
        /// keeps it while the pointer is over a track another clip cannot go on.
        rows: i64,
        /// The tracks as they were when the gesture opened. The automation under the clips
        /// moves from here at every mouse move, as the starts do, so the line a clip passes
        /// over comes back when it moves on.
        tracks: BTreeMap<InstanceId, TrackState>,
        /// The tracks whose lanes a mouse move of this drag wrote, which go back to how they
        /// were once no clip takes lanes from them or lands on them.
        lanes_written: BTreeSet<InstanceId>,
        /// Where the clips that take automation along put it, as the last mouse move wrote it.
        ghosts: Vec<LaneGhost>,
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
    /// An edge of an audio clip: the part of its file that plays. Every move starts from
    /// `origin`, and writes only what the edge moves onto the live clip.
    Trim {
        clip: Instance<AudioClip>,
        edge: Edge,
        origin: AudioClip,
        file: Info,
    },
    /// A fade handle of an audio clip, sideways from where it was.
    Fade {
        clip: Instance<AudioClip>,
        edge: Edge,
        origin: AudioClip,
        file: Info,
    },
    /// The gain handle of an audio clip, up and down from where it was. Shift pressed or let
    /// go goes on from where the gain is then, at the other speed.
    Gain {
        clip: Instance<AudioClip>,
        from_db: f32,
        from_y: f32,
        fine: bool,
    },
}

/// Where a dragged clip puts the automation it takes along: its track, its place now, and the
/// lanes it carries there.
#[derive(Clone, Debug, PartialEq)]
struct LaneGhost {
    clip: InstanceId,
    track: InstanceId,
    range: Range<Ticks>,
    lanes: Vec<AutomationLane>,
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
    /// What a press that comes up without a move does to the selection, as in the Finder.
    on_release: Option<OnRelease>,
}

/// What a click on a clip does when the button comes up without a move.
enum OnRelease {
    /// A plain click on one of several selected clips selects it alone.
    SelectAlone(InstanceId),
    /// A cmd-click adds the clip to the selection or takes it out. A cmd press that moves is
    /// a drag without the snap instead, and leaves the selection as it is.
    Toggle(InstanceId),
}

impl ClipDrag {
    fn label(&self) -> &'static str {
        match &self.kind {
            ClipDragKind::Move { clips, .. } => plural(clips.len(), "Move clip", "Move clips"),
            ClipDragKind::Resize { .. } => "Resize clip",
            ClipDragKind::Trim { .. } => "Trim clip",
            ClipDragKind::Fade {
                edge: Edge::Left, ..
            } => FADE_IN_LABEL,
            ClipDragKind::Fade {
                edge: Edge::Right, ..
            } => FADE_OUT_LABEL,
            ClipDragKind::Gain { .. } => GAIN_LABEL,
        }
    }

    /// The clip under the pointer.
    fn grabbed(&self) -> Option<&InstanceId> {
        match &self.kind {
            ClipDragKind::Move { clips, grabbed, .. } => {
                clips.get(*grabbed).map(|moved| &moved.clip)
            }
            ClipDragKind::Resize { clip, .. } => Some(clip.id()),
            ClipDragKind::Trim { clip, .. }
            | ClipDragKind::Fade { clip, .. }
            | ClipDragKind::Gain { clip, .. } => Some(clip.id()),
        }
    }

    /// Whether the drag ends when `id` is deleted: it is the clip under the pointer. The other
    /// clips of a move are left out of the next mouse move when they are gone.
    fn ends_without(&self, id: &InstanceId) -> bool {
        self.grabbed() == Some(id)
    }

    /// The cursor while it goes on.
    fn cursor(&self) -> Option<CursorStyle> {
        match &self.kind {
            ClipDragKind::Move { .. } => None,
            ClipDragKind::Resize { .. } | ClipDragKind::Trim { .. } | ClipDragKind::Fade { .. } => {
                Some(CursorStyle::ResizeLeftRight)
            }
            ClipDragKind::Gain { .. } => Some(CursorStyle::ResizeUpDown),
        }
    }
}

/// The undo steps of the handles of an audio clip, and of their knobs in the Clip card.
pub const FADE_IN_LABEL: &str = "Change fade in";
pub const FADE_OUT_LABEL: &str = "Change fade out";
pub const GAIN_LABEL: &str = "Change gain";

/// A drag on empty space: the clips it touches are selected. Its corners are a tick and a
/// height from the top of the first track, so a scroll during it keeps its start in place.
struct Marquee {
    from: (Ticks, f64),
    to: (Ticks, f64),
    /// What was selected before, which a drag with shift or cmd adds to.
    before: Vec<InstanceId>,
    /// What was selected at the press, with what came first, for escape.
    at_press: (Vec<InstanceId>, Option<InstanceId>),
}

/// A drag in an automation lane, from mouse down to mouse up. Every move starts from the lane
/// as it was at mouse down, so a drag there and back ends where it began. One gesture of the
/// session, which opens with the first change.
struct LaneDrag {
    track: Instance<TrackState>,
    /// The lane at mouse down, with the point a press added, and its place among the lanes of
    /// the track.
    origin: AutomationLane,
    index: usize,
    /// The top of the lane from the top of the first track, at mouse down.
    top: f64,
    kind: LaneDragKind,
    /// The undo step it makes.
    label: &'static str,
    begun: bool,
}

enum LaneDragKind {
    /// An alt-drag erases the points between the tick of the press and the pointer, once it has
    /// gone a few pixels. In project ticks: the lane counts from tick 0.
    Erase { from: Ticks, moving: bool },
    /// A point moves, by its place in `origin`, on the travel of `range`. `press` is where the
    /// press was: the tick under it, so a scroll during the drag keeps the point under the
    /// pointer, and its height in the lane. It waits until the pointer has gone a few pixels,
    /// so a click only selects it.
    Point {
        point: usize,
        range: ValueRange,
        press: (Ticks, f32),
        moving: bool,
    },
}

/// The undo steps of the points of an automation lane.
const ADD_POINT_LABEL: &str = "Add automation point";
const MOVE_POINT_LABEL: &str = "Move automation point";
const DELETE_POINT_LABEL: &str = "Delete automation point";
const ERASE_LABEL: &str = "Erase automation";

/// The select under the lanes of a track that adds one: a number of the track or of one of its
/// devices that has none yet.
struct LaneMenu {
    menu: Entity<DropdownMenu>,
    /// What it offered the last time it was filled, so it is filled again only when that
    /// changed.
    offered: Vec<SharedString>,
    _subscriptions: [Subscription; 2],
}

/// What the select that adds a lane says.
const ADD_LANE: &str = "Add lane";
/// The select that adds a lane is as wide as the shape of a selected header, and this tall.
const ADD_LANE_BUTTON_HEIGHT: f32 = 24.;
/// The hint near a drag of clips that takes automation along.
const AUTOMATION_HINT: &str = "Automation moves · alt to leave it";
/// What the toggle of the lanes of a track says.
const AUTOMATION: &str = "Automation";
/// The toggle of the lanes is the second line of a track header, from its edge to as far as
/// its words reach.
const LANES_TOGGLE_RIGHT: f32 = 128.;

/// What the toggle of the lanes says for a track with this many: `Automation`, and the count
/// when there are any.
fn lanes_label(lanes: usize) -> SharedString {
    match lanes {
        0 => SharedString::new_static(AUTOMATION),
        lanes => format!("{AUTOMATION} · {lanes}").into(),
    }
}

/// A drag of a track header. The track goes to the row under the pointer at once, so the rows
/// themselves show where it lands. The whole drag is one gesture of the session.
struct TrackDrag {
    track: InstanceId,
    /// The tracks as people saw them when the gesture opened, with their orders then. Every
    /// move starts from here, so a drag back to where it began writes nothing and is no undo
    /// step.
    origin: Vec<(Instance<TrackState>, u32)>,
    /// The rows at mouse down, which the pointer is hit against. The rows of now have the
    /// track where the last move put it, so with lanes of their own height a pointer that
    /// stands still would move it back.
    rows: Rows,
    from: usize,
    /// The place the last move gave the track, so a move inside one row publishes nothing.
    at: usize,
    /// Where the press was, from the top of the first track.
    press: f64,
    /// Past [`TRACK_DRAG_THRESHOLD`], so a click, or the first press of a double click, that
    /// shakes a little is still a click.
    moving: bool,
    /// Whether the gesture of the session is open. It opens with the first move to another
    /// row, so a click on a header is no undo step.
    begun: bool,
}

/// The undo step of a track drag and of alt-up and alt-down on a track.
const MOVE_TRACK_LABEL: &str = "Move track";
/// How far a press on a header moves before it drags the track, in points.
const TRACK_DRAG_THRESHOLD: f64 = 4.;

/// The name of a track while it is being edited in its header.
struct Rename {
    track: Instance<TrackState>,
    input: Entity<TextInput>,
    /// Kept apart from the field, because the field is being updated when it submits.
    focus: FocusHandle,
    /// A click anywhere else finishes the edit, as in the Finder.
    _blur: Subscription,
}

/// Where dropped audio files go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// An audio track, the first file at this tick and the others after it.
    Track(InstanceId, Ticks),
    /// A new audio track under the last one, named after the first file.
    NewTrack(Ticks),
}

/// Files from the Finder while they are dragged over the timeline.
#[derive(Default)]
struct Incoming {
    /// The files, as the drag says them. Filled in while the drag is drawn over the timeline.
    paths: Vec<PathBuf>,
    /// What each file is, once it is read on a background thread: its length for the ghost.
    /// `None` until then, and for a file that is no audio.
    files: Vec<Option<Info>>,
    /// Where they would go, from the last move of the pointer.
    target: Option<DropTarget>,
}

/// One clip of a move to another place: the clip now, the id it had when the move began and
/// where it was on the track of that id, the track it goes to and what it becomes there.
struct ClipMove {
    clip: InstanceId,
    home: InstanceId,
    was: Range<Ticks>,
    to: Instance<TrackState>,
    next: AnyClip,
}

/// What `moves` are for the automation they take along, see [`moved`].
fn lane_moves(moves: &[ClipMove]) -> Vec<LaneMove> {
    let moves = moves.iter().filter_map(|step| {
        Some(LaneMove {
            from: step.home.parent()?,
            range: step.was.clone(),
            to: step.to.id().clone(),
            start: step.next.start(),
        })
    });
    moves.collect()
}

/// Writes what `change` makes of the lanes of the tracks of `arrangement` to a group of
/// changes. It gets the records of the tracks by id, and the numbers of their devices.
fn change_lanes(
    project: &Project,
    changes: &mut Changes,
    arrangement: &InstanceId,
    change: impl FnOnce(&mut BTreeMap<InstanceId, TrackState>, &Travel<'_>),
) {
    let mut tracks = track_states(project, arrangement);
    change(&mut tracks, &travel_in(project));
    let lanes = tracks.into_iter().map(|(id, state)| (id, state.automation));
    automation::write(project, changes, lanes);
}

/// The automation that `moves` take along, to a group of changes, before the clips move.
fn move_lanes(
    project: &Project,
    changes: &mut Changes,
    arrangement: &InstanceId,
    moves: &[ClipMove],
) {
    let moves = lane_moves(moves);
    change_lanes(project, changes, arrangement, |tracks, travel| {
        for (track, lanes) in moved(tracks, &moves, travel).lanes {
            if let Some(state) = tracks.get_mut(&track) {
                state.automation = lanes;
            }
        }
    });
}

/// The numbers of `track`, whose record is `state`, that a lane can be added for, each as a
/// lane with no points: those with no lane yet and a value in their record to start from.
fn free_lanes(project: &Project, track: &InstanceId, state: &TrackState) -> Vec<AutomationLane> {
    let travel = travel_in(project);
    let lanes = automatable(project, track, state).into_iter();
    let lanes = lanes.filter(|lane| {
        let taken = state.automation.iter().any(|had| had.same_number(lane));
        let number = lane.number(track, state, &travel);
        !taken && number.is_some_and(|number| number.record.is_some())
    });
    lanes.collect()
}

/// The records of the tracks of an arrangement, by id.
fn track_states(project: &Project, arrangement: &InstanceId) -> BTreeMap<InstanceId, TrackState> {
    let tracks = project.children::<TrackState>(arrangement);
    let tracks = tracks.map(|(track, state)| (track.id().clone(), state.clone()));
    tracks.collect()
}

/// Where a clip is on the timeline now.
fn range_of(project: &Project, clip: &AnyClip) -> Range<Ticks> {
    clip.start()..clip.end(project)
}

/// Moves clips in one group of changes. A clip that stays on its track gets its new record. One
/// that goes to another track is a delete and a create, like moving a file: back on the track of
/// its `home` it takes that id again, elsewhere its name without a number at its end, or the
/// next free one. So `clip` moved down onto a track that has a `clip` is `clip-2` there, and
/// `clip` again when it comes back up. Gives the ids of the clips after the move, in the order
/// of `moves`.
///
/// A moved audio clip goes on top of the clips of its track, as a new one does, so where it
/// overlaps them it is heard. Moved together, they keep their order among themselves.
fn move_clips(
    project: &Project,
    changes: &mut Changes,
    mut moves: Vec<ClipMove>,
) -> Result<Vec<InstanceId>, ProjectError> {
    put_on_top(project, &mut moves);
    let mut free = FreeIds::default();
    let mut moved = Vec::new();
    for ClipMove {
        clip,
        home,
        to,
        next,
        ..
    } in moves
    {
        if clip.parent().as_ref() == Some(to.id()) {
            next.write(changes, clip.clone());
            moved.push(clip);
            continue;
        }
        changes.delete(&clip);
        // Back on the track of its home it takes its home again, else its name there without
        // a number, so down and up again gives the first id back.
        let id = match home.parent().as_ref() == Some(to.id()) {
            true => home,
            false => free.take(project, &to.id().child(unnumbered(home.name()))?)?,
        };
        next.write(changes, id.clone());
        moved.push(id);
    }
    Ok(moved)
}

/// The layers of moved audio clips: one above every clip of the track they go to that does not
/// move, in the order they had.
fn put_on_top(project: &Project, moves: &mut [ClipMove]) {
    let moving: Vec<InstanceId> = moves.iter().map(|step| step.clip.clone()).collect();
    let mut order: Vec<usize> = (0..moves.len()).collect();
    let layer = |step: &ClipMove| match &step.next {
        AnyClip::Audio(clip) => clip.layer,
        AnyClip::Notes(_) => 0,
    };
    order.sort_by_key(|index| moves.get(*index).map(layer));
    let mut next: BTreeMap<InstanceId, u32> = BTreeMap::new();
    for index in order {
        let Some(step) = moves.get_mut(index) else {
            continue;
        };
        let AnyClip::Audio(clip) = &mut step.next else {
            continue;
        };
        let track = step.to.id().clone();
        let layer = next.entry(track.clone()).or_insert_with(|| {
            top_layer(project, &track, &moving).map_or(0, |top| top.saturating_add(1))
        });
        clip.layer = *layer;
        *layer = layer.saturating_add(1);
    }
}

/// What the file of an audio clip is, from memory only: a press on the thread that draws does
/// not read the disk. `None` for a file that is missing, does not play, or is not known yet.
fn known_file(project: &Project, asset: &AudioAsset) -> Option<Info> {
    match sound_media::cached(project.assets(), asset) {
        Cached::Plays(file) => Some(file),
        Cached::DoesNotPlay(_) | Cached::Missing | Cached::Unknown => None,
    }
}

/// Whether an id is a clip of either kind.
fn is_clip_tool(project: &Project, id: &InstanceId) -> bool {
    project
        .tool_of(id)
        .is_some_and(|tool| tool == Clip::TOOL || tool == AudioClip::TOOL)
}

/// Why a clip cannot go where a paste would put it.
fn wrong_track(track: &InstanceId, name: &str, kind: TrackKind) -> ProjectError {
    let message = match kind {
        TrackKind::Instrument => {
            "a note clip goes on an instrument track, and this is an audio track"
        }
        TrackKind::Audio => "an audio clip goes on an audio track, and this is an instrument track",
    };
    let id = track.child(name).unwrap_or_else(|_| track.clone());
    ProjectError::WrongPlace {
        id,
        message: message.to_string(),
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
    drag: Option<ClipDrag>,
    marquee: Option<Marquee>,
    track_drag: Option<TrackDrag>,
    lane_drag: Option<LaneDrag>,
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
                    // Deleted under the drag, from outside. A drag to another track is not
                    // this: it names its new clips before this event arrives.
                    if timeline
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.ends_without(id))
                    {
                        timeline.end_drag(cx);
                    }
                    // The dragged track deleted from outside: that was the last write. So
                    // is the track of a lane being drawn.
                    if timeline
                        .track_drag
                        .as_ref()
                        .is_some_and(|drag| drag.track == *id)
                        || timeline
                            .lane_drag
                            .as_ref()
                            .is_some_and(|drag| drag.track.id() == id)
                    {
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
            let clip_drag = timeline.drag.take().is_some_and(|drag| drag.begun);
            let track_drag = timeline.track_drag.take().is_some_and(|drag| drag.begun);
            let lane_drag = timeline.lane_drag.take().is_some_and(|drag| drag.begun);
            if clip_drag || track_drag || lane_drag {
                let session = timeline.session.clone();
                session.update(cx, |session, cx| session.finish_gesture(cx));
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
            drag: None,
            marquee: None,
            track_drag: None,
            lane_drag: None,
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

    /// The select that adds a lane to a track, while the track shows its lanes.
    pub fn lane_menu(&self, track: &InstanceId) -> Option<&Entity<DropdownMenu>> {
        self.lane_menus.get(track).map(|lane_menu| &lane_menu.menu)
    }

    /// Whether a track shows its automation lanes.
    pub fn shows_lanes(&self, track: &InstanceId) -> bool {
        self.expanded.contains(track)
    }

    /// Shows the automation lanes of a track under it, with the select that adds one, or
    /// folds them away. Interface state: not saved, no undo step.
    pub fn show_lanes(&mut self, track: &InstanceId, shown: bool, cx: &mut Context<Self>) {
        if !shown {
            self.expanded.remove(track);
            self.lane_menus.remove(track);
            // A point that does not show is not selected: delete would take it unseen.
            if self
                .selected_point
                .as_ref()
                .is_some_and(|key| key.track == *track)
            {
                self.selected_point = None;
            }
            cx.notify();
            return;
        }
        if !self.expanded.insert(track.clone()) {
            return;
        }
        let menu = cx.new(|cx| {
            DropdownMenu::new(ADD_LANE, Vec::new(), cx)
                .debug_name(format!("add-lane-{}", track.name()))
                .trigger(Trigger::Select)
                .trigger_width(HEADER_WIDTH - 2. * HEADER_INSET)
                .width(240.)
                .max_height(320.)
        });
        let picked = cx.subscribe(&menu, {
            let track = track.clone();
            move |timeline, _, picked: &MenuPicked, cx| timeline.add_lane(&track, &picked.0, cx)
        });
        // What it offers is read again when it opens: a device may have come since.
        let opened = cx.observe(&menu, {
            let track = track.clone();
            move |timeline, menu, cx| {
                if menu.read(cx).is_open() {
                    timeline.fill_lane_menu(&track, cx);
                }
            }
        });
        self.lane_menus.insert(
            track.clone(),
            LaneMenu {
                menu,
                offered: Vec::new(),
                _subscriptions: [picked, opened],
            },
        );
        self.fill_lane_menu(track, cx);
        cx.notify();
    }

    /// Fills the select that adds a lane to `track` with what it can add now, when that is not
    /// what it holds.
    fn fill_lane_menu(&mut self, track: &InstanceId, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some((instance, state)) = project
            .resolve::<TrackState>(track)
            .and_then(|instance| Some((instance.clone(), project.state(&instance)?)))
        else {
            return;
        };
        // The numbers of one device come one after another, in one group.
        let mut groups: Vec<(Option<String>, Vec<MenuItem>)> = Vec::new();
        for lane in free_lanes(project, instance.id(), state) {
            let label = match lane.device {
                None => track_lanes::lane_name(None, &lane.parameter),
                Some(_) => track_lanes::number_name(&lane.parameter),
            };
            let value = track_lanes::menu_value(lane.device.as_deref(), &lane.parameter);
            let item = MenuItem::new(value, label).selectable(false);
            match groups.last_mut() {
                Some((last, items)) if *last == lane.device => items.push(item),
                _ => groups.push((lane.device, vec![item])),
            }
        }
        let offered: Vec<SharedString> = groups
            .iter()
            .flat_map(|(_, items)| items.iter().map(|item| item.value.clone()))
            .collect();
        let entries = groups.into_iter().map(|(device, items)| {
            let label = match device {
                None => SharedString::from("Track"),
                Some(device) => self.device_name(instance.id(), &device, cx),
            };
            MenuEntry::Group(MenuGroup::new().label(label).items(items))
        });
        let entries: Vec<MenuEntry> = entries.collect();
        let Some(lane_menu) = self.lane_menus.get_mut(track) else {
            return;
        };
        if lane_menu.offered != offered {
            lane_menu.offered = offered;
            let menu = lane_menu.menu.clone();
            menu.update(cx, |menu, cx| menu.set_entries(entries, cx));
        }
    }

    /// What a device of a track is called, as its card says: `Filter` for `filter`.
    fn device_name(&self, track: &InstanceId, device: &str, cx: &App) -> SharedString {
        let Ok(id) = track.child(device) else {
            return device.to_string().into();
        };
        match Devices::label_of(&self.session, &id, cx) {
            Some(label) => label.name,
            None => {
                let project = self.session.read(cx).project();
                project.tool_of(&id).unwrap_or(device).to_string().into()
            }
        }
    }

    /// The select of the lanes of a track picked a number: a lane for it, which holds the
    /// value of its record, so nothing sounds different yet. One undo step.
    fn add_lane(&mut self, track: &InstanceId, value: &str, cx: &mut Context<Self>) {
        let Some((device, field)) = track_lanes::from_menu_value(value) else {
            return;
        };
        let mut lane = AutomationLane {
            device: device.map(str::to_string),
            parameter: field.to_string(),
            points: Vec::new(),
        };
        let track = track.clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let Some(instance) = project.resolve::<TrackState>(&track) else {
                    return Ok(());
                };
                let Some(mut state) = project.state(&instance).cloned() else {
                    return Ok(());
                };
                let number = lane.number(&track, &state, &travel_in(project));
                let Some(record) = number.and_then(|number| number.record) else {
                    return Ok(());
                };
                lane.points.push(sound_notes::Point {
                    tick: Ticks(0),
                    value: AutomationValue(record),
                });
                state.automation.push(lane);
                let mut changes = Changes::new();
                changes.set(&instance, state);
                project.commit("Add automation", changes)
            })
        });
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

    /// Everything to paint into a timeline area of this size, read from the project now.
    fn scene(&self, width: f32, height: f32, cx: &App) -> Scene {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let tempo_map = &project.project_file().tempo_map;
        let time_signatures = tempo_map.time_signatures();
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

        let layout = self.rows(cx);
        let lane_ghosts = self.lane_ghosts();
        let mut scene = Scene {
            viewport,
            clips: Vec::new(),
            rows: Vec::new(),
            bars: viewport.ruler_bars(time_signatures, width),
            tempo,
            tempo_zones: Vec::new(),
            marquee,
            ghosts: self.ghosts(&viewport, &layout, project),
            assets: project.assets().clone(),
            lanes: Vec::new(),
            hint: None,
            layout,
        };
        let layout = scene.layout.clone();
        let recording = self.recording.read(cx);
        let visible =
            |start: Ticks, end: Ticks| start < visible_ticks.end && end > visible_ticks.start;
        for index in viewport.visible_tracks(&layout, height) {
            let Some(track) = self.order.get(index) else {
                break;
            };
            // Gone since the order was read: the render after its event leaves it out.
            let Some(state) = project.state(track) else {
                continue;
            };
            let accent = accent(state.colour, theme);
            let expanded = layout.lanes(index).is_some();
            scene.rows.push(TrackRow {
                y: viewport.y_of(&layout, index),
                name: state.name.clone().into(),
                accent,
                kind: state.kind,
                selected: self.selected_track.as_ref() == Some(track.id()),
                muted: state.mute,
                renaming: renaming == Some(track.id()),
                armed: state.kind == TrackKind::Audio && recording.is_armed(track.id()),
                lifted: self
                    .track_drag
                    .as_ref()
                    .is_some_and(|drag| drag.moving && drag.track == *track.id()),
                expanded,
                automated: !state.automation.is_empty(),
                lanes_label: lanes_label(state.automation.len()),
            });
            if expanded {
                let count = state.automation.len();
                let tops = (0..count).map(|lane| viewport.y_at(layout.lane_top(index, lane)));
                let area = (width, height);
                let shapes = self.lane_shapes(track.id(), state, &viewport, area, tops, cx);
                scene.lanes.extend(shapes);
            }
            let shape = |id: &InstanceId, rect: Rect, body: Body| ClipShape {
                id: id.clone(),
                rect,
                body,
                accent,
                selected: self.clips.contains(id),
                muted: state.mute,
                carries: !expanded && lane_ghosts.iter().any(|ghost| ghost.clip == *id),
            };
            // The order of `clips()`, by start and then by id, for the few that are visible:
            // it decides which of two overlapping clips is on top.
            let mut notes: Vec<_> = project
                .children::<Clip>(track.id())
                .filter(|(_, clip)| visible(clip.start, clip.end()))
                .collect();
            notes.sort_by(|(a, a_clip), (b, b_clip)| {
                (a_clip.start, a.id()).cmp(&(b_clip.start, b.id()))
            });
            for (clip, state) in notes {
                let rect = viewport.clip_rect(&layout, index, state.start, state.end());
                let body = Body::Notes(viewport.miniature(state, rect).collect());
                scene.clips.push(shape(clip.id(), rect, body));
            }
            // Audio clips in the order they are heard: the one on top of an overlap is the one
            // that plays, and the one a press takes.
            let mut audio: Vec<_> = project
                .children::<AudioClip>(track.id())
                .map(|(clip, state)| (clip, state, shown_end(project, state)))
                .filter(|(_, clip, end)| visible(clip.start, *end))
                .collect();
            audio.sort_by(|(a, a_clip, _), (b, b_clip, _)| {
                (a_clip.layer, a_clip.start, a.id()).cmp(&(b_clip.layer, b_clip.start, b.id()))
            });
            for (clip, state, end) in audio {
                let rect = viewport.clip_rect(&layout, index, state.start, end);
                let body = self.audio_shape(clip.id(), state, rect, &viewport, width, project);
                scene
                    .clips
                    .push(shape(clip.id(), rect, Body::Audio(Box::new(body))));
            }
        }
        // The hint goes in the top left corner of the clip under the pointer, while the drag
        // takes automation, as the value of a fade shows in its clip: what it covers there is
        // what the pointer holds.
        let grabbed = self.drag.as_ref().and_then(ClipDrag::grabbed);
        let grabbed = grabbed.filter(|_| !lane_ghosts.is_empty());
        let under = grabbed.and_then(|id| scene.clips.iter().find(|shape| shape.id == *id));
        scene.hint = under.map(|shape| (shape.rect.x + 4., shape.rect.y + 4.));
        scene
    }

    /// Where the clips of a drag put the automation they take along, as its last mouse move
    /// wrote it. Empty while no drag of clips takes any.
    fn lane_ghosts(&self) -> &[LaneGhost] {
        match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(ClipDragKind::Move { ghosts, .. }) => ghosts,
            _ => &[],
        }
    }

    /// Whether the drag of clips going on takes automation along: when its hint shows, with
    /// the ghosts of the lines, or the mark on a clip whose lanes are folded away.
    pub fn automation_moves(&self) -> bool {
        !self.lane_ghosts().is_empty()
    }

    /// The tracks as they were when a drag of clips opened its gesture, while it goes on.
    fn tracks_before_drag(&self) -> Option<&BTreeMap<InstanceId, TrackState>> {
        match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(ClipDragKind::Move { tracks, .. }) if !tracks.is_empty() => Some(tracks),
            _ => None,
        }
    }

    /// The lanes of `track`, whose record is `state`, whose tops in the timeline area are
    /// `tops`, as they show in an area of `(width, height)`: each with its name and its line,
    /// and while clips are dragged, where each lands with the lane it carries, with the line
    /// that was there before. Only the lanes that show.
    fn lane_shapes(
        &self,
        track: &InstanceId,
        state: &TrackState,
        viewport: &Viewport,
        (width, height): (f32, f32),
        tops: impl Iterator<Item = f32>,
        cx: &App,
    ) -> Vec<LaneShape> {
        let project = self.session.read(cx).project();
        let travel = travel_in(project);
        let accent = accent(state.colour, cx.theme());
        let visible = viewport.visible_ticks(width);
        let before = self.tracks_before_drag();
        let before = before.and_then(|tracks| tracks.get(track));
        let ghosts = self.lane_ghosts().iter();
        let ghosts: Vec<&LaneGhost> = ghosts.filter(|ghost| ghost.track == *track).collect();
        let lanes = state.automation.iter().zip(tops);
        let lanes = lanes.filter(|(_, y)| *y < height && *y + LANE_HEIGHT > 0.);
        let lanes = lanes.map(|(lane, y)| {
            let range = lane.number(track, state, &travel);
            let range = range.map(|number| number.range);
            let line = |lane: &AutomationLane, ticks: Range<Ticks>| {
                let line = range.map(|range| track_lanes::line(viewport, lane, range, ticks));
                line.unwrap_or_default()
            };
            let was = before.and_then(|before| {
                let mut lanes = before.automation.iter();
                lanes.find(|was| was.same_number(lane))
            });
            let ghosts = ghosts.iter().filter(|ghost| {
                let mut carried = ghost.lanes.iter();
                carried.any(|carried| carried.same_number(lane))
            });
            let ghosts = ghosts.map(|ghost| {
                let across = viewport.x_of(ghost.range.start)..viewport.x_of(ghost.range.end);
                let replaced = was.map(|was| line(was, ghost.range.clone()));
                (across, replaced.unwrap_or_default())
            });
            let name = lane.device.as_ref();
            let name = name.map(|device| self.device_name(track, device, cx));
            let is = |key: &Option<PointKey>, tick| {
                key.as_ref()
                    .is_some_and(|key| key.is_in(track, lane) && key.tick == tick)
            };
            let shown = lane
                .points
                .iter()
                .filter(|point| visible.contains(&point.tick));
            let points = range.map(|range| {
                let points = shown.map(|point| {
                    let (x, y) = track_lanes::place(viewport, range, point);
                    let hovered = is(&self.hovered_point, point.tick);
                    let selected = is(&self.selected_point, point.tick);
                    LanePoint {
                        x,
                        y,
                        hovered,
                        selected,
                    }
                });
                points.collect()
            });
            LaneShape {
                y,
                name: track_lanes::lane_name(name.as_deref(), &lane.parameter).into(),
                accent,
                muted: state.mute,
                line: line(lane, visible.clone()),
                ghosts: ghosts.collect(),
                points: points.unwrap_or_default(),
            }
        });
        lanes.collect()
    }

    /// The takes while they record, from where the recording began to the playhead, in the
    /// viewport painted last. `RecordingOverlay` paints them over the timeline every frame while
    /// they grow, so the timeline itself is not painted again for them.
    pub(super) fn take_shapes(&self, width: f32, cx: &App) -> Vec<ClipShape> {
        let project = self.session.read(cx).project();
        let recording = self.recording.read(cx);
        let playhead = self.playhead.read(cx).tick;
        let viewport = self.painted.get();
        let theme = cx.theme();
        let rows = self.rows(cx);
        let mut shapes = Vec::new();
        for take in recording.takes() {
            let Some(index) = self.row_of(&take.track) else {
                continue;
            };
            let Some(state) = self.order.get(index).and_then(|track| project.state(track)) else {
                continue;
            };
            let end = playhead.max(take.start + Ticks(1));
            let rect = viewport.clip_rect(&rows, index, take.start, end);
            let body = live_shape(take, rect, &viewport, width, project);
            shapes.push(ClipShape {
                id: take.track.clone(),
                rect,
                body: Body::Audio(Box::new(body)),
                accent: accent(state.colour, theme),
                selected: false,
                muted: state.mute,
                carries: false,
            });
        }
        shapes
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

    /// What an audio clip shows: the times of its file under each column on screen, its fades
    /// in points, its handles while the pointer is on it or it is selected, and while a drag
    /// changes it, the value that drag shows or the part of the file past the edge it moves.
    fn audio_shape(
        &self,
        id: &InstanceId,
        clip: &AudioClip,
        rect: Rect,
        viewport: &Viewport,
        width: f32,
        project: &Project,
    ) -> AudioShape {
        let clock = project.clock();
        let file = sound_media::cached(project.assets(), &clip.asset);
        let start = clock.seconds_of(clip.start);
        // The time of the file at a place across: the clip plays at the speed of its file.
        let file_time =
            |x: f32| clock.seconds_of(viewport.tick_at(x)) - start + clip.file_start_seconds;
        let edges = |from: f32, to: f32| -> (f32, Vec<f64>) {
            let (from, to) = (from.max(0.).floor(), to.min(width).ceil());
            let columns = (to - from).max(0.) as usize;
            let edges = (0..=columns).map(|column| file_time(from + column as f32));
            (from, edges.collect())
        };
        let (first, edges_inside) = edges(rect.x, rect.x + rect.width);
        let x_at = |seconds: f64| viewport.x_of(clock.tick_at_seconds(seconds));
        let fade_in = x_at(start + f64::from(clip.fade_in_ms) / 1000.) - rect.x;
        let end = rect.x + rect.width;
        let end_seconds = clock.seconds_of(viewport.tick_at(end));
        let fade_out = end - x_at(end_seconds - f64::from(clip.fade_out_ms) / 1000.);

        let dragged = self.drag.as_ref().filter(|drag| drag.grabbed() == Some(id));
        let (mut hidden, mut label) = (None, None);
        match dragged.map(|drag| &drag.kind) {
            Some(ClipDragKind::Trim { edge, file, .. }) => {
                // The whole file from where it starts on the timeline to where it ends.
                let file_start = x_at(start - clip.file_start_seconds);
                let file_end = x_at(start - clip.file_start_seconds + file.seconds());
                hidden = Some(match edge {
                    Edge::Left => edges(file_start, rect.x),
                    Edge::Right => edges(end, file_end),
                });
            }
            Some(ClipDragKind::Fade {
                edge: Edge::Left, ..
            }) => {
                let text = format!("Fade in {}", time_label(clip.fade_in_ms));
                label = Some((text.into(), ClipHandle::FadeIn));
            }
            Some(ClipDragKind::Fade {
                edge: Edge::Right, ..
            }) => {
                let text = format!("Fade out {}", time_label(clip.fade_out_ms));
                label = Some((text.into(), ClipHandle::FadeOut));
            }
            Some(ClipDragKind::Gain { .. }) => {
                label = Some((gain_label(clip.gain_db).into(), ClipHandle::Gain));
            }
            Some(ClipDragKind::Move { .. } | ClipDragKind::Resize { .. }) | None => {}
        }
        let missing = match file {
            Cached::Missing => Some(format!("{} is missing", clip.asset)),
            Cached::DoesNotPlay(_) => Some(format!("{} does not play", clip.asset)),
            Cached::Plays(_) | Cached::Unknown => None,
        };
        AudioShape {
            sound: Sound::File(clip.asset.clone()),
            first,
            edges: edges_inside,
            gain: crate::decibels::amplitude(clip.gain_db),
            fade_in: fade_in.max(0.),
            fade_out: fade_out.max(0.),
            hidden,
            label,
            missing: missing.map(SharedString::from),
            handles: self.hovered.as_ref() == Some(id) || self.clips.contains(id),
        }
    }

    /// The ghosts of the clips a drop of files would make, while files are dragged over.
    fn ghosts(&self, viewport: &Viewport, rows: &Rows, project: &Project) -> Option<Ghosts> {
        let incoming = self.incoming.as_ref()?;
        let (row, start) = match incoming.target.as_ref()? {
            DropTarget::Track(track, start) => (self.row_of(track)?, *start),
            DropTarget::NewTrack(start) => (self.order.len(), *start),
        };
        let clock = project.clock();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        let mut at = start;
        let mut clips = Vec::new();
        for (index, path) in incoming.paths.iter().enumerate() {
            // The length of the file once it is known, else a bar.
            let end = match incoming.files.get(index).copied().flatten() {
                Some(file) => {
                    let frames = sound_media::engine_frames(
                        file.frames,
                        file.sample_rate,
                        clock.sample_rate(),
                    );
                    clock.tick_at(sound_core::Frames(clock.frame_of(at).0 + frames))
                }
                None => at + time_signatures.bar_at(at).length(),
            };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let rect = viewport.clip_rect(rows, row, at, end.max(at + Ticks(1)));
            clips.push((rect, SharedString::from(name.unwrap_or_default())));
            at = end;
        }
        let new_track = matches!(incoming.target, Some(DropTarget::NewTrack(_)))
            .then(|| viewport.y_of(rows, row));
        Some(Ghosts { clips, new_track })
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
        let (shift, cmd) = (event.modifiers.shift, event.modifiers.platform);
        let adds = shift || cmd;
        let part = scene.viewport.part_at(&scene.layout, y);
        if x < 0.0 || y < 0.0 || !matches!(part, Some((_, Part::Lane(_)))) {
            self.select_point(None, cx);
        }
        if x < 0.0 {
            // The corner above the headers holds the snap setting, which takes its own clicks.
            if y < 0.0 {
                return;
            }
            // The header of a track, not of its lanes. Its second line is the toggle of its
            // lanes; elsewhere it is selected, and a double click edits its name.
            let Some((row, Part::Track)) = part else {
                return;
            };
            let in_row = y - scene.viewport.y_of(&scene.layout, row);
            if in_row >= LANES_MIDDLE - 8. && x + HEADER_WIDTH < LANES_TOGGLE_RIGHT {
                if let Some(track) = self.order.get(row).map(|track| track.id().clone()) {
                    let shown = self.shows_lanes(&track);
                    self.show_lanes(&track, !shown, cx);
                }
                return;
            }
            if let Some(track) = self.order.get(row).cloned() {
                self.select_track(Some(track.id().clone()), cx);
                cx.emit(TimelineEvent::OpenTrack(track.clone()));
                if double {
                    self.start_rename(track, window, cx);
                } else {
                    self.start_track_drag(track.id(), y, scene.layout.clone());
                }
            }
            return;
        }
        if y < 0.0 {
            self.on_ruler(x, double, scene, cx);
            return;
        }
        match part {
            Some((row, Part::Lane(lane))) => return self.press_lane(row, lane, event, x, y, cx),
            Some((_, Part::AddLane)) => return,
            Some((_, Part::Track)) | None => {}
        }
        let Some((shape, grip)) = scene.zone_at(x, y) else {
            if double {
                self.add_clip_at(x, y, scene, cx);
            } else {
                self.start_marquee(x, y, adds, cx);
            }
            return;
        };
        let id = shape.id.clone();
        if shift {
            self.toggle_clip(id, cx);
            return;
        }
        if double {
            self.select_clip(Some(id.clone()), cx);
            self.open(&id, cx);
            return;
        }
        let grab = self.painted.get().tick_at(x);
        let kind = match grip {
            Grip::Zone(Zone::Body) => self.start_move(&id, cmd, cx),
            Grip::Zone(zone @ (Zone::LeftEdge | Zone::RightEdge)) => {
                let edge = match zone {
                    Zone::LeftEdge => Edge::Left,
                    _ => Edge::Right,
                };
                self.start_edge(&id, edge, cmd, cx)
            }
            Grip::Handle(handle) => self.start_handle(&id, handle, y, cx),
        };
        let Some(kind) = kind else {
            return;
        };
        let several = matches!(&kind, ClipDragKind::Move { clips, .. } if clips.len() > 1);
        let on_release = match (cmd, several) {
            (true, _) => Some(OnRelease::Toggle(id)),
            (false, true) => Some(OnRelease::SelectAlone(id)),
            (false, false) => None,
        };
        self.drag = Some(ClipDrag {
            kind,
            grab,
            begun: false,
            on_release,
        });
    }

    /// Opens what a clip is edited in: the note editor of a note clip, the track panel of the
    /// track of an audio clip, whose Clip card shows it.
    fn open(&mut self, clip: &InstanceId, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        if let Some(notes) = project.resolve::<Clip>(clip) {
            cx.emit(TimelineEvent::OpenEditor(notes));
            return;
        }
        let track = clip
            .parent()
            .and_then(|track| project.resolve::<TrackState>(&track));
        if let Some(track) = track {
            cx.emit(TimelineEvent::OpenTrack(track));
        }
    }

    /// A press on an edge: a resize of a note clip, a trim of an audio clip.
    fn start_edge(
        &mut self,
        id: &InstanceId,
        edge: Edge,
        cmd: bool,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        // A cmd press changes the selection when it comes up, or with the first move.
        if !cmd {
            self.select_clip(Some(id.clone()), cx);
        }
        let project = self.session.read(cx).project();
        if let Some(clip) = project.resolve::<Clip>(id) {
            let state = project.state(&clip)?.clone();
            return Some(ClipDragKind::Resize {
                clip,
                edge,
                origin: state.clone(),
                written: state,
                delta: 0,
            });
        }
        let clip = project.resolve::<AudioClip>(id)?;
        let origin = project.state(&clip)?.clone();
        // A clip whose file is missing has nothing to trim.
        let file = known_file(project, &origin.asset)?;
        Some(ClipDragKind::Trim {
            clip,
            edge,
            origin,
            file,
        })
    }

    /// A press on a handle of an audio clip: a fade or the gain, from where it is.
    fn start_handle(
        &mut self,
        id: &InstanceId,
        handle: ClipHandle,
        y: f32,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        if !self.clips.contains(id) {
            self.select_clip(Some(id.clone()), cx);
        }
        let project = self.session.read(cx).project();
        let clip = project.resolve::<AudioClip>(id)?;
        let origin = project.state(&clip)?.clone();
        let file = known_file(project, &origin.asset)?;
        Some(match handle {
            ClipHandle::FadeIn => ClipDragKind::Fade {
                clip,
                edge: Edge::Left,
                origin,
                file,
            },
            ClipHandle::FadeOut => ClipDragKind::Fade {
                clip,
                edge: Edge::Right,
                origin,
                file,
            },
            ClipHandle::Gain => ClipDragKind::Gain {
                clip,
                from_db: origin.gain_db,
                from_y: y,
                fine: false,
            },
        })
    }

    /// Shift-click and cmd-click: the clip in or out of the selection.
    fn toggle_clip(&mut self, clip: InstanceId, cx: &mut Context<Self>) {
        let mut clips = self.clips.clone();
        clips.toggle(clip);
        let primary = clips.primary().cloned();
        self.set_clips(clips.iter().cloned().collect::<Vec<_>>(), primary, cx);
    }

    /// A press on the body of a clip: a move of it, or of every selected clip when it is one of
    /// them. With cmd held (`keeps`) the selection stays as it is until the first move, and the
    /// clip moves with it. A selected clip that the project no longer has is left out. `None`
    /// when the pressed clip is gone.
    fn start_move(
        &mut self,
        pressed: &InstanceId,
        keeps: bool,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        self.refresh_order(cx);
        let selected = self.clips.contains(pressed);
        let mut ids: Vec<InstanceId> = match (selected, keeps) {
            (false, false) => vec![pressed.clone()],
            _ => self.clips.iter().cloned().collect(),
        };
        if !selected && keeps {
            ids.push(pressed.clone());
        }
        if !keeps {
            self.set_clips(ids.clone(), Some(pressed.clone()), cx);
        }
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for id in ids {
            let Some(clip) = AnyClip::read(project, &id) else {
                continue;
            };
            let Some(row) = id.parent().and_then(|track| self.row_of(&track)) else {
                continue;
            };
            clips.push(MovedClip {
                clip: id.clone(),
                home: id,
                kind: clip.kind(),
                row,
                range: range_of(project, &clip),
                written: clip.start(),
            });
        }
        let grabbed = clips.iter().position(|moved| moved.clip == *pressed)?;
        let grab_row = clips.get(grabbed)?.row;
        Some(ClipDragKind::Move {
            clips,
            grab_row,
            grabbed,
            rows: 0,
            tracks: BTreeMap::new(),
            lanes_written: BTreeSet::new(),
            ghosts: Vec::new(),
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
        let tick = self.grid(cx).snap(scene.viewport.tick_at(x));
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
    fn add_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
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
        let row = scene.viewport.track_at(&scene.layout, y);
        let Some(track) = row.and_then(|row| self.order.get(row)).cloned() else {
            return;
        };
        // An audio track plays audio clips only, which come from a file and not from a click.
        let project = self.session.read(cx).project();
        if project
            .state(&track)
            .is_some_and(|state| state.kind == TrackKind::Audio)
        {
            return;
        }
        let clip = new_clip(scene.viewport.tick_at(x), &self.grid(cx));
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
        let at_press = (
            self.clips.iter().cloned().collect(),
            self.clips.primary().cloned(),
        );
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
            at_press,
        });
    }

    /// One mouse move of the rectangle: the clips it touches and what was selected before.
    fn marquee_to(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let viewport = self.painted.get();
        let layout = self.rows(cx);
        let Some(marquee) = &mut self.marquee else {
            return;
        };
        marquee.to = (viewport.tick_at(x), viewport.content_y(y));
        let (left, right) = ordered(marquee.from.0, marquee.to.0);
        let rows = layout.between(marquee.from.1, marquee.to.1);
        let project = self.session.read(cx).project();
        let mut selected = marquee.before.clone();
        for track in self.order.get(rows).unwrap_or_default() {
            let notes = project.children::<Clip>(track.id());
            let notes = notes.filter(|(_, clip)| clip.start <= right && clip.end() > left);
            selected.extend(notes.map(|(clip, _)| clip.id().clone()));
            let audio = project.children::<AudioClip>(track.id());
            let audio =
                audio.filter(|(_, clip)| clip.start <= right && shown_end(project, clip) > left);
            selected.extend(audio.map(|(clip, _)| clip.id().clone()));
        }
        let primary = self.clips.primary().cloned();
        self.set_clips(selected, primary, cx);
        cx.notify();
    }

    /// One mouse move of a drag: the clips become what the pointer says, through the gesture
    /// of the session, so sound and every other view follow. `free` is cmd held: no snap.
    /// `fine` is shift held: the gain moves ten times finer. `alone` is alt held: a moved clip
    /// leaves the automation where it is.
    fn drag_to(
        &mut self,
        x: f32,
        y: f32,
        (free, fine, alone): (bool, bool, bool),
        cx: &mut Context<Self>,
    ) {
        self.refresh_order(cx);
        let grid = match free {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        match self.drag.as_ref().map(|drag| &drag.kind) {
            Some(ClipDragKind::Move { .. }) => self.drag_move(x, y, grid, alone, cx),
            Some(ClipDragKind::Resize { .. }) => self.drag_resize(x, grid, cx),
            Some(ClipDragKind::Trim { .. }) => self.drag_trim(x, grid, cx),
            Some(ClipDragKind::Fade { .. }) => self.drag_fade(x, cx),
            Some(ClipDragKind::Gain { .. }) => self.drag_gain(y, fine, cx),
            None => {}
        }
    }

    /// Alt pressed or let go during a move of clips, or shift or cmd during a move of a point:
    /// the move again where the pointer is, so the automation goes along or stays, or the point
    /// takes its axis or the grid, at once and not at the next mouse move.
    fn modifiers_changed(&mut self, modifiers: Modifiers, window: &Window, cx: &mut Context<Self>) {
        let bounds = self.painted_bounds.get();
        let (x, y) = Self::timeline_position(bounds, window.mouse_position());
        let moving = self.drag.as_ref().map(|drag| &drag.kind);
        if matches!(moving, Some(ClipDragKind::Move { .. })) {
            let keys = (modifiers.platform, modifiers.shift, modifiers.alt);
            self.drag_to(x, y, keys, cx);
        }
        let point = self.lane_drag.as_ref().map(|drag| &drag.kind);
        if matches!(point, Some(LaneDragKind::Point { moving: true, .. })) {
            self.drag_lane(x, y, modifiers, cx);
        }
    }

    /// Whether the mouse has something: clips, a rectangle or a track. Keys then wait, as they
    /// would fight the next mouse move.
    fn dragging(&self) -> bool {
        self.drag.is_some()
            || self.marquee.is_some()
            || self.track_drag.is_some()
            || self.lane_drag.is_some()
    }

    /// A press on a track header, which may become a drag of the track.
    fn start_track_drag(&mut self, track: &InstanceId, y: f32, rows: Rows) {
        // The tracks are read at the first move, see `drag_track`.
        self.track_drag = Some(TrackDrag {
            track: track.clone(),
            rows,
            origin: Vec::new(),
            from: 0,
            at: 0,
            press: f64::from(y) + self.painted.get().scroll_y,
            moving: false,
            begun: false,
        });
    }

    /// One mouse move of a track drag: the track goes to the row under the pointer, above the
    /// first the first and below the last the last. The master is not a track and stays last.
    fn drag_track(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(mut drag) = self.track_drag.take() else {
            return;
        };
        let viewport = self.painted.get();
        let moved = (f64::from(y) + viewport.scroll_y - drag.press).abs();
        if !drag.moving && moved < TRACK_DRAG_THRESHOLD {
            self.track_drag = Some(drag);
            return;
        }
        if !drag.moving {
            drag.moving = true;
            cx.notify();
        }
        let project = self.session.read(cx).project();
        let tracks = drag.origin.len();
        match drag.begun {
            // Until the gesture opens, undo is free and an agent may write: the drag starts
            // from the tracks as they are now. Once it opens, it owns the orders.
            false => drag.origin = track_orders(project, self.arrangement.id()),
            // A track deleted from outside leaves the rows, and the others close up.
            true => drag
                .origin
                .retain(|(track, _)| project.state(track).is_some()),
        }
        let Some(from) = drag
            .origin
            .iter()
            .position(|(row, _)| *row.id() == drag.track)
        else {
            self.track_drag = Some(drag);
            return self.end_drag(cx);
        };
        if !drag.begun {
            (drag.from, drag.at) = (from, from);
        } else if drag.origin.len() != tracks {
            // The rows moved under the drag: publish again, wherever the pointer is.
            (drag.from, drag.at) = (from, usize::MAX);
        }
        let to = viewport
            .nearest_track(&drag.rows, y)
            .unwrap_or(drag.from)
            .min(drag.origin.len().saturating_sub(1));
        if to == drag.at {
            self.track_drag = Some(drag);
            return;
        }
        let begun = std::mem::replace(&mut drag.begun, true);
        self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(MOVE_TRACK_LABEL, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                move_track(project, &mut changes, &drag.origin, drag.from, to);
                project.publish(edit, changes)
            })
        });
        drag.at = to;
        self.track_drag = Some(drag);
    }

    /// Alt-up and alt-down: the selected track one place up or down, one undo step. Whether a
    /// track is selected.
    fn nudge_track(&mut self, step: i64, cx: &mut Context<Self>) -> bool {
        let Some(track) = self.selected_track.clone() else {
            return false;
        };
        let arrangement = self.arrangement.id().clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let tracks = track_orders(project, &arrangement);
                let Some(from) = tracks.iter().position(|(row, _)| *row.id() == track) else {
                    return Ok(());
                };
                let to = nudged_track(from, tracks.len(), step);
                let mut changes = Changes::new();
                move_track(project, &mut changes, &tracks, from, to);
                project.commit(MOVE_TRACK_LABEL, changes)
            })
        });
        true
    }

    /// The kind of the track on a row.
    fn kind_of_row(&self, row: usize, project: &Project) -> Option<TrackKind> {
        let track = self.order.get(row)?;
        project.state(track).map(|state| state.kind)
    }

    /// Whether every clip lands on a track of its own kind when moved by `rows`.
    fn fits(&self, clips: &[MovedClip], rows: i64, project: &Project) -> bool {
        clips.iter().all(|moved| {
            let row = moved.row.checked_add_signed(rows as isize);
            row.and_then(|row| self.kind_of_row(row, project)) == Some(moved.kind)
        })
    }

    /// A move of the selected clips: all by the same distance in time and in track rows. The
    /// earliest stops at tick 0 and the outer ones at the first and the last track, and the
    /// others keep their distance to them. A note clip goes on instrument tracks only and an
    /// audio clip on audio tracks only: over a track one of them cannot go on, they stay on
    /// the rows where they last could. Each clip takes the automation under it along, and
    /// with `alone` it leaves it where it is.
    fn drag_move(&mut self, x: f32, y: f32, grid: Grid, alone: bool, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let label = drag.label();
        let ClipDragKind::Move {
            clips,
            grab_row,
            grabbed,
            rows: last_rows,
            tracks,
            lanes_written,
            ghosts,
        } = &mut drag.kind
        else {
            self.drag = Some(drag);
            return;
        };
        let project = self.session.read(cx).project();
        // A clip deleted from outside is left out of the move. When it is the one under the
        // pointer, the drag ends: the delete was the last write.
        let Some(grabbed_id) = clips.get(*grabbed).map(|moved| moved.clip.clone()) else {
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        let lives: Vec<Option<AnyClip>> = clips
            .iter()
            .map(|moved| AnyClip::read(project, &moved.clip))
            .collect();
        let mut lives = lives.into_iter();
        clips.retain(|_| lives.next().flatten().is_some());
        let Some(index) = clips.iter().position(|moved| moved.clip == grabbed_id) else {
            self.drag = Some(drag);
            return self.end_drag(cx);
        };
        *grabbed = index;
        let lives: Vec<AnyClip> = clips
            .iter()
            .filter_map(|moved| AnyClip::read(project, &moved.clip))
            .collect();
        // Once it moves, the drag owns the starts. Before that, an undo under the press may
        // have moved a clip.
        if !drag.begun {
            for (moved, live) in clips.iter_mut().zip(&lives) {
                (moved.range, moved.written) = (range_of(project, live), live.start());
            }
            *tracks = track_states(project, self.arrangement.id());
        }
        let viewport = self.painted.get();
        let earliest = clips
            .iter()
            .map(|moved| moved.range.start.0)
            .min()
            .unwrap_or(0);
        // The clip under the pointer lands on the grid, and the others move with it.
        let anchor = clips
            .get(index)
            .map_or(Ticks(earliest), |moved| moved.range.start);
        let delta = grid.delta(anchor, drag.grab, viewport.tick_at(x));
        let delta = delta.max(-(earliest as i64));
        let rows = self.order.len();
        let (top, bottom) = (
            clips.iter().map(|moved| moved.row).min().unwrap_or(0),
            clips.iter().map(|moved| moved.row).max().unwrap_or(0),
        );
        let layout = self.rows(cx);
        let under_pointer = viewport.nearest_track(&layout, y).unwrap_or(*grab_row);
        let row_delta = (under_pointer as i64 - *grab_row as i64)
            .clamp(-(top as i64), rows.saturating_sub(1 + bottom) as i64);
        if self.fits(clips, row_delta, project) {
            *last_rows = row_delta;
        }
        let row_delta = *last_rows;
        // Each clip with the automation it would take along, one for one.
        let (mut moves, mut steps) = (Vec::new(), Vec::new());
        for (moved, live) in clips.iter().zip(lives) {
            let row = moved.row.saturating_add_signed(row_delta as isize);
            let (Some(to), Some(from)) = (self.order.get(row).cloned(), moved.home.parent()) else {
                self.drag = Some(drag);
                return;
            };
            let next = live.with_start(shifted(moved.range.start, delta));
            steps.push(LaneMove {
                from,
                range: moved.range.clone(),
                to: to.id().clone(),
                start: next.start(),
            });
            moves.push(ClipMove {
                clip: moved.clip.clone(),
                home: moved.home.clone(),
                was: moved.range.clone(),
                to,
                next,
            });
        }
        let taken = match alone {
            true => Moved::default(),
            false => moved(tracks, &steps, &travel_in(project)),
        };
        let mut lanes = taken.lanes;
        // A track the drag wrote before and leaves now goes back to how it was.
        for track in lanes_written.iter() {
            if let Some(state) = tracks.get(track) {
                let automation = || state.automation.clone();
                lanes.entry(track.clone()).or_insert_with(automation);
            }
        }
        lanes_written.extend(lanes.keys().cloned());
        let same_lanes = lanes.iter().all(|(track, automation)| {
            let track = project.resolve::<TrackState>(track);
            let state = track.and_then(|track| project.state(&track));
            state.is_none_or(|state| state.automation == *automation)
        });
        let unchanged = same_lanes
            && moves.iter().all(|step| {
                step.clip.parent().as_ref() == Some(step.to.id())
                    && AnyClip::read(project, &step.clip).map(|live| live.start())
                        == Some(step.next.start())
            });
        if unchanged {
            self.drag = Some(drag);
            return;
        }
        let starts: Vec<Ticks> = moves.iter().map(|step| step.next.start()).collect();
        let begun = std::mem::replace(&mut drag.begun, true);
        let moved = self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                let moved = move_clips(project, &mut changes, moves)?;
                automation::write(project, &mut changes, lanes);
                project.publish(edit, changes)?;
                Ok(moved)
            })
        });
        if let Some(moved) = moved {
            for ((clip, now), start) in clips.iter_mut().zip(moved).zip(starts) {
                (clip.clip, clip.written) = (now, start);
            }
        }
        // What each clip takes along, for the ghosts and the hint.
        let carried = clips.iter().zip(steps).zip(taken.carried);
        let carried = carried.filter(|(_, lanes)| !lanes.is_empty());
        *ghosts = carried
            .map(|((clip, step), lanes)| LaneGhost {
                clip: clip.clip.clone(),
                range: step.start..step.start + step.range.end.saturating_sub(step.range.start),
                track: step.to,
                lanes,
            })
            .collect();
        let selected: Vec<_> = clips.iter().map(|moved| moved.clip.clone()).collect();
        let primary = selected.get(*grabbed).cloned();
        self.drag = Some(drag);
        self.set_clips(selected, primary, cx);
    }

    /// Publishes one mouse move of a drag of one audio clip into the gesture of the session,
    /// which opens with the first move that changes something.
    fn publish_audio(
        &mut self,
        mut drag: ClipDrag,
        clip: Instance<AudioClip>,
        next: AudioClip,
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        if project.state(&clip) == Some(&next) {
            self.drag = Some(drag);
            return;
        }
        let label = drag.label();
        let begun = std::mem::replace(&mut drag.begun, true);
        self.drag = Some(drag);
        self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&clip, next);
                project.publish(edit, changes)
            })
        });
        cx.notify();
    }

    /// A move of an edge of an audio clip: the part of its file that plays. The left edge keeps
    /// the sound where it is in time.
    fn drag_trim(&mut self, x: f32, grid: Grid, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let ClipDragKind::Trim {
            clip,
            edge,
            origin,
            file,
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
        // An undo between mouse down and the first change may have changed the clip.
        if !drag.begun {
            *origin = live.clone();
        }
        let clock = project.clock();
        let anchor = match edge {
            Edge::Left => origin.start,
            Edge::Right => origin.end(Some(file), clock),
        };
        let delta = grid.delta(anchor, drag.grab, self.painted.get().tick_at(x));
        let unit = grid.unit_at(anchor);
        let next = match edge {
            Edge::Left => {
                let trimmed = trimmed_left(origin, file, clock, delta, unit);
                AudioClip {
                    start: trimmed.start,
                    file_start_seconds: trimmed.file_start_seconds,
                    ..live
                }
            }
            Edge::Right => {
                let trimmed = trimmed_right(origin, file, clock, delta, unit);
                AudioClip {
                    file_end_seconds: trimmed.file_end_seconds,
                    ..live
                }
            }
        };
        // The fades of the live clip, inside what it plays now.
        let next = fitted(next, file);
        let clip = clip.clone();
        self.publish_audio(drag, clip, next, cx);
    }

    /// A move of a fade handle: the fade grows by the time the pointer went, in the time of the
    /// clip. No snap: a fade is a time and not a place on the grid.
    fn drag_fade(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let ClipDragKind::Fade {
            clip,
            edge,
            origin,
            file,
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
        // An undo between mouse down and the first change may have changed the fades.
        if !drag.begun {
            *origin = live.clone();
        }
        let clock = project.clock();
        let went = clock.seconds_of(self.painted.get().tick_at(x)) - clock.seconds_of(drag.grab);
        let went = (went * 1000.) as f32;
        let next = match edge {
            Edge::Left => AudioClip {
                fade_in_ms: fade_in(&live, file, origin.fade_in_ms + went),
                ..live
            },
            Edge::Right => AudioClip {
                fade_out_ms: fade_out(&live, file, origin.fade_out_ms - went),
                ..live
            },
        };
        let clip = clip.clone();
        self.publish_audio(drag, clip, next, cx);
    }

    /// A move of the gain handle: up is louder, 200 pt for the whole range as on a knob, and
    /// ten times finer with shift.
    fn drag_gain(&mut self, y: f32, fine: bool, cx: &mut Context<Self>) {
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let ClipDragKind::Gain {
            clip,
            from_db,
            from_y,
            fine: was_fine,
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
        if fine != *was_fine {
            (*from_db, *from_y, *was_fine) = (live.gain_db, y, fine);
        }
        // An undo between mouse down and the first change may have changed the gain.
        if !drag.begun {
            *from_db = live.gain_db;
        }
        let (bottom, top) = GAIN_DB;
        let speed = if fine { 0.1 } else { 1. };
        let db = (*from_y - y) * (top - bottom) / GAIN_TRAVEL * speed;
        // No move up or down leaves the gain as it is, also a `-inf` under the range.
        let gain_db = match db == 0. {
            true => live.gain_db,
            false => gain_moved(*from_db, db),
        };
        let next = AudioClip { gain_db, ..live };
        let clip = clip.clone();
        self.publish_audio(drag, clip, next, cx);
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
        let anchor = match edge {
            Edge::Left => origin.start,
            Edge::Right => origin.end(),
        };
        let next_delta = grid.delta(anchor, drag.grab, pointer);
        if !rebased && next_delta == *delta {
            self.drag = Some(drag);
            return;
        }
        *delta = next_delta;
        let next = match edge {
            Edge::Left => resized_left(origin, next_delta, grid.unit_at(anchor)),
            Edge::Right => resized_right(origin, next_delta, grid.unit_at(anchor)),
        };
        if next == *written {
            self.drag = Some(drag);
            return;
        }
        let begun = std::mem::replace(&mut drag.begun, true);
        let (instance, wrote) = (clip.clone(), next.clone());
        let instance_id = clip.id().clone();
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
        // A cmd press left the selection alone until now: what is resized is selected.
        if !begun {
            self.select_clip(Some(instance_id), cx);
        }
    }

    /// Mouse up, or a clip went away under the drag: the gesture becomes one undo step. A press
    /// that did not move changes the selection as the click it was, see [`OnRelease`].
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.marquee = None;
        let track_drag = self.track_drag.take().is_some_and(|drag| drag.begun);
        let lane_drag = self.lane_drag.take().is_some_and(|drag| drag.begun);
        if track_drag || lane_drag {
            self.session
                .update(cx, |session, cx| session.finish_gesture(cx));
        }
        if let Some(drag) = self.drag.take() {
            if drag.begun {
                self.session
                    .update(cx, |session, cx| session.finish_gesture(cx));
            } else {
                match drag.on_release {
                    Some(OnRelease::SelectAlone(pressed)) => self.select_clip(Some(pressed), cx),
                    Some(OnRelease::Toggle(pressed)) => self.toggle_clip(pressed, cx),
                    None => {}
                }
            }
        }
        cx.notify();
    }

    /// Escape: the clips or the track go back to where they were at mouse down. Whether there
    /// was a drag.
    fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(marquee) = self.marquee.take() {
            let (clips, primary) = marquee.at_press;
            self.set_clips(clips, primary, cx);
            cx.notify();
            return true;
        }
        let track_drag = self.track_drag.take().map(|drag| drag.begun);
        let lane_drag = self.lane_drag.take().map(|drag| drag.begun);
        // The point goes back, or away when the press added it: it is not selected any more.
        if lane_drag.is_some() {
            self.selected_point = None;
        }
        if let Some(begun) = track_drag.or(lane_drag) {
            if begun {
                self.session
                    .update(cx, |session, cx| session.cancel_gesture(cx));
            }
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

    /// The cursor says what a drag from here does, and the audio clip under the pointer shows
    /// its handles.
    fn hover(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let inside = x >= 0.0 && y >= 0.0;
        let zone = scene.zone_at(x, y).filter(|_| inside);
        let cursor = zone.and_then(|(_, grip)| match grip {
            Grip::Zone(Zone::Body) => None,
            Grip::Zone(Zone::LeftEdge | Zone::RightEdge) => Some(CursorStyle::ResizeLeftRight),
            Grip::Handle(ClipHandle::FadeIn | ClipHandle::FadeOut) => {
                Some(CursorStyle::ResizeLeftRight)
            }
            Grip::Handle(ClipHandle::Gain) => Some(CursorStyle::ResizeUpDown),
        });
        let hovered = zone.map(|(shape, _)| shape.id.clone());
        // In a lane, a press on a dot takes the point, and anywhere else adds one.
        let in_lane = matches!(
            scene.viewport.part_at(&scene.layout, y),
            Some((_, Part::Lane(_)))
        );
        let point = self.point_at(x, y, scene, cx).filter(|_| inside);
        let cursor = match (inside && in_lane, &point) {
            (true, Some(_)) => Some(CursorStyle::PointingHand),
            (true, None) => Some(CursorStyle::Crosshair),
            (false, _) => cursor,
        };
        if self.hover_cursor != cursor || self.hovered != hovered || self.hovered_point != point {
            (self.hover_cursor, self.hovered) = (cursor, hovered);
            self.hovered_point = point;
            cx.notify();
        }
    }

    /// The pointer left the timeline: no clip shows its handles for it any more.
    fn unhover(&mut self, cx: &mut Context<Self>) {
        let left = self.hovered_point.take().is_some();
        if self.hovered.take().is_some() || self.hover_cursor.take().is_some() || left {
            cx.notify();
        }
    }

    fn cursor(&self) -> Option<CursorStyle> {
        if self.track_drag.as_ref().is_some_and(|drag| drag.moving) {
            return Some(CursorStyle::ClosedHand);
        }
        if let Some(drag) = &self.lane_drag {
            return Some(match drag.kind {
                LaneDragKind::Erase { .. } => CursorStyle::Crosshair,
                LaneDragKind::Point { .. } => CursorStyle::PointingHand,
            });
        }
        match &self.drag {
            Some(drag) => drag.cursor(),
            None => self.hover_cursor,
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

    /// Alt-up and alt-down: the gain of every selected audio clip by a decibel, as one undo
    /// step. Note clips have none. Whether there was one to change.
    fn step_gains(&mut self, step: f32, cx: &mut Context<Self>) -> bool {
        let project = self.session.read(cx).project();
        let clips: Vec<_> = self
            .clips
            .iter()
            .filter_map(|id| {
                let clip = project.resolve::<AudioClip>(id)?;
                let state = project.state(&clip)?.clone();
                Some((clip, state))
            })
            .collect();
        if clips.is_empty() {
            return false;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for (clip, state) in clips {
                    let gain_db = gain_moved(state.gain_db, step);
                    changes.set(&clip, AudioClip { gain_db, ..state });
                }
                project.commit(GAIN_LABEL, changes)
            })
        });
        true
    }

    /// The keys of the focused timeline. Whether the key was one of them.
    fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
        let key = event.keystroke.key.as_str();
        // Alt-up and alt-down: the gain of the selected audio clips, and with no clip selected,
        // the place of the selected track.
        if alt && !(control || shift || platform) && !self.dragging() {
            let project = self.session.read(cx).project();
            let primary = self.clips.primary();
            let clip_selected = primary.is_some_and(|clip| is_clip_tool(project, clip));
            return match (key, clip_selected) {
                ("up", true) => self.step_gains(GAIN_KEY_STEP_DB, cx),
                ("down", true) => self.step_gains(-GAIN_KEY_STEP_DB, cx),
                ("up", false) => self.nudge_track(-1, cx),
                ("down", false) => self.nudge_track(1, cx),
                _ => false,
            };
        }
        if control || alt || shift {
            return false;
        }
        if key == "escape" && !platform {
            if self.cancel_drag(cx) {
                return true;
            }
            // Then a selected tempo change or point lets go, and only then the panel below
            // closes.
            if self.selected_tempo.is_some() {
                self.select_tempo(None, cx);
                return true;
            }
            if self.selected_point.is_some() {
                self.select_point(None, cx);
                return true;
            }
            return false;
        }
        // The mouse has the clips: a key would fight the next mouse move.
        if self.dragging() {
            return false;
        }
        if platform {
            return self.on_command(key, cx);
        }
        if key == "t" {
            // While playing, the playhead is between two frames of any grid: the change goes
            // to the nearest step. Stopped, it is where a click on the ruler put it.
            let playhead = *self.playhead.read(cx);
            let tick = match playhead.playing {
                true => self.grid(cx).snap(playhead.tick),
                false => playhead.tick,
            };
            self.add_tempo_change(tick, cx);
            return true;
        }
        if let Some(tick) = self.selected_tempo
            && matches!(key, "backspace" | "delete")
        {
            self.remove_tempo_change(tick, cx);
            return true;
        }
        if matches!(key, "backspace" | "delete") && self.delete_point(cx) {
            return true;
        }
        let project = self.session.read(cx).project();
        let primary = self.clips.primary().cloned();
        let Some(clip) = primary.filter(|clip| is_clip_tool(project, clip)) else {
            return self.on_track_key(key, window, cx);
        };
        match key {
            "enter" => self.open(&clip, cx),
            "backspace" | "delete" => self.delete_clips("Delete clip", "Delete clips", None, cx),
            "left" => self.nudge_in_time(false, cx),
            "right" => self.nudge_in_time(true, cx),
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
                if let Some(copied) = self.copy(cx) {
                    self.delete_clips("Cut clip", "Cut clips", Some(&copied), cx);
                }
            }
            "v" => self.paste(cx),
            "d" => self.duplicate(cx),
            "down" => {
                let project = self.session.read(cx).project();
                let primary = self.clips.primary().cloned();
                if let Some(clip) = primary.filter(|clip| is_clip_tool(project, clip)) {
                    self.open(&clip, cx);
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
    /// select the track above or below, enter edits its name, and `a` shows its automation
    /// lanes or folds them away, as the toggle in its header does.
    fn on_track_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.refresh_order(cx);
        let selected = self.selected_track.as_ref();
        let Some(current) = selected.and_then(|id| self.row_of(id)) else {
            return false;
        };
        if key == "a" {
            let Some(track) = self.order.get(current).map(|track| track.id().clone()) else {
                return false;
            };
            let shown = self.shows_lanes(&track);
            self.show_lanes(&track, !shown, cx);
            return true;
        }
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
        let Some(name) = self
            .session
            .read(cx)
            .project()
            .state(&track)
            .map(|state| state.name.clone())
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
            let text = timeline
                .rename
                .as_ref()
                .map(|rename| rename.input.read(cx).text().to_string());
            timeline.finish_rename(text, window, cx);
        });
        window.focus(&focus, cx);
        self.rename = Some(Rename {
            track,
            input,
            focus,
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
        if rename.focus.is_focused(window) {
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
            let track = rename.track;
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
        let mut clips = Vec::new();
        for track in &self.order {
            let notes = project.children::<Clip>(track.id());
            clips.extend(notes.map(|(clip, _)| clip.id().clone()));
            let audio = project.children::<AudioClip>(track.id());
            clips.extend(audio.map(|(clip, _)| clip.id().clone()));
        }
        let primary = self.clips.primary().cloned();
        self.set_clips(clips, primary, cx);
    }

    /// What the selected clips are for the clipboard: each with its row, its name and the
    /// automation under it on the timeline.
    fn copied(&mut self, cx: &mut Context<Self>) -> Option<CopiedClips> {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let travel = travel_in(project);
        let clips = self
            .selected_states(cx)
            .into_iter()
            .filter_map(|(id, clip)| {
                let track = id.parent()?;
                let row = self.row_of(&track)?;
                let state = project.state(&project.resolve::<TrackState>(&track)?)?;
                let lanes = Carried::under(&track, state, range_of(project, &clip), &travel);
                Some((row, id.name().to_string(), clip, lanes))
            });
        CopiedClips::new(clips.collect::<Vec<_>>())
    }

    /// Cmd-c. What was copied, when there was something.
    fn copy(&mut self, cx: &mut Context<Self>) -> Option<CopiedClips> {
        let copied = self.copied(cx)?;
        *self.clipboard.borrow_mut() = Some(Copied::Clips(copied.clone()));
        Some(copied)
    }

    /// Cmd-v: the copied clips at the playhead, the top one on the track of the first selected
    /// clip, else on the selected track, else on the first track. One undo step.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let copied = match self.clipboard.borrow().as_ref() {
            Some(Copied::Clips(copied)) => copied.clone(),
            _ => return,
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

    /// Adds copies of clips as one undo step and selects them, each with the automation that
    /// was under it, over what the lanes had where it lands. A note clip goes on an instrument
    /// track only and an audio clip on an audio track only: a paste that would put one on the
    /// other kind is refused as a whole, and the notice says why.
    fn add_copies(
        &mut self,
        copied: &CopiedClips,
        at: Ticks,
        top: usize,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let order = self.order.clone();
        let arrangement = self.arrangement.id().clone();
        let placed = copied.placed(at, top, order.len());
        let project = self.session.read(cx).project();
        let wrong = placed.iter().find_map(|(row, name, clip, _)| {
            let track = order.get(*row)?;
            let kind = project.state(track)?.kind;
            (kind != clip.kind()).then(|| (track.id().clone(), name.to_string(), clip.kind()))
        });
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                if let Some((track, name, kind)) = wrong {
                    return Err(wrong_track(&track, &name, kind));
                }
                let mut changes = Changes::new();
                change_lanes(project, &mut changes, &arrangement, |tracks, travel| {
                    for (row, _, clip, lanes) in &placed {
                        let Some(track) = order.get(*row) else {
                            continue;
                        };
                        if let Some(state) = tracks.get_mut(track.id()) {
                            lanes.place(track.id(), state, clip.start(), travel);
                        }
                    }
                });
                let (mut notes, mut audio) = (Vec::new(), Vec::new());
                for (row, name, clip, _) in placed {
                    let Some(track) = order.get(row) else {
                        continue;
                    };
                    match clip {
                        AnyClip::Notes(clip) => notes.push((track, name, clip)),
                        // A copy of `take-2` is `take` when that is free, as for notes.
                        AnyClip::Audio(clip) => audio.push((track, unnumbered(name), clip)),
                    }
                }
                let mut added: Vec<InstanceId> = add_clips(project, &mut changes, notes)?
                    .into_iter()
                    .map(|clip| clip.id().clone())
                    .collect();
                let audio = add_audio_clips(project, &mut changes, audio)?;
                added.extend(audio.into_iter().map(|clip| clip.id().clone()));
                project.commit(label, changes)?;
                Ok(added)
            })
        });
        if let Some(ids) = added {
            let primary = ids.first().cloned();
            self.set_clips(ids, primary, cx);
        }
    }

    /// Deletes the selected clips as one undo step. A cut also takes the automation under them,
    /// `taken`, as a move does: each lane becomes a straight line where they were.
    fn delete_clips(
        &mut self,
        one: &'static str,
        several: &'static str,
        taken: Option<&CopiedClips>,
        cx: &mut Context<Self>,
    ) {
        let selected: Vec<_> = self.clips.iter().cloned().collect();
        let label = plural(selected.len(), one, several);
        let arrangement = self.arrangement.id().clone();
        let deleted = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for clip in &selected {
                    changes.delete(clip);
                }
                if let Some(taken) = taken {
                    // A cut takes every lane, as it would along its own track.
                    let taken: Vec<_> = taken.lanes().map(|lanes| (lanes, lanes.track())).collect();
                    change_lanes(project, &mut changes, &arrangement, |tracks, travel| {
                        for (track, state) in tracks {
                            automation::clear(track, state, &taken, travel);
                        }
                    });
                }
                project.commit(label, changes)
            })
        });
        if deleted.is_some() {
            let first = self.clips.primary().cloned();
            self.deleted = first.iter().cloned().collect();
            self.deleted
                .extend(selected.into_iter().filter(|id| Some(id) != first.as_ref()));
        }
    }

    /// The arrows left and right: every selected clip by one unit of the grid, as one undo
    /// step, with the automation under it. The earliest stops at tick 0. A moved audio clip goes
    /// on top of its track.
    fn nudge_in_time(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let selected = self.selected_states(cx);
        let earliest = selected.iter().map(|(_, clip)| clip.start().0).min();
        let earliest = earliest.unwrap_or(0);
        let delta = self.grid(cx).nudge(Ticks(earliest), forward);
        let delta = delta.max(-(earliest as i64));
        if delta == 0 || selected.is_empty() {
            return;
        }
        let project = self.session.read(cx).project();
        let moves: Vec<ClipMove> = selected
            .into_iter()
            .filter_map(|(clip, state)| {
                let track = project.resolve::<TrackState>(&clip.parent()?)?;
                let start = shifted(state.start(), delta);
                Some(ClipMove {
                    home: clip.clone(),
                    clip,
                    was: range_of(project, &state),
                    to: track,
                    next: state.with_start(start),
                })
            })
            .collect();
        let label = plural(moves.len(), "Nudge clip", "Nudge clips");
        let arrangement = self.arrangement.id().clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                move_lanes(project, &mut changes, &arrangement, &moves);
                move_clips(project, &mut changes, moves)?;
                project.commit(label, changes)
            })
        });
    }

    /// The arrows up and down: every selected clip to the nearest tracks above or below where
    /// each lands on a track of its own kind, as one undo step, with the volume and the pan
    /// under it. Nothing moves when there are none before the first or the last track.
    fn nudge_to_track(&mut self, step: i64, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for (clip, next) in self.selected_states(cx) {
            let Some(row) = clip.parent().and_then(|track| self.row_of(&track)) else {
                return;
            };
            let moved = MovedClip {
                home: clip.clone(),
                clip: clip.clone(),
                kind: next.kind(),
                row,
                range: range_of(project, &next),
                written: next.start(),
            };
            clips.push((moved, next));
        }
        let moved: Vec<MovedClip> = clips.iter().map(|(moved, _)| moved.clone()).collect();
        let tracks = self.order.len() as i64;
        let rows = (1..tracks)
            .map(|times| step * times)
            .find(|rows| self.fits(&moved, *rows, project));
        let Some(rows) = rows else {
            return;
        };
        let mut moves = Vec::new();
        for (moved, next) in clips {
            let row = moved.row.saturating_add_signed(rows as isize);
            let Some(to) = self.order.get(row).cloned() else {
                return;
            };
            moves.push(ClipMove {
                clip: moved.clip,
                home: moved.home,
                was: moved.range,
                to,
                next,
            });
        }
        if moves.is_empty() {
            return;
        }
        let label = plural(moves.len(), "Nudge clip", "Nudge clips");
        let primary = self.clips.primary().cloned();
        let index = moves
            .iter()
            .position(|step| Some(&step.clip) == primary.as_ref());
        let arrangement = self.arrangement.id().clone();
        let moved = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                move_lanes(project, &mut changes, &arrangement, &moves);
                let moved = move_clips(project, &mut changes, moves)?;
                project.commit(label, changes)?;
                Ok(moved)
            })
        });
        if let Some(ids) = moved {
            let primary = index.and_then(|index| ids.get(index).cloned());
            self.set_clips(ids, primary, cx);
        }
    }

    /// Where files dropped at a place of the timeline area would go: an audio track under the
    /// pointer, or a new one under the last track, from the snap step under the pointer. Over an
    /// instrument track, the ruler or the headers, nowhere.
    pub fn drop_target_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> Option<DropTarget> {
        self.refresh_order(cx);
        if x < 0. || y < 0. {
            return None;
        }
        let viewport = self.painted.get();
        let tick = self.grid(cx).floor(viewport.tick_at(x));
        let rows = self.rows(cx);
        let Some(row) = viewport.track_at(&rows, y) else {
            let below = y >= viewport.y_at(rows.height());
            return below.then_some(DropTarget::NewTrack(tick));
        };
        let track = self.order.get(row)?;
        let project = self.session.read(cx).project();
        let audio = project.state(track)?.kind == TrackKind::Audio;
        audio.then(|| DropTarget::Track(track.id().clone(), tick))
    }

    /// Files from the Finder are dragged over a place of the timeline area: the ghosts of the
    /// clips they would make follow the pointer. What each file is, for how long its ghost is,
    /// is read on a background thread the first time.
    pub fn drag_files_over(&mut self, paths: Vec<PathBuf>, x: f32, y: f32, cx: &mut Context<Self>) {
        let target = self.drop_target_at(x, y, cx);
        let known = self
            .incoming
            .as_ref()
            .is_some_and(|incoming| incoming.paths == paths);
        if !known {
            let count = paths.len();
            self.incoming = Some(Incoming {
                paths: paths.clone(),
                files: vec![None; count],
                target: None,
            });
            // The header of each file only, on a background thread.
            let reading = cx.background_spawn(async move {
                let read = |path: &PathBuf| sound_media::probe(path).ok();
                paths.iter().map(read).collect::<Vec<_>>()
            });
            cx.spawn(async move |timeline, cx| {
                let files = reading.await;
                timeline
                    .update(cx, |timeline, cx| {
                        if let Some(incoming) = &mut timeline.incoming
                            && incoming.files.len() == files.len()
                        {
                            incoming.files = files;
                            cx.notify();
                        }
                    })
                    .ok();
            })
            .detach();
        }
        if let Some(incoming) = &mut self.incoming
            && incoming.target != target
        {
            incoming.target = target;
            cx.notify();
        }
    }

    /// The drag of files left the timeline, or ended.
    pub fn forget_files(&mut self, cx: &mut Context<Self>) {
        self.dragged_paths.borrow_mut().clear();
        if self.incoming.take().is_some() {
            cx.notify();
        }
    }

    /// Where the files dragged over the timeline would go now.
    pub fn incoming_target(&self) -> Option<&DropTarget> {
        self.incoming.as_ref()?.target.as_ref()
    }

    /// Files dropped from the Finder: each is copied into `assets/audio/` on a background
    /// thread, then all become clips one after another on one track, from the target on, as
    /// one undo step. A file that is no audio this app plays is left out, and the notice says
    /// why. Under the last track the drop makes a new audio track named after the first file.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, target: DropTarget, cx: &mut Context<Self>) {
        let assets = self.session.read(cx).project().assets().clone();
        let importing = cx.background_spawn(async move {
            paths
                .iter()
                .map(|path| {
                    let name = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned());
                    (name.unwrap_or_default(), sound_media::import(&assets, path))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |timeline, cx| {
            let imported = importing.await;
            timeline
                .update(cx, |timeline, cx| {
                    timeline.add_dropped(imported, target, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The clips of files that were copied in, as one undo step, selected.
    fn add_dropped(
        &mut self,
        imported: Vec<(
            String,
            Result<sound_media::Imported, sound_media::MediaError>,
        )>,
        target: DropTarget,
        cx: &mut Context<Self>,
    ) {
        let mut files = Vec::new();
        for (name, result) in imported {
            match result {
                Ok(asset) => files.push((name, asset)),
                Err(error) => {
                    let session = self.session.clone();
                    session.update(cx, |session, cx| session.report(error, cx));
                }
            }
        }
        let Some((first_name, _)) = files.first() else {
            return;
        };
        let first_name = first_name.clone();
        let arrangement = self.arrangement.clone();
        let label = plural(files.len(), "Add audio clip", "Add audio clips");
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let (track, start) = match target {
                    DropTarget::Track(track, start) => {
                        let missing = || ProjectError::MissingInstance(track.clone());
                        (
                            project.resolve::<TrackState>(&track).ok_or_else(missing)?,
                            start,
                        )
                    }
                    DropTarget::NewTrack(start) => {
                        let count = tracks(project, arrangement.id()).len();
                        let colour = Colour::ALL[count % Colour::ALL.len()];
                        let track = add_audio_track(
                            project,
                            &mut changes,
                            arrangement.id(),
                            &first_name,
                            colour,
                        )?;
                        (track, start)
                    }
                };
                // One after another: each starts where the one before it ends.
                // The files are in memory here, so this reads nothing, and the track that plays
                // them reads nothing either while they are held.
                let clock = project.clock();
                let mut at = start;
                let mut clips = Vec::new();
                for (_, imported) in &files {
                    let clip = AudioClip::new(imported.asset.clone(), at);
                    at = clip.end(Some(&imported.audio.info()), clock);
                    clips.push((imported.asset.asset_name().name().to_string(), clip));
                }
                let clips = clips
                    .iter()
                    .map(|(name, clip)| (&track, name.as_str(), clip.clone()));
                let added = add_audio_clips(project, &mut changes, clips)?;
                project.commit(label, changes)?;
                Ok(added)
            })
        });
        if let Some(added) = added {
            let ids: Vec<InstanceId> = added.iter().map(|clip| clip.id().clone()).collect();
            let primary = ids.first().cloned();
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
    fn rename_field(&self, cx: &App) -> Option<gpui::AnyElement> {
        let rename = self.rename.as_ref()?;
        let row = self.row_of(rename.track.id())?;
        let rows = self.rows(cx);
        let top = self.painted.get().y_of(&rows, row);
        let top = RULER_HEIGHT + top + NAME_MIDDLE - RENAME_HEIGHT / 2.;
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

    /// The select that adds a lane, in the row under the lanes of each track that shows them.
    /// Clipped to the header column under the ruler, as the painted headers are, but not while
    /// one is open: GPUI clips a menu to where it was made.
    fn lane_menu_column(&self, cx: &App) -> impl IntoElement {
        let rows = self.rows(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(self.viewport, width, height, cx);
        let project = self.session.read(cx).project();
        let mut open = false;
        let mut selects = Vec::new();
        for (row, track) in self.order.iter().enumerate() {
            let (Some(lanes), Some(lane_menu)) = (rows.lanes(row), self.lane_menus.get(track.id()))
            else {
                continue;
            };
            // Only one that shows whole: one under the ruler would paint over an open one. And
            // only while there is something left to add.
            let top = viewport.y_at(rows.lane_top(row, lanes))
                + (ADD_LANE_HEIGHT - ADD_LANE_BUTTON_HEIGHT) / 2.;
            let shows = top >= 0. && top + ADD_LANE_BUTTON_HEIGHT <= height;
            let state = project.state(track);
            let free =
                state.is_some_and(|state| !free_lanes(project, track.id(), state).is_empty());
            if !shows || !free {
                continue;
            }
            open |= lane_menu.menu.read(cx).is_open();
            selects.push(
                div()
                    .absolute()
                    .top(px(top))
                    .left(px(HEADER_INSET))
                    .occlude()
                    .child(lane_menu.menu.clone()),
            );
        }
        div()
            .absolute()
            .top(px(RULER_HEIGHT))
            .bottom_0()
            .left_0()
            .w(px(HEADER_WIDTH - 1.))
            .when(!open, |column| column.overflow_hidden())
            .children(selects)
    }

    /// A press in an automation lane, at `y` in the timeline area. On the dot of a point it
    /// selects the point, and a drag moves it. Anywhere else it adds a point there, on the grid
    /// unless cmd is held, selected, and a drag goes on to move it. With alt a drag erases the
    /// points it covers. A number the project does not know only erases.
    fn press_lane(
        &mut self,
        row: usize,
        lane: usize,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(track) = self.order.get(row).cloned() else {
            return;
        };
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&track) else {
            return;
        };
        let Some(origin) = state.automation.get(lane).cloned() else {
            return;
        };
        let viewport = self.painted.get();
        let top = self.rows(cx).lane_top(row, lane);
        let in_lane = (viewport.content_y(y) - top) as f32;
        let range = origin.number(track.id(), state, &travel_in(project));
        let range = range.map(|number| number.range);
        let drag = |origin, kind, label| LaneDrag {
            track: track.clone(),
            origin,
            index: lane,
            top,
            kind,
            label,
            begun: false,
        };
        let range = match (event.modifiers.alt, range) {
            (false, Some(range)) => range,
            (true, _) => {
                let kind = LaneDragKind::Erase {
                    from: viewport.tick_at(x),
                    moving: false,
                };
                self.select_point(None, cx);
                self.lane_drag = Some(drag(origin, kind, ERASE_LABEL));
                return;
            }
            (false, None) => return,
        };
        let press = (viewport.tick_at(x), in_lane);
        let point_drag = |point| LaneDragKind::Point {
            point,
            range,
            press,
            moving: false,
        };
        if let Some(point) = track_lanes::point_at(&viewport, &origin, range, (x, in_lane))
            && let Some(pressed) = origin.points.get(point)
        {
            let key = PointKey::of(track.id(), &origin, pressed.tick);
            self.select_point(Some(key), cx);
            self.lane_drag = Some(drag(origin, point_drag(point), MOVE_POINT_LABEL));
            return;
        }
        let tick = viewport.tick_at(x);
        let tick = match event.modifiers.platform {
            true => tick,
            false => self.grid(cx).snap(tick),
        };
        let value = AutomationValue(range.value(LANE_BOX.share_at(in_lane)));
        let (added, point) = track_lanes::with_point(&origin, tick, value);
        self.select_point(Some(PointKey::of(track.id(), &added, tick)), cx);
        let lane_drag = drag(added.clone(), point_drag(point), ADD_POINT_LABEL);
        self.write_lane(lane_drag, Some(added), cx);
    }

    /// One mouse move of a drag in a lane, into the gesture of the session, which opens with
    /// the first change. A point moves on the grid unless cmd is held, and with shift only up
    /// and down or only sideways, the way the pointer went furthest.
    fn drag_lane(&mut self, x: f32, y: f32, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(mut drag) = self.lane_drag.take() else {
            return;
        };
        let viewport = self.painted.get();
        let grid = match modifiers.platform {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        let in_lane = (viewport.content_y(y) - drag.top) as f32;
        // An undo between mouse down and the first change took the lane the drag started from:
        // the drag ends, so it does not write that lane back.
        if !drag.begun {
            let project = self.session.read(cx).project();
            let state = project.state(&drag.track);
            let mut lanes = state.into_iter().flat_map(|state| state.automation.iter());
            if lanes.find(|lane| lane.same_number(&drag.origin)) != Some(&drag.origin) {
                self.selected_point = None;
                cx.notify();
                return;
            }
        }
        let next = match &mut drag.kind {
            LaneDragKind::Erase { from, moving } => {
                if !*moving && (x - viewport.x_of(*from)).abs() < DRAG_THRESHOLD {
                    self.lane_drag = Some(drag);
                    return;
                }
                *moving = true;
                let ticks = erase_range(&grid, *from, viewport.tick_at(x));
                track_lanes::erased(&drag.origin, &ticks)
            }
            LaneDragKind::Point {
                point,
                range,
                press,
                moving,
            } => {
                let (mut dx, mut dy) = (x - viewport.x_of(press.0), in_lane - press.1);
                if !*moving && dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
                    self.lane_drag = Some(drag);
                    return;
                }
                *moving = true;
                if modifiers.shift {
                    match dx.abs() >= dy.abs() {
                        true => dy = 0.,
                        false => dx = 0.,
                    }
                }
                let Some(&from) = drag.origin.points.get(*point) else {
                    self.lane_drag = Some(drag);
                    return;
                };
                let delta = match dx {
                    0. => 0,
                    _ => viewport.tick_at(x).0 as i64 - press.0.0 as i64,
                };
                let tick = match delta {
                    0 => from.tick,
                    delta => grid.snap(shifted(from.tick, delta)),
                };
                let value = match dy {
                    0. => from.value,
                    dy => {
                        let y = LANE_BOX.y_of(range.position(from.value.0)) + dy;
                        AutomationValue(range.value(LANE_BOX.share_at(y)))
                    }
                };
                let moved = track_lanes::moved_point(&drag.origin, *point, tick, value);
                let tick = moved.points.get(*point).map_or(tick, |point| point.tick);
                self.selected_point = Some(PointKey::of(drag.track.id(), &moved, tick));
                Some(moved)
            }
        };
        self.write_lane(drag, next, cx);
    }

    /// Puts `next`, the lane of a drag, in its place among the lanes of its track now, from
    /// where it was at mouse down: `None` takes it away, and a drag back puts it back there.
    /// The first change opens the gesture.
    fn write_lane(
        &mut self,
        mut drag: LaneDrag,
        next: Option<AutomationLane>,
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&drag.track) else {
            self.lane_drag = Some(drag);
            return self.end_drag(cx);
        };
        let mut automation = state.automation.clone();
        let found = automation
            .iter_mut()
            .find(|lane| lane.same_number(&drag.origin));
        match (found, next) {
            (Some(found), Some(lane)) => *found = lane,
            (Some(_), None) => automation.retain(|lane| !lane.same_number(&drag.origin)),
            (None, Some(lane)) => automation.insert(drag.index.min(automation.len()), lane),
            (None, None) => {}
        }
        if automation == state.automation {
            self.lane_drag = Some(drag);
            return;
        }
        let next = TrackState {
            automation,
            ..state.clone()
        };
        let begun = std::mem::replace(&mut drag.begun, true);
        let (track, label) = (drag.track.clone(), drag.label);
        self.lane_drag = Some(drag);
        self.session.update(cx, |session, cx| {
            if !begun {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&track, next);
                project.publish(edit, changes)
            })
        });
    }

    /// Selects a point of an automation lane and no clip or tempo change, so delete removes it.
    fn select_point(&mut self, point: Option<PointKey>, cx: &mut Context<Self>) {
        if point.is_some() {
            self.select_clip(None, cx);
            self.select_tempo(None, cx);
        }
        if self.selected_point != point {
            self.selected_point = point;
            cx.notify();
        }
    }

    /// The point of a lane whose dot is at `(x, y)` in the timeline area.
    fn point_at(&self, x: f32, y: f32, scene: &Scene, cx: &App) -> Option<PointKey> {
        let Some((row, Part::Lane(index))) = scene.viewport.part_at(&scene.layout, y) else {
            return None;
        };
        let track = self.order.get(row)?;
        let project = self.session.read(cx).project();
        let state = project.state(track)?;
        let lane = state.automation.get(index)?;
        let range = lane.number(track.id(), state, &travel_in(project))?.range;
        let top = scene.layout.lane_top(row, index);
        let in_lane = (scene.viewport.content_y(y) - top) as f32;
        let point = track_lanes::point_at(&scene.viewport, lane, range, (x, in_lane))?;
        Some(PointKey::of(track.id(), lane, lane.points.get(point)?.tick))
    }

    /// Delete with a point selected: it goes, and the lane with it when it was the last, as
    /// one undo step. Whether the point was there.
    fn delete_point(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.selected_point.take() else {
            return false;
        };
        cx.notify();
        let project = self.session.read(cx).project();
        let Some(track) = project.resolve::<TrackState>(&key.track) else {
            return false;
        };
        let Some(state) = project.state(&track) else {
            return false;
        };
        let mut state = state.clone();
        let is_lane = |lane: &AutomationLane| key.is_in(&key.track, lane);
        let Some(lane) = state.automation.iter_mut().find(|lane| is_lane(lane)) else {
            return false;
        };
        let Some(point) = lane.points.iter().position(|point| point.tick == key.tick) else {
            return false;
        };
        match track_lanes::without_point(lane, point) {
            Some(left) => *lane = left,
            None => state.automation.retain(|lane| !is_lane(lane)),
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(&track, state);
                project.commit(DELETE_POINT_LABEL, changes)
            })
        });
        true
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

/// Where the arm toggle of an audio track starts in its header: a 24 pt square that ends 8 pt
/// from the edge, as the icons of a card header do. The name of an audio track ends before it.
pub(super) const ARM_LEFT: f32 = HEADER_WIDTH - 8. - 24.;
/// Where the meter of the input of an armed track starts in its header, on the line of its
/// name: 45 pt, to 133.
pub(super) const ARMED_METER_LEFT: f32 = 88.;
/// The meter is the master meter of the transport, 45 x 8.
pub(super) const ARMED_METER_HEIGHT: f32 = 8.;
/// What the header of the track a drop would make says.
const NEW_AUDIO_TRACK: &str = "New audio track";

/// How far right of its tick a tempo label may start to still be seen when its tick is off the
/// left edge.
const TEMPO_LABEL_ROOM: f32 = 80.;
/// The name field of a renamed track: a small text input where the name is painted.
const RENAME_HEIGHT: f32 = 28.;
/// Its left edge, so that its text starts where the painted name does.
const RENAME_LEFT: f32 = NAME_LEFT - 8.;

/// What a take shows while it records: the times of its file under each column on screen,
/// lined up where the composer heard them, and no handles.
fn live_shape(
    take: &LiveTake,
    rect: Rect,
    viewport: &Viewport,
    width: f32,
    project: &Project,
) -> AudioShape {
    let clock = project.clock();
    let start = clock.seconds_of(take.start);
    let start_seconds = take.sound.as_ref().map_or(0., |sound| sound.start_seconds);
    let (from, to) = (
        rect.x.max(0.).floor(),
        (rect.x + rect.width).min(width).ceil(),
    );
    let columns = (to - from).max(0.) as usize;
    let edges = (0..=columns).map(|column| {
        clock.seconds_of(viewport.tick_at(from + column as f32)) - start + start_seconds
    });
    AudioShape {
        sound: Sound::Take(take.sound.as_ref().map(|sound| sound.overview.clone())),
        first: from,
        edges: edges.collect(),
        gain: 1.,
        fade_in: 0.,
        fade_out: 0.,
        hidden: None,
        label: None,
        missing: None,
        handles: false,
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
                } else if timeline.marquee.is_some() {
                    timeline.marquee_to(x, y, cx);
                } else if timeline.track_drag.is_some() {
                    timeline.drag_track(y, cx);
                } else if timeline.lane_drag.is_some() {
                    timeline.drag_lane(x, y, event.modifiers, cx);
                } else {
                    let modifiers = event.modifiers;
                    let keys = (modifiers.platform, modifiers.shift, modifiers.alt);
                    timeline.drag_to(x, y, keys, cx);
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

/// Paints the takes of [`Timeline::take_shapes`] over a timeline painted at `bounds`, inside
/// its area right of the headers and under the ruler.
pub(super) fn paint_takes(
    shapes: &[ClipShape],
    bounds: Bounds<Pixels>,
    assets: &Assets,
    window: &mut Window,
    cx: &mut App,
) {
    let timeline = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(
            bounds.size.width - px(HEADER_WIDTH),
            bounds.size.height - px(RULER_HEIGHT),
        ),
    );
    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for shape in shapes {
            if let Body::Audio(audio) = &shape.body {
                let body = placed(shape.rect, timeline.origin);
                let look = audio_look(shape, audio, body, timeline.origin, assets, cx);
                paint_audio_clip(&look, window, cx);
            }
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
    let (selection, selected_header, lane_text) =
        (theme.gray_950, theme.alpha_at(0.05), theme.gray_800);
    let (marquee_fill, marquee_border) = (theme.alpha_at(0.05), theme.alpha_at(0.20));
    let (drop_ring, ghost_text, muted_ring, muted_text, window_fill) = (
        theme.lavender,
        theme.gray_950,
        theme.gray_800,
        theme.gray_700,
        theme.gray_100,
    );
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
                    top + point(px(HEADER_INSET), px(4.)),
                    size(px(HEADER_WIDTH - 2. * HEADER_INSET), px(TRACK_HEIGHT - 8.)),
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
            // Where a dragged track lands: the ring of a drag, on the shape of a selected header.
            if row.lifted {
                let inside = Bounds::new(
                    top + point(px(HEADER_INSET), px(4.)),
                    size(px(HEADER_WIDTH - 2. * HEADER_INSET), px(TRACK_HEIGHT - 8.)),
                );
                let clear = Hsla::transparent_black();
                let solid = BorderStyle::Solid;
                window.paint_quad(quad(inside, px(6.), clear, px(2.), drop_ring, solid));
            }
            // An audio track keeps the room of its arm toggle, from 144 pt: its name ends 8 pt
            // before it, and before the meter of its input, from 88 pt, while it is armed.
            let name_width = match (row.kind, row.armed) {
                (TrackKind::Instrument, _) => HEADER_WIDTH - NAME_LEFT - 2. * HEADER_INSET,
                (TrackKind::Audio, false) => ARM_LEFT - 8. - NAME_LEFT,
                (TrackKind::Audio, true) => ARMED_METER_LEFT - 8. - NAME_LEFT,
            };
            // The field over the header shows the name that is being edited.
            let name = match row.renaming {
                true => SharedString::default(),
                false => row.name.clone(),
            };
            let label_size = (NAME_MIDDLE, name_width);
            paint_track_label(name, row.accent, top, label_size, row.muted, window, cx);
            paint_lanes_toggle(row, top, window, cx);
        }
        // The name of each lane, where the name of its track starts.
        for lane in &scene.lanes {
            let top = headers.origin + point(px(0.), px(lane.y.round()));
            let origin = top + point(px(NAME_LEFT), px(LANE_HEIGHT / 2. - 9.));
            let fit = Fit::Truncate(HEADER_WIDTH - NAME_LEFT - 16.);
            let color = lane_text.opacity(if lane.muted { 0.4 } else { 1. });
            let (name, weight) = (lane.name.clone(), FontWeight::NORMAL);
            paint_text(name, origin, 12., weight, color, fit, window, cx);
        }
        // A drop under the last track makes a new audio track, whose header says so.
        if let Some(y) = scene.ghosts.as_ref().and_then(|ghosts| ghosts.new_track) {
            let top = headers.origin + point(px(0.), px(y.round()));
            let ring = Bounds::new(
                top + point(px(DOT_LEFT), px(TRACK_HEIGHT / 2. - 4.)),
                size(px(8.), px(8.)),
            );
            let clear = Hsla::transparent_black();
            window.paint_quad(quad(
                ring,
                px(4.),
                clear,
                px(1.5),
                muted_ring,
                BorderStyle::Solid,
            ));
            let origin = top + point(px(NAME_LEFT), px(TRACK_HEIGHT / 2. - 10.));
            let fit = Fit::Truncate(HEADER_WIDTH - NAME_LEFT - 16.);
            let text = SharedString::from(NEW_AUDIO_TRACK);
            paint_text(
                text,
                origin,
                14.,
                FontWeight::MEDIUM,
                muted_text,
                fit,
                window,
                cx,
            );
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
    // A hairline over each lane, across the header and the timeline.
    let under_ruler_area =
        Bounds::new(headers.origin, size(bounds.size.width, headers.size.height));
    window.with_content_mask(
        Some(ContentMask {
            bounds: under_ruler_area,
        }),
        |window| {
            for lane in &scene.lanes {
                let line = Bounds::new(
                    headers.origin + point(px(0.), px(lane.y.round())),
                    size(bounds.size.width, px(1.)),
                );
                window.paint_quad(fill(line, hairline));
            }
        },
    );

    let assets = scene.assets.clone();
    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for lane in &scene.lanes {
            paint_lane(lane, timeline, window, cx);
        }
        for shape in &scene.clips {
            let body = placed(shape.rect, timeline.origin);
            let dim = if shape.muted { 0.4 } else { 1. };
            match &shape.body {
                Body::Notes(notes) => {
                    let border = if shape.selected {
                        selection.opacity(dim)
                    } else {
                        clip_border.opacity(dim)
                    };
                    let radius = px(6.).min(body.size.width / 2.);
                    let solid = BorderStyle::Solid;
                    let clip_fill = clip_fill.opacity(dim);
                    window.paint_quad(quad(body, radius, clip_fill, px(1.), border, solid));
                    for note in notes {
                        let note = placed(*note, timeline.origin);
                        window.paint_quad(fill(note, shape.accent.opacity(dim)));
                    }
                }
                Body::Audio(audio) => {
                    let look = audio_look(shape, audio, body, timeline.origin, &assets, cx);
                    paint_audio_clip(&look, window, cx);
                }
            }
            if shape.carries {
                paint_automation_mark(body, shape.accent.opacity(dim), window);
            }
        }

        if let Some(ghosts) = &scene.ghosts {
            for (rect, name) in &ghosts.clips {
                let area = placed(*rect, timeline.origin);
                let solid = BorderStyle::Solid;
                // Opaque: the clip it makes covers what it lies over, the newest on top.
                let clear = Hsla::transparent_black();
                window.paint_quad(quad(area, px(6.), window_fill, px(0.), clear, solid));
                window.paint_quad(quad(area, px(6.), clip_fill, px(2.), drop_ring, solid));
                let origin = area.origin + point(px(12.), px(8.));
                let fit = Fit::Truncate((f32::from(area.size.width) - 24.).max(0.));
                let weight = FontWeight::MEDIUM;
                paint_text(
                    name.clone(),
                    origin,
                    14.,
                    weight,
                    ghost_text,
                    fit,
                    window,
                    cx,
                );
            }
        }
        if let Some(marquee) = scene.marquee {
            let area = placed(marquee, timeline.origin);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(
                area,
                px(2.),
                marquee_fill,
                px(1.),
                marquee_border,
                solid,
            ));
        }
        if let Some((x, y)) = scene.hint {
            let origin = timeline.origin + point(px(x.max(4.).round()), px(y.round()));
            paint_hint(AUTOMATION_HINT, origin, window, cx);
        }
    });
}

/// The toggle of the lanes of a track, the second line of its header: a chevron under its dot,
/// to the right while they are folded away and down while they show, and its words under the
/// name. Brighter when the track has lanes, so a folded track says it has automation.
fn paint_lanes_toggle(row: &TrackRow, top: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let color = match row.automated {
        true => theme.gray_800,
        false => theme.gray_700,
    };
    let color = color.opacity(if row.muted { 0.4 } else { 1. });
    let (x, y) = (DOT_LEFT + 4., LANES_MIDDLE);
    let corners = match row.expanded {
        true => [(x - 4., y - 2.), (x, y + 2.), (x + 4., y - 2.)],
        false => [(x - 2., y - 4.), (x + 2., y), (x - 2., y + 4.)],
    };
    paint_polyline(&corners, top, 1.5, color, window);
    let origin = top + point(px(NAME_LEFT), px(LANES_MIDDLE - 9.));
    let fit = Fit::Truncate(LANES_TOGGLE_RIGHT - NAME_LEFT);
    let label = row.lanes_label.clone();
    paint_text(
        label,
        origin,
        12.,
        FontWeight::NORMAL,
        color,
        fit,
        window,
        cx,
    );
}

/// A line through `corners`, from `origin`.
fn paint_polyline(
    corners: &[(f32, f32)],
    origin: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(px(width));
    for (index, (x, y)) in corners.iter().enumerate() {
        let at = origin + point(px(*x), px(*y));
        match index {
            0 => path.move_to(at),
            _ => path.line_to(at),
        }
    }
    // A path that does not tessellate paints nothing, which is all there is to do about it.
    if corners.len() > 1
        && let Ok(path) = path.build()
    {
        window.paint_path(path, color);
    }
}

/// An automation lane in the timeline area: its line in the track colour, as a note is. While
/// clips are dragged, where each lands has a light band, and the line that was there before
/// shows faded under the one it gets.
fn paint_lane(lane: &LaneShape, timeline: Bounds<Pixels>, window: &mut Window, cx: &App) {
    let selection = cx.theme().gray_950;
    let origin = timeline.origin + point(px(0.), px(lane.y));
    // An area takes no track colour: the band is the fill of a marquee.
    let band_fill = cx.theme().alpha_at(0.05);
    let opacity = if lane.muted { 0.4 } else { 1. };
    for (across, replaced) in &lane.ghosts {
        let band = Bounds::new(
            origin + point(px(across.start.round()), px(1.)),
            size(
                px((across.end - across.start).round().max(1.)),
                px(LANE_HEIGHT - 1.),
            ),
        );
        window.paint_quad(fill(band, band_fill));
        let faded = lane.accent.opacity(0.3 * opacity);
        window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
            paint_polyline(replaced, origin, 1.5, faded, window);
        });
    }
    paint_polyline(
        &lane.line,
        origin,
        1.5,
        lane.accent.opacity(opacity),
        window,
    );
    // A dot on each point, bigger under the pointer, and a selected one with the ring of a
    // selected clip.
    let solid = BorderStyle::Solid;
    for dot in &lane.points {
        let radius: f32 = match dot.hovered || dot.selected {
            true => 4.5,
            false => 3.,
        };
        let bounds = Bounds::new(
            origin + point(px(dot.x - radius), px(dot.y - radius)),
            size(px(2. * radius), px(2. * radius)),
        );
        let (ring, edge) = match dot.selected {
            true => (px(1.5), selection),
            false => (px(0.), Hsla::transparent_black()),
        };
        let color = lane.accent.opacity(opacity);
        window.paint_quad(quad(bounds, px(radius), color, ring, edge, solid));
    }
}

/// The mark of a dragged clip whose automation goes along while its lanes are folded away: a
/// small fade in its top right corner.
fn paint_automation_mark(body: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    if body.size.width < px(40.) {
        return;
    }
    let origin = body.top_right();
    let corners = [(-28., 16.), (-20., 16.), (-10., 8.)];
    paint_polyline(&corners, origin, 1.5, color, window);
}

/// A short line of text in a box of the window colour, as a tempo label is, at `origin`.
fn paint_hint(text: &'static str, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let (background, border, color) = (theme.gray_100, theme.alpha_at(0.10), theme.gray_900);
    let run = TextRun {
        len: text.len(),
        font: typography::tabular(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text = SharedString::from(text);
    let shaped = window.text_system().shape_line(text, px(12.), &[run], None);
    let area = Bounds::new(origin, size(shaped.width + px(16.), px(24.)));
    let solid = BorderStyle::Solid;
    window.paint_quad(quad(area, px(6.), background, px(1.), border, solid));
    let at = origin + point(px(8.), px(4.));
    // A glyph that cannot be painted leaves a gap in a hint. Nothing else depends on it.
    if let Err(error) = shaped.paint(at, px(17.), TextAlign::Left, None, window, cx) {
        eprintln!("arrangement view: {error}");
    }
}

/// What an audio clip shows, with the peaks of its columns from the overview of its file. The
/// overview is asked for here, where there is an `App` to start one with, and is not there
/// until it is made on a background thread: the clip draws without its waveform until then.
fn audio_look(
    shape: &ClipShape,
    audio: &AudioShape,
    body: Bounds<Pixels>,
    origin: Point<Pixels>,
    assets: &Assets,
    cx: &mut App,
) -> AudioClipLook {
    let mut look = AudioClipLook::new(body, shape.accent);
    look.selected = shape.selected;
    look.muted = shape.muted;
    look.missing = audio.missing.clone();
    look.label = audio.label.clone();
    look.handles = audio.handles;
    look.gain = audio.gain;
    look.fade_in = audio.fade_in;
    look.fade_out = audio.fade_out;
    if audio.missing.is_some() {
        return look;
    }
    let overview = match &audio.sound {
        Sound::File(asset) => match Waveforms::overview(assets, asset, cx) {
            Some(overview) => Overview::File(overview),
            None => return look,
        },
        Sound::Take(take) => {
            look.recording = true;
            match take {
                Some(take) => Overview::Take(take.clone()),
                None => return look,
            }
        }
    };
    // Every column of one draw at one resolution, from one frame of the file to the next, so
    // together they cover every frame and no click falls between two of them.
    let columns = |first: f32, edges: &[f64]| {
        let peaks = |overview: &sound_media::Overview| {
            let rate = f64::from(overview.sample_rate());
            let frames: Vec<u64> = edges
                .iter()
                .map(|seconds| (seconds * rate).max(0.) as u64)
                .collect();
            overview.peaks(&frames)
        };
        Columns {
            left: origin.x + px(first),
            peaks: match &overview {
                Overview::File(overview) => peaks(overview),
                Overview::Take(take) => take.read(peaks),
            },
        }
    };
    look.waveform = columns(audio.first, &audio.edges);
    look.hidden = audio
        .hidden
        .as_ref()
        .map(|(first, edges)| columns(*first, edges));
    look
}

/// The overview a waveform is drawn from.
enum Overview {
    File(std::sync::Arc<sound_media::Overview>),
    Take(TakeOverview),
}

/// The tempo changes after tick 0 in the ruler: a line at the tick and a label, `140 bpm`, in
/// a box of the window colour that covers the bar numbers under it. On a bar line it starts
/// after the number of the bar. The selected one has a light border, as a selected clip. Gives
/// where each label is across, for the hit test of a press.
fn paint_tempo_marks(
    marks: &[TempoMark],
    bars: &[RulerBar],
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
            let bar = bars.iter().find(|bar| (bar.x.round() - x).abs() < 1.);
            let after = match bar {
                Some(bar) => {
                    let text: SharedString = bar.label().into();
                    let runs = [run(text.len(), unit)];
                    let shaped = window
                        .text_system()
                        .shape_line(text, font_size, &runs, None);
                    8. + f32::from(shaped.width) + 4.
                }
                None => 4.,
            };
            let text: SharedString = format!("{} bpm", mark.text).into();
            let runs = [
                run(mark.text.len(), value),
                run(text.len() - mark.text.len(), unit),
            ];
            let shaped = window
                .text_system()
                .shape_line(text, font_size, &runs, None);
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
