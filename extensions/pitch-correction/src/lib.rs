//! Pitch correction: the built-in effect that moves one voice, or one melodic instrument, to
//! the nearest note of a key and scale, or of the notes held on its `notes` input. A fast
//! retune is hard tuning, a slow one a natural correction. The length of the sound stays, and
//! its formants can stay too.
//!
//! The pitch comes from [`sound_media::analysis::PitchTracker`] and the shift from
//! [`sound_media::Shifter`], which a region can drive too.
//!
//! [`view`] is the card of the pitch correction, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, NOTES_INPUT};

pub use processor::{Meters, PitchCorrection};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "pitch-correction";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default
/// correction: to the nearest semitone, fast but not hard.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct PitchCorrectionState {
    /// The note the scale starts on.
    pub key: Key,
    /// Which notes of the key the voice is moved to. Notes held on the `notes` input take
    /// their place while they are held.
    pub scale: Scale,
    /// How long the voice takes to reach its note. 0 is hard tuning; 50 to 150 is a natural
    /// correction that keeps most of a vibrato.
    pub retune_ms: f32,
    /// How much gentler a long note gets: its retune time grows by this part of how long it
    /// has been held, up to 0.4 s. Short notes stay in tune, long ones keep their life.
    pub humanize: f32,
    /// The range of the voice, so its pitch is found and nothing outside it.
    pub voice: Voice,
    /// Keeps the tone of the voice, so a note moved up does not sound smaller.
    pub keep_formants: bool,
    /// How much of the corrected sound plays, 0 to 1; the rest is the voice as it comes.
    pub mix: f32,
}

/// The note a scale starts on.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Key {
    #[default]
    C,
    #[serde(rename = "C#")]
    CSharp,
    D,
    #[serde(rename = "D#")]
    DSharp,
    E,
    F,
    #[serde(rename = "F#")]
    FSharp,
    G,
    #[serde(rename = "G#")]
    GSharp,
    A,
    #[serde(rename = "A#")]
    ASharp,
    B,
}

impl Key {
    pub const ALL: [Self; 12] = [
        Self::C,
        Self::CSharp,
        Self::D,
        Self::DSharp,
        Self::E,
        Self::F,
        Self::FSharp,
        Self::G,
        Self::GSharp,
        Self::A,
        Self::ASharp,
        Self::B,
    ];

    /// Semitones above C.
    pub fn semitones(self) -> u32 {
        self as u32
    }
}

/// Which notes of the key a voice is moved to.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// Every semitone.
    #[default]
    Chromatic,
    Major,
    /// The natural minor.
    Minor,
    MajorPentatonic,
    MinorPentatonic,
}

impl Scale {
    pub const ALL: [Self; 5] = [
        Self::Chromatic,
        Self::Major,
        Self::Minor,
        Self::MajorPentatonic,
        Self::MinorPentatonic,
    ];

    /// The notes of the scale on C, one bit per semitone, C the lowest.
    fn on_c(self) -> u16 {
        let notes: &[u32] = match self {
            Self::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
        };
        notes.iter().fold(0, |notes, note| notes | 1 << note)
    }

    /// The notes of the scale on `key`, one bit per semitone above C.
    pub fn notes(self, key: Key) -> u16 {
        let on_c = u32::from(self.on_c());
        let moved = (on_c << key.semitones()) | (on_c >> (12 - key.semitones()));
        (moved & 0xfff) as u16
    }
}

/// The range a voice sings in, from its lowest note to its highest, so the pitch is found in
/// it and nothing outside it counts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    /// Soprano and alto: about C#3 to C#6.
    High,
    /// Tenor and alto: about F2 to F5.
    #[default]
    Middle,
    /// Baritone and bass: about B1 to A4.
    Low,
    /// A melodic instrument, such as a violin, a trumpet or a bass guitar: about B1 to G6.
    Instrument,
}

impl Voice {
    pub const ALL: [Self; 4] = [Self::High, Self::Middle, Self::Low, Self::Instrument];

    /// The lowest and the highest fundamental, in Hz.
    pub fn range_hz(self) -> (f64, f64) {
        match self {
            Self::High => (140.0, 1_100.0),
            Self::Middle => (85.0, 700.0),
            Self::Low => (60.0, 450.0),
            Self::Instrument => (60.0, 1_600.0),
        }
    }
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<PitchCorrectionState>;

pub const RETUNE: Parameter = Parameter {
    field: "retune_ms",
    min: 0.0,
    max: 400.0,
    default: 50.0,
    scale: sound_core::Scale::Linear,
    get: |state| state.retune_ms,
    set: |state, value| state.retune_ms = value,
};
pub const HUMANIZE: Parameter = Parameter {
    field: "humanize",
    min: 0.0,
    max: 1.0,
    default: 0.0,
    scale: sound_core::Scale::Linear,
    get: |state| state.humanize,
    set: |state, value| state.humanize = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    scale: sound_core::Scale::Linear,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 3] = [&RETUNE, &HUMANIZE, &MIX];

impl Default for PitchCorrectionState {
    fn default() -> Self {
        Self {
            key: Key::C,
            scale: Scale::Chromatic,
            retune_ms: RETUNE.default,
            humanize: HUMANIZE.default,
            voice: Voice::Middle,
            keep_formants: false,
            mix: MIX.default,
        }
    }
}

impl State for PitchCorrectionState {
    const TOOL: &'static str = "pitch-correction";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the pitch correction record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "pitch-correction",
    group: "Devices",
    when: "Pitch correction or hard tuning on a voice",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the pitch correction tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<PitchCorrectionState>(EXTENSION)?
        .behaviour(apply)
        .schema(PitchCorrection::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit.
fn apply(
    state: &PitchCorrectionState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let meters = Meters {
        heard: context.watch(Meters::HEARD),
        target: context.watch(Meters::TARGET),
        correction: context.watch(Meters::CORRECTION),
    };
    let correction =
        context.processor("pitch-correction", || PitchCorrection::new(*state, meters))?;
    context.update(correction, *state)?;
    context.input(
        AUDIO_INPUT,
        InputEndpoint::new(correction, PitchCorrection::INPUT),
    );
    context.input(
        NOTES_INPUT,
        InputEndpoint::new(correction, PitchCorrection::NOTES),
    );
    context.automation(correction, PitchCorrection::AUTOMATION);
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(correction, PitchCorrection::OUTPUT),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::testing::check_parameter_ranges;

    use super::*;

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        let state = PitchCorrectionState::default();
        assert_eq!(
            check_parameter_ranges(&state, &PARAMETERS, |state| state),
            Ok(())
        );
    }

    #[test]
    fn a_scale_on_a_key_holds_the_notes_of_that_key() {
        // A major: A, B, C#, D, E, F#, G#.
        let notes = Scale::Major.notes(Key::A);
        let expected = [9, 11, 1, 2, 4, 6, 8]
            .iter()
            .fold(0, |notes, note| notes | 1 << note);
        assert_eq!(notes, expected);
        assert_eq!(Scale::Chromatic.notes(Key::FSharp), 0xfff);
    }
}
