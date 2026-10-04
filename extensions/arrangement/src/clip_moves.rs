//! Moving clips, in time and between tracks, with the automation they take along. No view:
//! the timeline says which clips go where, and these functions write what the project becomes.
//!
//! A note clip and an audio clip move the same way, so a move holds an [`AnyClip`]. What differs
//! is here too: an audio clip has no length of its own, because it plays at the speed of its
//! file, so where it ends comes from its file and the tempo; and each kind goes on its own kind
//! of track only.

use std::collections::BTreeMap;
use std::ops::Range;

use sound_core::{Changes, Instance, InstanceId, Project, ProjectError, Ticks};
use sound_media::Cached;
use sound_notes::Clip;

use crate::{
    AudioClip, FreeIds, LaneMove, TrackKind, TrackState, Travel, automation, moved, top_layer,
    travel_in, unnumbered,
};

/// A clip of either kind, by value.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyClip {
    Notes(Clip),
    Audio(AudioClip),
}

impl AnyClip {
    /// The clip the project has at `id`, of either kind.
    pub fn read(project: &Project, id: &InstanceId) -> Option<Self> {
        if let Some(clip) = project.resolve::<Clip>(id) {
            return project.state(&clip).cloned().map(Self::Notes);
        }
        let clip = project.resolve::<AudioClip>(id)?;
        project.state(&clip).cloned().map(Self::Audio)
    }

    pub fn start(&self) -> Ticks {
        match self {
            Self::Notes(clip) => clip.start,
            Self::Audio(clip) => clip.start,
        }
    }

    pub fn with_start(self, start: Ticks) -> Self {
        match self {
            Self::Notes(clip) => Self::Notes(Clip { start, ..clip }),
            Self::Audio(clip) => Self::Audio(AudioClip { start, ..clip }),
        }
    }

    /// The kind of track that plays it.
    pub fn kind(&self) -> TrackKind {
        match self {
            Self::Notes(_) => TrackKind::Instrument,
            Self::Audio(_) => TrackKind::Audio,
        }
    }

    /// Where it ends on the timeline, see [`shown_end`] for an audio clip.
    pub fn end(&self, project: &Project) -> Ticks {
        match self {
            Self::Notes(clip) => clip.end(),
            Self::Audio(clip) => shown_end(project, clip),
        }
    }

    /// Puts it at `id`, a new clip or over the one that is there.
    pub fn write(self, changes: &mut Changes, id: InstanceId) {
        match self {
            Self::Notes(clip) => {
                changes.create(id, clip);
            }
            Self::Audio(clip) => {
                changes.create(id, clip);
            }
        }
    }
}

/// Where an audio clip ends on the timeline. It plays at the speed of its file, so this depends
/// on the file and the tempo. When the file is not there the clip still needs a place to be seen,
/// selected and deleted: as long as its trim says, or one bar when it plays to the end of a file
/// nobody can measure. The same while nothing knows yet what the file is: this never looks at
/// the disk, because the thread that draws calls it, see [`sound_media::cached`].
pub fn shown_end(project: &Project, clip: &AudioClip) -> Ticks {
    let clock = project.clock();
    if let Cached::Plays(file) = sound_media::cached(project.assets(), &clip.asset) {
        return clip.end(Some(&file), clock).max(clip.start + Ticks(1));
    }
    match clip.file_end_seconds {
        Some(end) => {
            let seconds = clock.seconds_of(clip.start) + (end - clip.file_start_seconds);
            clock.tick_at_seconds(seconds).max(clip.start + Ticks(1))
        }
        None => {
            let time_signatures = project.project_file().tempo_map.time_signatures();
            clip.start + time_signatures.bar_at(clip.start).length()
        }
    }
}

/// One clip of a move to another place: the clip now, the id it had when the move began and
/// where it was on the track of that id, the track it goes to and what it becomes there.
pub struct ClipMove {
    pub clip: InstanceId,
    pub home: InstanceId,
    pub was: Range<Ticks>,
    pub to: Instance<TrackState>,
    pub next: AnyClip,
}

/// Moves clips of `arrangement` with the automation under them, in one group of changes: what
/// a nudge writes. See [`move_records`] for the ids the clips get, which this gives back.
pub fn move_clips(
    project: &Project,
    changes: &mut Changes,
    arrangement: &InstanceId,
    moves: Vec<ClipMove>,
) -> Result<Vec<InstanceId>, ProjectError> {
    move_lanes(project, changes, arrangement, &moves);
    move_records(project, changes, moves)
}

/// What `moves` are for the automation they take along, see [`moved`].
pub(crate) fn lane_moves(moves: &[ClipMove]) -> Vec<LaneMove> {
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
pub(crate) fn change_lanes(
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

/// The records of the tracks of an arrangement, by id.
pub(crate) fn track_states(
    project: &Project,
    arrangement: &InstanceId,
) -> BTreeMap<InstanceId, TrackState> {
    let tracks = project.children::<TrackState>(arrangement);
    let tracks = tracks.map(|(track, state)| (track.id().clone(), state.clone()));
    tracks.collect()
}

/// Where a clip is on the timeline now.
pub(crate) fn range_of(project: &Project, clip: &AnyClip) -> Range<Ticks> {
    clip.start()..clip.end(project)
}

/// The clips of a move alone, to a group of changes: a drag writes the lanes itself, from the
/// tracks as they were when it began. A clip that stays on its track gets its new record. One
/// that goes to another track is a delete and a create, like moving a file: back on the track of
/// its `home` it takes that id again, elsewhere its name without a number at its end, or the
/// next free one. So `clip` moved down onto a track that has a `clip` is `clip-2` there, and
/// `clip` again when it comes back up. Gives the ids of the clips after the move, in the order
/// of `moves`.
///
/// A moved audio clip goes on top of the clips of its track, as a new one does, so where it
/// overlaps them it is heard. Moved together, they keep their order among themselves.
pub(crate) fn move_records(
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
