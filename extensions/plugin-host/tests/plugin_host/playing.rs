//! A hosted plugin plays: the notes land on the frames they were sent on, and the sustain
//! pedal reaches the plugin with its value.

use plugin_host::PluginFormat;
use sound_core::MAX_BLOCK;

use crate::support::{
    FORMATS, Harness, Played, record, tell_the_plugin, tell_the_plugin_to_go_silent,
};

#[test]
fn the_notes_reach_the_plugin_on_the_frames_they_were_sent_on() {
    for format in FORMATS {
        notes_on_their_frames(format);
    }
}

fn notes_on_their_frames(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
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
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    let render = harness.play(1024);
    let left = render.left();

    // The test plugin starts a cosine at full amplitude, so the note is audible from exactly
    // the frame it arrived on, and silent again from the frame of its note off.
    assert_eq!(render.first_sound(), Some(100), "{format:?}");
    assert_eq!(left[99], 0.0, "{format:?}");
    assert_ne!(left[100], 0.0, "{format:?}");
    assert_eq!(&left[300..], &vec![0.0; left.len() - 300][..], "{format:?}");
}

#[test]
fn the_sustain_pedal_reaches_the_plugin_with_its_value() {
    for format in FORMATS {
        pedal_with_its_value(format);
    }
}

fn pedal_with_its_value(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
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
    assert_eq!(right[63], 0.0, "{format:?}");
    assert_eq!(right[64], 100.0 / 127.0, "{format:?}");
    assert_eq!(right[255], 100.0 / 127.0, "{format:?}");
    assert_eq!(right[256], 40.0 / 127.0, "{format:?}");
}

/// The plugin has no idea the transport stopped. The contract's `AllOff` becomes a note off
/// for every key this wrapper started, and the pedal up.
#[test]
fn all_off_ends_every_key_the_wrapper_started_and_lifts_the_pedal() {
    for format in FORMATS {
        all_off_ends_everything(format);
    }
}

fn all_off_ends_everything(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
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
    assert_ne!(left[255], 0.0, "{format:?}");
    assert_eq!(&left[256..], &vec![0.0; 256][..], "{format:?}");
    assert_eq!(right[255], 1.0, "{format:?}");
    assert_eq!(right[256], 0.0, "{format:?}");
}

/// The realtime sanitizer runs over this whole render. It aborts on an allocation, a lock or a
/// system call anywhere in our own `process`, including the wrapper around the plugin.
#[test]
fn the_wrapper_makes_no_allocation_lock_or_system_call_while_it_plays() {
    for format in FORMATS {
        no_allocation_while_it_plays(format);
    }
}

fn no_allocation_while_it_plays(format: PluginFormat) {
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
    harness.add_track(record(format, "piano"), played);
    let render = harness.play(4096);
    assert!(
        render.samples().iter().any(|sample| *sample != 0.0),
        "{format:?}"
    );
}

/// A plugin may send events out. Nothing here reads them, and a host that gave the plugin a
/// buffer of its own would have to grow that buffer while the plugin pushed into it, on the
/// audio thread. The realtime sanitizer cannot see that, because it is switched off for the
/// plugin's own call, so this counts every allocation of the process instead: fifty thousand
/// events out of every block, and not one allocation.
#[test]
fn a_plugin_that_sends_more_events_than_any_buffer_holds_allocates_nothing() {
    for format in FORMATS {
        sends_more_than_any_buffer_holds(format);
    }
}

/// CLAP sends events out, VST 3 sends parameter changes. Both are things a host must have
/// somewhere to put that neither grows nor allocates on the audio thread.
fn sends_more_than_any_buffer_holds(format: PluginFormat) {
    tell_the_plugin(None, Some(50_000));
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
        vec![Played::On {
            frame: 0,
            pitch: 60,
            velocity: 100,
        }],
    );
    harness.project.engine().play();
    // From the very first block, because a buffer of the host's own would grow once and then
    // be big enough: the growth has to be inside the window that is counted.
    let (render, allocations) = harness.render_counting_allocations(8192);
    tell_the_plugin(None, None);
    assert_eq!(allocations, 0, "the audio thread allocated, {format:?}");
    assert!(render.first_sound().is_some(), "{format:?}");
}

/// VST 3 lets a plugin say its output is silent and leave the buffer as it is. A host that
/// does not clear its own output buffers would then play the block before over and over.
#[test]
fn a_plugin_that_says_its_output_is_silent_is_heard_as_silence() {
    tell_the_plugin_to_go_silent();
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![Played::On {
            frame: 0,
            pitch: 60,
            velocity: 100,
        }],
    );
    let render = harness.play(4096);
    let left = render.left();
    // The first block sounds, and from the second the plugin writes nothing at all.
    assert_ne!(left[0], 0.0);
    let after = &left[MAX_BLOCK..];
    assert!(
        after.iter().all(|sample| *sample == 0.0),
        "the block before was played again: {:?}",
        &after[..8]
    );
}
