//! Arrangement: tracks, clips and notes on the project timeline.
//!
//! Four tools. An `arrangement` owns tracks and is the master: it mixes every track, solo
//! included, and plays the sum through its volume and limiter to the main output. An
//! `arrangement.track` owns clips and its effects, and plays its clips through them to its
//! arrangement. It is an instrument track or an audio track, chosen when it is made. An
//! instrument track plays note clips through one instrument, the child named `instrument`. An
//! `arrangement.clip` is plain data: the [`Clip`] of the note contract crate. An audio track
//! plays [`AudioClip`]s, `arrangement.audio_clip`, stretches of files under `assets/audio/`.
//!
//! ```text
//! state/arrangement/instance.json            the arrangement
//! state/arrangement/piano/instance.json      an instrument track
//! state/arrangement/piano/instrument.json    its instrument: any tool with the ports of the note contract
//! state/arrangement/piano/verse-a.json       a note clip
//! state/arrangement/vocal/instance.json      an audio track, `"kind": "audio"`
//! state/arrangement/vocal/verse-take.json    an audio clip
//! ```
//!
//! This crate depends on no instrument and no effect. A track finds its instrument by the child
//! name `instrument`, and each effect by the name its record lists, and both by the port names
//! of the note contract. So any tool with those ports fits. A missing effect is skipped and
//! reported, and the sound goes through the rest.
//!
//! Tracks are not copied, only clips: a track owns devices of any tool, and the core creates a
//! record only of a type the caller knows. A known gap: a file edit of a clip while that clip
//! is dragged to another track comes back as a second clip, because its file still has the old
//! id then.
//!
//! `agent-doc.md` and `audio-agent-doc.md` in this crate have the record formats. The interface
//! is in [`view`]. Nothing else here uses GPUI.

mod audio;
mod automation;
mod clip_moves;
pub mod decibels;
mod input;
mod master;
mod mixer;
mod player;
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
use sound_notes::{
    AUDIO_INPUT, AUDIO_OUTPUT, Clip, NOTES_INPUT, Pitch, SIDECHAIN_INPUT, TRACK_TOOL, Velocity,
};

pub use audio::AudioClip;
pub use automation::{
    AutomationLane, AutomationValue, Carried, LaneMove, Moved, Travel, automatable, moved,
    travel_in,
};
pub(crate) use clip_moves::shown_end;
pub use clip_moves::{AnyClip, ClipMove, move_clips};
pub use input::InputChannels;
use master::Master;
pub use master::{LimiterState, MasterState};
pub use mixer::{ChannelGains, Mix, Mixer, RAMP_SECONDS};
pub use player::{AudioPlayer, AudioSnapshot, AudioUpdate, DECLICK_SECONDS};
pub use sequencer::{HELD_CAPACITY, PREVIEW_SECONDS, Sequencer, SequencerUpdate, TrackSnapshot};
pub use slot::{EffectSlot, Sidechain, Tap};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "arrangement";

/// The name of the child of a track that plays its notes.
pub const INSTRUMENT: &str = "instrument";

/// The name of the processor of a track that plays its clips: of notes, and of audio.
const SEQUENCER: &str = "sequencer";
const PLAYER: &str = "player";
/// The processors of an arrangement: the mixer of each track, by the name of the track after
/// this, and the master.
const MIXER: &str = "mixer/";
const MASTER: &str = "master";
/// The ports a track exposes for sidechains: the sound before its effects, and the `sidechain`
/// input of each keyed effect, by the name of its slot after this.
const PRE_FX: &str = "pre_fx";
const SIDECHAIN_OF: &str = "sidechain/";
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

/// What a track plays, chosen when it is made.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    /// Note clips, through the instrument of the track.
    #[default]
    Instrument,
    /// Audio clips, and no instrument.
    Audio,
}

impl TrackKind {
    fn is_instrument(&self) -> bool {
        *self == Self::Instrument
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackState {
    /// What people see. The id of the track is its folder name and never changes.
    pub name: String,
    /// Left out for an instrument track, so every track of before audio tracks existed loads
    /// unchanged and gives the same bytes.
    #[serde(default, skip_serializing_if = "TrackKind::is_instrument")]
    pub kind: TrackKind,
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
    /// The channels of the default input an audio track records. Channel 1 alone when left
    /// out, and a record that leaves it out is written back without it. An instrument track
    /// records notes, not audio, and does nothing with it.
    #[serde(default, skip_serializing_if = "InputChannels::is_default")]
    pub input: InputChannels,
    /// The lanes that move numbers of the devices of the track, and its own volume and pan,
    /// over the project timeline. Left out when there are none, so a track of before
    /// automation existed loads unchanged and gives the same bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
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
            kind: TrackKind::Instrument,
            colour,
            order,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            effects: Vec::new(),
            input: InputChannels::FIRST,
            automation: Vec::new(),
        }
    }

    /// Whether the track makes no sound: it is muted, or another track is soloed and it is
    /// not. `soloing` is whether any track of its arrangement is soloed.
    pub fn is_silent(&self, soloing: bool) -> bool {
        self.mute || (soloing && !self.solo)
    }

    /// Whether the effect in the child `name` is bypassed. `None` when the list does not
    /// name it.
    pub fn bypassed(&self, name: &str) -> Option<bool> {
        let slot = self.effects.iter().find(|slot| slot.name == name)?;
        Some(slot.bypass)
    }

    /// What keys the effect in the child `name`. `None` when nothing does or the list does not
    /// name it.
    pub fn sidechain(&self, name: &str) -> Option<&Sidechain> {
        let slot = self.effects.iter().find(|slot| slot.name == name)?;
        slot.sidechain.as_ref()
    }

    /// Keys the effect in the child `name` with `sidechain`, or with nothing. Whether the list
    /// names it.
    pub fn set_sidechain(&mut self, name: &str, sidechain: Option<Sidechain>) -> bool {
        let slot = self.effects.iter_mut().find(|slot| slot.name == name);
        slot.map(|slot| slot.sidechain = sidechain).is_some()
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
        for (
            index,
            EffectSlot {
                name, sidechain, ..
            },
        ) in self.effects.iter().enumerate()
        {
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
            let mut earlier = self.effects.iter().take(index);
            if earlier.any(|slot| slot.name == *name) {
                return Err(format!(
                    "effects[{index}] is {name:?}, which the list already has. One effect is one child record: copy the file under another name to use it twice"
                ));
            }
            if let Some(Sidechain { track, .. }) = sidechain
                && !is_child_name(track)
            {
                return Err(format!(
                    "effects[{index}].sidechain.track must be the folder name of a track: lowercase letters, digits, `-` and `_`, not {track:?}"
                ));
            }
        }
        AutomationLane::check_all(&self.automation)
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

/// The doc an agent opens for audio: audio tracks, audio clips and their files.
pub const AUDIO_AGENT_DOC: AgentDoc = AgentDoc {
    name: "audio",
    when: "You add, move, trim, fade or delete audio clips, or add an audio track",
    markdown: include_str!("../audio-agent-doc.md"),
};

/// Registers the four tools and the agent docs. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<ArrangementState>(EXTENSION)?
        .behaviour(apply_arrangement)
        .summary(summary::of_arrangement)
        .end(|project, arrangement| end(project, arrangement.id()));
    // A track whose clip names a file that is not there yet plays it once the file arrives.
    registry
        .tool::<TrackState>(EXTENSION)?
        .behaviour(apply_track)
        .rebinds_on_assets(sound_media::AUDIO_FOLDER);
    registry.tool::<Clip>(EXTENSION)?;
    registry.tool::<AudioClip>(EXTENSION)?;
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    registry.agent_doc(EXTENSION, AUDIO_AGENT_DOC)?;
    Ok(())
}

/// Runs when the track record or anything the track owns changes: one snapshot of all its
/// clips goes to the one processor that plays them, which keeps what it holds. The instrument
/// and the effects are found by the port names of the note contract, so any tool with those
/// ports fits.
///
/// The path of an instrument track is sequencer, instrument, the effects in the order of the
/// record that are not bypassed, and out through its `audio` output, which its arrangement
/// mixes. An audio track has its player where the other has sequencer and instrument. Every
/// processor keeps what it holds: a note goes on sounding through an edit of a clip. For its
/// arrangement to wire sidechains, it also exposes its sound before the effects and the
/// `sidechain` input of each keyed effect.
fn apply_track(
    track: &TrackState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let source = match track.kind {
        TrackKind::Instrument => play_notes(context)?,
        TrackKind::Audio => play_audio(context)?,
    };

    // The chain, from the instrument or the player through the effects. A slot that is not
    // there is reported and left out, so the sound goes on through the rest of the chain. A
    // bypassed slot is left out too: the sound goes past it untouched, and its latency with it.
    if let Some(source) = source {
        context.output(PRE_FX, source);
    }
    let mut sound = source;
    for slot in &track.effects {
        let EffectSlot { name, bypass, .. } = slot;
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
        expose_sidechain(slot, context);
        if let Some(sound) = sound {
            context.connect(sound.to(input))?;
        }
        sound = Some(output);
    }
    if let Some(sound) = sound {
        context.output(AUDIO_OUTPUT, sound);
    }
    automation::play(&track.automation, context)?;
    for name in unlisted_effects(track, context) {
        context.problem(format!(
            "the child {name:?} takes audio in and makes audio out, and the `effects` list of this track does not name it, so nothing goes through it. Add {name:?} to `effects` where you want it in the chain, or delete the file"
        ));
    }
    Ok(())
}

/// Lets the arrangement key the effect of `slot`: its `sidechain` input, as an input of the
/// track. What can never be live is reported here, where the slot is.
fn expose_sidechain(slot: &EffectSlot, context: &mut BehaviourContext<'_>) {
    let Some(Sidechain { track, tap }) = &slot.sidechain else {
        return;
    };
    let name = &slot.name;
    let Some(input) = context.child_input(name, SIDECHAIN_INPUT) else {
        context.problem(format!(
            "`effects` keys {name:?} with a sidechain, and {name}.json holds no tool with a `{SIDECHAIN_INPUT}` input, so nothing keys it. Use an effect that has one, such as the `compressor`, or take `sidechain` out"
        ));
        return;
    };
    // The sound of this track after this effect comes out of the effect itself.
    if track == context.id().name() && *tap != Tap::PreFx {
        context.problem(format!(
            "the sidechain of {name:?} takes the sound of this same track after {name:?} itself, which would key it in a loop, so nothing keys it. Use \"tap\": \"pre_fx\" to key it with this track before its effects, or take another track"
        ));
        return;
    }
    context.input(&format!("{SIDECHAIN_OF}{name}"), input);
}

/// The notes of an instrument track: its sequencer into its instrument. Gives where the sound
/// of the instrument comes out.
fn play_notes(
    context: &mut BehaviourContext<'_>,
) -> Result<Option<OutputEndpoint>, BehaviourError> {
    let snapshot = TrackSnapshot::new(context.children::<Clip>().map(|(_, clip)| clip));
    let sequencer = context.processor(SEQUENCER, Sequencer::default)?;
    context.update(sequencer, SequencerUpdate::Snapshot(Arc::new(snapshot)))?;
    if let Some(notes) = context.child_input(INSTRUMENT, NOTES_INPUT) {
        context.connect(OutputEndpoint::new(sequencer, Sequencer::NOTES).to(notes))?;
    }
    let audio_clips: Vec<String> = context
        .children::<AudioClip>()
        .map(|(name, _)| name.to_string())
        .collect();
    for name in audio_clips {
        context.problem(format!(
            "{name}.json is an audio clip, and this is an instrument track, which plays note clips only, so it is silent. Move it into an audio track"
        ));
    }
    Ok(context.child_output(INSTRUMENT, AUDIO_OUTPUT))
}

/// The audio clips of an audio track: one player. Gives where its sound comes out.
fn play_audio(
    context: &mut BehaviourContext<'_>,
) -> Result<Option<OutputEndpoint>, BehaviourError> {
    let rate = context.prepare_config().sample_rate;
    let (snapshot, problems) =
        audio::snapshot(context.children::<AudioClip>(), context.assets(), rate);
    let note_clips: Vec<String> = context
        .children::<Clip>()
        .map(|(name, _)| name.to_string())
        .collect();
    let has_instrument = context.child_names().any(|name| name == INSTRUMENT);
    for problem in problems {
        context.problem(problem);
    }
    for name in note_clips {
        context.problem(format!(
            "{name}.json is a note clip, and this is an audio track, which plays audio clips only, so it is silent. Move it into an instrument track"
        ));
    }
    if has_instrument {
        context.problem(format!(
            "an audio track has no instrument, so {INSTRUMENT}.json is not played. Delete it, or put it in an instrument track"
        ));
    }
    let player = context.processor(PLAYER, AudioPlayer::new)?;
    context.update(player, AudioUpdate::new(snapshot))?;
    Ok(Some(OutputEndpoint::new(player, AudioPlayer::OUTPUT)))
}

/// Runs when the arrangement record or anything below it changes: the mixer of every track,
/// with solo worked out across the tracks, into the master, and the master to the main output.
///
/// Solo is decided here and not in a track, because it is about all of them. While any track is
/// soloed, every track that is not gets the gains of a muted one, so soloing a track sounds
/// exactly as muting every other one does. Sidechains are wired here for the same reason: a
/// key comes from another track.
fn apply_arrangement(
    arrangement: &ArrangementState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    // Only what the mixers need, and no copy of a record: this runs on every edit below the
    // arrangement, a mouse move of a clip drag included.
    let soloing = context
        .children::<TrackState>()
        .any(|(_, track)| track.solo);
    let tracks: Vec<(String, Mix)> = context
        .children::<TrackState>()
        .map(|(name, track)| {
            let silent = track.is_silent(soloing);
            (
                name.to_string(),
                Mix {
                    silent,
                    ..Mix::of(track)
                },
            )
        })
        .collect();
    let sidechains: Vec<(String, String, Sidechain)> = context
        .children::<TrackState>()
        .flat_map(|(name, track)| {
            let slots = track.effects.iter();
            slots.filter_map(move |slot| {
                let sidechain = slot.sidechain.clone()?;
                Some((name.to_string(), slot.name.clone(), sidechain))
            })
        })
        .collect();

    let settings = arrangement.master.settings();
    let (peaks, reduction) = (context.peaks(MASTER_PEAKS), context.peaks(REDUCTION_PEAKS));
    let master = context.processor(MASTER, || Master::new(settings, peaks, reduction))?;
    context.update(master, settings)?;

    let mut mixers = BTreeMap::new();
    for (name, mix) in tracks {
        let peaks = context.peaks(&format!("{TRACK_PEAKS}{name}"));
        let mixer = context.processor(&format!("{MIXER}{name}"), || Mixer::new(mix, peaks))?;
        mixers.insert(name.clone(), mixer);
        context.update(mixer, mix)?;
        if let Some(sound) = context.child_output(&name, AUDIO_OUTPUT) {
            context.connect(sound.to(InputEndpoint::new(mixer, Mixer::INPUT)))?;
        }
        if let Some(lanes) = context.child_output(&name, automation::TRACK_AUTOMATION) {
            context.connect(lanes.to(InputEndpoint::new(mixer, Mixer::AUTOMATION.port())))?;
        }
        let into_master = InputEndpoint::new(master, Master::INPUT);
        context.connect(OutputEndpoint::new(mixer, Mixer::OUTPUT).to(into_master))?;
    }
    for (name, slot, Sidechain { track, tap }) in sidechains {
        let Some(mixer) = mixers.get(&track) else {
            context.problem(format!(
                "the sidechain of {slot:?} in track {name:?} takes track {track:?}, and this arrangement has no {track}/instance.json, so nothing keys it. Name the folder of a track, or take `sidechain` out"
            ));
            continue;
        };
        let sound = match tap {
            Tap::PreFx => context.child_output(&track, PRE_FX),
            Tap::PostFx => context.child_output(&track, AUDIO_OUTPUT),
            Tap::PostMixer => Some(OutputEndpoint::new(*mixer, Mixer::OUTPUT)),
        };
        // No input when the slot is bypassed, or its track said why.
        let key = context.child_input(&name, &format!("{SIDECHAIN_OF}{slot}"));
        if let Some((sound, key)) = sound.zip(key) {
            context.connect(sound.to(key))?;
        }
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

/// The tracks of an arrangement as people see them, each with its `order` now: where a move of
/// a track starts from, see [`move_track`].
pub fn track_orders(
    project: &Project,
    arrangement: &InstanceId,
) -> Vec<(Instance<TrackState>, u32)> {
    let tracks = tracks(project, arrangement).into_iter();
    tracks.map(|(track, state)| (track, state.order)).collect()
}

/// The `order` of each track after the one at `from` goes to place `to`, for orders given in
/// the order people see the tracks: the new order of the track at each place of `orders`. A
/// `to` past the last is the last. `None` when nothing moves.
///
/// The tracks are numbered again from 0, because tracks of the same order, which an agent may
/// write, have no free number between them.
pub fn reordered(orders: &[u32], from: usize, to: usize) -> Option<Vec<u32>> {
    let to = to.min(orders.len().checked_sub(1)?);
    if from == to || from >= orders.len() {
        return None;
    }
    let mut places: Vec<usize> = (0..orders.len()).collect();
    let moved = places.remove(from);
    places.insert(to, moved);
    let mut reordered = orders.to_vec();
    for (order, place) in (0u32..).zip(places) {
        if let Some(slot) = reordered.get_mut(place) {
            *slot = order;
        }
    }
    Some(reordered)
}

/// Moves the track at `from` to place `to`, to a group of changes. `tracks` are what
/// [`track_orders`] gave when the move began, so a drag that comes back to where it began
/// writes back the orders there were, ties included, and makes no undo step. Only a track
/// whose order changes is written; one deleted since is left out. The clips, the devices and
/// everything else of a track stay in its folder, so the track keeps its id.
pub fn move_track(
    project: &Project,
    changes: &mut Changes,
    tracks: &[(Instance<TrackState>, u32)],
    from: usize,
    to: usize,
) {
    let orders: Vec<u32> = tracks.iter().map(|(_, order)| *order).collect();
    let orders = reordered(&orders, from, to).unwrap_or(orders);
    for ((track, _), order) in tracks.iter().zip(orders) {
        let Some(state) = project.state(track) else {
            continue;
        };
        if state.order != order {
            changes.set(
                track,
                TrackState {
                    order,
                    ..state.clone()
                },
            );
        }
    }
}

/// The clips of a track by start, then by id.
pub fn clips<'a>(project: &'a Project, track: &InstanceId) -> Vec<(Instance<Clip>, &'a Clip)> {
    let mut clips: Vec<_> = project.children::<Clip>(track).collect();
    clips.sort_by(|(a, a_clip), (b, b_clip)| (a_clip.start, a.id()).cmp(&(b_clip.start, b.id())));
    clips
}

/// The audio clips of a track by start, then by id.
pub fn audio_clips<'a>(
    project: &'a Project,
    track: &InstanceId,
) -> Vec<(Instance<AudioClip>, &'a AudioClip)> {
    let mut clips: Vec<_> = project.children::<AudioClip>(track).collect();
    clips.sort_by(|(a, a_clip), (b, b_clip)| (a_clip.start, a.id()).cmp(&(b_clip.start, b.id())));
    clips
}

/// The first tick after an audio clip under the clock of the project. Its start when its file
/// is not there.
pub fn audio_clip_end(project: &Project, clip: &AudioClip) -> Ticks {
    let file = sound_media::info(project.assets(), &clip.asset).ok();
    clip.end(file.as_ref(), project.clock())
}

/// The end of the last clip of an arrangement. `None` when it has no clips.
pub fn end(project: &Project, arrangement: &InstanceId) -> Option<Ticks> {
    let tracks = project.children::<TrackState>(arrangement);
    let ends = tracks.filter_map(|(track, _)| {
        let notes = project
            .children::<Clip>(track.id())
            .map(|(_, clip)| clip.end());
        let audio = project.children::<AudioClip>(track.id());
        let audio = audio.map(|(_, clip)| audio_clip_end(project, clip));
        notes.chain(audio).max()
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

/// Adds an audio track after the last one to a group of changes: no instrument, no clips. The
/// id comes from the name, as for [`add_track`].
pub fn add_audio_track(
    project: &Project,
    changes: &mut Changes,
    arrangement: &InstanceId,
    name: &str,
    colour: Colour,
) -> Result<Instance<TrackState>, ProjectError> {
    let id = project.free_id(&arrangement.child(&id_name(name, "track"))?)?;
    let last = tracks(project, arrangement)
        .last()
        .map(|(_, track)| track.order);
    let order = last.map_or(0, |order| order.saturating_add(1));
    let mut state = TrackState::new(name, colour, order);
    state.kind = TrackKind::Audio;
    Ok(changes.create(id, state))
}

/// Adds an audio clip to a track, to a group of changes, over every clip the track has: its
/// layer is one above theirs, so where it overlaps them it is heard. The id comes from `name`,
/// as for a note clip.
pub fn add_audio_clip(
    project: &Project,
    changes: &mut Changes,
    track: &Instance<TrackState>,
    name: &str,
    clip: AudioClip,
) -> Result<Instance<AudioClip>, ProjectError> {
    let mut added = add_audio_clips(project, changes, [(track, name, clip)])?;
    added
        .pop()
        .ok_or_else(|| ProjectError::MissingInstance(track.id().clone()))
}

/// Adds audio clips to tracks in one group of changes, for a drop of several files, a paste or
/// a duplicate: one undo step. Each goes over every clip its track has, and a later one over an
/// earlier one of the same track, so the newest covers. Each id is its name, or the next free
/// one after it: `strum-2`, then `strum-2-2`. A paste gives names without their number, as
/// [`add_clips`] makes them.
pub fn add_audio_clips<'a>(
    project: &Project,
    changes: &mut Changes,
    clips: impl IntoIterator<Item = (&'a Instance<TrackState>, &'a str, AudioClip)>,
) -> Result<Vec<Instance<AudioClip>>, ProjectError> {
    let mut free = FreeIds::default();
    let mut layers: BTreeMap<InstanceId, u32> = BTreeMap::new();
    let mut added = Vec::new();
    for (track, name, mut clip) in clips {
        let layer = layers.entry(track.id().clone()).or_insert_with(|| {
            top_layer(project, track.id(), &[]).map_or(0, |top| top.saturating_add(1))
        });
        clip.layer = *layer;
        *layer = layer.saturating_add(1);
        let name = id_name(name, "clip");
        let id = free.take(project, &track.id().child(&name)?)?;
        added.push(changes.create(id, clip));
    }
    Ok(added)
}

/// The highest layer of the audio clips of a track, leaving out `except`: what a clip placed
/// over them goes one above. `None` when there are none.
pub fn top_layer(project: &Project, track: &InstanceId, except: &[InstanceId]) -> Option<u32> {
    let clips = project.children::<AudioClip>(track);
    clips
        .filter(|(clip, _)| !except.contains(clip.id()))
        .map(|(_, clip)| clip.layer)
        .max()
}

/// The slots of a track in the order the sound goes through them: the instrument of an
/// instrument track, then the effects the record names. A slot may hold nothing; a name with no record is a slot all the
/// same, because that is what the record says the track has.
pub fn device_slots(
    project: &Project,
    track: &Instance<TrackState>,
) -> Result<Vec<InstanceId>, ProjectError> {
    let instrument = track.id().child(INSTRUMENT)?;
    let Some(state) = project.state(track) else {
        return Ok(vec![instrument]);
    };
    // An audio track has no instrument, so its rack starts with its first effect.
    let mut slots = match state.kind {
        TrackKind::Instrument => vec![instrument],
        TrackKind::Audio => Vec::new(),
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

/// The default project: one arrangement with no tracks. The tempo map of a new project is
/// already 120 bpm in 4/4.
pub fn create_default_project(project: &mut Project) -> Result<(), ProjectError> {
    let mut changes = Changes::new();
    let arrangement = InstanceId::new(DEFAULT_ARRANGEMENT)?;
    changes.create(arrangement, ArrangementState::default());
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
    use super::{LimiterState, TrackState, id_name, reordered};

    #[test]
    fn a_moved_track_takes_its_new_place_and_the_others_close_up() {
        let orders = [0, 1, 2, 3];
        // Up one, and down one.
        assert_eq!(reordered(&orders, 2, 1), Some(vec![0, 2, 1, 3]));
        assert_eq!(reordered(&orders, 1, 2), Some(vec![0, 2, 1, 3]));
        // To the first place, and to the last.
        assert_eq!(reordered(&orders, 3, 0), Some(vec![1, 2, 3, 0]));
        assert_eq!(reordered(&orders, 0, 3), Some(vec![3, 0, 1, 2]));
        // A place past the last is the last.
        assert_eq!(reordered(&orders, 0, 10), Some(vec![3, 0, 1, 2]));
    }

    #[test]
    fn a_track_let_go_where_it_was_changes_nothing() {
        assert_eq!(reordered(&[0, 1, 2], 1, 1), None);
        assert_eq!(reordered(&[0, 1, 2], 2, 5), None);
        assert_eq!(reordered(&[0, 1, 2], 3, 0), None);
        assert_eq!(reordered(&[], 0, 0), None);
    }

    /// Orders an agent wrote: gaps and ties. Tracks of the same order show by id, so a move
    /// among them needs new numbers, and the order people saw is kept for the others.
    #[test]
    fn tracks_with_gaps_or_the_same_order_are_numbered_again() {
        assert_eq!(reordered(&[0, 0, 0], 2, 0), Some(vec![1, 2, 0]));
        assert_eq!(reordered(&[0, 0, 0], 0, 0), None);
        assert_eq!(reordered(&[5, 10, 10, 40], 3, 1), Some(vec![0, 2, 3, 1]));
    }

    /// The ranges are written once, in the states. The agent docs give the same numbers, so they
    /// cannot drift from them. The doc of the master sends an agent to the doc of the Limiter
    /// effect for the fields they share, so that doc must give the ranges of the master too.
    #[test]
    fn the_docs_give_the_ranges_of_the_mixer_and_the_limiter() {
        let arrangement = include_str!("../agent-doc.md");
        let limiter = include_str!("../../limiter/agent-doc.md");
        let docs = [
            (
                "agent-doc.md",
                arrangement,
                &[TrackState::PAN, LimiterState::LOOKAHEAD_MS][..],
            ),
            (
                "the Limiter doc",
                limiter,
                &[
                    LimiterState::GAIN_DB,
                    LimiterState::CEILING_DB,
                    LimiterState::RELEASE_MS,
                ][..],
            ),
        ];
        for (name, doc, ranges) in docs {
            for (min, max) in ranges {
                let range = format!("{min} to {max}");
                assert!(doc.contains(&range), "{name} does not say {range}");
            }
        }
        let most = format!("up to {}", TrackState::MAX_GAIN_DB);
        assert!(
            arrangement.contains(&most),
            "agent-doc.md does not say {most}"
        );
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
