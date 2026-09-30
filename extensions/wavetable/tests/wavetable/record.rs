//! The record: `{}` is the default patch, and a value out of range names its field.

use sound_core::State;
use wavetable::{Destination, MAX_ROUTES, Route, Source, WavetableState};

use crate::support::{Harness, id};

/// Writes a record with this state, as an agent would, and reads back what loaded.
fn load(harness: &mut Harness, state: &str) -> Option<WavetableState> {
    let record = format!("{{\"tool\": \"wavetable\", \"state\": {state}}}");
    harness.write_and_apply("state/synth.json", &record);
    let instance = harness.project.resolve::<WavetableState>(&id("synth"))?;
    harness.project.state(&instance).cloned()
}

/// Every problem the project lists, one per line.
fn problems(harness: &Harness) -> String {
    let problems = harness.project.problems();
    let messages = problems.iter().map(|problem| problem.message.clone());
    messages.collect::<Vec<_>>().join("\n")
}

#[test]
fn an_empty_state_is_the_default_patch() {
    let mut harness = Harness::new();
    let loaded = load(&mut harness, "{}").unwrap();
    assert_eq!(loaded, WavetableState::default());
    assert_eq!(loaded.validate(), Ok(()));
    // A nested object left partly out takes the defaults of its type.
    let loaded = load(&mut harness, r#"{"osc_2": {"position": 0.25}}"#).unwrap();
    assert_eq!(loaded.osc_2.position, 0.25);
    assert_eq!(loaded.osc_2.detune_cents, 0.0);
    assert_eq!(loaded.osc_1, WavetableState::default().osc_1);
}

#[test]
fn a_value_out_of_range_or_a_field_that_does_not_exist_does_not_load() {
    let cases = [
        (r#"{"osc_1": {"position": 2.0}}"#, "osc_1.position"),
        (r#"{"filter_2": {"cutoff_hz": 5.0}}"#, "filter_2.cutoff_hz"),
        (r#"{"env_3": {"sustain": -1.0}}"#, "env_3.sustain"),
        (r#"{"unison": {"voices": 9}}"#, "unison.voices"),
        (r#"{"voicing": {"polyphony": 0}}"#, "voicing.polyphony"),
        (
            r#"{"matrix": [{"source": "key", "destination": "pan", "amount": 3.0}]}"#,
            "matrix[0].amount",
        ),
        (r#"{"osc_1": {"postion": 0.5}}"#, "postion"),
        (r#"{"sub": {"octave": -3}}"#, "octave"),
        (r#"{"filter_1": {"slope": 18}}"#, "slope"),
        (r#"{"osc_1": {"table": "nope"}}"#, "table"),
    ];
    for (state, field) in cases {
        let mut harness = Harness::new();
        assert_eq!(load(&mut harness, state), None, "{state}");
        let problems = problems(&harness);
        assert!(problems.contains(field), "{state}: {problems}");
    }
}

#[test]
fn a_matrix_has_at_most_sixteen_routes() {
    let route = Route {
        source: Source::Lfo1,
        destination: Destination::Pan,
        amount: 0.1,
    };
    let mut state = WavetableState {
        matrix: vec![route; MAX_ROUTES],
        ..WavetableState::default()
    };
    assert_eq!(state.validate(), Ok(()));
    state.matrix.push(route);
    assert!(
        state
            .validate()
            .is_err_and(|error| error.contains("matrix"))
    );
}
