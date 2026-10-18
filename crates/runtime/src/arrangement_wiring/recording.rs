//! Where a keyboard plays, and what a finished take becomes.
//!
//! This is the whole meeting point of MIDI input and the arrangement. The MIDI extension knows
//! nothing of tracks and the arrangement nothing of MIDI: they share the note contract, and
//! the runtime puts the two together here, as it does for views. So does a finished audio take,
//! which the recorder places in time and this makes a clip.

use anyhow::Result;
use arrangement::{AudioClip, TrackKind, TrackState};
use midi::Take;
use sound_core::{
    Changes, Clock, Frames, InputEndpoint, Instance, InstanceId, Project, ProjectError,
    StreamTiming, Ticks,
};
use sound_media::{AudioAsset, Imported, Info};
use sound_notes::NOTES_INPUT;

use super::main_arrangement;
use crate::recorder::{Placement, take_seconds_at};

/// The label of the undo step a recording makes.
pub const LABEL: &str = "Record";

/// The name a recorded clip gets: `take`, then `take-2`, `take-3`.
const CLIP_NAME: &str = "take";

/// The track a keyboard plays into and a recording is written to: the selected track, or the
/// first track of the arrangement when nothing is selected, so a keyboard always sounds. An
/// audio track plays no notes, so it is never the one: with one selected, nothing is.
pub fn target_track(
    project: &Project,
    selected: Option<&InstanceId>,
) -> Option<Instance<TrackState>> {
    let plays_notes = |track: &Instance<TrackState>| {
        let state = project.state(track);
        state.is_some_and(|state| state.kind == TrackKind::Instrument)
    };
    if let Some(selected) = selected.and_then(|id| project.resolve::<TrackState>(id)) {
        return plays_notes(&selected).then_some(selected);
    }
    let arrangement = main_arrangement(project)?;
    let tracks = arrangement::tracks(project, arrangement.id());
    let mut tracks = tracks.into_iter().map(|(track, _)| track);
    tracks.find(plays_notes)
}

/// The `notes` input of that track, which plays each of its devices that takes notes.
pub fn notes_input(project: &Project, track: &Instance<TrackState>) -> Option<InputEndpoint> {
    project.input_port(track.id(), NOTES_INPUT)
}

/// Where the live input goes now. Read it after every change: the port moves when an instance
/// is built again.
///
/// A project with no track that plays notes, such as one experiment at its top, gives the
/// keyboard to the first instance at the top that takes notes.
pub fn live_notes_input(project: &Project, selected: Option<&InstanceId>) -> Option<InputEndpoint> {
    if let Some(track) = target_track(project, selected) {
        return notes_input(project, &track);
    }
    let mut top = project.instances().filter(|(id, _)| id.parent().is_none());
    top.find_map(|(id, _)| project.input_port(id, NOTES_INPUT))
}

/// Adds the clip of a finished MIDI take to its track, to a group of changes. `take_name` is
/// the raw take the clip came from, which is already on disk, or `None` when writing it failed.
/// Gives the id of the clip; a take with no notes adds nothing.
/// `clock` is the one the take was played under: a change of the tempo map ends a take, so its
/// clip is placed under the tempo it was heard at.
pub fn add_take_clip(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    (take, take_name): (&Take, Option<String>),
    clock: &Clock,
) -> Result<Option<InstanceId>, ProjectError> {
    let Some(mut clip) = take.clip(clock) else {
        return Ok(None);
    };
    // A clip never names a take that is not there: a failed write leaves the field out.
    clip.take = take_name;
    let clip = arrangement::add_clip(project, changes, track, CLIP_NAME, clip)?;
    Ok(Some(clip.id().clone()))
}

/// Writes the raw take under a name of its own and gives that name, for the clip to keep.
///
/// It runs before the clip is made and whatever happens to the clip, because the take is the
/// only copy of what the composer played. The name is never one that was used before, and the
/// file is created and never opened again, so no take can be written over.
pub fn write_take(project: &Project, take: &Take) -> Result<String> {
    Ok(take.raw(project.clock()).write(project.assets())?)
}

/// Adds the clip of each audio take to its track, to a group of changes, over every clip the
/// track has. The clip is named after its file. A track that went away while it recorded gets
/// nothing: its file stays in `assets/audio/`, as every asset does.
pub fn add_audio_take_clips(
    project: &Project,
    changes: &mut Changes,
    clips: Vec<(InstanceId, AudioClip)>,
) -> Result<(), ProjectError> {
    // The tracks and names are lent to the arrangement and the clips handed over, so no clip
    // is copied.
    let (named, clips): (Vec<_>, Vec<_>) = clips
        .into_iter()
        .filter_map(|(track, clip)| {
            let track = project.resolve::<TrackState>(&track)?;
            let name = clip.asset.asset_name().name().to_string();
            Some(((track, name), clip))
        })
        .unzip();
    let placed = named
        .iter()
        .zip(clips)
        .map(|((track, name), clip)| (track, name.as_str(), clip));
    arrangement::add_audio_clips(project, changes, placed)?;
    Ok(())
}

/// The clip of a take: where the composer heard it, from `start`, the playhead the recording
/// began at, up to `end`, where it ended. What the file holds before `start` and after `end`
/// is trimmed away, so the clip plays what was heard in the recording and nothing else.
/// `None` when the file holds nothing of that stretch.
///
/// A take that began before `start` was heard starts at `start`. One whose first frame came
/// later starts at the first tick of its sound. Either way the frame of the file that the
/// clip plays at its start is the one captured when that frame was heard.
pub fn take_clip(
    asset: AudioAsset,
    file: &Info,
    head: i128,
    (start, end): (Ticks, Ticks),
    clock: &Clock,
) -> Option<AudioClip> {
    let start_frame = i128::from(clock.frame_of(start).0);
    let start = match head <= start_frame {
        true => start,
        false => clock.tick_at(Frames(u64::try_from(head).ok()?)),
    };
    let file_start = take_seconds_at(head, clock.frame_of(start), clock);
    let file_end = take_seconds_at(head, clock.frame_of(end), clock);
    let seconds = file.seconds();
    if start >= end || file_start >= seconds.min(file_end) {
        return None;
    }
    let mut clip = AudioClip::new(asset, start);
    clip.file_start_seconds = file_start;
    clip.file_end_seconds = (file_end < seconds).then_some(file_end);
    Some(clip)
}

/// The clip of each finished take of a recording placed by `placement`, up to `end`. A take
/// that holds nothing heard in the recording gets none, and neither does any while the
/// recording is not tied to the timeline, which takes two polls of the engine while it plays.
pub fn take_clips(
    placement: &Placement,
    takes: &[(InstanceId, Imported)],
    end: Ticks,
    timing: Option<&StreamTiming>,
) -> Vec<(InstanceId, AudioClip)> {
    let clock = placement.clock();
    let Some(head) = placement.head(timing) else {
        return Vec::new();
    };
    let start = placement.start;
    let clips = takes.iter().filter_map(|(track, take)| {
        let file = take.audio.info();
        let clip = take_clip(take.asset.clone(), &file, head, (start, end), clock)?;
        Some((track.clone(), clip))
    });
    clips.collect()
}
