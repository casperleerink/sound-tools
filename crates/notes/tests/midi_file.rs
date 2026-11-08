//! Standard MIDI files: a file as other programs write them reads into the parts it plays, and
//! a clip written as a file reads back as it was.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use sound_core::Ticks;
use sound_notes::midi_file::{MidiFileError, read, write};
use sound_notes::{
    Amount, Bend, Clip, Controller, ExpressionValue, Length, Note, Pedal, PedalChange, Pitch,
    Point, Velocity, value_at,
};

/// A file of `tracks`, each the bytes of its events, at `division` ticks a quarter.
fn file(format: u16, division: u16, tracks: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"MThd".to_vec();
    bytes.extend(6u32.to_be_bytes());
    bytes.extend(format.to_be_bytes());
    bytes.extend((tracks.len() as u16).to_be_bytes());
    bytes.extend(division.to_be_bytes());
    for track in tracks {
        bytes.extend(b"MTrk");
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(track);
    }
    bytes
}

/// A track's events: each a delta in ticks and the bytes after it, as they are in a file.
fn track(events: &[(u32, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (delta, event) in events {
        let mut groups = vec![(delta & 0x7F) as u8];
        let mut rest = delta >> 7;
        while rest > 0 {
            groups.push((rest & 0x7F) as u8 | 0x80);
            rest >>= 7;
        }
        bytes.extend(groups.iter().rev());
        bytes.extend(*event);
    }
    bytes
}

fn note(start: u64, length: u64, pitch: u8, velocity: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

fn point<V>(tick: u64, value: V) -> Point<V> {
    Point {
        tick: Ticks(tick),
        value,
    }
}

fn amount(value: u8) -> Amount {
    Amount::new(value).unwrap()
}

/// A conductor track, then a track of a band with bass on channel 1 and drums on channel 10,
/// then keys on channel 2: running status, offs as ons of velocity 0, a reset and setup the
/// file sends first, overlapping notes of one pitch, and a note left on. 480 ticks a quarter,
/// half of ours.
#[test]
fn a_file_as_other_programs_write_it_reads_into_its_parts() {
    let conductor = track(&[
        (0, b"\xFF\x03\x04Song"),
        (0, b"\xFF\x51\x03\x07\xA1\x20"),
        (0, b"\xFF\x58\x04\x04\x02\x18\x08"),
        (0, b"\xFF\x2F\x00"),
    ]);
    let band = track(&[
        (0, b"\xFF\x03\x04Band"),
        (0, b"\xF0\x05\x7E\x7F\x09\x01\xF7"),
        (0, b"\xC0\x21"),
        // The pitch bend range, as parameter numbers and their data, in running status.
        (0, b"\xB0\x65\x00"),
        (0, b"\x64\x00"),
        (0, b"\x06\x02"),
        (0, b"\x07\x64"),
        (0, b"\x90\x28\x64"),
        (0, b"\x99\x24\x70"),
        (240, b"\x89\x24\x40"),
        (240, b"\x90\x28\x00"),
        (0, b"\x2B\x50"),
        (480, b"\x2B\x00"),
        (0, b"\xB0\x07\x32"),
        (0, b"\xE0\x00\x50"),
        (960, b"\xFF\x2F\x00"),
    ]);
    let keys = track(&[
        (0, b"\xFF\x03\x04Keys"),
        (0, b"\xB1\x40\x7F"),
        (0, b"\x91\x3C\x5A"),
        (240, b"\x3C\x50"),
        (240, b"\x81\x3C\x00"),
        (240, b"\x81\x3C\x00"),
        (240, b"\x91\x40\x46"),
        (960, b"\xB1\x40\x00"),
        (0, b"\xFF\x2F\x00"),
    ]);
    let parts = read(&file(1, 480, &[conductor, band, keys]), "song").unwrap();

    let titles: Vec<&str> = parts.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(titles, ["Band 1", "Band 2", "Keys"]);
    let [(_, bass), (_, drums), (_, piano)] = &parts[..] else {
        panic!("three parts");
    };

    let mut expected = Clip::new(
        Ticks(0),
        Length::new(Ticks(3840)).unwrap(),
        vec![note(0, 960, 40, 100), note(960, 960, 43, 80)],
    );
    // The volume steps at its second value; the bend rests until it moves.
    let volume = Controller::new(7).unwrap();
    expected.controllers = BTreeMap::from([(
        volume,
        vec![
            point(0, amount(100)),
            point(1919, amount(100)),
            point(1920, amount(50)),
        ],
    )]);
    let up = Bend::new(2048).unwrap();
    expected.bend = vec![
        point(0, Bend::MIDDLE),
        point(1919, Bend::MIDDLE),
        point(1920, up),
    ];
    assert_eq!(bass, &expected);

    assert_eq!(
        drums,
        &Clip::new(
            Ticks(0),
            Length::new(Ticks(3840)).unwrap(),
            vec![note(0, 480, 36, 112)]
        )
    );

    // Of two notes of one pitch, an off ends the older; the note left on ends with its
    // track, and so does the pedal, which needs no lift there.
    let mut expected = Clip::new(
        Ticks(0),
        Length::new(Ticks(3840)).unwrap(),
        vec![
            note(0, 960, 60, 90),
            note(480, 960, 60, 80),
            note(1920, 1920, 64, 70),
        ],
    );
    expected.pedal = vec![PedalChange {
        start: Ticks(0),
        value: Pedal::new(127).unwrap(),
    }];
    assert_eq!(piano, &expected);
}

/// At 19,200 ticks a quarter twenty of the file's ticks are one of ours: of two values on one
/// tick the later stays, and a note of one tick at the very end makes its clip a tick longer.
#[test]
fn a_fine_file_keeps_the_later_value_of_a_tick_and_every_note() {
    let events = track(&[
        (0, b"\xB0\x4A\x0A"),
        (1, b"\xB0\x4A\x14"),
        (199, b"\x90\x3C\x64"),
        (1, b"\x80\x3C\x00"),
        (0, b"\xFF\x2F\x00"),
    ]);
    let parts = read(&file(0, 19200, &[events]), "fine").unwrap();
    let [(title, clip)] = &parts[..] else {
        panic!("one part");
    };
    assert_eq!(title, "fine");
    assert_eq!(clip.notes, [note(10, 1, 60, 100)]);
    assert_eq!(clip.length.ticks(), Ticks(11));
    let brightness = Controller::new(74).unwrap();
    assert_eq!(clip.controllers[&brightness], [point(0, amount(20))]);
    clip.validate().unwrap();
}

#[test]
fn files_that_cannot_become_parts_are_refused() {
    let notes = track(&[
        (0, b"\x90\x3C\x64"),
        (480, b"\x80\x3C\x00"),
        (0, b"\xFF\x2F\x00"),
    ]);
    assert_eq!(
        read(&file(2, 480, std::slice::from_ref(&notes)), "x"),
        Err(MidiFileError::Sequential)
    );
    // 25 frames a second, 40 ticks a frame.
    assert_eq!(
        read(&file(1, 0xE728, &[notes]), "x"),
        Err(MidiFileError::Frames)
    );
    let silent = track(&[(0, b"\xB0\x07\x64"), (0, b"\xFF\x2F\x00")]);
    assert_eq!(
        read(&file(1, 480, &[silent]), "x"),
        Err(MidiFileError::NoNotes)
    );
    assert_eq!(read(b"RIFF....WAVE", "x"), Err(MidiFileError::Unreadable));
}

/// Every lane within `steps` steps of `before` at every tick, and moving where it does.
fn same_lane<V: ExpressionValue>(after: &[Point<V>], before: &[Point<V>], length: u64, steps: i32) {
    for tick in 0..length {
        let (Some(was), Some(is)) = (value_at(before, Ticks(tick)), value_at(after, Ticks(tick)))
        else {
            assert_eq!(before.is_empty(), after.is_empty());
            continue;
        };
        let off = (was.number() - is.number()).abs();
        assert!(
            off <= steps * V::STEP,
            "{tick}: {} became {}",
            was.number(),
            is.number()
        );
    }
}

#[test]
fn a_clip_written_as_a_file_reads_back_as_it_was() {
    let length = 4 * 3840;
    let mut clip = Clip::new(
        Ticks(0),
        Length::new(Ticks(length)).unwrap(),
        vec![
            note(0, 480, 60, 100),
            note(0, 480, 64, 90),
            note(960, 1440, 67, 1),
            note(3840, 960, 60, 127),
        ],
    );
    clip.pedal = vec![
        PedalChange {
            start: Ticks(0),
            value: Pedal::new(127).unwrap(),
        },
        PedalChange {
            start: Ticks(1900),
            value: Pedal::UP,
        },
    ];
    clip.bend = vec![
        point(960, Bend::new(-8192).unwrap()),
        point(1920, Bend::new(8191).unwrap()),
        point(2400, Bend::MIDDLE),
    ];
    clip.mod_wheel = vec![point(0, amount(0)), point(3840, amount(127))];
    clip.pressure = vec![point(500, amount(40))];
    let brightness = Controller::new(74).unwrap();
    clip.controllers = BTreeMap::from([(
        brightness,
        vec![
            point(1000, amount(10)),
            point(5000, amount(90)),
            point(5001, amount(0)),
        ],
    )]);

    let parts = read(&write(&clip, "Lead", 133.0, (7, 8)), "lead").unwrap();
    let [(title, back)] = &parts[..] else {
        panic!("one part");
    };
    assert_eq!(title, "Lead");
    assert_eq!(back.length, clip.length);
    assert_eq!(back.notes, clip.notes);
    assert_eq!(back.pedal, clip.pedal);
    // A file sends a lane in steps: back as lines, within the steps that took.
    same_lane(&back.bend, &clip.bend, length, 2);
    same_lane(&back.mod_wheel, &clip.mod_wheel, length, 1);
    same_lane(&back.pressure, &clip.pressure, length, 1);
    let lane = &back.controllers[&brightness];
    assert_eq!(
        lane.first().map(|point| point.tick),
        Some(Ticks(1000)),
        "a controller starts at its first point"
    );
    same_lane(lane, &clip.controllers[&brightness], length, 1);
}

/// A note longer than its clip ends where the clip does, and a pedal still down goes up on the
/// last tick, as they sound.
#[test]
fn a_clip_is_written_as_it_sounds() {
    let mut clip = Clip::new(
        Ticks(7680),
        Length::new(Ticks(960)).unwrap(),
        vec![note(480, 3840, 60, 100)],
    );
    clip.pedal = vec![PedalChange {
        start: Ticks(0),
        value: Pedal::new(127).unwrap(),
    }];
    clip.bend = vec![point(0, Bend::new(8191).unwrap())];
    let bytes = write(&clip, "", 120.0, (4, 4));
    // The bend rests where the clip ends, as it plays, for whatever plays the file.
    let smf = midly::Smf::parse(&bytes).unwrap();
    let mut at = 0;
    let bends: Vec<(u32, i16)> = (smf.tracks[0].iter())
        .filter_map(|event| {
            at += event.delta.as_int();
            match event.kind {
                midly::TrackEventKind::Midi {
                    message: midly::MidiMessage::PitchBend { bend },
                    ..
                } => Some((at, bend.as_int())),
                _ => None,
            }
        })
        .collect();
    assert_eq!(bends, [(0, 8191), (960, 0)]);
    let parts = read(&bytes, "cut").unwrap();
    let [(title, back)] = &parts[..] else {
        panic!("one part");
    };
    assert_eq!(title, "cut");
    assert_eq!(back.start, Ticks(0));
    assert_eq!(back.notes, [note(480, 480, 60, 100)]);
    // And reads back without the rest, which the clip's end plays.
    assert_eq!(back.bend, clip.bend);
    assert_eq!(
        back.pedal,
        [
            PedalChange {
                start: Ticks(0),
                value: Pedal::new(127).unwrap()
            },
            PedalChange {
                start: Ticks(959),
                value: Pedal::UP
            }
        ]
    );
}
