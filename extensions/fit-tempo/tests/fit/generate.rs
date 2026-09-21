//! A deterministic generator of takes with a tempo curve nobody has to guess.
//!
//! Every test take is made here, from a tempo curve, a way of playing and an amount of jitter,
//! with a random number generator of its own so that the same case gives the same bytes on
//! every run. The true beat times come out with the take, which is what makes a bound on the
//! beat finder a number and not an opinion.

use sound_core::TimeSignature;
use sound_notes::{RawEvent, RawTake};

/// How the tempo moves while the take is played. The tempo is quarter notes a minute.
#[derive(Copy, Clone, Debug)]
pub enum Curve {
    Steady(f64),
    /// A slow give and take around `base`, `depth` of it at most, over `beats` beats.
    Rubato {
        base: f64,
        depth: f64,
        beats: f64,
    },
    /// Slows from `from` to `to` over the whole take.
    Ritardando {
        from: f64,
        to: f64,
    },
    /// `from` until beat `at`, then `to`.
    Sudden {
        from: f64,
        to: f64,
        at: usize,
    },
}

impl Curve {
    fn bpm(self, beat: usize, beats: usize) -> f64 {
        let part = beat as f64 / beats.max(1) as f64;
        match self {
            Self::Steady(bpm) => bpm,
            Self::Rubato { base, depth, beats } => {
                let phase = beat as f64 / beats * std::f64::consts::TAU;
                base * (1.0 + depth * phase.sin())
            }
            Self::Ritardando { from, to } => from + (to - from) * part,
            Self::Sudden { from, to, at } => {
                if beat < at {
                    from
                } else {
                    to
                }
            }
        }
    }
}

/// What the hand plays on the grid.
#[derive(Copy, Clone, Debug)]
pub enum Playing {
    /// A chord of three notes on every beat.
    Chords,
    /// Chords on beats, with notes between them and some beats left empty.
    Syncopated,
    /// Four notes spread over each beat, one after the other.
    Arpeggiated,
}

/// One case: everything a take is made from.
#[derive(Copy, Clone, Debug)]
pub struct Case {
    pub name: &'static str,
    pub time_signature: &'static str,
    pub curve: Curve,
    pub playing: Playing,
    /// How far a note may land from its place on the grid, in milliseconds.
    pub jitter_ms: f64,
    pub bars: usize,
    /// Where the recording begins on the project timeline, in seconds.
    pub starts_at_seconds: f64,
    /// How long the composer waits before the first note, in seconds.
    pub silence_seconds: f64,
}

impl Case {
    pub fn signature(&self) -> TimeSignature {
        self.time_signature
            .parse()
            .unwrap_or_else(|_| panic!("{} is no time signature", self.time_signature))
    }

    /// The take, and the true beat times on the project timeline in microseconds.
    pub fn take(&self) -> (RawTake, Vec<u64>) {
        let signature = self.signature();
        let beats = self.bars * signature.numerator() as usize;
        let mut random = Random::new(self.name);

        // The beats, from the curve. A beat of the time signature, not a quarter note.
        let mut times = Vec::with_capacity(beats + 1);
        let mut second = self.silence_seconds;
        for beat in 0..=beats {
            times.push(second);
            let bpm = self.curve.bpm(beat, beats);
            second += 240.0 / (f64::from(signature.denominator()) * bpm);
        }

        // The notes, from the way of playing.
        let mut notes: Vec<(f64, u8)> = Vec::new();
        let jitter = |random: &mut Random| self.jitter_ms / 1000.0 * random.symmetric();
        for (index, beat) in times.iter().enumerate().take(beats) {
            let step = times[index + 1] - beat;
            let root = 48 + [0, 4, 7, 5, 2, 9, 11][index % 7];
            match self.playing {
                Playing::Chords => {
                    for pitch in [root, root + 4, root + 7] {
                        notes.push((beat + jitter(&mut random), pitch));
                    }
                }
                Playing::Syncopated => {
                    // Every fourth beat is silent, and every third has a note after it.
                    if index % 4 != 3 {
                        for pitch in [root, root + 7] {
                            notes.push((beat + jitter(&mut random), pitch));
                        }
                    }
                    if index % 3 == 0 {
                        notes.push((beat + step * 0.5 + jitter(&mut random), root + 12));
                    }
                }
                Playing::Arpeggiated => {
                    for (part, pitch) in [root, root + 4, root + 7, root + 12].iter().enumerate() {
                        let at = beat + step * part as f64 / 4.0;
                        notes.push((at + jitter(&mut random), *pitch));
                    }
                }
            }
        }
        notes.sort_by(|a, b| a.0.total_cmp(&b.0));

        let start_us = (self.starts_at_seconds * 1_000_000.0).round() as u64;
        let mut events = Vec::with_capacity(notes.len() * 2);
        for (second, pitch) in &notes {
            let on = (second * 1_000_000.0).round() as u64;
            let off = on + 200_000;
            events.push(RawEvent::On {
                time_us: on,
                sounded_us: sounded(on),
                pitch: *pitch,
                velocity: 60 + (pitch % 40),
            });
            events.push(RawEvent::Off {
                time_us: off,
                sounded_us: sounded(off),
                pitch: *pitch,
                velocity: 0,
            });
        }
        events.sort_by_key(|event| (event.time_us(), matches!(event, RawEvent::On { .. })));
        let end_us = events.last().map_or(0, |event| event.time_us()) + 500_000;
        let take = RawTake {
            start_us,
            end_us: start_us + end_us,
            start_tick: 0,
            end_tick: 0,
            pedal_at_start: 0,
            events,
        };
        let true_beats = times
            .iter()
            .map(|second| start_us + (second * 1_000_000.0).round() as u64)
            .collect();
        (take, true_beats)
    }
}

/// The engine sounds a message at the start of the next block of 64 frames at 48 kHz, so a
/// generated take carries the same wait a recorded one does.
fn sounded(time_us: u64) -> u64 {
    let frame = time_us * 48_000 / 1_000_000;
    frame.div_ceil(64) * 64 * 1_000_000 / 48_000
}

/// A small generator with a fixed start, so that a case is the same take on every run and on
/// every machine. The constants are Knuth's.
pub struct Random(u64);

impl Random {
    pub fn new(seed: &str) -> Self {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        for byte in seed.bytes() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(u64::from(byte));
        }
        Self(state)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// A number from -1 to 1, as the sum of three draws so that the middle is more likely,
    /// which is what a hand does.
    pub fn symmetric(&mut self) -> f64 {
        let draw = |it: &mut Self| (it.next() >> 11) as f64 / (1_u64 << 53) as f64 * 2.0 - 1.0;
        (draw(self) + draw(self) + draw(self)) / 3.0 * 1.5
    }
}
