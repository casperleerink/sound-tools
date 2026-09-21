//! The take: what it keeps, the clip it becomes and the file it is written to.

use midi::{Played, RawEvent, RawTake, Take, TakeEvent, take_path};
use sound_core::{InstanceId, State, Ticks};
use sound_notes::Pedal;

use crate::support::{off, on, pedal, pitch};

fn event(time_us: u64, tick: u64, played: Played) -> TakeEvent {
    TakeEvent {
        time_us,
        tick: Ticks(tick),
        played,
    }
}

fn take(start: u64, end: u64, events: Vec<TakeEvent>) -> Take {
    Take {
        start: Ticks(start),
        end: Ticks(end),
        events,
    }
}

#[test]
fn a_take_becomes_a_clip_at_the_ticks_the_engine_played() {
    let events = vec![
        event(0, 960, on(60, 88)),
        event(500_000, 1920, off(60)),
        event(500_000, 1920, on(64, 70)),
        event(1_000_000, 2880, off(64)),
    ];
    let clip = take(960, 4800, events).clip().unwrap();
    assert_eq!(clip.start, Ticks(960));
    assert_eq!(clip.length.ticks(), Ticks(3840));
    let notes: Vec<(u64, u64, u8, u8)> = clip
        .notes
        .iter()
        .map(|note| {
            (
                note.start.0,
                note.length.ticks().0,
                note.pitch.number(),
                note.velocity.value(),
            )
        })
        .collect();
    assert_eq!(notes, [(0, 960, 60, 88), (960, 960, 64, 70)]);
    assert!(clip.pedal.is_empty());
    assert_eq!(clip.validate(), Ok(()));
}

#[test]
fn a_note_that_is_still_held_when_recording_ends_ends_there() {
    let events = vec![event(0, 0, on(60, 88))];
    let clip = take(0, 1920, events).clip().unwrap();
    assert_eq!(clip.length.ticks(), Ticks(1920));
    assert_eq!(clip.notes[0].length.ticks(), Ticks(1920));
    assert_eq!(clip.validate(), Ok(()));
}

/// A key that was already down when recording began sends only its note off. Half a note is
/// not music, so it is left out of the clip. The raw take still holds it.
#[test]
fn a_note_off_without_its_note_on_is_left_out_of_the_clip() {
    let events = vec![event(0, 0, off(60)), event(100_000, 480, on(64, 70))];
    let take = take(0, 1920, events);
    let clip = take.clip().unwrap();
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].pitch.number(), 64);
    assert_eq!(take.events.len(), 2);
}

/// Two presses of one pitch that overlap: each note off ends the press that began first.
#[test]
fn two_presses_of_one_pitch_each_get_their_own_note() {
    let events = vec![
        event(0, 0, on(60, 100)),
        event(1, 480, on(60, 60)),
        event(2, 960, off(60)),
        event(3, 1440, off(60)),
    ];
    let clip = take(0, 1920, events).clip().unwrap();
    let notes: Vec<(u64, u64, u8)> = clip
        .notes
        .iter()
        .map(|note| (note.start.0, note.length.ticks().0, note.velocity.value()))
        .collect();
    assert_eq!(notes, [(0, 960, 100), (480, 960, 60)]);
}

/// Some pedals send a stream of the same value. A move that changes nothing is left out.
#[test]
fn the_pedal_is_in_the_clip_without_the_moves_that_change_nothing() {
    let events = vec![
        event(0, 0, pedal(127)),
        event(1, 240, pedal(127)),
        event(2, 480, on(60, 88)),
        event(3, 960, pedal(0)),
        event(4, 1200, pedal(0)),
        event(5, 1440, pedal(64)),
    ];
    let clip = take(0, 1920, events).clip().unwrap();
    let pedal: Vec<(u64, u8)> = clip
        .pedal
        .iter()
        .map(|change| (change.start.0, change.value.value()))
        .collect();
    assert_eq!(pedal, [(0, 127), (960, 0), (1440, 64)]);
    assert_eq!(clip.validate(), Ok(()));
}

/// The clip always covers every message, also when recording was stopped in the same block as
/// the last note. Else the record would not load: a note starts inside its clip.
#[test]
fn the_clip_covers_a_message_on_the_tick_the_recording_ended() {
    let events = vec![event(0, 1920, on(60, 88))];
    let clip = take(960, 1920, events).clip().unwrap();
    assert_eq!(clip.length.ticks(), Ticks(961));
    assert_eq!(clip.notes[0].start, Ticks(960));
    assert_eq!(clip.validate(), Ok(()));
}

#[test]
fn a_take_with_nothing_in_it_makes_no_clip() {
    let take = take(0, 3840, Vec::new());
    assert!(take.is_empty());
    assert_eq!(take.clip(), None);
}

#[test]
fn the_raw_take_file_holds_the_times_as_they_arrived_and_both_velocities() {
    let folder = tempfile::tempdir().unwrap();
    let clip = InstanceId::new("arrangement/piano/take-1").unwrap();
    let events = vec![
        event(0, 0, pedal(127)),
        event(15_230, 40, on(60, 88)),
        event(
            412_870,
            990,
            Played::Off {
                pitch: pitch(60),
                velocity: 31,
            },
        ),
    ];
    let path = take(0, 3840, events).write(folder.path(), &clip).unwrap();
    assert_eq!(path, take_path(folder.path(), &clip));
    assert!(path.ends_with("assets/takes/arrangement/piano/take-1.json"));

    let text = std::fs::read_to_string(&path).unwrap();
    let raw: RawTake = serde_json::from_str(&text).unwrap();
    assert_eq!(raw.clip, "arrangement/piano/take-1");
    assert_eq!(raw.start_tick, 0);
    assert_eq!(raw.end_tick, 3840);
    assert_eq!(
        raw.events,
        [
            RawEvent::Pedal {
                time_us: 0,
                value: 127
            },
            RawEvent::On {
                time_us: 15_230,
                pitch: 60,
                velocity: 88
            },
            RawEvent::Off {
                time_us: 412_870,
                pitch: 60,
                velocity: 31
            },
        ]
    );
    // The clip drops the key up velocity. The take is the only place that has it.
    let _ = Pedal::UP;
}
