//! Instrument: one subtractive synth. It plays the note events of the `sound-notes` contract.
//!
//! A record on disk, `state/<name>.json`, or `instrument.json` inside a track folder:
//!
//! ```json
//! {
//!   "tool": "instrument.synth",
//!   "state": {
//!     "waveform": "saw",
//!     "cutoff_hz": 2000.0,
//!     "resonance": 0.2,
//!     "attack_seconds": 0.005,
//!     "decay_seconds": 0.2,
//!     "sustain": 0.7,
//!     "release_seconds": 0.3,
//!     "gain": 0.15
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the units, ranges and ports. [`view`] is the interface of the
//! synth, and the only module here that uses GPUI.

mod synth;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry, RegistryError, State,
};
use sound_notes::{AUDIO_OUTPUT, NOTES_INPUT};

pub use synth::{Synth, VOICES};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "instrument";

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Saw,
    Square,
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets.
/// Voices, phases and envelope levels are runtime state and are not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default synth.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SynthState {
    pub waveform: Waveform,
    /// Where the low-pass filter starts to cut.
    pub cutoff_hz: f32,
    /// 0 is a flat filter. 1 is a strong peak at the cutoff. It never oscillates on its own.
    /// The filter input is turned down as this goes up, so the peak does not overload.
    pub resonance: f32,
    /// From note on to full level.
    pub attack_seconds: f32,
    /// From full level to within 0.1% of the sustain level.
    pub decay_seconds: f32,
    /// The level a held note settles at, as a part of full level.
    pub sustain: f32,
    /// From note off to silence, for a note at full level. A quieter note ends a little sooner.
    pub release_seconds: f32,
    /// Linear output gain. With the default filter one note at velocity 127 peaks at about
    /// this value. Tracks sum to the device with no mixer yet, so the default is low.
    pub gain: f32,
}

/// One number of the saved state: its field, its range and its default. The range and the
/// default of a field are written here and nowhere else. `validate`, `Default`, the knobs of
/// the view and a test of the docs all read them.
pub struct Parameter {
    pub field: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub get: fn(&SynthState) -> f32,
    pub set: fn(&mut SynthState, f32),
}

impl Parameter {
    /// An envelope time. The lower end keeps every stage long enough not to click.
    const fn time(
        field: &'static str,
        default: f32,
        get: fn(&SynthState) -> f32,
        set: fn(&mut SynthState, f32),
    ) -> Self {
        let (min, max) = (0.001, 10.0);
        Self {
            field,
            min,
            max,
            default,
            get,
            set,
        }
    }

    fn check(&self, state: &SynthState) -> Result<(), String> {
        let (
            Self {
                field, min, max, ..
            },
            value,
        ) = (self, (self.get)(state));
        if (*min..=*max).contains(&value) {
            return Ok(());
        }
        Err(format!("{field} must be from {min} to {max}, not {value}"))
    }
}

pub const CUTOFF: Parameter = Parameter {
    field: "cutoff_hz",
    min: 20.0,
    max: 20_000.0,
    default: 2_000.0,
    get: |state| state.cutoff_hz,
    set: |state, value| state.cutoff_hz = value,
};
pub const RESONANCE: Parameter = Parameter {
    field: "resonance",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    get: |state| state.resonance,
    set: |state, value| state.resonance = value,
};
pub const ATTACK: Parameter = Parameter::time(
    "attack_seconds",
    0.005,
    |state| state.attack_seconds,
    |state, value| state.attack_seconds = value,
);
pub const DECAY: Parameter = Parameter::time(
    "decay_seconds",
    0.2,
    |state| state.decay_seconds,
    |state, value| state.decay_seconds = value,
);
pub const SUSTAIN: Parameter = Parameter {
    field: "sustain",
    min: 0.0,
    max: 1.0,
    default: 0.7,
    get: |state| state.sustain,
    set: |state, value| state.sustain = value,
};
pub const RELEASE: Parameter = Parameter::time(
    "release_seconds",
    0.3,
    |state| state.release_seconds,
    |state, value| state.release_seconds = value,
);
pub const GAIN: Parameter = Parameter {
    field: "gain",
    min: 0.0,
    max: 1.0,
    default: 0.15,
    get: |state| state.gain,
    set: |state, value| state.gain = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 7] = [
    &CUTOFF, &RESONANCE, &ATTACK, &DECAY, &SUSTAIN, &RELEASE, &GAIN,
];

impl Default for SynthState {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            cutoff_hz: CUTOFF.default,
            resonance: RESONANCE.default,
            attack_seconds: ATTACK.default,
            decay_seconds: DECAY.default,
            sustain: SUSTAIN.default,
            release_seconds: RELEASE.default,
            gain: GAIN.default,
        }
    }
}

impl State for SynthState {
    const TOOL: &'static str = "instrument.synth";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// Registers the synth tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<SynthState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, include_str!("../agent-doc.md"));
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so held
/// notes go on through parameter edits.
fn apply(state: &SynthState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let synth = context.processor("synth", || Synth::new(*state))?;
    context.update(synth, *state)?;
    context.input(NOTES_INPUT, InputEndpoint::new(synth, Synth::NOTES));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(synth, Synth::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The docs give the ranges and the defaults to agents and to people. They are checked
    /// against the one definition, so they cannot drift from it.
    #[test]
    fn the_docs_give_the_range_and_the_default_of_every_parameter() {
        let docs = [
            ("agent-doc.md", include_str!("../agent-doc.md")),
            ("README.md", include_str!("../README.md")),
        ];
        for (name, doc) in docs {
            for Parameter {
                field,
                min,
                max,
                default,
                ..
            } in PARAMETERS
            {
                let row = format!("| `{field}` |");
                let row = doc.lines().find(|line| line.starts_with(&row));
                let row = row.unwrap_or_else(|| panic!("{name} has no row for {field}"));
                assert!(
                    row.contains(&format!("| {min} to {max} |")),
                    "{name}: {row}"
                );
                // The agent doc gives the defaults in its example, which another test loads.
                if name == "README.md" {
                    assert!(row.contains(&format!("| {default} |")), "{name}: {row}");
                }
            }
        }
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for parameter in PARAMETERS {
            for value in [parameter.min, parameter.default, parameter.max] {
                let mut state = SynthState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = SynthState::default();
            (parameter.set)(&mut state, parameter.max * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }
}
