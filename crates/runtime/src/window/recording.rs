//! Where a keyboard plays, and what a finished take becomes.
//!
//! This is the whole meeting point of MIDI input and the arrangement. The MIDI extension knows
//! nothing of tracks and the arrangement nothing of MIDI: they share the note contract, and
//! the window puts the two together here, as it does for views.

use anyhow::Result;
use arrangement::{AudioClip, TrackKind, TrackState};
use midi::{Played, Take};
use sound_core::{Changes, Clock, InputEndpoint, Instance, InstanceId, Project, ProjectError};
use sound_notes::NOTES_INPUT;
use sound_typescript::Midi;

use crate::main_arrangement;

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

/// The `notes` port of the instrument of that track. `None` while the track has none, or while
/// its instrument is a tool without the ports of the note contract.
pub fn notes_input(project: &Project, track: &Instance<TrackState>) -> Option<InputEndpoint> {
    let instrument = track.id().child(arrangement::INSTRUMENT).ok()?;
    project.input_port(&instrument, NOTES_INPUT)
}

/// Where the live input goes now. Read it after every change: the port moves when the
/// instrument is built again.
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

/// What a tool of the project hears of a message of the MIDI keyboard. The pedal and the mod
/// wheel are the controllers they are; the key pressure is left out.
pub fn tool_message(played: Played) -> Option<Midi> {
    let fraction = |value: u8| f32::from(value) / 127.0;
    Some(match played {
        Played::On { pitch, velocity } => Midi::NoteOn {
            pitch: pitch.number(),
            velocity: fraction(velocity.value()),
        },
        Played::Off { pitch, .. } => Midi::NoteOff {
            pitch: pitch.number(),
        },
        Played::Pedal(pedal) => Midi::Cc {
            controller: SUSTAIN_PEDAL,
            value: fraction(pedal.value()),
        },
        Played::ModWheel(amount) => Midi::Cc {
            controller: MOD_WHEEL,
            value: amount.fraction(),
        },
        Played::Control { controller, value } => Midi::Cc {
            controller,
            value: fraction(value),
        },
        Played::Bend(bend) => Midi::Bend {
            value: bend.fraction(),
        },
        Played::Pressure(_) => return None,
    })
}

/// The MIDI controllers of the sustain pedal and the mod wheel.
const SUSTAIN_PEDAL: u8 = 64;
const MOD_WHEEL: u8 = 1;

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
