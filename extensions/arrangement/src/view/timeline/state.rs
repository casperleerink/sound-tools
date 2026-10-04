//! What the timeline holds between events: what the mouse drags, a marquee, a rename, and
//! files dragged in from the Finder.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use gpui::{CursorStyle, Entity, FocusHandle, Subscription};
use sound_core::{Instance, InstanceId, Ticks, ValueRange};
use sound_media::Info;
use sound_notes::Clip;
use sound_ui::DragEdit;
use sound_ui::components::text_input::TextInput;

use crate::view::layout::Rows;
use crate::{AudioClip, AutomationLane, TrackKind, TrackState};

#[derive(Clone)]
/// One selected clip during a move: where it is now, the id it had at mouse down, the row of
/// its track then, and where it was.
pub(super) struct MovedClip {
    /// The clip now. Its id changes when the drag takes it to another track.
    pub(super) clip: InstanceId,
    /// A drag that comes back to the first track takes this id again, so a drag there and back
    /// leaves the file where it was.
    pub(super) home: InstanceId,
    /// The kind of track it goes on, which is the kind it is.
    pub(super) kind: TrackKind,
    pub(super) row: usize,
    /// From its start to its end before the move. The length of an audio clip in ticks depends
    /// on the tempo where it is, so it is measured there and not where the drag has it now.
    pub(super) range: Range<Ticks>,
}

/// What a drag of clips does. Each kind holds its own state, which its mouse move is handed.
/// A trim and a fade hold the same.
pub(super) enum ClipDragKind {
    Move(MoveDrag),
    Resize(ResizeDrag),
    /// An edge of an audio clip: the part of its file that plays. Every move starts from
    /// `origin`, and writes only what the edge moves onto the live clip.
    Trim(EdgeDrag),
    /// A fade handle of an audio clip, sideways from where it was.
    Fade(EdgeDrag),
    Gain(GainDrag),
}

/// Every selected clip, by the same distance in time and in rows. A move writes only the
/// start and the track, so it keeps what else changed.
pub(super) struct MoveDrag {
    pub(super) clips: Vec<MovedClip>,
    /// The row of the clip under the pointer at mouse down, and its place in `clips`.
    pub(super) grab_row: usize,
    pub(super) grabbed: usize,
    /// The last distance in rows at which every clip was on a track of its kind. The move
    /// keeps it while the pointer is over a track another clip cannot go on.
    pub(super) rows: i64,
    /// The tracks as they were when the gesture opened. The automation under the clips
    /// moves from here at every mouse move, as the starts do, so the line a clip passes
    /// over comes back when it moves on.
    pub(super) tracks: BTreeMap<InstanceId, TrackState>,
    /// The tracks whose lanes a mouse move of this drag wrote, which go back to how they
    /// were once no clip takes lanes from them or lands on them.
    pub(super) lanes_written: BTreeSet<InstanceId>,
    /// Where the clips that take automation along put it, as the last mouse move wrote it.
    pub(super) ghosts: Vec<LaneGhost>,
}

/// Only a resize keeps a whole clip, because `Clip::set_length` drops notes for good:
/// every move starts from `origin` again, so going in and out loses nothing. One clip.
pub(super) struct ResizeDrag {
    pub(super) clip: Instance<Clip>,
    pub(super) edge: Edge,
    pub(super) origin: Clip,
    pub(super) written: Clip,
    /// The delta of the last move, to skip a move inside the same snap step cheaply.
    pub(super) delta: i64,
}

/// An end of an audio clip, as a trim or a fade drags it from how the clip was.
pub(super) struct EdgeDrag {
    pub(super) clip: Instance<AudioClip>,
    pub(super) edge: Edge,
    pub(super) origin: AudioClip,
    pub(super) file: Info,
}

/// The gain handle of an audio clip, up and down from where it was. Shift pressed or let
/// go goes on from where the gain is then, at the other speed.
pub(super) struct GainDrag {
    pub(super) clip: Instance<AudioClip>,
    pub(super) from_db: f32,
    pub(super) from_y: f32,
    pub(super) fine: bool,
}

/// Where a dragged clip puts the automation it takes along: its track, its place now, and the
/// lanes it carries there.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LaneGhost {
    pub(super) clip: InstanceId,
    pub(super) track: InstanceId,
    pub(super) range: Range<Ticks>,
    pub(super) lanes: Vec<AutomationLane>,
}

#[derive(Copy, Clone)]
pub(super) enum Edge {
    Left,
    Right,
}

/// What every drag of clips keeps, from mouse down to mouse up, whatever its kind.
pub(super) struct ClipDrag {
    /// The tick under the pointer at mouse down.
    pub(super) grab: Ticks,
    /// It opens with the first move that changes something, so a plain click is no undo step.
    pub(super) edit: DragEdit,
    /// What a press that comes up without a move does to the selection, as in the Finder.
    pub(super) on_release: Option<OnRelease>,
}

/// What a click on a clip does when the button comes up without a move.
pub(super) enum OnRelease {
    /// A plain click on one of several selected clips selects it alone.
    SelectAlone(InstanceId),
    /// A cmd-click adds the clip to the selection or takes it out. A cmd press that moves is
    /// a drag without the snap instead, and leaves the selection as it is.
    Toggle(InstanceId),
}

impl ClipDragKind {
    /// The clip under the pointer.
    pub(super) fn grabbed(&self) -> Option<&InstanceId> {
        match self {
            ClipDragKind::Move(MoveDrag { clips, grabbed, .. }) => {
                clips.get(*grabbed).map(|moved| &moved.clip)
            }
            ClipDragKind::Resize(ResizeDrag { clip, .. }) => Some(clip.id()),
            ClipDragKind::Trim(EdgeDrag { clip, .. })
            | ClipDragKind::Fade(EdgeDrag { clip, .. })
            | ClipDragKind::Gain(GainDrag { clip, .. }) => Some(clip.id()),
        }
    }

    /// Whether the drag ends when `id` is deleted: it is the clip under the pointer. The other
    /// clips of a move are left out of the next mouse move when they are gone.
    pub(super) fn ends_without(&self, id: &InstanceId) -> bool {
        self.grabbed() == Some(id)
    }

    /// The cursor while it goes on.
    pub(super) fn cursor(&self) -> Option<CursorStyle> {
        match self {
            ClipDragKind::Move(_) => None,
            ClipDragKind::Resize(_) | ClipDragKind::Trim(_) | ClipDragKind::Fade(_) => {
                Some(CursorStyle::ResizeLeftRight)
            }
            ClipDragKind::Gain(_) => Some(CursorStyle::ResizeUpDown),
        }
    }
}

/// The undo steps of the handles of an audio clip, and of their knobs in the Clip card.
pub(crate) const FADE_IN_LABEL: &str = "Change fade in";
pub(crate) const FADE_OUT_LABEL: &str = "Change fade out";
pub(crate) const GAIN_LABEL: &str = "Change gain";

/// A drag on empty space: the clips it touches are selected. Its corners are a tick and a
/// height from the top of the first track, so a scroll during it keeps its start in place.
pub(super) struct Marquee {
    pub(super) from: (Ticks, f64),
    pub(super) to: (Ticks, f64),
    /// What was selected before, which a drag with shift or cmd adds to.
    pub(super) before: Vec<InstanceId>,
    /// What was selected at the press, with what came first, for escape.
    pub(super) at_press: (Vec<InstanceId>, Option<InstanceId>),
}

/// A drag in an automation lane, from mouse down to mouse up. Every move starts from the lane
/// as it was at mouse down, so a drag there and back ends where it began. One gesture of the
/// session, which opens with the first change.
pub(super) struct LaneDrag {
    pub(super) track: Instance<TrackState>,
    /// The lane at mouse down, with the point a press added, and its place among the lanes of
    /// the track.
    pub(super) origin: AutomationLane,
    pub(super) index: usize,
    /// The top of the lane from the top of the first track, at mouse down.
    pub(super) top: f64,
    pub(super) kind: LaneDragKind,
    /// The undo step it makes.
    pub(super) label: &'static str,
    pub(super) edit: DragEdit,
}

pub(super) enum LaneDragKind {
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

/// A drag of a track header. The track goes to the row under the pointer at once, so the rows
/// themselves show where it lands. The whole drag is one gesture of the session.
pub(super) struct TrackDrag {
    pub(super) track: InstanceId,
    /// The tracks as people saw them when the gesture opened, with their orders then. Every
    /// move starts from here, so a drag back to where it began writes nothing and is no undo
    /// step.
    pub(super) origin: Vec<(Instance<TrackState>, u32)>,
    /// The rows at mouse down, which the pointer is hit against. The rows of now have the
    /// track where the last move put it, so with lanes of their own height a pointer that
    /// stands still would move it back.
    pub(super) rows: Rows,
    pub(super) from: usize,
    /// The place the last move gave the track, so a move inside one row publishes nothing.
    pub(super) at: usize,
    /// Where the press was, from the top of the first track.
    pub(super) press: f64,
    /// Past [`TRACK_DRAG_THRESHOLD`], so a click, or the first press of a double click, that
    /// shakes a little is still a click.
    pub(super) moving: bool,
    /// It opens with the first move to another row, so a click on a header is no undo step.
    pub(super) edit: DragEdit,
}

/// The undo step of a track drag and of alt-up and alt-down on a track.
pub(super) const MOVE_TRACK_LABEL: &str = "Move track";
/// How far a press on a header moves before it drags the track, in points.
pub(super) const TRACK_DRAG_THRESHOLD: f64 = 4.;

/// What the mouse holds, from mouse down to mouse up. A press takes one thing, so the mouse
/// never holds two. A timeline has one, so the size of the largest does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Default)]
pub(super) enum Held {
    #[default]
    Nothing,
    Clips(ClipDrag, ClipDragKind),
    Marquee(Marquee),
    Track(TrackDrag),
    Lane(LaneDrag),
}

impl Held {
    /// The gesture of the session it may have opened, which then ends with it.
    pub(super) fn edit(&mut self) -> Option<&mut DragEdit> {
        match self {
            Held::Nothing | Held::Marquee(_) => None,
            Held::Clips(drag, _) => Some(&mut drag.edit),
            Held::Track(drag) => Some(&mut drag.edit),
            Held::Lane(drag) => Some(&mut drag.edit),
        }
    }

    /// What the drag of clips does, while that is what the mouse holds.
    pub(super) fn clip_drag(&self) -> Option<&ClipDragKind> {
        match self {
            Held::Clips(_, kind) => Some(kind),
            _ => None,
        }
    }
}

/// What a mouse move says of what the mouse holds: it goes on, or it ends as on mouse up.
#[must_use]
pub(super) enum After {
    Keep,
    End,
}

/// The name of a track while it is being edited in its header.
pub(super) struct Rename {
    pub(super) track: Instance<TrackState>,
    pub(super) input: Entity<TextInput>,
    /// Kept apart from the field, because the field is being updated when it submits.
    pub(super) focus: FocusHandle,
    /// A click anywhere else finishes the edit, as in the Finder.
    pub(super) _blur: Subscription,
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
pub(super) struct Incoming {
    /// The files, as the drag says them. Filled in while the drag is drawn over the timeline.
    pub(super) paths: Vec<PathBuf>,
    /// What each file is, once it is read on a background thread: its length for the ghost.
    /// `None` until then, and for a file that is no audio.
    pub(super) files: Vec<Option<Info>>,
    /// Where they would go, from the last move of the pointer.
    pub(super) target: Option<DropTarget>,
}
