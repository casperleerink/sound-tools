//! A hosted plugin plays: the notes land on the frames they were sent on, and the sustain
//! pedal reaches the plugin with its value.

use crate::support::{Harness, Played, record};

#[test]
fn the_notes_reach_the_plugin_on_the_frames_they_were_sent_on() {
    let mut harness = Harness::new();
    harness.add_track(
        record("piano"),
        vec![
            Played::On {
                frame: 100,
                pitch: 60,
                velocity: 100,
            },
            Played::Off {
                frame: 300,
                pitch: 60,
            },
        ],
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = harness.play(1024);
    let left = render.left();

    // The test plugin starts a cosine at full amplitude, so the note is audible from exactly
    // the frame it arrived on, and silent again from the frame of its note off.
    assert_eq!(render.first_sound(), Some(100));
    assert_eq!(left[99], 0.0);
    assert_ne!(left[100], 0.0);
    assert_eq!(&left[300..], &vec![0.0; left.len() - 300][..]);
}

#[test]
fn the_sustain_pedal_reaches_the_plugin_with_its_value() {
    let mut harness = Harness::new();
    harness.add_track(
        record("piano"),
        vec![
            Played::Pedal {
                frame: 64,
                value: 100,
            },
            Played::Pedal {
                frame: 256,
                value: 40,
            },
        ],
    );
    let render = harness.play(512);
    let right = render.right();

    // The test plugin writes the pedal it received into the right channel, as a number.
    assert_eq!(right[63], 0.0);
    assert_eq!(right[64], 100.0 / 127.0);
    assert_eq!(right[255], 100.0 / 127.0);
    assert_eq!(right[256], 40.0 / 127.0);
}

/// The plugin has no idea the transport stopped. The contract's `AllOff` becomes a note off
/// for every key this wrapper started, and the pedal up.
#[test]
fn all_off_ends_every_key_the_wrapper_started_and_lifts_the_pedal() {
    let mut harness = Harness::new();
    harness.add_track(
        record("piano"),
        vec![
            Played::On {
                frame: 0,
                pitch: 60,
                velocity: 100,
            },
            Played::On {
                frame: 0,
                pitch: 64,
                velocity: 100,
            },
            Played::Pedal {
                frame: 0,
                value: 127,
            },
            Played::AllOff { frame: 256 },
        ],
    );
    let render = harness.play(512);
    let (left, right) = (render.left(), render.right());
    assert_ne!(left[255], 0.0);
    assert_eq!(&left[256..], &vec![0.0; 256][..]);
    assert_eq!(right[255], 1.0);
    assert_eq!(right[256], 0.0);
}

/// The realtime sanitizer runs over this whole render. It aborts on an allocation, a lock or a
/// system call anywhere in our own `process`, including the wrapper around the plugin.
#[test]
fn the_wrapper_makes_no_allocation_lock_or_system_call_while_it_plays() {
    let mut harness = Harness::new();
    let played = (0..64_u64)
        .flat_map(|index| {
            let pitch = 40 + (index % 40) as u8;
            [
                Played::On {
                    frame: index * 13,
                    pitch,
                    velocity: 100,
                },
                Played::Pedal {
                    frame: index * 13 + 3,
                    value: (index % 128) as u8,
                },
                Played::Off {
                    frame: index * 13 + 7,
                    pitch,
                },
            ]
        })
        .collect();
    harness.add_track(record("piano"), played);
    let render = harness.play(4096);
    assert!(render.samples().iter().any(|sample| *sample != 0.0));
}
