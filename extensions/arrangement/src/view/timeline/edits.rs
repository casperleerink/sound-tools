//! Edits of the project that need no view: moving clips between tracks, with the automation
//! they take along.

use std::collections::BTreeMap;
use std::ops::Range;

use sound_core::{Changes, Instance, InstanceId, Project, ProjectError, State, Ticks};
use sound_media::{AudioAsset, Cached, Info};
use sound_notes::Clip;

use crate::view::clips::AnyClip;
use crate::{
    AudioClip, AutomationLane, FreeIds, LaneMove, TrackKind, TrackState, Travel, automatable,
    automation, moved, top_layer, travel_in, unnumbered,
};

/// One clip of a move to another place: the clip now, the id it had when the move began and
/// where it was on the track of that id, the track it goes to and what it becomes there.
pub(super) struct ClipMove {
    pub(super) clip: InstanceId,
    pub(super) home: InstanceId,
    pub(super) was: Range<Ticks>,
    pub(super) to: Instance<TrackState>,
    pub(super) next: AnyClip,
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
pub(super) fn change_lanes(
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
pub(super) fn move_lanes(
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
pub(super) fn free_lanes(
    project: &Project,
    track: &InstanceId,
    state: &TrackState,
) -> Vec<AutomationLane> {
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
pub(super) fn track_states(
    project: &Project,
    arrangement: &InstanceId,
) -> BTreeMap<InstanceId, TrackState> {
    let tracks = project.children::<TrackState>(arrangement);
    let tracks = tracks.map(|(track, state)| (track.id().clone(), state.clone()));
    tracks.collect()
}

/// Where a clip is on the timeline now.
pub(super) fn range_of(project: &Project, clip: &AnyClip) -> Range<Ticks> {
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
pub(super) fn move_clips(
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
pub(super) fn known_file(project: &Project, asset: &AudioAsset) -> Option<Info> {
    match sound_media::cached(project.assets(), asset) {
        Cached::Plays(file) => Some(file),
        Cached::DoesNotPlay(_) | Cached::Missing | Cached::Unknown => None,
    }
}

/// Whether an id is a clip of either kind.
pub(super) fn is_clip_tool(project: &Project, id: &InstanceId) -> bool {
    project
        .tool_of(id)
        .is_some_and(|tool| tool == Clip::TOOL || tool == AudioClip::TOOL)
}

/// Why a clip cannot go where a paste would put it.
pub(super) fn wrong_track(track: &InstanceId, name: &str, kind: TrackKind) -> ProjectError {
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
