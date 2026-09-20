//! Arrangement: tracks, clips and notes on the project timeline.
//!
//! Three tools. An `arrangement` owns tracks. An `arrangement.track` owns clips and one
//! instrument, the child named `instrument`, and plays the clips through it to the main
//! output. An `arrangement.clip` is plain data: the [`Clip`] of the note contract crate.
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

mod mixer;
mod sequencer;
mod summary;
pub mod view;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, Changes, InputEndpoint, Instance, InstanceId,
    OutputEndpoint, Place, Project, ProjectError, Registry, RegistryError, State, Ticks,
};
use sound_notes::{AUDIO_OUTPUT, Clip, NOTES_INPUT, Pitch, TRACK_TOOL, Velocity};

pub use mixer::{ChannelGains, Mixer, RAMP_SECONDS, channel_gains};
pub use sequencer::{HELD_CAPACITY, PREVIEW_SECONDS, Sequencer, SequencerUpdate, TrackSnapshot};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "arrangement";

/// The name of the child of a track that plays its notes.
pub const INSTRUMENT: &str = "instrument";

/// The names of the two processors of a track: what plays its clips, and its mixer.
const SEQUENCER: &str = "sequencer";
const MIXER: &str = "mixer";

/// The id of the arrangement in the default project.
pub const DEFAULT_ARRANGEMENT: &str = "arrangement";

/// The root of a piece. It has no settings yet: it is the owner of the tracks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangementState {}

impl State for ArrangementState {
    const TOOL: &'static str = "arrangement";
    const OWNS_CHILDREN: bool = true;
    const PLACE: Place = Place::Root;
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
    /// instrument, and a record that leaves it out gets that.
    #[serde(default)]
    pub gain_db: f32,
    /// Where the track sits between the two channels: -1 hard left, 0 the middle, 1 hard right.
    #[serde(default)]
    pub pan: f32,
    /// A muted track is silent and keeps everything else as it is.
    #[serde(default)]
    pub mute: bool,
}

impl TrackState {
    /// The lowest and the highest gain in decibels. Written here and nowhere else: `validate`,
    /// the knob of the track panel and the docs read them.
    pub const GAIN_DB: (f32, f32) = (-60.0, 6.0);
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
        }
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
        in_range("gain_db", self.gain_db, Self::GAIN_DB)?;
        in_range("pan", self.pan, Self::PAN)
    }
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
/// clips goes to its one sequencer, which keeps its held notes, and the gain, pan and mute of
/// the record go to its mixer as one gain per channel. The instrument is found by the port
/// names of the note contract, so any tool with those ports fits.
///
/// The path of a track is sequencer, instrument, mixer, main output. Both processors keep
/// what they hold: a note goes on sounding through a pan edit, and a gain ramp through a
/// clip edit.
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

    let gains = channel_gains(track);
    let mixer = context.processor(MIXER, || Mixer::new(gains))?;
    context.update(mixer, gains)?;
    if let Some(audio) = context.child_output(INSTRUMENT, AUDIO_OUTPUT) {
        context.connect(audio.to(InputEndpoint::new(mixer, Mixer::INPUT)))?;
    }
    // The main output, for now: the stereo mixer on the first two device channels.
    if context.device_channels() > 0 {
        context.connect(OutputEndpoint::new(mixer, Mixer::OUTPUT).to_device(0))?;
    }
    Ok(())
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
    changes.create(arrangement.clone(), ArrangementState {});
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
    use super::{TrackState, id_name};

    /// The ranges are written once, in `TrackState`. The docs tell people and agents the same
    /// numbers, so they cannot drift from it.
    #[test]
    fn the_docs_give_the_range_of_the_gain_and_the_pan() {
        let docs = [
            ("agent-doc.md", include_str!("../agent-doc.md")),
            ("README.md", include_str!("../README.md")),
        ];
        let ranges = [TrackState::GAIN_DB, TrackState::PAN];
        for (name, doc) in docs {
            for (min, max) in ranges {
                let range = format!("{min} to {max}");
                assert!(doc.contains(&range), "{name} does not say {range}");
            }
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
