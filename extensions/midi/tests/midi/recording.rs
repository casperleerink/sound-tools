//! Recording through the real engine: the take gets the tick the engine sounded each message
//! on, and the times it arrived.

use sound_core::Ticks;

use crate::support::{Harness, TICK, off, on, pedal};

/// Plays from tick 0 and records. Every message is sent at the frame the test chooses, so the
/// ticks in the take are the ticks the engine really played them on.
fn recorded(messages: &[(usize, midi::Played)], frames: usize) -> midi::Take {
    let mut harness = Harness::new();
    harness.control.play();
    // One block so the transport is playing before the first message.
    harness.run(64, 64);
    harness.keyboard.start_recording(Ticks(0));
    let mut sent = 0;
    for (at, played) in messages {
        harness.run(at.saturating_sub(sent), 64);
        sent = *at;
        harness.input.send(*played);
    }
    harness.run(frames.saturating_sub(sent), 64);
    harness.keyboard.poll(None);
    let end = Ticks((frames / TICK) as u64);
    harness.keyboard.finish_recording(end).unwrap()
}

#[test]
fn a_take_holds_the_tick_the_engine_sounded_each_message_on() {
    // A quarter note from tick 96 to tick 192: at 120 bpm and 48 kHz that is frame 2400 to
    // 4800. The engine takes a message at the start of the next block of 64 frames.
    let take = recorded(&[(2400, on(60, 88)), (4800, off(60))], 9600);
    let ticks: Vec<u64> = take.events.iter().map(|event| event.tick.0).collect();
    // Frame 2464 is tick 98.56 and frame 4864 is tick 194.56, and the tick of a frame is the
    // first tick at or after it.
    assert_eq!(ticks, [99, 195]);
    let clip = take.clip().unwrap();
    assert_eq!(clip.start, Ticks(0));
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].start, Ticks(99));
    assert_eq!(clip.notes[0].length.ticks(), Ticks(96));
}

/// The take's times are those the messages arrived at, counted from the start of the
/// recording. They are real time, so they hold whatever the tempo map does.
#[test]
fn the_take_holds_the_times_the_messages_arrived_at() {
    let take = recorded(&[(2400, on(60, 88)), (4800, off(60))], 9600);
    let times: Vec<u64> = take.events.iter().map(|event| event.time_us).collect();
    assert_eq!(times.len(), 2);
    // The test sends them one after the other with no wait in between, so only their order
    // is a fact here. The real times come from the device thread.
    assert!(times[0] <= times[1], "{times:?}");
}

#[test]
fn the_pedal_is_recorded_as_it_was_played() {
    let take = recorded(
        &[
            (640, pedal(127)),
            (2400, on(60, 88)),
            (4800, off(60)),
            (7200, pedal(0)),
        ],
        9600,
    );
    let clip = take.clip().unwrap();
    let values: Vec<u8> = clip.pedal.iter().map(|it| it.value.value()).collect();
    assert_eq!(values, [127, 0]);
    assert_eq!(clip.notes.len(), 1);
}

/// Keys that are down when recording begins are not part of the take: their note ons sounded
/// before it. The note off that follows is in the take but makes no note.
#[test]
fn a_key_held_before_the_start_is_left_out_of_the_clip() {
    let mut harness = Harness::new();
    harness.control.play();
    harness.run(64, 64);
    harness.input.send(on(60, 88));
    harness.run(128, 64);
    harness.keyboard.poll(None);
    let from = harness.playhead();
    harness.keyboard.start_recording(from);
    harness.input.send(off(60));
    harness.input.send(on(64, 70));
    harness.run(640, 64);
    harness.keyboard.poll(None);
    let until = harness.playhead();
    let take = harness.keyboard.finish_recording(until).unwrap();
    assert_eq!(take.events.len(), 2);
    let clip = take.clip().unwrap();
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.notes[0].pitch.number(), 64);
}

/// Nothing arrives between the start and the end of the recording: no clip and nothing to
/// write.
#[test]
fn a_recording_with_nothing_played_gives_an_empty_take() {
    let mut harness = Harness::new();
    harness.control.play();
    harness.run(64, 64);
    harness.keyboard.start_recording(Ticks(0));
    harness.run(4800, 64);
    harness.keyboard.poll(None);
    let take = harness.keyboard.finish_recording(Ticks(192)).unwrap();
    assert!(take.is_empty());
    assert_eq!(take.clip(), None);
}

#[test]
fn what_is_played_while_the_project_does_not_play_is_not_recorded() {
    let mut harness = Harness::new();
    harness.keyboard.start_recording(Ticks(0));
    harness.input.send(on(60, 88));
    harness.run(640, 64);
    harness.keyboard.poll(None);
    assert!(harness.keyboard.is_recording());
    let take = harness.keyboard.finish_recording(Ticks(0)).unwrap();
    assert!(take.is_empty());
}

#[test]
fn recording_stops_and_a_second_take_is_a_take_of_its_own() {
    let mut harness = Harness::new();
    harness.control.play();
    harness.run(64, 64);
    harness.keyboard.start_recording(Ticks(0));
    harness.input.send(on(60, 88));
    harness.run(640, 64);
    harness.keyboard.poll(None);
    let first = harness.keyboard.finish_recording(Ticks(30)).unwrap();
    assert_eq!(first.events.len(), 1);
    assert!(!harness.keyboard.is_recording());
    assert_eq!(harness.keyboard.finish_recording(Ticks(30)), None);

    let from = harness.playhead();
    harness.keyboard.start_recording(from);
    harness.input.send(on(64, 88));
    harness.run(640, 64);
    harness.keyboard.poll(None);
    let until = harness.playhead();
    let second = harness.keyboard.finish_recording(until).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.start, from);
}
