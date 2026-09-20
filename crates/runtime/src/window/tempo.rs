//! The math and the text of the tempo control in the transport. Pure functions, so the drag,
//! the keys and the readout cannot disagree about a value.
//!
//! The transport shows the tempo in effect at the playhead, and an edit changes that tempo
//! change. The control keeps no copy of it: it reads the tempo map when it renders, so a
//! `project.json` written from outside shows at once.

use sound_core::{ClockError, Tempo, TempoMap, Ticks};

/// Bpm per pixel of a plain drag, and of a fine drag with shift.
const DRAG_PER_PIXEL: f64 = 0.5;
const FINE_DRAG_PER_PIXEL: f64 = 0.05;
/// The step a drag lands on, so the number does not gain and lose a decimal under the pointer.
const DRAG_STEP: f64 = 1.0;
const FINE_DRAG_STEP: f64 = 0.1;
/// What one arrow key adds, plain and with shift.
pub const KEY_STEP: f64 = 1.0;
pub const FINE_KEY_STEP: f64 = 0.1;

/// The name of the undo step of every tempo edit.
pub const LABEL: &str = "Change tempo";

/// The index of the tempo change in effect at `tick`: the last one at or before it. A tempo
/// map always has a change at tick 0, so this is always a real index.
pub fn change_at(tempo_map: &TempoMap, tick: Ticks) -> usize {
    tempo_map
        .tempo_changes()
        .partition_point(|change| change.tick <= tick)
        .saturating_sub(1)
}

/// The same tempo map with the change at `index` set to `bpm`. The tempo is kept inside the
/// bounds of the clock, so a drag that runs past an end stops there. It fails only on a tempo
/// map that is already wrong, which a live project cannot hold.
pub fn with_bpm(tempo_map: &TempoMap, index: usize, bpm: f64) -> Result<TempoMap, ClockError> {
    let bpm = Tempo::from_bpm(bpm.clamp(Tempo::MIN_BPM, Tempo::MAX_BPM))?;
    let mut changes = tempo_map.tempo_changes().to_vec();
    if let Some(change) = changes.get_mut(index) {
        change.bpm = bpm;
    }
    TempoMap::new(tempo_map.time_signature(), changes)
}

/// The tempo that a drag of `pixels` up from `start` asks for. A plain drag lands on whole
/// bpm and a fine drag on tenths.
pub fn dragged_bpm(start: f64, pixels: f32, fine: bool) -> f64 {
    let (per_pixel, step) = match fine {
        true => (FINE_DRAG_PER_PIXEL, FINE_DRAG_STEP),
        false => (DRAG_PER_PIXEL, DRAG_STEP),
    };
    let bpm = start + f64::from(pixels) * per_pixel;
    (bpm / step).round() * step
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
            .map(|&(tick, bpm)| sound_core::TempoChange {
                tick: Ticks(tick),
                bpm: Tempo::from_bpm(bpm).unwrap(),
            })
            .collect();
        TempoMap::new("4/4".parse().unwrap(), changes).unwrap()
    }

    #[test]
    fn the_playhead_picks_the_tempo_change_in_effect() {
        let map = map(&[(0, 120.0), (3840, 60.0), (7680, 93.5)]);
        assert_eq!(change_at(&map, Ticks(0)), 0);
        assert_eq!(change_at(&map, Ticks(3839)), 0);
        assert_eq!(change_at(&map, Ticks(3840)), 1);
        assert_eq!(change_at(&map, Ticks(7680)), 2);
        assert_eq!(change_at(&map, Ticks(u64::MAX)), 2);
        let one = self::map(&[(0, 120.0)]);
        assert_eq!(change_at(&one, Ticks(99_999)), 0);
    }

    #[test]
    fn an_edit_changes_one_tempo_change_and_leaves_the_rest() {
        let before = map(&[(0, 120.0), (3840, 60.0)]);
        let after = with_bpm(&before, 1, 140.0).unwrap();
        assert_eq!(after.time_signature(), before.time_signature());
        assert_eq!(after.tempo_changes()[0], before.tempo_changes()[0]);
        assert_eq!(after.tempo_changes()[1].tick, Ticks(3840));
        assert_eq!(after.tempo_changes()[1].bpm.bpm(), 140.0);

        // Past an end it stops there, and an index that is gone changes nothing.
        assert_eq!(with_bpm(&before, 0, 5.0).unwrap().tempo_changes()[0].bpm.bpm(), 10.0);
        assert_eq!(
            with_bpm(&before, 0, 5_000.0).unwrap().tempo_changes()[0]
                .bpm
                .bpm(),
            1000.0
        );
        assert_eq!(with_bpm(&before, 7, 140.0).unwrap(), before);
    }

    #[test]
    fn a_drag_up_speeds_up_and_lands_on_whole_numbers() {
        assert_eq!(dragged_bpm(120.0, 0.0, false), 120.0);
        assert_eq!(dragged_bpm(120.0, 10.0, false), 125.0);
        assert_eq!(dragged_bpm(120.0, -10.0, false), 115.0);
        assert_eq!(dragged_bpm(93.5, 1.0, false), 94.0);
        // A fine drag lands on tenths and needs ten times the travel.
        assert_eq!(dragged_bpm(120.0, 10.0, true), 120.5);
        assert!((dragged_bpm(120.0, -2.0, true) - 119.9).abs() < 1e-9);
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
