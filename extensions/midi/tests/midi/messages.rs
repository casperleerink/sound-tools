//! Reading MIDI bytes: what this application uses and what it leaves alone.

use midi::Played;
use sound_notes::{NoteEvent, Pedal};

use crate::support::{off, on, pedal, pitch};

#[test]
fn note_on_note_off_and_the_sustain_pedal_are_read() {
    // Channel 1 and channel 10: all channels are merged, so both give the same message.
    assert_eq!(Played::from_bytes(&[0x90, 60, 88]), Some(on(60, 88)));
    assert_eq!(Played::from_bytes(&[0x99, 60, 88]), Some(on(60, 88)));
    assert_eq!(
        Played::from_bytes(&[0x80, 60, 64]),
        Some(Played::Off {
            pitch: pitch(60),
            velocity: 64
        })
    );
    assert_eq!(Played::from_bytes(&[0xB0, 64, 127]), Some(pedal(127)));
    assert_eq!(Played::from_bytes(&[0xB0, 64, 0]), Some(pedal(0)));
    // Half pedal is kept as it was played.
    assert_eq!(Played::from_bytes(&[0xB0, 64, 40]), Some(pedal(40)));
}

/// Every keyboard may end a note with a note on of velocity 0. The note contract has no
/// velocity 0, so it would not load as a note on at all.
#[test]
fn a_note_on_of_velocity_zero_is_a_note_off() {
    assert_eq!(
        Played::from_bytes(&[0x90, 60, 0]),
        Some(Played::Off {
            pitch: pitch(60),
            velocity: 0
        })
    );
}

#[test]
fn other_controllers_and_messages_are_left_alone() {
    let ignored: [&[u8]; 6] = [
        &[0xB0, 1, 127], // mod wheel
        &[0xE0, 0, 96],  // pitch bend
        &[0xD0, 80],     // channel pressure
        &[0xA0, 60, 80], // polyphonic key pressure
        &[0xF8],         // MIDI clock
        &[0xC0, 4],      // program change
    ];
    for bytes in ignored {
        assert_eq!(Played::from_bytes(bytes), None, "{bytes:?}");
    }
    // Not a message at all.
    assert_eq!(Played::from_bytes(&[]), None);
    assert_eq!(Played::from_bytes(&[60, 88]), None);
}

#[test]
fn a_message_becomes_the_event_the_note_contract_has() {
    assert_eq!(
        on(60, 88).event(),
        NoteEvent::On {
            pitch: pitch(60),
            velocity: sound_notes::Velocity::new(88).unwrap()
        }
    );
    assert_eq!(off(60).event(), NoteEvent::Off { pitch: pitch(60) });
    assert_eq!(
        pedal(127).event(),
        NoteEvent::Pedal(Pedal::new(127).unwrap())
    );
}
