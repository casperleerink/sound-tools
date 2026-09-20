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
//! `README.md` in this crate has the units, ranges and ports.

mod synth;

use std::ops::RangeInclusive;

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

impl Default for SynthState {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            cutoff_hz: 2_000.0,
            resonance: 0.2,
            attack_seconds: 0.005,
            decay_seconds: 0.2,
            sustain: 0.7,
            release_seconds: 0.3,
            gain: 0.15,
        }
    }
}

/// Envelope times. The lower end keeps every stage long enough not to click.
const TIME_RANGE: RangeInclusive<f32> = 0.001..=10.0;

impl State for SynthState {
    const TOOL: &'static str = "instrument.synth";

    fn validate(&self) -> Result<(), String> {
        let check = |field: &str, value: f32, range: RangeInclusive<f32>| {
            if range.contains(&value) {
                return Ok(());
            }
            let (low, high) = range.into_inner();
            Err(format!("{field} must be from {low} to {high}, not {value}"))
        };
        check("cutoff_hz", self.cutoff_hz, 20.0..=20_000.0)?;
        check("resonance", self.resonance, 0.0..=1.0)?;
        check("attack_seconds", self.attack_seconds, TIME_RANGE)?;
        check("decay_seconds", self.decay_seconds, TIME_RANGE)?;
        check("sustain", self.sustain, 0.0..=1.0)?;
        check("release_seconds", self.release_seconds, TIME_RANGE)?;
        check("gain", self.gain, 0.0..=1.0)
    }
}

/// Registers the synth tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<SynthState>(EXTENSION)?.behaviour(apply);
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
