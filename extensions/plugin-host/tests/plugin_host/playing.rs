//! A hosted plugin plays: notes, the sustain pedal, the wheels, the key pressure and other
//! controllers reach it on their frames.

use plugin_host::PluginFormat;
use sound_core::MAX_BLOCK;
use test_plugin_support::Wheel;

use crate::support::{
    FORMATS, Harness, Played, record, tell_the_plugin,
    tell_the_plugin_not_to_implement_set_processing, tell_the_plugin_to_go_silent,
    tell_the_plugin_to_map_no_wheels, tell_the_plugin_to_show,
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
    // the frame it arrived on.
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

    // The test plugin writes the pedal it received into the right channel.
    assert_eq!(right[63], 0.0, "{format:?}");
    assert_eq!(right[64], 100.0 / 127.0, "{format:?}");
    assert_eq!(right[255], 100.0 / 127.0, "{format:?}");
    assert_eq!(right[256], 40.0 / 127.0, "{format:?}");
}

/// The plugin has no idea the transport stopped.
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

const WHEELS: [Wheel; 3] = [Wheel::Bend, Wheel::ModWheel, Wheel::Pressure];

fn name(wheel: Wheel) -> &'static str {
    match wheel {
        Wheel::Bend => "bend",
        Wheel::ModWheel => "mod_wheel",
        Wheel::Pressure => "pressure",
        Wheel::Controller => "controller",
    }
}

/// The bend goes to both ends.
fn moves(wheel: Wheel) -> [i16; 2] {
    match wheel {
        Wheel::Bend => [8191, -8192],
        Wheel::ModWheel | Wheel::Pressure | Wheel::Controller => [127, 40],
    }
}

fn played(wheel: Wheel, frame: u64, value: i16) -> Played {
    let amount = value as u8;
    match wheel {
        Wheel::Bend => Played::Bend { frame, value },
        Wheel::ModWheel => Played::ModWheel {
            frame,
            value: amount,
        },
        Wheel::Pressure => Played::Pressure {
            frame,
            value: amount,
        },
        Wheel::Controller => Played::Control {
            frame,
            controller: test_plugin_support::CONTROLLER,
            value: amount,
        },
    }
}

/// What the test plugin shows in its right channel for a wheel at `value`.
fn heard(wheel: Wheel, value: i16) -> f32 {
    let heard = match wheel {
        Wheel::Bend => f64::from(value + 8192) / 16383.0,
        Wheel::ModWheel | Wheel::Pressure | Wheel::Controller => f64::from(value) / 127.0,
    };
    heard as f32
}

/// A VST 3 plugin gets them as the parameters it maps them to, a CLAP one as MIDI.
#[test]
fn each_wheel_reaches_the_plugin_with_its_value_on_its_frame() {
    for format in FORMATS {
        for wheel in WHEELS {
            wheel_on_its_frames(format, wheel);
        }
    }
}

fn wheel_on_its_frames(format: PluginFormat, wheel: Wheel) {
    tell_the_plugin_to_show(name(wheel));
    let [first, second] = moves(wheel);
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
        vec![played(wheel, 64, first), played(wheel, 256, second)],
    );
    let right = harness.play(512).right();
    assert_eq!(right[63], wheel.rest() as f32, "{format:?} {wheel:?}");
    assert_eq!(right[64], heard(wheel, first), "{format:?} {wheel:?}");
    assert_eq!(right[255], heard(wheel, first), "{format:?} {wheel:?}");
    assert_eq!(right[256], heard(wheel, second), "{format:?} {wheel:?}");
}

/// The plugin has no idea the transport stopped, so a bend would stay stuck.
#[test]
fn all_off_puts_a_moved_wheel_back_at_rest() {
    for format in FORMATS {
        for wheel in WHEELS {
            all_off_puts_the_wheel_back(format, wheel);
        }
    }
}

fn all_off_puts_the_wheel_back(format: PluginFormat, wheel: Wheel) {
    tell_the_plugin_to_show(name(wheel));
    let [value, _] = moves(wheel);
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
        vec![played(wheel, 0, value), Played::AllOff { frame: 256 }],
    );
    let right = harness.play(512).right();
    let rest = wheel.rest() as f32;
    assert_eq!(right[255], heard(wheel, value), "{format:?} {wheel:?}");
    assert_eq!(right[256], rest, "{format:?} {wheel:?}");
    assert_eq!(right[511], rest, "{format:?} {wheel:?}");
}

/// A controller other than the wheels has no rest.
#[test]
fn another_controller_reaches_the_plugin_on_its_frame_and_all_off_leaves_it() {
    let wheel = Wheel::Controller;
    for format in FORMATS {
        wheel_on_its_frames(format, wheel);
        let mut harness = Harness::new();
        let played = vec![played(wheel, 0, 127), Played::AllOff { frame: 256 }];
        harness.add_track(record(format, "piano"), played);
        assert_eq!(harness.play(512).right()[511], 1.0, "{format:?}");
    }
}

/// A host that sent the wheel anyway would move whatever parameter the plugin keeps for it.
#[test]
fn a_wheel_the_vst3_plugin_does_not_map_is_not_sent() {
    tell_the_plugin_to_map_no_wheels();
    for wheel in WHEELS {
        tell_the_plugin_to_show(name(wheel));
        let [value, _] = moves(wheel);
        let mut harness = Harness::new();
        harness.add_track(
            record(PluginFormat::Vst3, "piano"),
            vec![
                Played::On {
                    frame: 0,
                    pitch: 60,
                    velocity: 100,
                },
                played(wheel, 64, value),
            ],
        );
        let render = harness.play(512);
        let rest = wheel.rest() as f32;
        assert!(
            render.right().iter().all(|sample| *sample == rest),
            "{wheel:?}"
        );
        assert_eq!(render.first_sound(), Some(0), "{wheel:?}");
        assert_eq!(harness.problems(), Vec::<String>::new(), "{wheel:?}");
    }
}

/// The realtime sanitizer runs over this whole render and aborts on any of them in our own
/// `process`.
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
                Played::Bend {
                    frame: index * 13 + 5,
                    value: (index as i16 * 256) - 8192,
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

/// A host that gave the plugin a buffer of its own for events out would have to grow it on the
/// audio thread, inside the plugin's call, where the realtime sanitizer is switched off.
#[test]
fn a_plugin_that_sends_more_events_than_any_buffer_holds_allocates_nothing() {
    for format in FORMATS {
        sends_more_than_any_buffer_holds(format);
    }
}

/// CLAP sends events out, VST 3 sends parameter changes.
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
    // From the very first block: a buffer of the host's own would grow only once.
    let (render, allocations) = harness.render_counting_allocations(8192);
    tell_the_plugin(None, None);
    assert_eq!(allocations, 0, "the audio thread allocated, {format:?}");
    assert!(render.first_sound().is_some(), "{format:?}");
}

/// VST 3 takes the pedal as points of a parameter, and a block holds a fixed number of them. A
/// pedal that came up and was refused would hold for ever, and the `AllOff` of a stop would
/// not end it either, as that is a pedal move as well.
#[test]
fn a_pedal_that_moves_more_often_than_a_block_holds_still_ends_where_it_was_played() {
    let mut harness = Harness::new();
    // Fifty moves inside the first block of sixty-four frames, ending with the pedal up. The
    // host keeps room for thirty-two points.
    let mut played = vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }];
    played.extend((0..49).map(|step| Played::Pedal {
        frame: step,
        value: 127 - (step as u8 % 8),
    }));
    played.push(Played::Pedal {
        frame: 49,
        value: 0,
    });
    harness.add_track(record(PluginFormat::Vst3, "piano"), played);

    let render = harness.play(512);
    let right = render.right();
    assert_eq!(
        right[63],
        0.0,
        "the pedal never came up: {:?}",
        &right[56..64]
    );
    assert!(
        right[MAX_BLOCK..].iter().all(|sample| *sample == 0.0),
        "the pedal is still down after the block: {:?}",
        &right[MAX_BLOCK..MAX_BLOCK + 8]
    );
}

/// More edits at once than a block has room for, as a preset load can make. A pedal that came
/// up in a block the edits filled would hold for ever.
#[test]
fn a_flood_of_edits_leaves_room_for_the_pedal() {
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::FLOOD_VARIABLE, "1") };
    let mut harness = Harness::new();
    // The host sends the edits after every buffer of 512 frames, and they fill the first
    // blocks of the next one. The pedal goes down where the edits have left room, and comes
    // up in the second block of the next buffer.
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![
            Played::Pedal {
                frame: 256,
                value: 127,
            },
            Played::Pedal {
                frame: 576,
                value: 0,
            },
        ],
    );
    let render = harness.play(2048);
    let right = render.right();
    assert_eq!(right[575], 1.0);
    assert!(
        right[576..].iter().all(|sample| *sample == 0.0),
        "the pedal never came up"
    );
}

/// The VST 3 SDK's own default for `setProcessing` answers that it is not implemented, and
/// other hosts play such a plugin.
#[test]
fn a_plugin_that_does_not_implement_set_processing_plays() {
    tell_the_plugin_not_to_implement_set_processing();
    let mut harness = Harness::new();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![Played::On {
            frame: 0,
            pitch: 60,
            velocity: 100,
        }],
    );
    assert_ne!(harness.play(MAX_BLOCK).left()[0], 0.0);
}

/// VST 3 lets a plugin say its output is silent and leave the buffer as it is. A host that
/// does not clear its output buffers would play the block before over and over.
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
    assert_ne!(left[0], 0.0);
    let after = &left[MAX_BLOCK..];
    assert!(
        after.iter().all(|sample| *sample == 0.0),
        "the block before was played again: {:?}",
        &after[..8]
    );
}
