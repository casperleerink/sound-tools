//! Wavetable: a synth that plays frames of single cycles and morphs between them. It plays the
//! note events of the `sound-notes` contract.
//!
//! Per voice: two wavetable oscillators, each with unison copies and one effect (FM, sync, warp
//! or fold), a sine sub, two filters in series, in parallel or split, an amp envelope, two
//! more envelopes and two LFOs, all tied together by a modulation matrix. The tables are made
//! in code ([`Table`]) and band-limited per octave with an FFT, so no note aliases.
//!
//! A record on disk, `state/<name>.json`, or `instrument.json` inside a track folder. `{}` is
//! the default patch:
//!
//! ```json
//! {
//!   "tool": "wavetable",
//!   "state": {
//!     "osc_1": { "table": "basic_shapes", "position": 0.5 },
//!     "filter_1": { "cutoff_hz": 800.0 },
//!     "matrix": [{ "source": "env_2", "destination": "filter_1_cutoff", "amount": 0.5 }]
//!   }
//! }
//! ```
//!
//! [`state`] is the record and its `Parameter` constants, [`Table`] the built-in tables and
//! [`Wavetable`] their frames, which [`view`] draws on the card of the synth.

mod dsp;
mod matrix;
pub mod state;
mod synth;
mod tables;
pub mod view;
mod voice;

use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError,
};
use sound_notes::{AUDIO_OUTPUT, NOTES_INPUT};

use crate::synth::{Update, WavetableSynth};

pub use matrix::{Destination, KEY_SEMITONES, MAX_ROUTES, ROUTE_AMOUNT, Route, Source};
pub use state::WavetableState;
pub use tables::{Category, FRAME_LENGTH, Table, Wavetable, wavetable};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "wavetable";

/// The doc of the wavetable record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "wavetable",
    when: "A track plays the `wavetable` synth: morphing pads, basses, leads",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the wavetable tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<WavetableState>(EXTENSION)?
        .behaviour(apply)
        .schema(WavetableSynth::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so held
/// notes go on through edits. A table is made the first time a record picks it.
fn apply(state: &WavetableState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let table = |table| wavetable(table).map_err(BehaviourError::Other);
    let tables = [table(state.osc_1.table)?, table(state.osc_2.table)?];
    let synth = context.processor("wavetable", || {
        WavetableSynth::new(state.clone(), tables.clone())
    })?;
    context.update(synth, Update::new(state.clone(), tables))?;
    context.input(
        NOTES_INPUT,
        InputEndpoint::new(synth, WavetableSynth::NOTES),
    );
    context.automation(synth, WavetableSynth::AUTOMATION);
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(synth, WavetableSynth::OUTPUT),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::Parameter;
    use sound_core::testing::check_parameter_ranges;

    use super::*;
    use crate::state::{Adsr, Filter, LfoSettings, Oscillator, Sub, Unison, Voicing};

    const DOC: &str = include_str!("../agent-doc.md");

    /// The doc says the pad of its example keeps the routes of the default patch first.
    #[test]
    fn the_example_of_the_doc_keeps_the_matrix_of_the_default_patch() {
        let start = DOC.find("\"tool\": \"wavetable\"").unwrap();
        let start = DOC[..start].rfind('{').unwrap();
        let end = start + DOC[start..].find("\n```").unwrap();
        let record: serde_json::Value = serde_json::from_str(&DOC[start..end]).unwrap();
        let state: WavetableState = serde_json::from_value(record["state"].clone()).unwrap();
        let default = WavetableState::default().matrix;
        assert_eq!(state.matrix[..default.len()], default[..]);
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        fn check<S>(
            parameters: &[&Parameter<S>],
            state: &WavetableState,
            of: fn(&mut WavetableState) -> &mut S,
        ) {
            assert_eq!(check_parameter_ranges(state, parameters, of), Ok(()));
        }
        let state = WavetableState {
            matrix: vec![Route {
                source: Source::Key,
                destination: Destination::Pan,
                amount: 0.5,
            }],
            ..WavetableState::default()
        };
        check(&Oscillator::PARAMETERS, &state, |state| &mut state.osc_2);
        check(&Sub::PARAMETERS, &state, |state| &mut state.sub);
        check(&Unison::PARAMETERS, &state, |state| &mut state.unison);
        check(&Filter::PARAMETERS, &state, |state| &mut state.filter_2);
        check(&Adsr::PARAMETERS, &state, |state| &mut state.env_3);
        check(&LfoSettings::PARAMETERS, &state, |state| &mut state.lfo_2);
        check(&Voicing::PARAMETERS, &state, |state| &mut state.voicing);
        check(&Route::PARAMETERS, &state, |state| &mut state.matrix[0]);
        check(&WavetableState::PARAMETERS, &state, |state| state);
    }
}
