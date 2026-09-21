//! A take with a known tempo curve, for the tests and the window snapshots of a fit.
//!
//! It is made and not recorded, so the numbers are known: a hand that plays a chord on every
//! beat with a tempo that gives and takes, and the jitter of a real hand. It is written into a
//! project exactly as a recording writes one.

use sound_notes::{RawEvent, RawTake};

/// Where the take begins on the project timeline: two seconds in, so the fit has to keep it
/// there and the grid has a bar of lead in front of it.
pub const STARTS_AT_US: u64 = 2_000_000;

/// A take of `bars` bars of 4/4, one chord a beat, with a tempo that gives and takes around 96
/// bpm and the jitter of a hand. Deterministic: the same bytes on every run.
pub fn generated_take(bars: usize) -> RawTake {
    let beats = bars * 4;
    let mut random = 0x9E37_79B9_7F4A_7C15_u64;
    let mut jitter = || {
        random = random
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((random >> 33) as f64 / (1_u64 << 31) as f64 - 0.5) * 0.030
    };
    let mut events = Vec::new();
    let mut second = 0.8_f64;
    for beat in 0..beats {
        let bpm = 96.0 * (1.0 + 0.10 * (beat as f64 / 16.0 * std::f64::consts::TAU).sin());
        let root = 48 + [0, 4, 7, 5][beat % 4];
        for pitch in [root, root + 4, root + 7] {
            let on = ((second + jitter()) * 1_000_000.0).round() as u64;
            let off = on + 250_000;
            events.push(RawEvent::On {
                time_us: on,
                sounded_us: sounded(on),
                pitch,
                velocity: 88,
            });
            events.push(RawEvent::Off {
                time_us: off,
                sounded_us: sounded(off),
                pitch,
                velocity: 0,
            });
        }
        second += 60.0 / bpm;
    }
    events.sort_by_key(|event| (event.time_us(), matches!(event, RawEvent::On { .. })));
    let end_us = events.last().map_or(0, |event| event.time_us()) + 400_000;
    RawTake {
        start_us: STARTS_AT_US,
        end_us: STARTS_AT_US + end_us,
        start_tick: 0,
        end_tick: 0,
        pedal_at_start: 0,
        events,
    }
}

/// The engine sounds a message at the start of the next block of 64 frames at 48 kHz.
fn sounded(time_us: u64) -> u64 {
    let frame = time_us * 48_000 / 1_000_000;
    frame.div_ceil(64) * 64 * 1_000_000 / 48_000
}
