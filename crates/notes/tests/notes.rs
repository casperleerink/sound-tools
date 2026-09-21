//! The saved note format and the pitch helper.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use sound_core::{State, Ticks};
use sound_notes::{Clip, Length, Note, NoteError, NoteEvent, Pedal, PedalChange, Pitch, Velocity};

const LINE: &str = r#"{"start":0,"length":480,"pitch":60,"velocity":100}"#;

#[test]
fn a_note_line_round_trips_as_plain_numbers() {
    let note: Note = serde_json::from_str(LINE).unwrap();
    assert_eq!(note.start, Ticks(0));
    assert_eq!(note.length.ticks(), Ticks(480));
    assert_eq!(note.end(), Ticks(480));
    assert_eq!(note.pitch.number(), 60);
    assert_eq!(note.velocity.value(), 100);
    assert_eq!(serde_json::to_string(&note).unwrap(), LINE);
}

#[test]
fn the_form_the_runtime_writes_loads_to_the_same_note() {
    let spaced = r#"{"start": 0, "length": 480, "pitch": 60, "velocity": 100}"#;
    let note: Note = serde_json::from_str(LINE).unwrap();
    assert_eq!(serde_json::from_str::<Note>(spaced).unwrap(), note);
}

#[test]
fn the_ends_of_both_ranges_load() {
    let line = r#"{"start":0,"length":1,"pitch":0,"velocity":1}"#;
    assert!(serde_json::from_str::<Note>(line).is_ok());
    let line = r#"{"start":0,"length":1,"pitch":127,"velocity":127}"#;
    assert!(serde_json::from_str::<Note>(line).is_ok());
}

fn load_error(line: &str) -> String {
    serde_json::from_str::<Note>(line).unwrap_err().to_string()
}

#[test]
fn values_out_of_range_are_rejected_with_the_range_in_the_message() {
    let cases = [
        ("128", "100", "pitch must be from 0 to 127, not 128"),
        ("-1", "100", "pitch must be from 0 to 127, not -1"),
        ("300", "100", "pitch must be from 0 to 127, not 300"),
        ("60", "0", "velocity must be from 1 to 127, not 0"),
        ("60", "128", "velocity must be from 1 to 127, not 128"),
    ];
    for (pitch, velocity, message) in cases {
        let line = format!(r#"{{"start":0,"length":480,"pitch":{pitch},"velocity":{velocity}}}"#);
        let error = load_error(&line);
        assert!(error.starts_with(message), "{error}");
    }
    let error = load_error(r#"{"start":0,"length":0,"pitch":60,"velocity":100}"#);
    assert!(
        error.starts_with("length must be 1 tick or more, not 0"),
        "{error}"
    );
    assert_eq!(Length::new(Ticks(0)), Err(NoteError::Length));
    assert_eq!(Pitch::new(128), Err(NoteError::Pitch(128)));
    assert_eq!(Velocity::new(0), Err(NoteError::Velocity(0)));
}

#[test]
fn a_misspelled_field_a_fraction_and_a_negative_time_are_rejected() {
    let error = load_error(r#"{"start":0,"length":480,"pitch":60,"velocity":100,"pan":0}"#);
    assert!(error.starts_with("unknown field `pan`"), "{error}");
    let error = load_error(r#"{"start":0,"length":480,"pitch":60.5,"velocity":100}"#);
    assert!(error.starts_with("invalid type: floating point"), "{error}");
    let error = load_error(r#"{"start":-1,"length":480,"pitch":60,"velocity":100}"#);
    assert!(error.starts_with("invalid value: integer `-1`"), "{error}");
}

#[test]
fn pitch_69_is_440_hz_and_each_octave_doubles() {
    let hz = |number| Pitch::new(number).unwrap().frequency_hz();
    assert_eq!(hz(69), 440.0);
    assert!((hz(60) - 261.63).abs() < 0.005, "{}", hz(60));
    assert_eq!(hz(81), 880.0);
    assert_eq!(hz(57), 220.0);
    assert!((hz(0) - 8.176).abs() < 0.001, "{}", hz(0));
    assert!((hz(127) - 12_543.85).abs() < 0.05, "{}", hz(127));
}

#[test]
fn a_note_gives_its_own_events() {
    let note: Note = serde_json::from_str(LINE).unwrap();
    let (pitch, velocity) = (note.pitch, note.velocity);
    assert_eq!(note.on(), NoteEvent::On { pitch, velocity });
    assert_eq!(note.off(), NoteEvent::Off { pitch });
}

#[test]
fn the_pedal_is_a_number_from_0_to_127_and_is_down_from_64() {
    assert_eq!(Pedal::UP.value(), 0);
    assert!(!Pedal::UP.is_down());
    assert!(!Pedal::new(63).unwrap().is_down());
    assert!(Pedal::new(64).unwrap().is_down());
    assert!(Pedal::new(127).unwrap().is_down());
    assert_eq!(Pedal::new(128), Err(NoteError::Pedal(128)));
    assert_eq!(Pedal::nearest(-5).value(), 0);
    assert_eq!(Pedal::nearest(500).value(), 127);
    let line = r#"{"start":480,"value":127}"#;
    let change: PedalChange = serde_json::from_str(line).unwrap();
    assert_eq!(change.start, Ticks(480));
    assert_eq!(change.value.value(), 127);
    assert_eq!(serde_json::to_string(&change).unwrap(), line);
}

/// A clip of before the pedal existed loads and is written back byte for byte as it was, so
/// nothing in an old project is rewritten by opening it.
#[test]
fn a_clip_without_pedal_loads_and_saves_without_the_field() {
    let line = r#"{"start":0,"length":3840,"notes":[]}"#;
    let clip: Clip = serde_json::from_str(line).unwrap();
    assert!(clip.pedal.is_empty());
    assert_eq!(serde_json::to_string(&clip).unwrap(), line);
    assert_eq!(
        clip,
        Clip::new(Ticks(0), Length::new(Ticks(3840)).unwrap(), Vec::new())
    );
}

#[test]
fn a_clip_holds_the_pedal_and_places_it_on_the_timeline() {
    let line = r#"{"start":960,"length":3840,"notes":[],"pedal":[{"start":0,"value":127},{"start":960,"value":0}]}"#;
    let mut clip: Clip = serde_json::from_str(line).unwrap();
    assert_eq!(serde_json::to_string(&clip).unwrap(), line);
    let placed: Vec<_> = clip.placed_pedal().map(|it| (it.start, it.value)).collect();
    assert_eq!(
        placed,
        vec![
            (Ticks(960), Pedal::new(127).unwrap()),
            (Ticks(1920), Pedal::UP),
        ]
    );
    // A shorter clip cannot hold a pedal move past its end, as it cannot hold such a note.
    clip.set_length(Length::new(Ticks(480)).unwrap());
    assert_eq!(clip.pedal.len(), 1);
    assert!(clip.validate().is_ok());
}

#[test]
fn a_pedal_move_outside_the_clip_does_not_load() {
    let line = r#"{"start":0,"length":480,"notes":[],"pedal":[{"start":480,"value":64}]}"#;
    let clip: Clip = serde_json::from_str(line).unwrap();
    let error = clip.validate().unwrap_err();
    assert!(
        error.starts_with("pedal[0].start must be less than"),
        "{error}"
    );
}

#[test]
fn interface_math_gets_the_nearest_valid_value() {
    assert_eq!(Pitch::nearest(-5).number(), 0);
    assert_eq!(Pitch::nearest(64).number(), 64);
    assert_eq!(Pitch::nearest(500).number(), 127);
    assert_eq!(Velocity::nearest(0).value(), 1);
    assert_eq!(Velocity::nearest(100).value(), 100);
    assert_eq!(Velocity::nearest(128).value(), 127);
    assert_eq!(Length::at_least_one(Ticks(0)).ticks(), Ticks(1));
    assert_eq!(Length::at_least_one(Ticks(480)).ticks(), Ticks(480));
}
