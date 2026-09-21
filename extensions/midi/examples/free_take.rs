//! Plays a free piano take into a virtual MIDI source, for a check by hand.
//!
//! No agent can play a keyboard, and the checks of this milestone need a take played with no
//! click. This makes a virtual source that the runtime finds like any other keyboard and plays
//! a chord progression with a tempo that wanders and the jitter of a hand, with the sustain
//! pedal. The same bytes at the same moments on every run.
//!
//! Run it as a built program, not through `cargo run`, when a `cargo` command is already
//! running: they share one build lock and the player would wait for it.
//!
//! ```sh
//! cargo build -p midi --example free_take
//! /private/tmp/sound-tools-timing/target/debug/examples/free_take 60 "Free Take"
//! ```

use std::time::{Duration, Instant};

use midir::MidiOutput;
use midir::os::unix::VirtualOutput;

/// The chords of the take, one per bar: the bass note and the right hand over it.
const BARS: [(u8, [u8; 3]); 8] = [
    (41, [65, 69, 72]),
    (45, [64, 69, 72]),
    (43, [62, 67, 71]),
    (48, [64, 67, 72]),
    (41, [65, 69, 72]),
    (46, [65, 69, 74]),
    (43, [62, 67, 71]),
    (36, [60, 64, 67]),
];

fn main() {
    let mut arguments = std::env::args().skip(1);
    let seconds: f64 = arguments
        .next()
        .and_then(|it| it.parse().ok())
        .unwrap_or(60.0);
    let name = arguments.next().unwrap_or_else(|| "Free Take".to_string());

    let (messages, beats) = take(Duration::from_secs_f64(seconds));
    let notes = messages.iter().filter(|it| it.1[0] == 0x90).count();

    let output = MidiOutput::new("sound-tools free take").expect("a midi client");
    let mut port = output.create_virtual(&name).expect("a virtual source");
    println!("virtual source {name:?} is open: {notes} notes over {beats} beats");
    // The runtime reads its port list once a second, so give it time to find this one.
    std::thread::sleep(Duration::from_millis(1500));

    let started = Instant::now();
    for (when, bytes) in &messages {
        let elapsed = started.elapsed();
        if *when > elapsed {
            std::thread::sleep(*when - elapsed);
        }
        port.send(bytes).expect("a message");
    }
    println!("played {notes} notes in {:?}", started.elapsed());
}

/// Every message of the take with the moment it is played, in order. Building it first means
/// one thread can play it: a note off never has to wait for a note on that is already past.
fn take(seconds: Duration) -> (Vec<(Duration, [u8; 3])>, u64) {
    let mut messages: Vec<(Duration, [u8; 3])> = Vec::new();
    let mut jitter = Jitter::new();
    let mut at = Duration::from_millis(500);
    let mut beat = 0_u64;
    while at < seconds {
        let bar = (beat / 4) as usize % BARS.len();
        let (bass, chord) = BARS[bar];
        let in_bar = beat % 4;
        // The tempo wanders between about 88 and 104 bpm in a slow curve, so the grid a fit
        // finds is nowhere near one number.
        let phase = beat as f64 / 11.0;
        let bpm = 96.0 + 8.0 * (phase * std::f64::consts::TAU).sin();
        let beat_length = Duration::from_secs_f64(60.0 / bpm);

        if in_bar == 0 {
            // The pedal comes up just before the bar and goes down on it, as a pianist pedals.
            messages.push((at.saturating_sub(Duration::from_millis(12)), [0xB0, 64, 0]));
            messages.push((at + Duration::from_millis(18), [0xB0, 64, 127]));
        }
        // The left hand marks the beat on 1 and 3.
        if in_bar.is_multiple_of(2) {
            let when = at + jitter.wait(18.0);
            messages.push((when, [0x90, bass, 70 + jitter.velocity()]));
            messages.push((when + beat_length.mul_f64(0.9), [0x80, bass, 0]));
        }
        // The right hand plays the chord on the beat, spread a little.
        for (index, pitch) in chord.iter().enumerate() {
            let when = at + jitter.wait(14.0) + Duration::from_micros(index as u64 * 6000);
            messages.push((when, [0x90, *pitch, 58 + jitter.velocity()]));
            messages.push((when + beat_length.mul_f64(0.45), [0x80, *pitch, 0]));
        }
        // An eighth after the off beats, so not every onset is on a beat.
        if !in_bar.is_multiple_of(2) {
            let pitch = chord[2] + 5;
            let when = at + beat_length.mul_f64(0.5) + jitter.wait(16.0);
            messages.push((when, [0x90, pitch, 52]));
            messages.push((when + beat_length.mul_f64(0.4), [0x80, pitch, 0]));
        }

        at += beat_length;
        beat += 1;
    }
    // The end: the pedal up and every key released, whatever the take left down.
    messages.push((at, [0xB0, 64, 0]));
    for pitch in 21..109_u8 {
        messages.push((at, [0x80, pitch, 0]));
    }
    messages.sort_by_key(|(when, _)| *when);
    (messages, beat)
}

/// A small repeatable wobble, so the take is not a machine and is the same every run.
struct Jitter(u64);

impl Jitter {
    fn new() -> Self {
        Self(0x2545_F491_4F6C_DD1D)
    }

    fn next_number(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A wait of up to `milliseconds`, never negative, so a note is always after its beat.
    fn wait(&mut self, milliseconds: f64) -> Duration {
        let part = (self.next_number() % 1000) as f64 / 1000.0;
        Duration::from_secs_f64(part * milliseconds / 1000.0)
    }

    fn velocity(&mut self) -> u8 {
        (self.next_number() % 24) as u8
    }
}
