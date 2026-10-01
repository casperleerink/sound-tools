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
    when: "A track plays the Wavetable synth: morphing pads, basses and leads",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the wavetable tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<WavetableState>(EXTENSION)?.behaviour(apply);
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
    use serde::Serialize;
    use sound_core::{FilterType, LfoShape, Parameter, State};

    use super::*;
    use crate::state::{
        Adsr, Effect, Filter, LfoSettings, Oscillator, Routing, Sub, Unison, VoiceMode, Voicing,
    };

    const DOC: &str = include_str!("../agent-doc.md");

    /// The field, the range and the default of each parameter of an object type.
    fn rows<S>(parameters: &[&Parameter<S>]) -> Vec<(&'static str, f32, f32, f32)> {
        let row = |parameter: &&Parameter<S>| {
            let Parameter {
                field,
                min,
                max,
                default,
                ..
            } = **parameter;
            (field, min, max, default)
        };
        parameters.iter().map(row).collect()
    }

    /// Every parameter of every object type, under the heading of the doc that describes it.
    fn sections() -> Vec<(&'static str, Vec<(&'static str, f32, f32, f32)>)> {
        vec![
            ("## Oscillators", rows(&Oscillator::PARAMETERS)),
            ("## Sub", rows(&Sub::PARAMETERS)),
            ("## Unison", rows(&Unison::PARAMETERS)),
            ("## Filters", rows(&Filter::PARAMETERS)),
            ("## Envelopes", rows(&Adsr::PARAMETERS)),
            ("## LFOs", rows(&LfoSettings::PARAMETERS)),
            ("## Voicing", rows(&Voicing::PARAMETERS)),
            ("## Matrix", rows(&Route::PARAMETERS)),
            ("## Output", rows(&WavetableState::PARAMETERS)),
        ]
    }

    /// The agent doc gives the ranges and the defaults to agents. They are checked against the
    /// one definition, so they cannot drift from it.
    #[test]
    fn the_doc_gives_the_range_and_the_default_of_every_parameter() {
        for (heading, rows) in sections() {
            let start = DOC.find(&format!("{heading}\n")).expect(heading);
            let section = &DOC[start + heading.len()..];
            let section = &section[..section.find("\n## ").unwrap_or(section.len())];
            for (field, min, max, default) in rows {
                let row = format!("| `{field}` |");
                let row = section.lines().find(|line| line.starts_with(&row));
                let row = row.unwrap_or_else(|| panic!("{heading} has no row for {field}"));
                assert!(row.contains(&format!("| {min} to {max} |")), "{row}");
                assert!(row.ends_with(&format!("| {default} |")), "{row}");
            }
        }
    }

    /// Every name a record can hold is in the doc, as it is saved.
    #[test]
    fn the_doc_names_every_choice() {
        fn names<T: Serialize>(all: &[T]) -> Vec<String> {
            let name = |value| serde_json::to_string(value).unwrap().replace('"', "");
            all.iter().map(name).collect()
        }
        let all = [
            names(&Table::ALL),
            names(&Effect::ALL),
            names(&Source::ALL),
            names(&Destination::ALL),
            names(&LfoShape::ALL),
            names(&Routing::ALL),
            names(&VoiceMode::ALL),
            names(&FilterType::ALL),
        ];
        for name in all.iter().flatten() {
            let quoted = format!("`\"{name}\"`");
            let bare = format!("`{name}`");
            assert!(DOC.contains(&quoted) || DOC.contains(&bare), "{name}");
        }
        assert!(DOC.contains("`amp_level`"));
    }

    /// The record in the doc is the default patch.
    #[test]
    fn the_example_of_the_doc_is_the_default_patch() {
        let start = DOC.find("\"tool\": \"wavetable\"").unwrap();
        let start = DOC[..start].rfind('{').unwrap();
        let end = start + DOC[start..].find("\n```").unwrap();
        let record: serde_json::Value = serde_json::from_str(&DOC[start..end]).unwrap();
        let state: WavetableState = serde_json::from_value(record["state"].clone()).unwrap();
        assert_eq!(state, WavetableState::default());
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        fn check<S>(
            parameters: &[&Parameter<S>],
            state: &WavetableState,
            of: fn(&mut WavetableState) -> &mut S,
        ) {
            for parameter in parameters {
                for value in [parameter.min, parameter.default, parameter.max] {
                    let mut state = state.clone();
                    (parameter.set)(of(&mut state), value);
                    assert_eq!((parameter.get)(of(&mut state)), value);
                    assert_eq!(state.validate(), Ok(()));
                }
                let mut state = state.clone();
                (parameter.set)(of(&mut state), parameter.max * 2.0 + 1.0);
                let error = state.validate().unwrap_err();
                assert!(error.contains(parameter.field), "{error}");
            }
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
