//! The math and the text of the tempo control in the transport. Pure functions, so the drag,
//! the keys and the readout cannot disagree about a value.
//!
//! The transport shows the tempo in effect at the playhead, and an edit changes that tempo
//! change. The control keeps no copy of the tempo map: it reads the one the project has when
//! it renders and when it publishes, so a `project.json` written from outside shows at once
//! and is kept by the next move of a drag.

use sound_core::{Tempo, TempoChange, TempoMap, Ticks};

/// Bpm per pixel of a plain drag, and of a fine drag with shift.
const DRAG_PER_PIXEL: f64 = 0.5;
const FINE_DRAG_PER_PIXEL: f64 = 0.05;
/// The step a drag moves by, from the tempo it began on. It does not snap the result.
const DRAG_STEP: f64 = 1.0;
const FINE_DRAG_STEP: f64 = 0.1;
/// What one arrow key adds, plain and with shift.
pub const KEY_STEP: f64 = 1.0;
pub const FINE_KEY_STEP: f64 = 0.1;

/// The name of the undo step of every tempo edit.
pub const LABEL: &str = "Change tempo";

/// The tempo change in effect at `tick`: the last one at or before it. An edit is about this
/// change, and it is named by its tick, never by its place in the list: an outside edit may
/// add or remove a tempo change while a drag is going on, and a drag must never change one
/// that only took the place of the one the composer grabbed.
pub fn change_at(tempo_map: &TempoMap, tick: Ticks) -> TempoChange {
    let changes = tempo_map.tempo_changes();
    let index = changes
        .partition_point(|change| change.tick <= tick)
        .saturating_sub(1);
    // A tempo map always has a change at tick 0, so the fallback is never used.
    changes.get(index).copied().unwrap_or(TempoChange {
        tick: Ticks(0),
        bpm: Tempo::default(),
    })
}

/// The same tempo map with the tempo change that starts exactly at `at` set to `bpm`, kept
/// inside the bounds of the clock so a drag past an end stops there. `None` when the map has
/// no change at that tick any more.
pub fn with_bpm(tempo_map: &TempoMap, at: Ticks, bpm: f64) -> Option<TempoMap> {
    tempo_map.with_tempo_at(at, bounded(bpm))
}

/// A tempo inside the bounds of the clock. `from_bpm` refuses only a tempo outside them, and
/// this one is inside them, so the fallback is never used.
fn bounded(bpm: f64) -> Tempo {
    Tempo::from_bpm(bpm.clamp(Tempo::MIN_BPM, Tempo::MAX_BPM)).unwrap_or_default()
}

/// The tempo that a drag of `pixels` up from `start` asks for. Like a clip drag, it moves by
/// whole steps from where it began and does not snap the result, so a tempo written by hand
/// keeps its fraction and a drag there and back ends on exactly the tempo it started from. A
/// plain drag steps by a bpm, a fine drag by a tenth.
pub fn dragged_bpm(start: f64, pixels: f32, fine: bool) -> f64 {
    let (per_pixel, step) = match fine {
        true => (FINE_DRAG_PER_PIXEL, FINE_DRAG_STEP),
        false => (DRAG_PER_PIXEL, DRAG_STEP),
    };
    let steps = (f64::from(pixels) * per_pixel / step).round();
    start + steps * step
}

/// The tempo as the transport shows it: up to three decimals with no zeros at the end, so a
/// value at rest is short. `120`, `93.5`, `120.125`.
pub fn tempo_text(tempo: Tempo) -> String {
    let text = format!("{:.3}", tempo.bpm());
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(changes: &[(u64, f64)]) -> TempoMap {
        let changes = changes
            .iter()
            .map(|&(tick, bpm)| TempoChange {
                tick: Ticks(tick),
                bpm: Tempo::from_bpm(bpm).unwrap(),
            })
            .collect();
        TempoMap::new("4/4".parse().unwrap(), changes).unwrap()
    }

    #[test]
    fn the_playhead_picks_the_tempo_change_in_effect() {
        let map = map(&[(0, 120.0), (3840, 60.0), (7680, 93.5)]);
        let tick_at = |tick| change_at(&map, Ticks(tick)).tick;
        assert_eq!(tick_at(0), Ticks(0));
        assert_eq!(tick_at(3839), Ticks(0));
        assert_eq!(tick_at(3840), Ticks(3840));
        assert_eq!(tick_at(7680), Ticks(7680));
        assert_eq!(tick_at(u64::MAX), Ticks(7680));
        assert_eq!(change_at(&map, Ticks(9999)).bpm.bpm(), 93.5);
        let one = self::map(&[(0, 120.0)]);
        assert_eq!(change_at(&one, Ticks(99_999)).tick, Ticks(0));
    }

    #[test]
    fn an_edit_changes_the_tempo_change_at_its_tick_and_leaves_the_rest() {
        let before = map(&[(0, 120.0), (3840, 60.0)]);
        let after = with_bpm(&before, Ticks(3840), 140.0).unwrap();
        assert_eq!(after.time_signature(), before.time_signature());
        assert_eq!(after.tempo_changes()[0], before.tempo_changes()[0]);
        assert_eq!(after.tempo_changes()[1].tick, Ticks(3840));
        assert_eq!(after.tempo_changes()[1].bpm.bpm(), 140.0);

        // Past an end it stops there. A tick with no tempo change has nothing to set.
        let bpm_of = |map: TempoMap| map.tempo_changes()[0].bpm.bpm();
        assert_eq!(bpm_of(with_bpm(&before, Ticks(0), 5.0).unwrap()), 10.0);
        assert_eq!(
            bpm_of(with_bpm(&before, Ticks(0), 5_000.0).unwrap()),
            1000.0
        );
        assert_eq!(with_bpm(&before, Ticks(1920), 140.0), None);
    }

    #[test]
    fn a_drag_moves_by_whole_steps_from_where_it_began() {
        assert_eq!(dragged_bpm(120.0, 0.0, false), 120.0);
        assert_eq!(dragged_bpm(120.0, 10.0, false), 125.0);
        assert_eq!(dragged_bpm(120.0, -10.0, false), 115.0);

        // A tempo written by hand keeps its fraction, and a drag there and back is exact.
        assert_eq!(dragged_bpm(93.5, 2.0, false), 94.5);
        assert_eq!(dragged_bpm(93.5, -2.0, false), 92.5);
        assert_eq!(dragged_bpm(93.5, 0.0, false), 93.5);
        assert_eq!(dragged_bpm(93.5, 0.9, false), 93.5);
        assert_eq!(dragged_bpm(120.125, 20.0, false), 130.125);
        assert_eq!(dragged_bpm(120.125, 0.0, false), 120.125);

        // A fine drag steps by a tenth and needs ten times the travel.
        assert_eq!(dragged_bpm(120.0, 10.0, true), 120.5);
        assert_eq!(dragged_bpm(93.5, 0.0, true), 93.5);
        assert!((dragged_bpm(93.5, 2.0, true) - 93.6).abs() < 1e-9);
    }

    #[test]
    fn the_readout_is_short() {
        let text = |bpm: f64| tempo_text(Tempo::from_bpm(bpm).unwrap());
        assert_eq!(text(120.0), "120");
        assert_eq!(text(93.5), "93.5");
        assert_eq!(text(120.125), "120.125");
        assert_eq!(text(10.0), "10");
        assert_eq!(text(1000.0), "1000");
    }
}
