//! Arrangement: tracks, clips and notes on the project timeline.
//!
//! Three tools. An `arrangement` owns tracks and is the master: it mixes every track, solo
//! included, and plays the sum through its volume and limiter to the main output. An
//! `arrangement.track` owns clips, one instrument, the child named `instrument`, and its
//! effects, and plays the clips through them to its arrangement. An `arrangement.clip` is plain
//! data: the [`Clip`] of the note contract crate.
//!
//! ```text
//! state/arrangement/instance.json            the arrangement
//! state/arrangement/piano/instance.json      a track
//! state/arrangement/piano/instrument.json    its instrument: any tool with the ports of the note contract
//! state/arrangement/piano/verse-a.json       a clip
//! ```
//!
//! `agent-doc.md` in this crate has the record formats. `README.md` is for extension and
//! interface authors. The interface is in [`view`]. Nothing else here uses GPUI.

pub mod decibels;
mod master;
mod mixer;
mod sequencer;
mod slot;
mod summary;
pub mod view;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, Changes, InputEndpoint, Instance, InstanceId,
    OutputEndpoint, Peaks, Place, Project, ProjectError, Registry, RegistryError, State, Ticks,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, Clip, NOTES_INPUT, Pitch, TRACK_TOOL, Velocity};

use master::Master;
pub use master::{LimiterState, MasterState};
pub use mixer::{ChannelGains, Mixer, RAMP_SECONDS, channel_gains};
pub use sequencer::{HELD_CAPACITY, PREVIEW_SECONDS, Sequencer, SequencerUpdate, TrackSnapshot};
pub use slot::EffectSlot;

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "arrangement";

/// The name of the child of a track that plays its notes.
pub const INSTRUMENT: &str = "instrument";

/// The name of the processor of a track that plays its clips.
const SEQUENCER: &str = "sequencer";
/// The processors of an arrangement: the mixer of each track, by the name of the track after
/// this, and the master.
const MIXER: &str = "mixer/";
const MASTER: &str = "master";
/// The peaks an arrangement keeps: of each track, by its name after this, of the master, and
/// the reduction of the limiter. No child name has a `/`, so no track takes the name of the
/// master.
const TRACK_PEAKS: &str = "track/";
const MASTER_PEAKS: &str = "master";
const REDUCTION_PEAKS: &str = "reduction";

/// The id of the arrangement in the default project.
pub const DEFAULT_ARRANGEMENT: &str = "arrangement";

/// The root of a piece: the owner of the tracks, and their master.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangementState {
    /// The volume and the limiter of the sum of every track. A record that leaves it out, as
    /// every record of before it did, gets 0 dB and the limiter on.
    #[serde(default)]
    pub master: MasterState,
}

impl State for ArrangementState {
    const TOOL: &'static str = "arrangement";
    const OWNS_CHILDREN: bool = true;
    const PLACE: Place = Place::Root;

    fn validate(&self) -> Result<(), String> {
        self.master.validate()
    }
}

/// One list gives each colour its variant and its name, so the name in records, the name in
/// messages and the design token cannot drift apart.
macro_rules! colours {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// The accents of the DESIGN.md palette. A track shows its colour on dots and clip edges.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Colour {
            $(#[serde(rename = $name)] $variant,)+
        }

        impl Colour {
            /// Every colour, in the order of the palette.
            pub const ALL: &'static [Colour] = &[$(Self::$variant,)+];

            /// The name in records and in the design tokens.
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }
    };
}

colours! {
    Blue => "blue",
    Sapphire => "sapphire",
    Sky => "sky",
    Teal => "teal",
    Green => "green",
    Yellow => "yellow",
    Peach => "peach",
    Red => "red",
    Maroon => "maroon",
    Mauve => "mauve",
    Pink => "pink",
    Lavender => "lavender",
    Rosewater => "rosewater",
    Flamingo => "flamingo",
}

/// The colour of a track record that names none.
impl Default for Colour {
    fn default() -> Self {
        Self::Blue
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackState {
    /// What people see. The id of the track is its folder name and never changes.
    pub name: String,
    #[serde(default)]
    pub colour: Colour,
    /// Tracks show from the lowest order to the highest. Tracks with the same order show in
    /// the order of their ids.
    #[serde(default)]
    pub order: u32,
    /// How much louder or quieter the track plays, in decibels. 0 is the sound of its
    /// instrument, and a record that leaves it out gets that. `-inf`, saved as `"-inf"`, is
    /// silence.
    #[serde(default, with = "decibels")]
    pub gain_db: f32,
    /// Where the track sits between the two channels: -1 hard left, 0 the middle, 1 hard right.
    #[serde(default)]
    pub pan: f32,
    /// A muted track is silent and keeps everything else as it is.
    #[serde(default)]
    pub mute: bool,
    /// While any track of the arrangement is soloed, only the soloed ones play: every other
    /// one sounds as if it were muted. Left out when off, so a track of before it gives the
    /// same bytes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    /// The effects of the track, by the name of the child that holds each one, in the order
    /// the sound goes through them: instrument, then these, then the gain, pan and mute. Each
    /// slot may be bypassed.
    ///
    /// One place decides the order, so a reorder is one record and one undo step. A record
    /// that leaves the field out has no effects and is written back without it, so a track of
    /// before effects existed loads unchanged and gives the same bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectSlot>,
}

impl TrackState {
    /// The highest gain in decibels. Anything under it is a gain too, down to `-inf`. Written
    /// here and nowhere else: `validate`, the volume of the track panel and the docs read it.
    pub const MAX_GAIN_DB: f32 = 6.0;
    /// Hard left to hard right.
    pub const PAN: (f32, f32) = (-1.0, 1.0);

    /// A track of this name, with the mixer where a record that says nothing puts it.
    pub fn new(name: impl Into<String>, colour: Colour, order: u32) -> Self {
        Self {
            name: name.into(),
            colour,
            order,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            effects: Vec::new(),
        }
    }

    /// Whether the effect in the child `name` is bypassed. `None` when the list does not
    /// name it.
    pub fn bypassed(&self, name: &str) -> Option<bool> {
        let slot = self.effects.iter().find(|slot| slot.name == name)?;
        Some(slot.bypass)
    }
}

/// One number of a record against its range, with a message an agent can act on.
fn in_range(field: &str, value: f32, (min, max): (f32, f32)) -> Result<(), String> {
    if (min..=max).contains(&value) {
        return Ok(());
    }
    Err(format!("{field} must be from {min} to {max}, not {value}"))
}

impl State for TrackState {
    const TOOL: &'static str = TRACK_TOOL;
    const OWNS_CHILDREN: bool = true;
    /// Only an arrangement shows tracks. Anywhere else a track would play and be hidden.
    const PLACE: Place = Place::In(ArrangementState::TOOL);

    fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        decibels::check("gain_db", self.gain_db, Self::MAX_GAIN_DB)?;
        in_range("pan", self.pan, Self::PAN)?;
        // A name in the list is the name of a file in the track folder, and the list decides an
        // order. A name that is not a child name, a name twice and the name of the instrument
        // are all lists with no order to read, so they are refused here and the agent is told
        // where the mistake is. A name with no record is not: that file may still arrive, and
        // the behaviour reports it.
        for (index, EffectSlot { name, .. }) in self.effects.iter().enumerate() {
            if !is_child_name(name) {
                return Err(format!(
                    "effects[{index}] must be the name of a file in the track folder without `.json`: lowercase letters, digits, `-` and `_`, not {name:?}"
                ));
            }
            if name == INSTRUMENT {
                return Err(format!(
                    "effects[{index}] must not be {INSTRUMENT:?}: the instrument of a track is its own slot and plays before every effect"
                ));
            }
            if self.effects[..index].iter().any(|slot| slot.name == *name) {
                return Err(format!(
                    "effects[{index}] is {name:?}, which the list already has. One effect is one child record: copy the file under another name to use it twice"
                ));
            }
        }
        Ok(())
    }
}

/// Whether `name` can be the name of a child record, which is the rule for an instance name.
fn is_child_name(name: &str) -> bool {
    !name.is_empty()
        && name != "instance"
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || "-_".contains(character)
        })
}

/// The doc an agent opens for anything musical: the records of the three tools and how to add,
/// move and delete a part or a track.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "arrangement",
    when: "You add, change, move or delete music: tracks, clips and notes",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the three tools and the agent doc. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<ArrangementState>(EXTENSION)?
        .behaviour(apply_arrangement)
        .summary(summary::of_arrangement)
        .end(|project, arrangement| end(project, arrangement.id()));
    registry
        .tool::<TrackState>(EXTENSION)?
        .behaviour(apply_track);
    registry.tool::<Clip>(EXTENSION)?;
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs when the track record or anything the track owns changes: one snapshot of all its
/// clips goes to its one sequencer, which keeps its held notes. The instrument and the effects
/// are found by the port names of the note contract, so any tool with those ports fits.
///
/// The path of a track is sequencer, instrument, the effects in the order of the record that
/// are not bypassed, and out through its `audio` output, which its arrangement mixes. Every
/// processor keeps what it holds: a note goes on sounding through an edit of a clip.
fn apply_track(
    track: &TrackState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let snapshot = TrackSnapshot::new(context.children::<Clip>().map(|(_, clip)| clip));
    let sequencer = context.processor(SEQUENCER, Sequencer::default)?;
    context.update(sequencer, SequencerUpdate::Snapshot(Arc::new(snapshot)))?;
    if let Some(notes) = context.child_input(INSTRUMENT, NOTES_INPUT) {
        context.connect(OutputEndpoint::new(sequencer, Sequencer::NOTES).to(notes))?;
    }

    // The chain, from the instrument through the effects. A slot that is not there is
    // reported and left out, so the sound goes on through the rest of the chain. A bypassed
    // slot is left out too: the sound goes past it untouched, and its latency with it.
    let mut sound = context.child_output(INSTRUMENT, AUDIO_OUTPUT);
    for EffectSlot { name, bypass } in &track.effects {
        let ports = context
            .child_input(name, AUDIO_INPUT)
            .zip(context.child_output(name, AUDIO_OUTPUT));
        let Some((input, output)) = ports else {
            let message = missing_effect(context, name);
            context.problem(message);
            continue;
        };
        if *bypass {
            continue;
        }
        if let Some(sound) = sound {
            context.connect(sound.to(input))?;
        }
        sound = Some(output);
    }
    if let Some(sound) = sound {
        context.output(AUDIO_OUTPUT, sound);
    }
    for name in unlisted_effects(track, context) {
        context.problem(format!(
            "the child {name:?} takes audio in and makes audio out, and the `effects` list of this track does not name it, so nothing goes through it. Add {name:?} to `effects` where you want it in the chain, or delete the file"
        ));
    }
    Ok(())
}

/// Runs when the arrangement record or anything below it changes: the mixer of every track,
/// with solo worked out across the tracks, into the master, and the master to the main output.
///
/// Solo is decided here and not in a track, because it is about all of them. While any track is
/// soloed, every track that is not gets the gains of a muted one, so soloing a track sounds
/// exactly as muting every other one does.
fn apply_arrangement(
    arrangement: &ArrangementState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    // Only what the mixers need, and no copy of a record: this runs on every edit below the
    // arrangement, a mouse move of a clip drag included.
    let soloing = context
        .children::<TrackState>()
        .any(|(_, track)| track.solo);
    let tracks: Vec<(String, ChannelGains)> = context
        .children::<TrackState>()
        .map(|(name, track)| {
            let gains = match soloing && !track.solo {
                true => [0.0; sound_core::CHANNELS],
                false => channel_gains(track),
            };
            (name.to_string(), gains)
        })
        .collect();

    let settings = arrangement.master.settings();
    let (peaks, reduction) = (context.peaks(MASTER_PEAKS), context.peaks(REDUCTION_PEAKS));
    let master = context.processor(MASTER, || Master::new(settings, peaks, reduction))?;
    context.update(master, settings)?;

    for (name, gains) in tracks {
        let peaks = context.peaks(&format!("{TRACK_PEAKS}{name}"));
        let mixer = context.processor(&format!("{MIXER}{name}"), || Mixer::new(gains, peaks))?;
        context.update(mixer, gains)?;
        if let Some(sound) = context.child_output(&name, AUDIO_OUTPUT) {
            context.connect(sound.to(InputEndpoint::new(mixer, Mixer::INPUT)))?;
        }
        let into_master = InputEndpoint::new(master, Master::INPUT);
        context.connect(OutputEndpoint::new(mixer, Mixer::OUTPUT).to(into_master))?;
    }

    // The main output, for now: the stereo master on the first two device channels.
    if context.device_channels() > 0 {
        context.connect(OutputEndpoint::new(master, Master::OUTPUT).to_device(0))?;
    }
    Ok(())
}

/// The peaks of what a track sends to the master, after its volume, pan, mute and solo: its
/// meter. `None` before its arrangement has run.
pub fn track_peaks(project: &Project, track: &InstanceId) -> Option<Peaks> {
    let arrangement = track.parent()?;
    project.peaks(&arrangement, &format!("{TRACK_PEAKS}{}", track.name()))
}

/// The peaks of what the master sends out, after its limiter: the meter of the master.
pub fn master_peaks(project: &Project, arrangement: &InstanceId) -> Option<Peaks> {
    project.peaks(arrangement, MASTER_PEAKS)
}

/// The largest reduction of the limiter since the last take, in its first channel, as the
/// factor by which the sound was above what came out: 2 is 6 dB of reduction.
pub fn reduction_peaks(project: &Project, arrangement: &InstanceId) -> Option<Peaks> {
    project.peaks(arrangement, REDUCTION_PEAKS)
}

/// Why a name in `effects` has no effect behind it: no record at all, or one whose tool has
/// not the ports of an effect.
fn missing_effect(context: &BehaviourContext<'_>, name: &str) -> String {
    match context.child_names().any(|child| child == name) {
        true => format!(
            "`effects` names {name:?}, and {name}.json in this track holds no tool with an `audio` input and an `audio` output, so the sound passes it by. Put a plugin record there, or take {name:?} out of `effects`"
        ),
        false => format!(
            "`effects` names {name:?}, and this track has no {name}.json, so the sound passes it by. Write that record, or take {name:?} out of `effects`"
        ),
    }
}

/// Children that look like an effect and are not in the list. They are silent otherwise, and
/// an agent that wrote the record and forgot the list would be left guessing.
fn unlisted_effects(track: &TrackState, context: &BehaviourContext<'_>) -> Vec<String> {
    let children = context.child_names();
    let unlisted = children.filter(|name| {
        *name != INSTRUMENT
            && track.bypassed(name).is_none()
            && context.child_input(name, AUDIO_INPUT).is_some()
            && context.child_output(name, AUDIO_OUTPUT).is_some()
    });
    unlisted.map(str::to_string).collect()
}

/// The tracks of an arrangement as people see them: by `order`, then by id.
pub fn tracks<'a>(
    project: &'a Project,
    arrangement: &InstanceId,
) -> Vec<(Instance<TrackState>, &'a TrackState)> {
    let mut tracks: Vec<_> = project.children::<TrackState>(arrangement).collect();
    tracks.sort_by(|(a, a_state), (b, b_state)| {
        (a_state.order, a.id()).cmp(&(b_state.order, b.id()))
    });
    tracks
}

/// The clips of a track by start, then by id.
pub fn clips<'a>(project: &'a Project, track: &InstanceId) -> Vec<(Instance<Clip>, &'a Clip)> {
    let mut clips: Vec<_> = project.children::<Clip>(track).collect();
    clips.sort_by(|(a, a_clip), (b, b_clip)| (a_clip.start, a.id()).cmp(&(b_clip.start, b.id())));
    clips
}

/// The end of the last clip of an arrangement. `None` when it has no clips.
pub fn end(project: &Project, arrangement: &InstanceId) -> Option<Ticks> {
    let tracks = project.children::<TrackState>(arrangement);
    let ends = tracks.filter_map(|(track, _)| {
        let clips = project.children::<Clip>(track.id());
        clips.map(|(_, clip)| clip.end()).max()
    });
    ends.max()
}

/// Adds a track after the last one, with `instrument` as its instrument, to a group of
/// changes. The id comes from the name: "Warm Pad" becomes `warm-pad`, or `warm-pad-2` when
/// that is taken. The arrangement may be created earlier in the same group.
///
/// The instrument is a parameter because this crate knows no instrument: pass the state of
/// any tool with the ports of the note contract, such as `SynthState::default()`.
pub fn add_track<I: State>(
    project: &Project,
    changes: &mut Changes,
    arrangement: &InstanceId,
    name: &str,
    colour: Colour,
    instrument: I,
) -> Result<Instance<TrackState>, ProjectError> {
    let id = project.free_id(&arrangement.child(&id_name(name, "track"))?)?;
    let last = tracks(project, arrangement)
        .last()
        .map(|(_, track)| track.order);
    let order = last.map_or(0, |order| order.saturating_add(1));
    let state = TrackState::new(name, colour, order);
    let track = changes.create(id, state);
    changes.create(track.id().child(INSTRUMENT)?, instrument);
    Ok(track)
}

/// The slots of a track in the order the sound goes through them: the instrument, then the
/// effects the record names. A slot may hold nothing; a name with no record is a slot all the
/// same, because that is what the record says the track has.
pub fn device_slots(
    project: &Project,
    track: &Instance<TrackState>,
) -> Result<Vec<InstanceId>, ProjectError> {
    let mut slots = vec![track.id().child(INSTRUMENT)?];
    let Some(state) = project.state(track) else {
        return Ok(slots);
    };
    for slot in &state.effects {
        slots.push(track.id().child(&slot.name)?);
    }
    Ok(slots)
}

/// Adds an effect slot at the end of the chain of a track, to a group of changes: a free child
/// id, and the track record with that name appended.
///
/// The record of the effect itself is the caller's, as the instrument of [`add_track`] is:
/// this crate knows no effect. Put any tool with an `audio` input and an `audio` output in the
/// id this gives back, in the same group, so that adding an effect is one undo step.
pub fn add_effect(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    name: &str,
) -> Result<InstanceId, ProjectError> {
    let missing = || ProjectError::MissingInstance(track.id().clone());
    let mut state = project.state(track).ok_or_else(missing)?.clone();
    // A free id: no record of this track, no file on disk, and no name the list already has.
    // The last of those is what `free_id` cannot see, because a listed name may have no record.
    let mut slot = project.free_id(&track.id().child(&id_name(name, "effect"))?)?;
    while state.bypassed(slot.name()).is_some() {
        let next = format!("{}-2", slot.name());
        slot = project.free_id(&track.id().child(&next)?)?;
    }
    state.effects.push(EffectSlot::new(slot.name()));
    changes.set(track, state);
    Ok(slot)
}

/// Takes an effect off a track: the record and its name in the list go in one group, so
/// removing an effect is one undo step and undo brings it back where it was.
pub fn remove_effect(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    slot: &InstanceId,
) -> Result<(), ProjectError> {
    let missing = || ProjectError::MissingInstance(track.id().clone());
    let mut state = project.state(track).ok_or_else(missing)?.clone();
    state.effects.retain(|effect| effect.name != slot.name());
    changes.set(track, state);
    changes.delete(slot);
    Ok(())
}

/// Moves an effect of a track to another place in its chain, to a group of changes: `to` is
/// its place among the effects, 0 right after the instrument, and a place past the last is the
/// last. Only the list in the track record changes, so the slot keeps its record, its state and
/// whether it is bypassed, and a reorder is one record and one undo step. Whether it moved.
pub fn move_effect(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    slot: &InstanceId,
    to: usize,
) -> Result<bool, ProjectError> {
    let missing = || ProjectError::MissingInstance(track.id().clone());
    let mut state = project.state(track).ok_or_else(missing)?.clone();
    let Some(from) = state
        .effects
        .iter()
        .position(|effect| effect.name == slot.name())
    else {
        return Err(ProjectError::MissingInstance(slot.clone()));
    };
    let to = to.min(state.effects.len() - 1);
    if from == to {
        return Ok(false);
    }
    let effect = state.effects.remove(from);
    state.effects.insert(to, effect);
    changes.set(track, state);
    Ok(true)
}

/// Adds a clip to a track, to a group of changes. The id comes from `name`, as for a track.
pub fn add_clip(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    name: &str,
    clip: Clip,
) -> Result<Instance<Clip>, ProjectError> {
    let id = project.free_id(&track.id().child(&id_name(name, "clip"))?)?;
    Ok(changes.create(id, clip))
}

/// Adds clips to tracks in one group of changes, for a paste or a duplicate: one undo step. Each
/// id comes from the name of its clip on its track without a number at its end, then the next
/// free one: a copy of `verse-2` is `verse` when that is free, else `verse-2`, `verse-3` and so
/// on. No two clips of the group get the same id.
pub fn add_clips<'a>(
    project: &Project,
    changes: &mut Changes,
    clips: impl IntoIterator<Item = (&'a Instance<TrackState>, &'a str, Clip)>,
) -> Result<Vec<Instance<Clip>>, ProjectError> {
    let mut free = FreeIds::default();
    let mut added = Vec::new();
    for (track, name, clip) in clips {
        let name = id_name(name, "clip");
        let id = free.take(project, &track.id().child(unnumbered(&name))?)?;
        added.push(changes.create(id, clip));
    }
    Ok(added)
}

/// A name without the `-2` that [`Project::free_id`] puts at the end of a taken one.
pub(crate) fn unnumbered(name: &str) -> &str {
    match name.rsplit_once('-') {
        Some((base, number)) if !base.is_empty() && number.parse::<u32>().is_ok() => base,
        _ => name,
    }
}

/// Free ids for the clips of one group of changes. [`Project::free_id`] sees the project and the
/// disk, not the group that is being built, so this remembers what it gave out. It also goes on
/// from the last number it tried for a name, so many clips of one name read the disk once each,
/// not once for every number taken before them.
#[derive(Default)]
pub(crate) struct FreeIds {
    given: BTreeSet<InstanceId>,
    next: BTreeMap<InstanceId, u32>,
}

impl FreeIds {
    pub(crate) fn take(
        &mut self,
        project: &Project,
        wanted: &InstanceId,
    ) -> Result<InstanceId, ProjectError> {
        let mut number = self.next.get(wanted).copied().unwrap_or(1);
        loop {
            let candidate = match number {
                1 => wanted.clone(),
                _ => InstanceId::new(&format!("{}-{number}", wanted.as_str()))?,
            };
            number += 1;
            if !self.given.contains(&candidate) && project.free_id(&candidate)? == candidate {
                self.next.insert(wanted.clone(), number);
                self.given.insert(candidate.clone());
                return Ok(candidate);
            }
        }
    }
}

/// Moves a clip to another track: a delete and a create in one group, like moving the file.
/// It keeps its name when the other track has none like it.
pub fn move_clip(
    project: &Project,
    changes: &mut Changes,
    clip: &Instance<Clip>,
    to_track: &Instance<TrackState>,
) -> Result<Instance<Clip>, ProjectError> {
    let missing = || ProjectError::MissingInstance(clip.id().clone());
    let state = project.state(clip).ok_or_else(missing)?.clone();
    let id = project.free_id(&to_track.id().child(clip.id().name())?)?;
    changes.delete(clip.id());
    Ok(changes.create(id, state))
}

/// Plays one note now through the instrument of a track, for [`PREVIEW_SECONDS`], also while
/// the project does not play: what a note editor calls when a note is clicked, drawn or moved
/// to a new pitch. The off comes from the sequencer of the track, so the caller has nothing
/// to end. Not an edit: nothing is saved.
pub fn preview_note(
    project: &mut Project,
    track: &InstanceId,
    pitch: Pitch,
    velocity: Velocity,
) -> Result<(), ProjectError> {
    let preview = SequencerUpdate::Preview { pitch, velocity };
    project.send::<Sequencer>(track, SEQUENCER, preview)
}

/// The default project: one arrangement with one track and its instrument, no clips. The
/// tempo map of a new project is already 120 bpm in 4/4.
pub fn create_default_project<I: State>(
    project: &mut Project,
    instrument: I,
) -> Result<(), ProjectError> {
    let mut changes = Changes::new();
    let arrangement = InstanceId::new(DEFAULT_ARRANGEMENT)?;
    changes.create(arrangement.clone(), ArrangementState::default());
    add_track(
        project,
        &mut changes,
        &arrangement,
        "Track 1",
        Colour::Blue,
        instrument,
    )?;
    project.commit("Create default project", changes)
}

/// A valid instance name from a display name: lowercase letters and digits, `-` for the rest.
fn id_name(display: &str, fallback: &str) -> String {
    let mut name = String::new();
    for character in display.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            name.push(character);
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    // `instance` is the one name the core keeps for itself.
    if name.is_empty() || name == "instance" {
        fallback.to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{LimiterState, TrackState, id_name};

    /// The ranges are written once, in the states. The docs tell people and agents the same
    /// numbers, so they cannot drift from them.
    #[test]
    fn the_docs_give_the_ranges_of_the_mixer_and_the_limiter() {
        let docs = [
            ("agent-doc.md", include_str!("../agent-doc.md")),
            ("README.md", include_str!("../README.md")),
        ];
        let ranges = [
            TrackState::PAN,
            LimiterState::GAIN_DB,
            LimiterState::CEILING_DB,
            LimiterState::RELEASE_MS,
            LimiterState::LOOKAHEAD_MS,
        ];
        for (name, doc) in docs {
            for (min, max) in ranges {
                let range = format!("{min} to {max}");
                assert!(doc.contains(&range), "{name} does not say {range}");
            }
            let most = format!("up to {}", TrackState::MAX_GAIN_DB);
            assert!(doc.contains(&most), "{name} does not say {most}");
        }
    }

    #[test]
    fn ids_come_from_display_names() {
        assert_eq!(id_name("Warm Pad", "track"), "warm-pad");
        assert_eq!(id_name("  Bass (bars 5-8)! ", "clip"), "bass-bars-5-8");
        assert_eq!(id_name("Ünïcode 9", "clip"), "n-code-9");
        assert_eq!(id_name("???", "clip"), "clip");
        assert_eq!(id_name("Instance", "track"), "track");
    }
}
