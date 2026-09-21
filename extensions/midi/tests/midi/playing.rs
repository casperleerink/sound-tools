//! Playing: a message reaches the instrument at the start of the next audio block, whether the
//! project plays or not, and nothing is lost on the way.

use midi::INPUT_CAPACITY;
use sound_core::Ticks;
use sound_notes::NoteEvent;

use crate::support::{Harness, off, on, pedal, pitch};

#[test]
fn a_key_sounds_at_the_start_of_the_next_block() {
    let mut harness = Harness::new();
    // The engine has run for a while, so the frame is not zero by accident.
    harness.run(512, 512);
    assert_eq!(harness.heard(), []);

    harness.input.send(on(60, 88));
    harness.run(512, 512);
    let heard = harness.heard();
    assert_eq!(heard.len(), 1, "{heard:?}");
    // The first frame of the block that came after the message.
    assert_eq!(heard[0].0, 512);
    assert!(matches!(heard[0].1, NoteEvent::On { .. }));
}

/// Nothing about MIDI input waits for the project to play: a keyboard sounds while the
/// transport is stopped, which is how a composer tries a sound.
#[test]
fn a_key_sounds_while_the_project_is_stopped() {
    let mut harness = Harness::new();
    harness.input.send(on(60, 88));
    harness.input.send(off(60));
    harness.run(64, 64);
    let heard = harness.heard();
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert_eq!(
        heard[0],
        (
            0,
            NoteEvent::On {
                pitch: pitch(60),
                velocity: sound_notes::Velocity::new(88).unwrap()
            }
        )
    );
    assert_eq!(heard[1], (0, NoteEvent::Off { pitch: pitch(60) }));
}

/// The engine splits a device buffer into sub-blocks of 64 frames and takes what arrived at
/// the start of each of them, so the wait never grows with the device buffer size beyond one
/// buffer: a message that arrives during a callback sounds at the start of the next one.
#[test]
fn every_message_of_a_burst_arrives_in_order_on_one_block() {
    let mut harness = Harness::new();
    let notes = [60, 64, 67, 72];
    for pitch in notes {
        harness.input.send(on(pitch, 100));
    }
    harness.input.send(pedal(127));
    harness.run(512, 512);
    let heard = harness.heard();
    assert_eq!(heard.len(), 5, "{heard:?}");
    assert!(heard.iter().all(|(frame, _)| *frame == 0));
    let pitches: Vec<u8> = heard
        .iter()
        .filter_map(|(_, event)| match event {
            NoteEvent::On { pitch, .. } => Some(pitch.number()),
            _ => None,
        })
        .collect();
    assert_eq!(pitches, notes);
    assert!(matches!(heard[4].1, NoteEvent::Pedal(_)));
}

/// A stop or a seek says nothing about a keyboard. The sequencer of the track sends `AllOff`,
/// which the instrument obeys; the MIDI input itself sends nothing of its own.
#[test]
fn a_stop_and_a_seek_send_nothing_from_the_keyboard() {
    let mut harness = Harness::new();
    harness.control.play();
    harness.run(512, 512);
    harness.input.send(on(60, 88));
    harness.run(512, 512);
    harness.control.seek(Ticks(3840));
    harness.control.stop();
    harness.run(512, 512);
    let heard = harness.heard();
    assert_eq!(heard.len(), 1, "{heard:?}");
}

/// The event buffer of a port holds a fixed number of events per block. What does not fit
/// stays in the ring and goes out in the next block, so a key press is never lost.
#[test]
fn a_burst_larger_than_one_event_buffer_sounds_over_several_blocks() {
    let mut harness = Harness::new();
    let capacity = harness.control.config().event_capacity;
    let count = capacity + 40;
    for index in 0..count {
        harness.input.send(on(36 + (index % 60) as u8, 100));
    }
    harness.run(64, 64);
    assert_eq!(harness.heard().len(), capacity);
    harness.run(64, 64);
    assert_eq!(harness.heard().len(), 40);
    assert_eq!(harness.keyboard.lost(), midi::Lost::default());
}

/// The input ring is far larger than any burst. When it does fill up, the messages that did
/// not fit are counted, so the interface can say that something was lost.
#[test]
fn messages_that_do_not_fit_the_input_ring_are_counted() {
    let harness = Harness::new();
    for _ in 0..INPUT_CAPACITY {
        assert!(harness.input.send(on(60, 100)));
    }
    assert!(!harness.input.send(on(60, 100)));
    assert_eq!(harness.keyboard.lost().input, 1);
}

/// The destination is the `notes` port of the instrument of the selected track. Setting the
/// same one again costs nothing, and setting none leaves the keyboard silent.
#[test]
fn the_keyboard_plays_where_it_is_wired_and_nowhere_else() {
    let mut harness = Harness::new();
    let notes = harness.notes_input();
    harness
        .keyboard
        .play_into(&mut harness.control, Some(notes))
        .unwrap();
    assert_eq!(harness.keyboard.destination(), Some(notes));

    harness
        .keyboard
        .play_into(&mut harness.control, None)
        .unwrap();
    harness.input.send(on(60, 88));
    harness.run(512, 512);
    assert_eq!(harness.heard(), []);

    harness
        .keyboard
        .play_into(&mut harness.control, Some(notes))
        .unwrap();
    harness.input.send(on(62, 88));
    harness.run(512, 512);
    assert_eq!(harness.heard().len(), 1);
}
