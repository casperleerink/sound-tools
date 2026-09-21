//! Where a keyboard plays, and what a finished take becomes.
//!
//! This is the whole meeting point of MIDI input and the arrangement. The MIDI extension knows
//! nothing of tracks and the arrangement nothing of MIDI: they share the note contract, and
//! the window puts the two together here, as it does for views.

use anyhow::Result;
use arrangement::TrackState;
use midi::Take;
use sound_core::{InputEndpoint, Instance, InstanceId, Project, ProjectError};
use sound_notes::NOTES_INPUT;

use crate::main_arrangement;

/// The label of the undo step a recording makes.
pub const LABEL: &str = "Record";

/// The name a recorded clip gets: `take`, then `take-2`, `take-3`.
const CLIP_NAME: &str = "take";

/// The track a keyboard plays into and a recording is written to: the selected track, or the
/// first track of the arrangement when nothing is selected, so a keyboard always sounds.
pub fn target_track(
    project: &Project,
    selected: Option<&InstanceId>,
) -> Option<Instance<TrackState>> {
    let selected = selected.and_then(|id| project.resolve::<TrackState>(id));
    if selected.is_some() {
        return selected;
    }
    let arrangement = main_arrangement(project)?;
    let tracks = arrangement::tracks(project, arrangement.id());
    tracks.into_iter().next().map(|(track, _)| track)
}

/// The `notes` port of the instrument of that track. `None` while the track has none, or while
/// its instrument is a tool without the ports of the note contract.
pub fn notes_input(project: &Project, track: &Instance<TrackState>) -> Option<InputEndpoint> {
    let instrument = track.id().child(arrangement::INSTRUMENT).ok()?;
    project.input_port(&instrument, NOTES_INPUT)
}

/// Where the live input goes now. Read it after every change: the port moves when the
/// instrument is built again.
pub fn live_notes_input(project: &Project, selected: Option<&InstanceId>) -> Option<InputEndpoint> {
    let track = target_track(project, selected)?;
    notes_input(project, &track)
}

/// Adds the clip of a finished take to the track, as one undo step. `take_name` is the raw
/// take the clip came from, which is already on disk, or `None` when writing it failed.
pub fn add_take_clip(
    project: &mut Project,
    track: &Instance<TrackState>,
    take: &Take,
    take_name: Option<String>,
) -> Result<Option<InstanceId>, ProjectError> {
    let Some(mut clip) = take.clip() else {
        return Ok(None);
    };
    // A clip never names a take that is not there: a failed write leaves the field out.
    clip.take = take_name;
    let mut changes = sound_core::Changes::new();
    let clip = arrangement::add_clip(project, &mut changes, track, CLIP_NAME, clip)?;
    let id = clip.id().clone();
    project.commit(LABEL, changes)?;
    Ok(Some(id))
}

/// Writes the raw take under a name of its own and gives that name, for the clip to keep.
///
/// It runs before the clip is made and whatever happens to the clip, because the take is the
/// only copy of what the composer played. The name is never one that was used before, and the
/// file is created and never opened again, so no take can be written over.
pub fn write_take(project: &Project, take: &Take) -> Result<String> {
    Ok(take.write(project.assets())?)
}
